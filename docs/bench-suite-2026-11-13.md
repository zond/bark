# Acceptance and stress tests on four boards

**Status:** design, 2026-11-13. The runner, the standing checks, the
bench build, the phone test and scenarios A1 to A5, S2 and S3 are built.
A1, A2 and A3 have passed on the boards; the others and the phone test
have not run yet. The heartbeat check waits for a line the firmware does
not log, each transmission's channel and airtime. Four T-Echo 433 boards on
USB on one desk and one Android phone on adb. A quoted name after "spec"
is a heading of `docs/protocol.md`.

Every fault found on boards so far was of one of five kinds, and each
test below aims at one or more of them:

- the real radio: channels unequally noisy, the turn-round between two
  frames, radio errors;
- long runs: a stall that shows after hours;
- interruptions: a reboot, a power cut while the loader swaps images, a
  neighbour that goes away;
- a change of role or place: plugged in and unplugged, a new anchor, a
  new home channel;
- the bench's own tools: a log read by two readers, a stale build packed.

## Parts

**The runner** (`bark-fw/scripts/`, Python 3, no dependencies) runs one
**scenario**: a file that names the build each board gets, a timed list
of actions, and the conditions that pass it. Its actions are those
`bench.py` has: flash a board, silence it in its bootloader, bring it
back, type a console command into it, and build it deaf to another
(`BARK_IGNORE`, `bringup-2026-11-06.md`), which makes a line on one desk.
It logs every board through one reader a port, with the host's time on
each line, and refuses to start while another process holds a port. It
gives every build of a run its own image version and checks the version
each board reports after flashing.

**The standing checks** run on every scenario's logs, whatever it tests,
and any one fails the run:

- a reset the scenario did not ask for;
- a radio error, or a message dropped;
- a heartbeat skipped for a busy channel while no other board sent there;
- the stack's free room below its margin (`core-interface.md`, Memory);
- a gap in a board's log longer than its status line's period.

The checks are functions of log lines, tested on the host against
recorded logs.

**The bench build** is the node's firmware with a cargo feature, `bench`,
that no release sets. It adds commands on the serial line and nothing
else: send a text to a node, say what power the node is on, set the
transmit power, send a carrier on a channel, reboot, and give what the
node's own lines do not (`bark-fw/README.md`, "Bench build"). The
feature lives in one module, which the rest of the firmware names where
the serial line's input goes, where the node's application is made and
between two of the driver's steps. A hook that cannot be added that way
is not added, and its test stays pending: pinning the home channel,
which the core takes only as it starts.

**The phone test** is an integration test of the app, run on the phone
over adb against a board: it pairs, sends, receives and gives the node an
image, in a set order, and reports each step.

## Acceptance tests

Run before a release.

| | test | passes when | time |
|---|---|---|---|
| A1 | boot and join | every board holds its three neighbours and a home channel | 5 min |
| A2 | a text along a line A, B, C | delivered and acknowledged each way within the bound of spec "Hand-over" for two hops | 10 min |
| A3 | mail to and from a sleepy node | delivered in the node's window; its own text arrives | 15 min |
| A4 | relay update | two relays install an image given to the third | 3 h |
| A5 | sleepy update | a sleepy node installs the image and is sleepy again on it | 10 h |
| A6 | three sleepy nodes on one anchor | all three install, the last within an hour of the first | 10 h |
| A7 | the phone | pairs, sends, receives, and updates the node it is paired with | 30 min |

## Stress tests

| | test | passes when |
|---|---|---|
| S1 | texts at three times the design rate while an update runs | none dropped; what is refused is refused at its sender with a time to retry; the update completes |
| S2 | the middle relay silenced during a text and during an update, then a sleepy node's anchor silenced | every text arrives or is refused at its sender; the download goes on from the blocks held |
| S3 | every board rebooted at random for hours during an update | the blocks gathered, the stored home channel and the mail held are there after each reboot; the update completes |
| S4 | a carrier on a relay's home channel | the relay moves within the time of spec "Channel moves", its sleepy node follows, and the channel's floor is back within two hours of the carrier's end |
| S5 | boards moved between USB and battery during a download and with mail queued | no mail lost; the download completes |
| S6 | two relays that hold the image on one home channel, one sleepy node | the relay that is not its anchor sends it no piece |
| S7 | mail to a silenced sleepy node until its holder is full | refused at the sender from then on; delivered when the node returns |
| S8 | every board at its lowest power | pending: on one desk every board hears every other with tens of dB to spare; needs attenuators or distance |
| S9 | light traffic for 72 h | every error count as at the start, beats a period apart, each channel's floor within 3 dB of its first hour's |

S5 tests the node's decisions with the power it is told it is on. Its
charger and USB detection are tested only where the hub can switch a
port's power, which is to be found out.

## When

- **Smoke**, 30 min: A1 to A3.
- **Night**: A4, A5 and one stress test in turn.
- **Weekend**: S9.

## Running a scenario

The scenarios are TOML files in `bark-fw/bench/`, as the simulator's
are; `bark-fw/scripts/scenario.py` describes the format. From `bark-fw/`:

    scripts/bench.py run bench/a1-boot.toml A=f4f5 B=3112 C=c367 D=bac7
    scripts/bench.py check target/bench/runs/a1-boot-<time>

A board the last run left in its bootloader has no serial port to find it
by, and is named by all twelve digits of its ID.

`run` builds every board's image, signs it with the bench key and packs
it with the loader; puts every board in its bootloader, then flashes each
and checks the version it reports; does the actions; and judges the logs.
The first run is given `--first-version N`, above every version a board
has run or gathered; later runs count on from the last.
Everything goes to `target/bench/runs/<scenario>-<time>/`, with the result
in `result.txt`.

`check` judges a directory of logs again, or any other with one log a
board, as `bench.py capture` writes them: `check DIR SCENARIO A=f4f5 ...`
names the boards, and `--stack-room` gives the stack's room where no run
recorded it. What needs the host's clock or the run's actions is reported
unchecked.

A board holds and serves its own image only where its scenario says
`serves`: otherwise every board would take the highest version of the run.
An image carries what its board was built with, its place and the board
it is deaf to, so a board that installs another's image takes those too;
the update scenarios are laid out for it.

`python3 -m unittest`, run in `bark-fw/scripts/`, tests the checks on
pieces of recorded logs, and every scenario file.
