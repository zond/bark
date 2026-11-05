# The phone link

**Date:** 2026-11-05. **Status:** the contract is written:
spec "Phone link", roadmap D117 to D122, and its codecs in `bark-link`;
the firmware serves it (`bark-fw/src/ble.rs`), not yet run on a board
(below, "On the bench"). Spec references are
to `protocol.md`; section numbers with "Vol" are the Bluetooth Core
Specification 5.0's.

Before this note the spec's "Phone link" was one paragraph, and open
item O20 asked what the app is told when its message is refused. This
note gives the reasons and the alternatives; the spec holds the rules,
and an app can be built from the spec and `bark-link` alone.

## What the link has to carry

From the core's application interface (`core-interface.md` §4):

| core | link |
|---|---|
| `accepts_send` | Status, ready flag |
| `AppCommand::Send { dst_pos, dst_id, payload }` | Send |
| `AppEvent::GivenBack { msg_id, retry_at }` | Given back, the wait in seconds left |
| `AppEvent::Message { msg_id, payload }` | Inbox |
| `AppEvent::Boot { relays, ends }` | Status: first rotation flag, relays, seconds left |
| `AppCommand::SetPosition`, `ClearMapPoint` | Map point |
| `Node::sleepy`, `carried`, `anchorless`, `address`, `placed`, the map point | Status flags and address |
| the power the firmware reads, the battery it reads | Status: external power flag, battery |
| `Config::node_id`, the image version | Node |

`AppCommand::Fix`, `Power` and `Battery` come from the board, not the
phone; there is no sleepy or carried setting (roadmap D71).

## Decisions and alternatives

### The phone is the queue (D117, O20)

The node holds one message of its application's at a time, until its
first hop takes it, and gives back what its gate, or a sleepy node's
anchor, refuses, with the wait the refusal asks for
(spec "Receiving DATA", "A sleepy node's own message"; D54, D115). So the link needs
only: when does the node take a message, did it take this one, and
which came back with what wait. The app keeps everything else.

- **Ready.** Status bit 0 is `accepts_send` (and, in the firmware, an
  empty hand-over slot). It is notified when it changes, so an app
  waiting to send wakes when the node will take its message.
- **The write is answered when the node has the message.** The ATT
  write response can wait: the transaction timeout is 30 s (Vol 3
  Part F §3.3.3), and the driver loop offers a message within one of
  its steps, at worst after a full frame's airtime, 0.5 s. So success
  means the core took it, and a not-ready error, 0x80, the first of the
  application error codes (Vol 3 Part F §3.4.1.1), means it did not:
  `accepts_send` can turn false between the status the app read and
  the write, a ban or the bucket, and then the app simply waits for
  ready again. No message sits in a buffer between the link and the
  core.
- **Given back stays readable.** Taken, a message can still come back:
  at once, if the gate refuses it on offer, or, a sleepy node's, when
  its anchor refuses it. The phone may be gone by then. Since the node
  takes nothing else until the wait is over, one value suffices, and
  it stays until the node takes the next message. Its wait is seconds
  left, rounded up, so an app that waits them is never early; the
  phone and the node share no clock (spec "Position"), so an
  absolute time would mean nothing to the app. 16 bits of seconds hold
  18 h; the longest wait a refusal tells is 89 min (spec "Bans").
- **Matching by ID.** The app computes each sent message's ID, FNV-1a
  over `dst_pos || dst_id || payload` (spec "Packet formats"), as the
  node does; `bark-link`'s test `vectors` gives one worked example.

*Alternative: the node queues several of the app's messages.* That is
what D29 and D54 rule out: a busy network makes its users wait, at the
origin, where the person sees the wait, rather than letting messages
age in a node.

*Alternative: given back as the write's error.* Only the gate's refusal
is known while the write is open; the anchor's comes later. One path
for both is simpler for the app.

### The inbox (D118, D122)

A message for the app is announced by the core once, as
`AppEvent::Message`, and the node has already ACKed it to the radio's
sender. If the phone is not connected the node must keep it.

- **Read and confirmed, not notified.** A notification is unconfirmed;
  an indication is confirmed by the phone's Bluetooth stack (Vol 3
  Part F §3.4.7.2), which on both phone systems happens whether or not
  the app has kept the value. Writing the ID back is the app's own
  acknowledgement, after it has stored the message. Reading also works
  at any MTU by long reads, where a 231-byte notification would not.
  The cost is one read and one write per message instead of one
  indication, at 23 a day nothing.
- **Status's inbox count is the signal.** Status is notified at every
  change, so the app needs one subscription for everything the node
  says of itself.
