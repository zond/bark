# bark-cli

`bark` is a phone for a bark node on the command line. It speaks the phone
link (`docs/protocol.md`, "Phone link") as the app does, over a Linux
computer's Bluetooth, so that a person or a script can use a node without
a phone. The node needs no other firmware.

```
cargo build -p bark-cli
target/debug/bark scan
target/debug/bark status 03ef88
target/debug/bark pair 03ef88 --code 123456
target/debug/bark neighbours 03ef88
```

## Naming a node

A command names a node by its ID, the twelve hex digits its screen and its
Bluetooth name show, or the last few of them, as long as only one node
ends so. A guest's command looks for the node in a scan: a whole ID ends
the scan as the node is heard, a part of one two advertising intervals
after a node that ends so was heard, if no other was. A paired node's
command names it among the paired nodes and connects to it without a
scan.

## A guest's commands

Until it pairs, the tool is a guest, and a guest uses only a public node
(protocol.md, "Public nodes"): it refuses a private one before it
connects.

- `scan`: the nodes in reach, strongest first, one a line: ID, RSSI in
  dBm, public or private, whether this computer is paired with it, and the
  Bluetooth address.
- `node <id>`: the node's Node value: link version, the oldest it serves,
  its ID, its firmware image's version, and `room`, how many more phones
  may pair with it.
- `status <id>`: the node's Status value: the flags that are set, by
  name, its address's position or `none`, battery in millivolts (0 on
  USB), the relays it knows, the neighbours it hears, the seconds left of
  its first rotation, the image it installs, the messages waiting for
  this phone, and the count that moves when its phones change.

## An owner's commands

- `pair <id> [--code N]`: pairs, by the six-digit code the node's screen
  shows when the pairing asks for it; without `--code` the tool asks for
  it then. A bond this computer held with the node goes first, as the app
  pairs only without one.
- `unpair <id>`: the node forgets this computer, and this computer forgets
  the node.
- `neighbours <id>`: the node's noise floor, its own position, and a line
  for each neighbour: ID, position, averaged RSSI in dBm, the link's
  quality (protocol.md, "The neighbour map"), seconds since it was heard,
  and its flags.
- `ask <id> <neighbour>`: asks the neighbour for its status through the
  node, three times at most, as the app does, and prints its answer.
- `clock <id>`: the node's time, its UTC offset and where its time came
  from. Every owner's command gives the node this computer's time first,
  as the app does at every connection.
- `map-point <id> set <lat> <lon>`, `map-point <id> clear`: sets or
  clears the node's map point, then prints its status.
- `public <id> [on|off]`: makes the node public or private, and prints
  what it is.
- `phones <id>`: a line for each record the node keeps: its number, its
  flags (`mine` is this computer's), its tag, the day its phone was last
  linked, and its name.

Output is one fact a line, a name and then its value. A command that
fails exits non-zero and prints one line on stderr saying why.

## Options

- `-v`: log every D-Bus call to BlueZ, each error BlueZ answers,
  verbatim, the nodes it reports, and the pairing agent's steps, to
  stderr. `-vv` logs every other device heard too.
- `--adapter hci1`: the Bluetooth adapter; else the first.
- `--scan-time 30`: seconds to listen for a node at most; by default ten
  of a node's advertising intervals, 10.2 s.
- `--dir <dir>`: where the tool keeps the nodes it is paired with; by
  default `$XDG_DATA_HOME/bark`, else `~/.local/share/bark`. The bonds
  themselves are BlueZ's, one set for the computer's adapter, so two
  directories on one computer are not two phones to a node.

## How it is built

`src/link.rs` is what the tool needs of Bluetooth: scan, connect, pair,
read, write and subscribe. `src/phone.rs` is what the tool does above it,
the app's steps, and is tested against a fake node (`src/fake.rs`) built
on `bark-link`'s codecs, as the firmware is. `src/bluez.rs` is the
transport: BlueZ, over the system D-Bus, with a D-Bus library written in
Rust, so the build needs no system libraries.

A node has a random static address and no BR/EDR, as its advertisement's
flags say, so the tool scans for LE alone, by the service's and the
public node's UUIDs, and connects through the device BlueZ made from that
scan. Should BlueZ still try BR/EDR, its error names it
(`br-connection-...`), and the tool asks the adapter to connect to the
address as a random one instead, which BlueZ offers only with its
experimental interfaces on (`bluetoothd -E`).

Another program may keep a discovery running all the time, and then the
controller reports a node only now and then. So a scan counts as heard a
node whose device BlueZ holds with an RSSI, which BlueZ keeps while any
discovery runs and clears when the last one stops; and the tool leaves
BlueZ's devices in place. While another program discovers, a node that
went out of reach lately may therefore still be listed, at the RSSI it
was last heard at.

BlueZ adds to a device the UUIDs it was ever seen with and the services
found on a link, so a scan judges a node public by the public node's
UUID, which only the advertisement carries. A node made private since
BlueZ last saw it public still looks public; it refuses the guest's
first read, and the tool says it is private now. Whether a paired node
is public, `public <id>` says.

An attempt to connect to a node without a bond is given ten advertising
intervals; one that fails is stopped by removing the device from BlueZ,
the only way BlueZ stops one, and the tool listens for the node again,
up to three scans' time, before the next of three tries. An attempt over
a bond is left to BlueZ to end, as removing the device would remove the
bond.

Pairing is by passkey entry: the node shows the code and the tool types
it. The tool answers BlueZ as its pairing agent, with a keyboard alone,
and refuses every other kind of pairing.
