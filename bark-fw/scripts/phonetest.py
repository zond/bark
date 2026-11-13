#!/usr/bin/env python3
"""The phone test, row A7 of docs/bench-suite-2026-11-13.md, on a phone over adb.

No dependencies beyond Python 3, adb and Flutter. It builds the app with its
phone test (bark-app/README.md, "The phone test"), installs it over the app on
the phone, keeping the app's files and so its identity and pairings, starts
it, and reads its steps from the Android log. It types the code from the
node's log into Android's pairing dialog, has a board send the text the test
asks for, and prints one line a step:

    PASS 1 find: ID 275C1AB9BAC7, -61 dBm, public
    FAIL 4 receive: nothing from Bench friend in 5 min; ...

    scripts/phonetest.py --node ID --to ID [--node-log FILE] [--to-log FILE]
        [--image IMG] [--pair any|bond|fresh] [--send CMD] [--serial S]

--node is the board the phone pairs with, --to the one its text goes to,
twelve hex digits each. --node-log and --to-log are their logs, one line per
log line with the host's time first (bench.py capture). --send is a command
for a board to send the text the test made, with {dst}, {dst_id}, {payload}
and {id} in it; without it the script says what to send and waits.

Exit status: 0 every step passed, the update skipped without --image; 1 a
step failed; 2 the run did not come to an end.
"""

from __future__ import annotations

import argparse
import queue
import re
import shlex
import subprocess
import sys
import threading
import time
import xml.etree.ElementTree as ET
from pathlib import Path

APP = Path(__file__).resolve().parents[2] / "bark-app"
PACKAGE = "org.bark.bark_app"
TARGET = "integration_test/phone_test.dart"
APK = APP / "build/app/outputs/flutter-apk/app-debug.apk"

# Where the test looks for the image: the app's files, which
# path_provider's application support directory is on Android
# (integration_test/phone_test.dart, imageFile).
IMAGE = "files/bark-phone-test.img"

# The permissions the app asks the person for; granted here so that no
# dialog of Android's stops the test. The test answers the app's own.
PERMISSIONS = [
    "android.permission.BLUETOOTH_SCAN",
    "android.permission.BLUETOOTH_CONNECT",
    "android.permission.ACCESS_FINE_LOCATION",
    "android.permission.POST_NOTIFICATIONS",
]

# How long the dialog and the code may take to show after the test says
# it pairs: the app's connect gives up after 30 s, and the node shows the
# code as the pairing asks for it (spec "Phone link", Pairing).
PAIRING_S = 60

# How long the second node may take to keep the text once the phone's node
# took it. A choice: one hop takes seconds; the rest is room for a busy
# channel.
DELIVERY_S = 120


# ---- the lines -------------------------------------------------------------


def parse(line: str) -> tuple[str, str] | None:
    """A line of the test's, as integration_test/phone_steps.dart writes it:
    its word and the rest.

    >>> parse("bark-test pass 1 find: ID FA4E00000001, -60 dBm, public")
    ('pass', '1 find: ID FA4E00000001, -60 dBm, public')
    >>> parse("bark-test done 5/5")
    ('done', '5/5')
    >>> parse("Connected, MTU 247") is None
    True
    """
    m = re.match(r"bark-test (\w+) ?(.*)$", line.strip())
    return (m.group(1), m.group(2)) if m else None


def fields(rest: str) -> dict[str, str]:
    """The key=value words of a line.

    >>> fields("dst=47c0490ad9ff80fa4e00000001 payload=02e2 id=a9fd9e20")
    {'dst': '47c0490ad9ff80fa4e00000001', 'payload': '02e2', 'id': 'a9fd9e20'}
    """
    return dict(w.split("=", 1) for w in rest.split() if "=" in w)