- **23 places.** The texts a person receives in a day at the design
  rate, 23 (spec "Capacity and sizing"), for a phone that connects
  once a day: `INBOX` in `tables/delivery.rs`, the delivery queue's
  formula for one node over a day.
- **In the core, by the delivery queue's rule** (D122, decided by the
  user). The simulator and the firmware share the core, so the node's
  messages are kept where both run the same code; and a full inbox
  needs no rule of its own: a stale message makes way for a new one,
  and while every one is fresh the node refuses the next with a wait
  and its sender holds it, so nothing taken is lost for want of room.
  Each place is a delivery-queue entry, 264 B, 6 088 B in all. The
  simulator's driver and the tests' confirm each message at once, so
  their runs do not change: S4 seed 1 at the design rate, release, is
  main's to the byte, the JSON, the CSV and the message log, and the
  text output but for its wall time.
- **A reboot loses the inbox**, as it loses the message the node holds
  for its app. Keeping it in flash would cost a page and wear (spec
  "Implementation notes"); not done for the prototype.

### The MTU (D119)

The longest value is a Send of 7 + 6 + 227 = 240 bytes. An ATT write
request carries MTU − 3 bytes of value (Vol 3 Part F §3.4.5.1), so at
the default MTU, 23 (§3.2.8), it fits 20.

- **MTU exchange up to 247.** 247 is the longest link-layer payload
  with the data length extension, 251 bytes (Vol 6 Part B §2.4), less
  the 4-byte L2CAP header (Vol 3 Part A §3.1): one ATT PDU, one
  link-layer packet, one connection event. Android apps must ask
  (`requestMtu(247)`); iOS chooses the MTU itself and the app cannot
  raise it.
- **Long writes and long reads as well.** Prepare and Execute Write
  (Vol 3 Part F §3.4.6) and Read Blob (§3.4.4.5) work at any MTU, and
  both phone systems use them on their own when a value is longer than
  one PDU. So an iPhone that settles on a smaller MTU still sends and
  reads whole messages. TrouBLE 0.8 serves both (its `PrepareWrite`,
  `ExecuteWrite` and `ReadBlob` requests).
- **Notifications have no long form**: only the first MTU − 3 bytes are
  sent (§3.4.7.1). Status, 14 bytes, and Given back, 6, are notified,
  and fit the 20 bytes of the default MTU.

*Long writes alone* would work for Send but not for a notified message,
and cost three requests a message; *the MTU alone* fails on a phone
that will not raise it.

### Advertising (D120)

- **Advertisement**: Flags (LE General Discoverable, no BR/EDR) and the
  Complete List of 128-bit Service UUIDs, 3 + 18 bytes of the 31 a
  legacy advertisement holds (Vol 6 Part B §2.3.1). The app scans for
  the service UUID.
- **Scan response**: the Complete Local Name, `bark-` and the ID in
  hex, 17 bytes, which does not fit beside the UUID. The display shows
  the same twelve digits at boot (`bark-fw/src/main.rs`), so a person
  picks their node from a list by reading it.
- **Interval 1022.5 ms**: one of the intervals Apple's accessory
  guidelines and Technical Q&A QA1931 list for an accessory an iPhone
  should find quickly, the one nearest a second. Its cost, estimated
  from the nRF52840 Product Specification's radio currents: an
  advertising event sends a 37-byte packet on each of three channels,
  296 µs each at 1 Mbps, and listens about 200 µs after each for a scan
  or connect request, with the ramp-ups about 2 ms of the radio at
  4.6 to 4.8 mA, 10 µC, plus the crystal's start, about 11 µA averaged.
  A sleepy node's duty-cycled receive averages 129 µA (spec
  "Duty-cycled receive and the long preamble"), so advertising adds
  near a tenth. Measured on the bench with the board's current, step 6.
- **One connection**, no advertising while connected. Two phones would
  share one inbox and one message slot, and the second would empty the
  first's inbox.

Connection parameters are the node's to ask for and are not part of the
contract. The firmware does not ask yet, and takes what the phone
chooses; a sleepy node would ask, by an L2CAP connection parameter
update (Vol 3 Part A §4.20), for an interval and peripheral latency that
skip most connection events while nothing is to be said, within the
ranges iOS accepts (QA1931), once the bench has measured what a
connection costs.

### Security (D121, O23)

Open for the prototype: no pairing, no bonding.

