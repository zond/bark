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
ends so. A guest's command connects without a scan to a node BlueZ
still holds, heard in the last half minute or so, when it is the only one
held that the ID names; else it looks for the node in a scan: a whole ID
ends the scan as the node is heard, a part of one two advertising
intervals after a node that ends so was heard, if no other was. A paired
node's command names it among the paired nodes and connects to it
without a scan.

While another program scans for Bluetooth devices, connects often fail,
as its scan takes the radio's time; the tool says so, and closing that
program helps.

## A guest's commands

Until it pairs, the tool is a guest, and a guest uses only a public node
(protocol.md, "Public nodes"): it refuses a private one before it
connects. A command on a paired node uses a paired phone's link instead. As
the app does, every link writes this person's recipient tag at the node
first, and a paired one their name too, so the node hands them their mail.

- `scan`: the nodes in reach, strongest first, one a line: ID, RSSI in
  dBm, `public`, `private` or `unknown`, whether this computer is paired
  with it, and the Bluetooth address. `unknown` is a node BlueZ has seen
  advertise both ways, or linked to: see below.
- `node <id>`: the node's Node value: link version, the oldest it serves,
  its ID, its firmware image's version, and `room`, how many more phones
  may pair with it.
- `status <id>`: the node's Status value: the flags that are set, by
  name, its address's position or `none`, battery in millivolts (0 on
  USB), the relays it knows, the neighbours it hears, the seconds left of
  its first rotation, the image it installs, the messages waiting for
  this phone, and `phones_mark`, which is no count: a number that moves,
  wrapping, whenever the node's list of phones changes, so that a phone
  knows to read it again (protocol.md, "The service").

## A paired phone's commands

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
  from. Every command here gives the node this computer's time first,
  as the app does at every connection.
- `map-point <id> set <lat> <lon>`, `map-point <id> clear`: sets or
  clears the node's map point, then prints its status.
- `public <id> [on|off]`: makes the node public or private, and prints
  what it is.
- `phones <id>`: a line for each record the node keeps: its number, its
  flags (`mine` is this computer's, `owner` the node's owner's), its tag,
  the day its person was last seen, and its name. The tool neither removes a
  record nor makes an owner; the app does both, by the code the node's
  screen shows.

## Messages

The tool is a phone for one person, whose identity it makes the first time
it needs one, from a random seed kept in `--dir`.

- `me [--name N]`: this person's identity, name, home, and card, the
  `BARK:` text another person adds to reach them.
- `contact add <card> [--name N]`: adds the person whose card it is, or
  takes their newer card.
- `contacts`: a line for each contact: name, identity, whether a boxed
  message from them has opened here, their home, and the node they were
  last known at.
- `send <id> <contact> <text>`: sends the text through the node, to where
  the app sends one nobody placed: where the contact was last known while
  that is a minute old at most, else their home, as protocol.md, "The
  payload", says. The first message to a contact carries this person's
  card; once one of theirs has opened here, messages go boxed. A text too
  long for one message goes in parts. Each line names a message the node
  took; one it gives back later shows in `inbox`.
- `inbox <id>`: reads every message waiting at the node, opens it, keeps
  what it says of its sender, and confirms it, as a paired phone does; as
  a guest, it reads the node's ring from the last item it took there. A
  line for each: who, when, which part, whether it is verified, and the
  text. A reply from a home's mailbox is listed, not read: the tool does
  not ask its home for mail.
- `home <id> on|off`: makes the paired node this person's home, whose
  mailbox keeps their mail, or no longer, and puts it on their card,
  which is then to be sent again.

## Updating a node

- `update <id> <image>`: updates the paired node's firmware from an image
  file, a release's `bark-fw-<version>.img` or one built and signed here
  (`bark-fw/README.md`, "Updates"), as the app does (protocol.md,
  "Updates from the phone"): the header to begin, the image in pieces,
  seven between reads of what the node holds, so that a lost piece or
  link goes on from there, then the commit and the reboot. An image not
  newer than the node runs is refused before anything is sent. A line at
  each step and at each tenth of the image sent. The node is away about a
  minute as it swaps; `node <id>` shows it back, with its new version.

Output is one fact a line, a name and then its value. A command that
fails exits non-zero and prints one line on stderr saying why.

## Options

- `-v`: log every D-Bus call to BlueZ, each error BlueZ answers,
  verbatim, the nodes it reports, and the pairing agent's steps, to
  stderr. `-vv` logs every other device heard too.
- `--adapter hci1`: the Bluetooth adapter; else the first.
- `--scan-time 30`: seconds to listen for a node at most; by default ten
  of a node's advertising intervals, 10.2 s.
- `--dir <dir>`: where the tool keeps the nodes it is paired with, the
  person's identity and their contacts; by
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
scan, which BlueZ then reaches over LE.

Another program may keep a discovery running all the time, and then the
controller reports a node only now and then. So a scan counts as heard a
node whose device BlueZ holds with an RSSI, which BlueZ keeps while any
discovery runs and clears when the last one stops; and the tool leaves
BlueZ's devices in place. While another program discovers, a node that
went out of reach lately may therefore still be listed, at the RSSI it
was last heard at.

BlueZ adds to a device the UUIDs it was ever seen with and the services
found on a link, so a scan judges a node public by the public node's
UUID, which only the advertisement carries, and the service's alone as
private. A device BlueZ has seen with both, as after a link or a change,
may be either now, and BlueZ reports no UUID seen again, so the scan
says `unknown` rather than an answer that may be stale. A guest still
connects to an `unknown` node; a private one refuses its first read, and
the tool says the node is private now. `public <id>` reads the truth on
a paired node.

An attempt to connect is given 70 s, longer than BlueZ's own bound: 40 s
for the link, then the discovery of the node's table. One BlueZ has not
answered by then is stopped with Disconnect, which keeps the device; the
tool then tries once more on the same device. With `-v` it logs when the
link comes up and when the table is read, in seconds since Connect.

Pairing is by passkey entry: the node shows the code and the tool types
it. The tool answers BlueZ as its pairing agent, with a keyboard alone,
and refuses every other kind of pairing.