def passkey(text: str) -> str | None:
    """The code a node's log shows for a pairing; "" for a protected build,
    which logs that it shows one and not which (spec "Phone link", Pairing).

    >>> passkey("1731489201.512 ble: pairing, the code shown: 042113")
    '042113'
    >>> passkey("1731489201.512 ble: pairing, the code shown")
    ''
    >>> passkey("1731489201.512 ble: removal, the code shown: 123456") is None
    True
    """
    m = re.search(r"ble: pairing, the code shown(?:: (\d{6}))?\s*$", text)
    return None if m is None else m.group(1) or ""


def kept(text: str, msg_id: str) -> bool:
    """Whether a node's log line says it kept message `msg_id` for its phones.

    >>> kept("1731489201.512 mailbox: id=a9fd9e20 kept as 7", "a9fd9e20")
    True
    >>> kept("1731489201.512 mailbox: id=a9fd9e20 kept as 7", "00000000")
    False
    """
    return re.search(rf"mailbox: id={msg_id} kept\b", text) is not None


def dialog(dump: str) -> tuple[tuple[int, int] | None, tuple[int, int] | None]:
    """The centres of the code's field and the confirming button in a
    uiautomator dump, of a window not the app's: Android's pairing dialog.

    >>> d = '''<hierarchy><node package="com.android.settings">
    ...  <node class="android.widget.EditText" package="com.android.settings"
    ...   bounds="[100,500][980,600]" />
    ...  <node class="android.widget.Button" resource-id="android:id/button1"
    ...   package="com.android.settings" text="PAIR" bounds="[700,900][980,1000]" />
    ... </node></hierarchy>'''
    >>> dialog(d)
    ((540, 550), (840, 950))
    >>> dialog('<hierarchy><node class="android.widget.EditText" '
    ...        'package="org.bark.bark_app" bounds="[0,0][10,10]" /></hierarchy>')
    (None, None)
    """
    field = button = None
    try:
        root = ET.fromstring(dump)
    except ET.ParseError:
        return None, None
    for n in root.iter("node"):
        if n.get("package") == PACKAGE:
            continue
        m = re.fullmatch(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]", n.get("bounds", ""))
        if not m:
            continue
        x1, y1, x2, y2 = map(int, m.groups())
        centre = ((x1 + x2) // 2, (y1 + y2) // 2)
        if n.get("class") == "android.widget.EditText" and field is None:
            field = centre
        elif n.get("resource-id") == "android:id/button1" or (
            n.get("class") == "android.widget.Button"
            and n.get("text", "").strip().lower() in ("ok", "pair")
        ):
            button = centre
    return field, button


# ---- the phone -------------------------------------------------------------


class Phone:
    def __init__(self, serial: str | None):
        self.base = ["adb"] + (["-s", serial] if serial else [])

    def run(self, *args: str, stdin=None, check: bool = True) -> str:
        r = subprocess.run(self.base + list(args), stdin=stdin, capture_output=True)
        out = r.stdout.decode(errors="replace")
        if check and r.returncode != 0:
            err = r.stderr.decode(errors="replace")
            raise RuntimeError(f"adb {' '.join(args)}: {(out + err).strip()}")
        return out

    def shell(self, command: str, check: bool = True) -> str:
        return self.run("shell", command, check=check)

    def tap(self, at: tuple[int, int]) -> None:
        self.shell(f"input tap {at[0]} {at[1]}")

    def dump(self) -> str:
        out = self.run("exec-out", "uiautomator", "dump", "/dev/tty", check=False)
        return out[: out.rfind(">") + 1]


def type_code(phone: Phone, code: str) -> str | None:
    """Types `code` into Android's pairing dialog and confirms it; why not,
    if it could not."""
    deadline = time.monotonic() + PAIRING_S
    while True:
        field, _ = dialog(phone.dump())
        if field:
            break
        if time.monotonic() > deadline:
            return "no pairing dialog on the phone"
        time.sleep(1)
    phone.tap(field)
    phone.shell(f"input text {code}")
    _, button = dialog(phone.dump())
    if button:
        phone.tap(button)
    else:
        phone.shell("input keyevent KEYCODE_ENTER")
    return None


def code_from(log: Path, offset: int) -> str | None:
    """The first code in `log` after `offset`, waiting for it."""
    deadline = time.monotonic() + PAIRING_S
    with open(log, errors="replace") as f:
        f.seek(offset)
        while time.monotonic() < deadline:
            line = f.readline()
            if not line:
                time.sleep(0.2)
                continue
            code = passkey(line)
            if code is not None:
                return code
    return None


def size(p: Path | None) -> int:
    return p.stat().st_size if p and p.exists() else 0


def wait_kept(log: Path, offset: int, msg_id: str) -> bool:
    deadline = time.monotonic() + DELIVERY_S
    with open(log, errors="replace") as f:
        f.seek(offset)
        while time.monotonic() < deadline:
            line = f.readline()
            if not line:
                time.sleep(0.5)
            elif kept(line, msg_id):
                return True
    return False


# ---- the run ---------------------------------------------------------------


def build(a: argparse.Namespace) -> None:
    defines = {
        "BARK_TEST_NODE": a.node,
        "BARK_TEST_TO": a.to,
        "BARK_TEST_PAIR": a.pair,
        "BARK_TEST_RECEIVE_S": str(a.receive_s),
    }
    cmd = ["flutter", "build", "apk", "--debug", "-t", TARGET] + [
        f"--dart-define={k}={v}" for k, v in defines.items()
    ]
    print("building:", " ".join(cmd), flush=True)
    if subprocess.run(cmd, cwd=APP).returncode != 0:
        sys.exit(2)


def prepare(phone: Phone, a: argparse.Namespace) -> None:
    out = phone.run("install", "-r", "-t", str(APK), check=False)
    if "Success" not in out:
        # Never uninstalled: that would delete the person's identity.
        sys.exit(
            f"installing failed: {out.strip()}\nAn app signed with another key is "
            "installed (a release build?). Back up its words, uninstall it by hand, "
            "and run again."
        )
    for p in PERMISSIONS:
        phone.shell(f"pm grant {PACKAGE} {p}", check=False)
    if a.image:
        with open(a.image, "rb") as f:
            phone.run(
                "exec-in", "run-as", PACKAGE, "sh", "-c",
                f"mkdir -p files && cat > {IMAGE}", stdin=f,
            )
    else:
        phone.shell(f"run-as {PACKAGE} rm -f {IMAGE}", check=False)
    phone.shell("input keyevent KEYCODE_WAKEUP", check=False)
    phone.shell("wm dismiss-keyguard", check=False)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--node", required=True, help="the node the phone pairs with")
    ap.add_argument("--to", required=True, help="the node its text goes to")
    ap.add_argument("--node-log", type=Path, help="--node's log, for the code")
    ap.add_argument("--to-log", type=Path, help="--to's log, for the text's arrival")
    ap.add_argument("--image", type=Path, help="a signed image newer than --node's")
    ap.add_argument("--pair", choices=["any", "bond", "fresh"], default="any")
    ap.add_argument("--send", help="a board's command to send {payload} to {dst}")
    ap.add_argument("--serial", help="the phone, if adb sees more than one")
    ap.add_argument("--receive-s", type=int, default=300)
    ap.add_argument("--timeout-s", type=int, default=45 * 60)
    a = ap.parse_args()
    a.node, a.to = a.node.lower(), a.to.lower()
    for p in (a.node_log, a.to_log, a.image):
        if p and not p.exists():
            sys.exit(f"{p}: no such file")

    phone = Phone(a.serial)
    build(a)
    prepare(phone, a)
    since = phone.shell("date '+%m-%d %H:%M:%S.000'").strip()
    log = subprocess.Popen(
        phone.base + ["logcat", "-v", "raw", "-T", since, "-s", "flutter:I"],
        stdout=subprocess.PIPE, text=True, errors="replace",
    )
    lines: queue.Queue[str | None] = queue.Queue()

    def read() -> None:
        for line in log.stdout:
            lines.put(line.rstrip("\n"))
        lines.put(None)

    threading.Thread(target=read, daemon=True).start()
    phone.shell(f"am force-stop {PACKAGE}", check=False)
    phone.shell(f"am start -n {PACKAGE}/.MainActivity")

    results: dict[str, str] = {}
    other: list[str] = []
    found_at = 0
    lock = threading.Lock()

    def say(word: str, n: str, text: str, key: str | None = None) -> None:
        with lock:
            results[key or n] = word
            print(f"{word.upper()} {n} {text}", flush=True)

    def pair(node: str) -> None:
        if a.node_log is None:
            print(f"type the code on {node}'s screen into the phone", flush=True)
            return
        code = code_from(a.node_log, found_at)
        if code is None:
            print(f"no code in {a.node_log} within {PAIRING_S} s", flush=True)
            return
        if code == "":
            print("the node is a protected build: type the code on its screen", flush=True)
            return
        why = type_code(phone, code)
        print(f"code {code}: {why or 'typed'}", flush=True)

    def send(f: dict[str, str]) -> None:
        if not a.send:
            print(f"have a board send to {f['dst']}: {f['payload']}", flush=True)
            return
        words = {**f, "dst_id": f["dst"][-12:], "node": a.node, "to": a.to}
        cmd = [w.format(**words) for w in shlex.split(a.send)]
        r = subprocess.run(cmd, capture_output=True, text=True)
        print(f"sent by {cmd[0]}: exit {r.returncode} {r.stdout.strip()} "
              f"{r.stderr.strip()}".rstrip(), flush=True)

    def delivered(msg_id: str, offset: int) -> None:
        if wait_kept(a.to_log, offset, msg_id):
            say("pass", "3", f"delivered: {a.to} kept {msg_id}", key="3 delivered")
        else:
            say("fail", "3", f"delivered: {a.to} did not keep {msg_id} in "
                f"{DELIVERY_S} s", key="3 delivered")

    deadline = time.monotonic() + a.timeout_s
    ended = False
    arrivals: list[threading.Thread] = []
    to_offset = size(a.to_log)
    try:
        while not ended:
            try:
                line = lines.get(timeout=max(0.0, deadline - time.monotonic()))
            except queue.Empty:
                break
            if line is None:
                break
            p = parse(line)
            if p is None:
                other.append(line)
                del other[:-40]
                # The test's own reporter, when it ends without its last line.
                ended = "Some tests failed" in line or "All tests passed" in line
                continue
            word, rest = p
            if word in ("pass", "fail", "skip"):
                n, _, text = rest.partition(" ")
                if word == "pass" and n == "1":
                    found_at = size(a.node_log)
                say(word, n, text)
            elif word == "pairing":
                threading.Thread(target=pair, args=(rest,), daemon=True).start()
            elif word == "sent" and a.to_log:
                t = threading.Thread(
                    target=delivered, args=(fields(rest)["id"], to_offset), daemon=True
                )
                t.start()
                arrivals.append(t)
            elif word == "send":
                threading.Thread(target=send, args=(fields(rest),), daemon=True).start()
            elif word == "done":
                ended = True
        for t in arrivals:
            t.join(DELIVERY_S)
    finally:
        log.terminate()
        phone.shell(f"am force-stop {PACKAGE}", check=False)
        # The boot receiver would start the test at the phone's next start.
        print("the phone runs the test's build now: build and install the app "
              "again (flutter build apk --debug, adb install -r)", flush=True)

    if not ended:
        print("the test did not end; its last lines:", *other, sep="\n", flush=True)
        sys.exit(2)
    words = set(results.values())
    if "fail" in words:
        sys.exit(1)
    if "1" not in results:
        sys.exit(2)
    skipped = [n for n, w in results.items() if w == "skip"]
    if skipped and (skipped != ["5"] or a.image):
        sys.exit(1)


if __name__ == "__main__":
    main()
