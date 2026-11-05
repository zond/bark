# The phone link

**Date:** 2026-11-05. **Status:** the contract is written:
spec "Phone link", roadmap D117 to D121, and its codecs in `bark-link`. The firmware's
side is still to build (below, "What comes next"). Spec references are
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

### The inbox (D118, O22)

A message for the app is delivered by the core once, as
`AppEvent::Message`, and the node has already ACKed it to the radio's
sender. If the phone is not connected the firmware must keep it.

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
  once a day. Each place is the ID, a length and the payload,
  4 + 1 + 227 = 232 bytes: 5.2 KiB in all, to be checked against the
  RAM left beside the BLE stack in the firmware's next step.
- **Full.** The core does not know the inbox; today a message for a
  full inbox would be lost after the radio's sender was told it was
  taken. The fix in the spirit of D29 is to refuse it: the node refuses
  DATA for its own application while the inbox is full, as it refuses
  one for a sleepy node whose delivery queue is full, and the sender
  holds it. That needs the core told, an application command or a
  query; it is O22, a protocol decision, not taken here.
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
contract: the firmware will ask, by an L2CAP connection parameter update
(Vol 3 Part A §4.20), for an interval and peripheral latency that let a
sleepy node skip most connection events while nothing is to be said,
within the ranges iOS accepts (QA1931).

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

The node image built with the stack linked: trouble-host 0.8.0 (peripheral,
GATT, long writes), nrf-sdc 0.4.0 with the peripheral-only controller and
nrf-mpsl 0.4.0, pinned (roadmap D5); one connection, three link-layer
buffers each way of 251 bytes, the service's attribute table with room
for every value's longest form, and advertising (`bark-fw/src/ble.rs`).
The service is not wired to the node yet. Release build, `size -A`:

| | before | with the stack | change |
|---|---|---|---|
| `.text` | 183 676 | 293 172 | +109 496 |
| `.rodata` | 19 464 | 36 528 | +17 064 |
| `.data` | 2 120 | 6 288 | +4 168 |
| `.bss` | 192 860 | 204 136 | +11 276 |
| flash (with the 256-byte vector table) | 205 516 | 336 244 | +130 728 |
| static RAM | 194 980 | 210 424 | +15 444 |

**Flash.** The image slot is 88 pages, 360 448 B (`bark-image/src/layout.rs`):
336 244 B is 93 %, 24 204 B, 23.6 KiB, left. Of the growth, the
controller and the MPSL's library are about 48 KB of code and the host
about 40 KB. The wiring of stage 3 and the inbox in the core come out of
that 23.6 KiB. If they do not fit: `opt-level = "z"` makes this image
29 KB smaller (measured, 306 812 B at the stack's first build), at a
speed of the core's `handle` not measured; the 148 KiB of the
SoftDevice's slot, kept and never enabled (`bark-fw/README.md`), and the
92 KiB of update staging, unused until updates have a transport, are
the layout's room, a change to the layout.

**RAM.** The application's RAM is 261 120 B; static memory takes
210 424 B, the relay 181 512 of it, which leaves 50 696 B, 49.5 KiB. The
BLE stack's share: the controller's memory 3 104 B (by `sdc.h`'s
formulas; the controller's own count is logged at boot), the host's
packet pool 4 036 B and resources 2 016 B, the libraries' own state, and
the main task's future, which holds the host, 13 176 B in all. The core's
inbox, 23 places of a delivery-queue entry's 264 B, 6 072 B, leaves
44 624 B, 43.6 KiB, for the call stack. Its depth is not measured: the
earlier estimate of the stacks was 16 KiB (spec "Implementation notes").

What setting the stack up changed beside the service:

- The MPSL and the controller take RTC0, TIMER0, TEMP and PPI channels
  17 to 31, and run RADIO, RTC0 and TIMER0 at interrupt priority 0. Every interrupt of ours in the node build, the
  embassy clock's and GPIOTE's included, now runs at 2, below it.
- The die temperature for the radio's oscillator schedule comes from the
  MPSL in the node build (`radio::Thermometer`); the console still reads
  the sensor.
- The 32 MHz crystal is asked of the MPSL while on USB, not started and
  stopped by hand: the MPSL owns the clock.
- The critical section is nrf-mpsl's in both builds, masking every
  interrupt but the MPSL's three; cortex-m's single-core one is dropped.
- The core's seed is drawn from the RNG before the controller takes it.

## What comes next

Stage 3: the inbox in the core (O22), the service wired to the driver
loop, flash writes through the MPSL, and the bench procedure in nRF
Connect, added to this note.

## Unsure

- The advertising and connection currents are estimates; the bench
  measures them.
- iOS's chosen MTU varies by model and version; long writes and reads
  cover it, untested here.
- O22 needs a core decision before the inbox can be full without loss.
- Flash writes while the MPSL runs: the stored state (`store.rs`) and the
  update's confirmation (`update.rs`) write and erase through the NVMC
  directly, which halts the CPU for up to 85 ms an erase; the MPSL's
  radio timing cannot wait that long. Until stage 3 routes them through
  the MPSL's flash timeslots (`nrf_mpsl::Flash`), this image is not for
  the bench.
- The call stack's depth beside 43.6 KiB, which a painted stack's high
  water mark in the 10 s log line would measure.
