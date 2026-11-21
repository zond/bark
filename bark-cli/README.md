# bark-cli

`bark` is a phone for a bark node on the command line. It speaks the phone
link (`docs/protocol.md`, "Phone link") as the app does, over a Linux
computer's Bluetooth, so that a person or a script can use a node without
a phone. The node needs no other firmware.

```
cargo build -p bark-cli
target/debug/bark scan
target/debug/bark node 03ef88
target/debug/bark status 03ef88
```

## Commands

Every command that names a node takes its ID, the twelve hex digits its
screen and its Bluetooth name show, or the last few of them, as long as
only one node in reach ends so. With a whole ID the tool connects as soon
as it hears the node; with a part it listens the whole scan first.

- `scan`: the nodes in reach, strongest first, one a line: ID, RSSI in
  dBm, public or private, and the Bluetooth address.
- `node <id>`: the node's Node value: link version, the oldest it serves,
  its ID, its firmware image's version, and `room`, how many more phones
  may pair with it.
- `status <id>`: the node's Status value: the flags that are set, by
  name, its address's position or `none`, battery in millivolts (0 on
  USB), the relays it knows, the neighbours it hears, the seconds left of
  its first rotation, the image it installs, the messages waiting for
  this phone, and the count that moves when its phones change.

Until it pairs, the tool is a guest, and a guest uses only a public node
(protocol.md, "Public nodes"): it refuses a private one before it
connects.

Output is one fact a line, a name and then its value. A command that
fails exits non-zero and prints one line on stderr saying why.

## Options

- `-v`: log every D-Bus call to BlueZ, every change BlueZ reports, and
  each error BlueZ answers, verbatim, to stderr.
- `--adapter hci1`: the Bluetooth adapter; else the first.
- `--scan-time 4`: seconds to listen for nodes; by default ten of a
  node's advertising intervals, 10.2 s.

## How it is built

`src/link.rs` is what the tool needs of Bluetooth: scan, connect, read
and write a characteristic. `src/phone.rs` is what the tool does above
it, the app's steps, and is tested against a fake node (`src/fake.rs`)
built on `bark-link`'s codecs, as the firmware is. `src/bluez.rs` is the
transport: BlueZ, over the system D-Bus, with a D-Bus library written in
Rust, so the build needs no system libraries.

BlueZ chooses how to reach a device itself. A node has a random static
address and no BR/EDR, as its advertisement's flags say, so the tool
scans for LE alone, by the service's and the public node's UUIDs, and
connects through the device BlueZ made from that scan. Should BlueZ still
try BR/EDR, its error names it (`br-connection-...`), and the tool asks
the adapter to connect to the address as a random one instead, which
BlueZ offers only with its experimental interfaces on (`bluetoothd -E`).