- What the link carries: payloads, which are the app's ciphertext (O1);
  destinations, which every DATA frame carries in the clear on 433 MHz;
  the node's ID, position and flags, which its heartbeats carry; its
  battery and inbox count. A listener on the Bluetooth link learns
  little more than one on the 433 MHz air.
- What an open link allows: anyone within Bluetooth range, while the
  owner's phone is not connected, can connect and empty the inbox,
  move the node by a map point, or send as it (the network carries no
  sender in any case, spec "Addresses").
- The fix for O23 is LE Secure Connections with Passkey Entry (Vol 3
  Part H §2.3.5.6.3), the node showing the six digits on its display
  and the person typing them, bonded to one phone so that the node
  then takes connections from that phone only. A characteristic that
  requires encryption answers 0x05 or 0x0F, and both phone systems then
  start pairing on their own, so an app built for the open link keeps
  working. Not now: the bench and the first app gain nothing from it,
  and a forgotten bond on a bench board costs a reflash.

### Versions (D117)

Node's first byte is the link version, 1. Read values grow at their
end, as a heartbeat may (spec "Packet formats"): `bark-link`'s decoders
of Node, Status and Given back ignore what follows the fields they
know. Written values are taken only whole, and change only with the
version; the app reads Node first and writes the form the node speaks.
A change to what an existing field means takes a new service UUID: an
old app finds no node and asks for an update, rather than misreading
one.

## What the app must do

1. Scan for the service UUID; list each node by the name in its scan
   response.
2. Connect; on Android, ask for an MTU of 247.
3. Read Node; if its version is above the app's, tell the person the
   node is newer, and use only what the app knows.
4. Subscribe to Status and Given back (the subscriptions are not kept
   across connections: there is no bond), then read both.
5. While Status's inbox count is above 0: read Inbox, store the
   message, write its ID to Inbox.
6. To send: wait for ready, write Send; on 0x80 wait for ready again.
   Keep the message until it is answered; then keep its ID, in case it
   is given back.
7. On a Given back value: find the message by ID, show the wait, and
   offer it again once ready, or give it up.
8. To set the node's position by hand: write 1 and the position to Map
   point; to clear it, write 0.
9. Make every payload unique (spec "Phone link").

## Does it fit

The node image with the service wired, release build, `size -A`, beside
the image before the phone link and the one with only the stack linked
(trouble-host 0.8.0 with long writes, nrf-sdc 0.4.0 peripheral-only,
nrf-mpsl 0.4.0, pinned, roadmap D5; one connection, three link-layer
buffers each way of 251 bytes). The first three are built without a
place; the last with `BARK_LAT=50.4501 BARK_LON=30.5234`, as the bench
builds it:

| | before | the stack | the service and the inbox | with a place |
|---|---|---|---|---|
| `.text` | 183 676 | 293 172 | 302 204 | 302 172 |
| `.rodata` | 19 464 | 36 528 | 39 644 | 39 572 |
| `.data` | 2 120 | 6 288 | 6 936 | 6 936 |
| `.bss` | 192 860 | 204 136 | 210 568 | 210 568 |
| flash (with the 256-byte vector table) | 205 516 | 336 244 | 349 040 | 348 936 |
| static RAM | 194 980 | 210 424 | 217 504 | 217 504 |

**Flash.** The image slot is 88 pages, 360 448 B (`bark-image/src/layout.rs`):
349 040 B is 97 %, 11 408 B, 11.1 KiB, left. Of that, the log lines
at info and above of the host and of the link's connection steps, which
the bench reads (below), are about 11 KB;
every crate's debug and trace lines are left out of the build
(`log`'s `release_max_level_info`), as the log's filter drops them
anyway. Of the stack's growth, the
controller and the MPSL's library are about 48 KB of code and the host
about 40 KB. The node image formats and parses no floating-point
numbers: `build.rs` converts the built place to the position's counts
with the core's conversion, and positions are logged in integer
millionths of a degree (`Position`'s `Display`). Parsing the place at
run time had pulled in the code to parse and print floats, about 13 KB,
and the image built with a place did not link. `opt-level = "z"` for the
BLE crates alone, by per-package profile overrides, is the first room if
a later change does not fit.

**RAM.** The application's RAM is 261 120 B; static memory takes
217 504 B, the relay with its inbox 187 608 of it, which leaves 43 616 B,
42.6 KiB, for the call stack. The BLE stack's share: the controller's
memory 3 104 B (by `sdc.h`'s formulas; the controller's own count is
logged at boot), the host's packet pool 4 036 B and resources 2 016 B,
the libraries' own state, and the main task's future, which holds the
host. The call stack's depth is measured on the board: the 10 s log line
gives `stack_used`, the high-water mark of a stack painted at boot
(`bark-fw/src/stack.rs`); the estimate was 16 KiB
(spec "Implementation notes").

## The firmware's side

What setting the stack up changed:

- The 32.768 kHz clock, on which the controller keeps a connection's
  timing, runs from the crystal if the board has one: the node tries it
  at boot for 1 s, four times its typical start-up (Product
  Specification, tSTART,LFXO), and otherwise falls back to the internal
  RC oscillator (`lfclk.rs`). The MPSL is told the crystal is good to
  ±250 ppm, the tolerance the Product Specification requires of one for
  a BLE stack, since the fitted crystal's own is not known; the RC
  oscillator it calibrates every 4 s, which holds it to ±500 ppm. The
  MPSL offers no report of a calibration to log.
- The MPSL and the controller take RTC0, TIMER0, TEMP and PPI channels
  17 to 31, and run RADIO, RTC0 and TIMER0 at interrupt priority 0.
  Every interrupt of ours in the node build, the embassy clock's and
  GPIOTE's included, runs at 2, below it.
- The die temperature for the radio's oscillator schedule comes from the
  MPSL in the node build (`radio::Thermometer`); the console still reads
  the sensor.
- The 32 MHz crystal is asked of the MPSL while on USB, not started and
  stopped by hand: the MPSL owns the clock.
- The critical section is nrf-mpsl's in both builds, masking every
  interrupt but the MPSL's three; cortex-m's single-core one is dropped.
- The core's seed is drawn from the RNG before the controller takes it.
- Flash: the stored state is written through the MPSL's flash timeslots
  (`store.rs`), after the step that asked, since an erase halts the CPU
  for 85 ms, which the controller's radio timing could not wait for. The
  update's confirmation still writes through the NVMC directly
  (`bark-image`'s `confirm` is blocking); it runs once at boot, and the
  controller starts only after it (`ble::START`).

How the service meets the node (`ble.rs`, `node.rs`):

- The attribute server answers reads from the values it holds, for every
  kind of read: by handle, by type (as `gatttool --char-read --uuid`
  reads), several at once. So the link puts Status, Inbox and Given back
  into it before it answers each request, but for the later parts of a
  long read, and when the node signals a change.
- Advertising runs from boot and again as soon as a connection ends.
  Its interval is given to the host in ticks of the embassy clock,
  1/32 768 s, which the host truncates to the 0.625 ms units: 1 022 500
  µs would come out 1635 units, 1021.875 ms, so the firmware gives the
  ticks just over 1636 units, and the build checks it.
- For the bench the log has every step of a connection: `ble:
  connected` with the peer and the interval, latency and supervision
  timeout; each parameter, PHY or data length update; and each ATT
  request, a write by its length; with the host's own info, warnings
  and errors (`[host] …`). A central that never gets as far as
  `ble: connected` did not connect at the link layer, which the
  controller does without the host.
- After every step of the driver the node publishes the status and a
  copy of the inbox's oldest message to the link. The status is notified
  when its flags, its relays, the inbox's count or the address change,
  and when the battery has moved by 6 mV since it was last told: 1 % of
  a cell's charge between 20 % and full, (4177 − 3741) mV / 80, rounded
  up, from the open-circuit voltages the core's levels use. The first
  rotation's seconds left are worked out at each read, not notified.
- A write to Send, Inbox or Map point is a request: the link hands it to
  the node, which takes it in its next step (`bark_driver::Request`, or
  the driver's offer for a message), and answers the write only after
  that step has published what it changed, so that what the phone reads
  next shows its write. A Send the core does not take now is answered
  0x80; a confirmation of another ID than the oldest, 0x13. The link
  waits for the answer at most 15 s, half the ATT transaction's 30 s.
- A message given back sets Given back and notifies it; the next
  message taken clears it.

## On the bench

With nRF Connect for Mobile (Nordic Semiconductor), on Android, which
lets the MTU be asked for; iOS works the same but for step 3. A node
image built with `BARK_LAT` and `BARK_LON` relays on USB with a place.
Each step says what the phone and the node's log must show; the node's
log is the USB serial port (`bark-fw/README.md`). A new image needs no
button: `stty -F /dev/ttyACM1 1200`, on the board's port, resets it
into its bootloader's `TECHOBOOT` drive, which takes the UF2
(`bark-fw/README.md`, Flash).

1. **Boot.** The log shows the 32.768 kHz clock's source:
   `lfclk: crystal, started in N ms`, or the warning
   `lfclk: no crystal started; RC oscillator, calibrated`. Then
   `ble: controller memory N of 3104 bytes`,
   N at most 3104, `ble: advertising`, `ble: status flags=0x22 …` with
   the built address in degrees (bits 1 and 5, USB and placed; bit 6
   while the first rotation runs), `display: shown`, and no `ble:` error.
   The 10 s line ends in `stack_used=`, which is the number to note.
   A host that opens the port seconds after the board starts misses
   these lines; the first 10 s report repeats them in one line, `boot:
   lfclk=… display=… controller_memory=N of 3104 image_version=…
   image=N of 360448 B`.
2. **Scan.** The scanner lists `bark-` and the twelve hex digits on the
   node's display, advertising about once a second (nRF Connect shows
   the interval), with the 128-bit service UUID
   `28bc0001-a1b8-4893-9537-2ee3559d7644` among its services.
3. **Connect.** The log shows `ble: connected`, and the node is gone
   from the scanner. Ask for an MTU of 247 (the menu's "Request MTU");
   247 is granted.
4. **Node.** Read `28bc0002-…`: 11 bytes, `01`, the ID's six bytes as
   on the display, then the image version, four bytes big-endian: 0
   unless the image was built with `BARK_IMAGE_VERSION`, which a bench
   build need not set (`bark-fw/README.md`, Updates).
5. **Status.** Enable notifications on `28bc0003-…` and read it: 14
   bytes. On USB with a built place, byte 0 has bits 1 (USB) and 5
   (placed), and bit 0 (ready) once the node takes messages; bit 6 and
   bytes 2–3 count down the first rotation, 195 s, at each read; bytes
   4–10 are the position's seven wire bytes; byte 13 is 0. A second
   board switched on nearby raises byte 1 within its first rotation's
   burst, about 15 s, with a notification.
6. **Send.** Write to `28bc0004-…` the 13 bytes of the second board's
   address, its position's wire bytes and its ID, then a few payload
   bytes. The write succeeds, the log shows `ble: send id=… taken`, and
   the second board's log shows `message id=…` with the same ID; the ID
   is FNV-1a over the 13 bytes and the payload (`bark-link`'s test
   `vectors` works one example). Written twice in quick succession, the
   second may fail with 0x80 while the node still holds the first.
7. **Receive.** Connect to the second board the same way and enable
   notifications on its Status: byte 13 is 1 and was notified. Read
   `28bc0005-…`: the message's ID, four bytes, then its payload. Write
   the four ID bytes back: it succeeds, the log shows
   `ble: confirm id=… ok=true`, Status's byte 13 drops to 0 and a read
   of Inbox is empty. Writing the ID again fails with 0x13.
8. **Map point.** Write `01` and seven position bytes to `28bc0007-…`:
   the log shows `ble: map point` with the degrees, Status's bit 4 is
   set and bytes 4–10 are the new position. Write `00`: `ble: map point
   cleared`, bit 4 clear. A write of `02` fails with 0x13, one of 3 bytes
   with 0x0D.
9. **Long writes.** With the MTU left at 23 (reconnect without asking),
   write a Send of 240 bytes: nRF Connect sends it as a long write, and
   it is taken as in step 6.
10. **Disconnect.** The log shows `ble: disconnected`, then
    `ble: advertising`, and the node is back in the scanner within a
    second or two.
11. **Given back.** On a node whose every neighbour has refused it, a
    Send is given back: Given back (`28bc0006-…`), notified, holds the
    message's ID and the seconds of the wait, and Status's ready bit
    stays clear until the wait is over. This needs the second board to
    refuse, which the bench can arrange by loading it past its cap; it
    may be easier to see in the second board's log first.

## Unsure

- Whether the T-Echo has the 32.768 kHz crystal: the boot log says. On
  the RC oscillator the node's clock is good to ±500 ppm, ten times the
  ±50 ppm spec "Heartbeat" assumes of a node's clock.

- The advertising and connection currents are estimates; the bench
  measures them.
- iOS's chosen MTU varies by model and version; long writes and reads
  cover it, untested here.
- The update's confirmation writes the flash directly with the MPSL
  running and the controller not yet started; nothing radio-timed is
  scheduled then, but whether the MPSL's own timers tolerate the halt is
  for the bench (an image staged and confirmed, step "A swap" of
  `docs/roadmap.md`'s bench list).
- The call stack's depth beside 42.6 KiB, which step 1 measures.
- Whether a USB cable plugged in after a boot on battery now enumerates,
  the crystal being asked of the MPSL at once.
