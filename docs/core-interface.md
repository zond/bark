# Core interface

**Status:** live contract, changed only by roadmap decisions (first D3). The types are in
`bark-proto/src/{time,radio,position,ids,event,node}.rs`; this page is the contract
around them that a driver (the simulator, the firmware) must honour and the core may rely on.

## 1. Shape

The core is a state machine behind the `Core` trait:

```rust
fn handle(&mut self, event: Event<'_>, now: Instant, emit: &mut dyn FnMut(Action));
fn next_wakeup(&self) -> Option<Instant>;
fn accepts_send(&self, now: Instant) -> bool;
```

One struct, `Node`, for every node; its power makes it a relay or a sleepy node
(`AppCommand::Power`, §4; spec "Nodes", "Sleepy nodes"). `Relay` is the node with its
heard table and delivery queue, and `SleepyNode` the node without them, which a sleepy
node does not keep: it is sleepy whatever its power, for a driver whose node never
switches, and a `Relay` on battery keeps them empty and behaves the same (§6). `Node::sleepy()` says which it is now. A sleepy node's radio receives
duty-cycled whenever the node is not in a window or listening for its anchor's heartbeat;
when and until when, the core says with `Action::Sniff` (§3 rule 10). `Node::anchorless()`
says whether it is a sleepy node without an anchor (spec "Sleepy nodes alone").

The core never blocks, never sleeps, never allocates, and never reads a clock: `now` is an
argument. Every random choice comes from the `Rng` seeded at construction, so a core's
behaviour is a function of its config, its seed and its event sequence.

## 2. Time

`Instant` and `Duration` are integer microseconds; there is no other time unit to mix
them up with. `now` never decreases across calls on one core. Different cores have unrelated clocks; the protocol has no network time (spec "Position").

Wakeups: after every `handle` the driver reads `next_wakeup()`. When its clock reaches that
instant it calls `handle(Event::Wakeup, now)`. Late is allowed; early is not. The core keeps
all its own deadlines internally and exposes only the earliest, so there is one timer per
core and no timer identifiers cross the boundary.

Boot: the first `handle` call a core receives is `Event::Wakeup`, delivered at start
whatever `next_wakeup()` says. The core's first rotation (spec "Channel at boot") starts
there. The driver then reports the power at once, at the same instant (§4); until it does,
a `Relay` takes itself to be on external power. The driver builds the core with what it
kept of the core's state before the reboot, if anything, in `Config::stored` (§4).

## 3. Radio

The driver owns one half-duplex LoRa radio that is either listening on a channel or away on
an **excursion**: a listen-before-talk (`Action::Lbt`), a signal strength reading
(`Action::Rssi`) or a transmission (`Action::Transmit`, or `Action::Serve` for an
update's piece, §4). `Action::Listen(c)` is not an excursion: it sets the resting channel,
takes effect at once, and the radio listens on `c` whenever no excursion is in progress.

Rules:

1. **One excursion outstanding.** After emitting `Lbt`, `Rssi`, `Transmit` or `Serve` the core
   waits for its completion event (`LbtDone`, `RssiSample`, `TxDone`) before emitting another.
   A driver may treat a violation as a bug and panic.
2. **Every excursion completes with exactly one event**, then the radio is listening on the
   resting channel again. The driver pays the retune, settle, CAD, RSSI and airtime costs and
   is deaf during them; the core learns how long an excursion took by comparing `now` at the
   completion event with `now` when it emitted the action. The thirteen-channel sweep is
   thirteen `Rssi` excursions issued one after the other.
3. **`Received` is a CRC-valid LoRa payload** heard on the resting channel, with the
   packet RSSI and SNR the radio reports. Nothing is received during an excursion.
4. **`Transmit` frames** satisfy `preamble_symbols >= radio::MIN_PREAMBLE_SYMBOLS` (12 at
   SF5, SX1262 datasheet §6.1.4) and `len <= MAX_FRAME`. The driver transmits at
   `Config::tx_power_dbm` on `Frame::channel` with `Frame::preamble_symbols` of preamble,
   then returns to the resting channel. The core asks for the shortest preamble, or for
   `sniff::LONG_PREAMBLE_SYMBOLS` (927, 237.3 ms) on a frame to a node that receives
   duty-cycled (rule 10). The receiver is programmed for the long preamble, the longest a
   frame can have, which the datasheet asks where the length varies (§6.1.4); it still
   receives the short one.
5. **Listen-before-talk is the core's job.** The driver never checks the channel on its own;
   the core issues `Lbt` or `Rssi` and decides. `Lbt(c)` is one channel activity detection
   on `c` with the instantaneous RSSI read while it samples, four symbol periods in
   (`radio::RSSI_SETTLE`), reported together in `LbtDone`: whether a preamble was seen, and
   the reading on the footing of one taken while listening, which the driver puts it on
   (spec "Heartbeat"). From listening to listening it costs the CAD's preparation and
   sampling and the return, about 3 ms (spec "Heartbeat").
6. **Duty cycle is the core's job.** The core keeps the duty-cycle budget (spec "Duty cycle cap") and
   defers; the driver never drops a frame.
7. **`Rssi` on the resting channel is not an excursion.** The radio keeps receiving while it
   reads its own channel, so the core may issue it at any time; it still completes with
   `RssiSample`. `Rssi` on another channel is an excursion: a retune, a wait of
   `radio::RSSI_SETTLE` (four symbol periods, 1.02 ms) after the receiver starts there, the
   read and a retune back, 1.9 ms (spec "Busy estimate").
8. **The driver may hold an `Lbt` or `Rssi` excursion while a reception is in progress**, by at
   most the airtime of a full frame with the long preamble, 372.8 ms, so that a sweep does not
   cut a packet short (spec "Busy estimate").
   `Transmit` is never held. The core's timeouts allow for the hold.
9. **`Action::Trace` carries no obligation.** It reports protocol events (a dropped message
   and why, a deferral, a forward, a return to the parent, a demotion, a channel move) for the simulator's
   metrics and the firmware's log. A driver may ignore it.
10. **`Action::Sniff(until)` lets the radio receive duty-cycled** on the resting channel
    until `until`: the SX1262's SetRxDutyCycle (datasheet §13.1.7) with a receive period of
    `sniff::SNIFF_RX` (3.75 ms, 240 steps of 15.625 µs) and a sleep period of
    `sniff::SNIFF_SLEEP` (233.625 ms, 14 952 steps), or the periods `sniff::periods` gives
    for the oscillator delay the driver probed, which keep the gap between two windows the
    same for every node. The radio then catches every frame
    whose preamble is `sniff::LONG_PREAMBLE_SYMBOLS` long and may miss any other; what it
    catches is `Received` as ever, and after it the driver starts the cycle again. The
    sniff ends at `until`, or sooner at the core's next `Listen`, `Lbt`, `Rssi` or
    `Transmit`, which the driver carries out as ever, the radio then listening on the
    resting channel again; `until` is when the core next needs the radio of its own
    accord. The driver may let a packet already under way end first, by at most the
    airtime of rule 8, since either receive hears it; a packet that outlasts that
    starts the cycle again. A driver that keeps listening breaks nothing: the core handles whatever it
    receives. Every sleepy node asks for it whenever it has nothing under way and is
    neither in a window nor listening for its anchor's heartbeat (spec "Sleepy nodes",
    "Delivery", "Relay choice"); a node that is not sleepy never does.

The modulation is fixed (SF5, 125 kHz, CR 4/5, `radio.rs`) and is not negotiated.

## 4. Application

Every node has the same application interface: the readiness query
`accepts_send`, the inbox's `inbox`, `inbox_len` and `confirm`, the commands `AppCommand::Send`, `AppCommand::SetPosition`,
`AppCommand::ClearMapPoint`, `AppCommand::Fix`, `AppCommand::NoFix`, `AppCommand::Power`,
`AppCommand::ForceRole` and `AppCommand::Battery`, the events
`AppEvent::Message`, `AppEvent::GivenBack` and `AppEvent::Boot`, the store,
`Action::Store` and `Config::stored`, the GPS: the request `Action::Locate` and
the queries `gps_tracks` and `fix_in_reach`, and the update: the commands
`AppCommand::Holds`, `AppCommand::Gather` and `AppCommand::Block` and the actions
`Action::Offered`, `Action::StorePiece` and `Action::Serve`, and a neighbour's status: the
command `AppCommand::Ask` and the event `AppEvent::Status`.

`AppCommand::Send` originates a message to `(dst_pos, dst_id)` with an opaque payload of
at most 227 bytes (§7). The driver offers one only while `accepts_send(now)` is true, and
asks again after every `handle`, so an application waits for its node; a firmware's async
send awaits it. The core reports nothing back about the message but a give-back (below), not even the first hop's
ACK, which a user could read as delivered: from the moment it is taken the node is either
sending it or waiting to. The protocol has no end-to-end acknowledgement (spec "Addresses"), so
whether the message arrived is for the app to find out inside its payload. A message the
node drops is reported only as `Trace::Dropped` (§3 rule 9).

A node holds one message of its application's at a time, until its first hop takes it.
It accepts a message from its boot on while it holds none, its send queue takes a full
frame on top of what it holds (spec "Send queue"), and not every neighbour is banned, or
for a sleepy node with a live anchor, the anchor (spec "Receiving DATA"). When only its
bucket and bans keep it from accepting, its `next_wakeup()` includes the moment both allow
it. A message the node holds waits for bans as spec "Bans" says; one for a
sleepy node it holds for that finds the delivery queue full waits for a
place (spec "Receiving DATA"). A message the node's own gate refuses
when it is offered (spec "Receiving DATA"), or a sleepy node's own message that
its first hop refuses, goes back to the application (spec "A sleepy node's own message"):
the core emits `AppEvent::GivenBack` with the message's ID and `retry_at`, the end of the
wait the refusal asked for, and `Trace::Dropped` with `DropReason::Pushback` for the log;
`accepts_send` stays false until `retry_at`, which `next_wakeup()` includes. The driver
hands the event to the application, which may offer the message again then or give it up. One whose hand-over goes
unanswered waits in the node as in that section.

A `Send` while `accepts_send` is false, or with a longer payload, breaks the contract like
a second outstanding excursion: the core ignores it, and the test and simulator drivers
panic or never offer it.

The firmware's driver too offers the application's message after every `handle`, before
it waits again, so a message that waits goes whatever woke the loop. While it waits it
also waits for its application with `bark_driver::App::ready`, which resolves only for
something new for the driver since it last resolved: a request, a change of power, or,
while the core accepts one, a message. A signal with nothing behind it, such as a power
event that changed nothing, the application takes and keeps waiting; a message that
arrives while the core accepts none it holds for a later offer or refuses. A readiness
that gives the driver nothing breaks this: the driver counts it, does not wait on the
application again until its clock or its radio wakes it, and logs it at most once a
minute. An application that kept saying it is ready and giving nothing would otherwise
keep the loop from ever waiting: on a board that starves every other task of the
processor and drains the battery.

`AppCommand::SetPosition` sets own position from a map point set in the app, which wins
over GPS: the node takes it and is fixed (spec "Position"). `AppCommand::ClearMapPoint`
clears it: the node stays where it is and its next accepted fix is its place (roadmap
D84). `AppCommand::Fix` hands the core a GPS fix that passed the checks of spec
"Position": the satellites' C/N0, which the driver makes from what the receiver reports,
and the distance to the relays the node knows, which it asks of the core with
`fix_in_reach(position)`: within the hearing radius, 3 km (`gps::HEARING_RADIUS_M`), of
one of them, or any fix while the node knows none or has no place. The driver hands each
fix that passes, and none while a map point is set, and the core takes none then. A node decides from the fixes whether it is carried; a carried node, or a
sleepy one without an anchor, takes each fix, a fixed relay keeps its position, and a
sleepy node with an anchor takes none and keeps the flag it had: its position is its
anchor's, which it beats and rebuilds the short positions it hears near, and
`Node::address` gives it (roadmap D70, D72, D91). A node's address changes with its
position, and a relay's sleepy nodes' with it (spec "Node identity").

`AppCommand::NoFix` says the GPS runs but gives no fix the driver hands: the receiver
lost its fix, its satellites failed the C/N0 check, or the fix was beyond reach. The
driver sends it at the change from handing fixes to not, and the time until the next fix
does not count towards a run of far fixes (spec "Position", roadmap D136).

`Config::placed` says whether `Config::position` is the node's place: a board built
with one, or the simulator's nodes. Without it and without a stored place the node has
none (`Node::placed` false): it sends no heartbeat, stores nothing, and takes its first
accepted fix as its place (roadmap D83). Until then the driver checks its fixes against
no neighbour, since the core cannot rebuild their positions.

`Action::Locate(until)` asks the driver to run the GPS until `until` and hand the first
fix that passes its checks as `AppCommand::Fix`, then stop it; a later one moves the end,
and one at or before `now` ends the search at once. A sleepy node asks while it has no
anchor, and ends the search under way when it takes one (spec "Beats", roadmap D80, D91);
a resting relay asks at boot and at its radio's sign of motion (spec "Position", roadmap
D82).

`gps_tracks()` says whether the GPS is to run all the time now: a relay's on external
power, for a fixed relay's grace on battery, while it follows a far fix and while it is
carried, never with a map point; a sleepy node's never, a carried node's on battery among
them. The driver reads it after every `handle` call, as
it reads `next_wakeup`, runs the receiver while it holds and keeps it in standby when it
does not, but for a search under way, and hands each fix as `AppCommand::Fix`. The end of
a fixed relay's grace on battery is a wakeup of its own.

`AppCommand::Power` says whether external power, USB, is present. The driver sends it
right after the boot wakeup and at every change, and may repeat it. On external power
the node is a relay, on battery sleepy (spec "Nodes"): while it boots, from the report
on; running, from the change on, as spec "Sleepy nodes" says.

`AppCommand::ForceRole` forces the other role, for a person at the node who presses its
button (spec "Nodes", roadmap D160): a relay leaves as at its low battery and turns
sleepy, a sleepy node turns relay at once, and a press while a role is forced forces the
other. `forced()` says which role is forced, if any, for the node's screen. The force
lasts until an `AppCommand::Power` that changes the power, which ends it, and a forced
relay on battery still turns sleepy at its low battery level, which ends it too. A node
that cannot relay, or one on battery at its low level, does not take a force to relay. A
reboot forgets it. `bark-driver` takes it as `Request::ForceRole`.

`AppCommand::Battery` reports the battery's voltage in millivolts as the board's converter
reads it, uncalibrated. The driver sends it only on battery, never on external power,
where the reading is the charger's; once a minute (`bark_driver::BATTERY_EVERY`); and only
between radio operations, never while the radio transmits, whose current would pull the
cell down. The core calibrates the readings at a rested full charge and keeps the scale in
its stored state, and compares them with `Config::battery`'s levels (spec "Sleepy nodes"):
a fixed relay on battery relays until a reading at or below the low level, and a node its
battery turned sleepy relays again at the high level. A driver whose board reads no
battery builds the core with `Config::battery` `None`, and the core then turns a fixed
relay sleepy at its grace's end, since it would otherwise relay until it browns out. A
carried relay has no grace with or without levels: on battery it turns sleepy at once.

A message for this node's application waits in its inbox, oldest first, until the
application confirms it (spec "Phone link"; roadmap D122). The core announces each with
`AppEvent::Message` and its ID; `inbox()` gives the oldest's ID and payload, `inbox_len()`
how many wait, and `confirm(msg_id)` forgets the oldest if it is `msg_id`, and answers
whether it was. The driver reads them between `handle` calls; confirming needs no
`handle` call and changes nothing else. A full inbox follows the delivery queue's rule
(spec "Delivery queue"): a stale message makes way, and while every one is fresh a
message for the application is refused with a wait, so the simulator's driver and the
tests', which confirm each message at once, never fill it. The firmware's driver takes
the application's confirmations, and its map point set and clear, as
`bark_driver::Request`s when the application is ready. The core never inspects a
payload (spec "Phone link": the app owns identity and encryption).

**The update** (spec "Updates over the mesh"). The core carries an image's pieces
and decides what to send and when; their bytes, the code that makes them, the flash
they wait in, the policy and the signature check are the driver's. In the simulator
the pieces carry no bytes and a block decodes when its count of distinct pieces is in.

- `AppCommand::Holds(image)`: the image the node holds whole and has checked, the one
  it runs or one staged after its signature passed, with its name, version, block
  count, the pieces of its last block and its key; at boot and after each change. The
  core tells it in its heartbeats' section and serves it.
- `Action::Offered(image)`: a neighbour's heartbeat named an image above the version the
  node runs and the one it gathers; once for each image. The driver answers
  `AppCommand::Gather { image, from }` if its policy takes it, `from` the first block it
  lacks, 0 unless it gathered some before a reboot, which it may also say at boot
  unasked; else it answers nothing. The keys and the signatures stay out of the core,
  which keeps no answer.
- `Action::StorePiece(piece)`: keep this piece. The core hands pieces of every block
  the node lacks, each once, of an image of at most 48 blocks, so the driver holds at
  most a lane of 64 a block, 48 lanes (`bark-image`, `pieces.rs`). When a
  block's pieces decode it answers `AppCommand::Block { index, ok }`, `ok` if the block
  matched its hash; the core then takes no more of it, or takes its pieces again.
  Once every block has decoded the driver checks the image and says `Holds`.
- `Action::Serve(frame)`: transmit this piece frame as `Transmit`, after writing into
  it, from `wire::PIECE_HEADER_LEN` on, the bytes of the piece its header names, of
  the image the driver said it holds or of the one it gathers, a block it said
  decoded and passed; completes with `TxDone`. The driver holds the
  bytes, so the core needs no copy of a piece to send one, and a coded piece is
  made only when it goes. A piece the driver cannot fill in time it does not send,
  and still completes with `TxDone`: to the core it is a piece lost on the air.

`bark-driver` hands these to its application, `App::offered`, `App::store_piece` and
`App::serve`, and the application's answers come back as `Request::Holds`,
`Request::Gather` and `Request::Block`, which the driver delivers as the commands.
The firmware's application is `bark-fw/src/update/mesh.rs` (§5).

**A neighbour's status** (spec "Neighbour status"). `AppCommand::Ask { neighbour }` asks
a live neighbour for its status, once; the app sends one for each neighbour it shows when a
person opens or refreshes a map of them, never on its own. The core ignores an ask of a
node it does not know. An answer addressed to the node comes back as
`AppEvent::Status { from, status }`, at most once an ask and never if a frame was lost.
`Node::status(now)` is what the node itself tells when asked: what its screen shows, so a
driver draws its screen from it too. Two parts of it only the driver knows: the image
version, `Config::version`, set at build (0 in the simulator), which the heartbeats'
section also tells as the version run, and the battery, the last
`AppCommand::Battery` reading, which the core now keeps. `Status::gathering` is the
image the core gathers, its version and blocks, the blocks held from the first and the
pieces of the next, all zero when none (roadmap D158). `bark-driver` takes the
application's ask as `Request::Ask` and hands the answer to `App::message`.

`AppEvent::Boot { relays, ends }` tells the application how the node's first rotation goes
(spec "Channel at boot", "Phone link"): at boot, with no relay known and when the rotation
ends; each time the node learns a relay in it, with the relays it knows; and once at its
end, `ends` then `None`. A sleepy node's first rotation ends at the first relay it hears,
or at once when it resumes a stored anchor.

`Action::Store(stored)` asks the driver to keep `stored` across a reboot, in place of
what it kept before (spec "Implementation notes"). The core emits it at boot and then only
when the state changes: the home channel, but a carried sleepy node's, the place a fixed
node stands, the carried flag, a fixed sleepy node's anchor, whether it is a relay and its
battery readings' scale. The driver hands the last one back in `Config::stored`
when it builds the core at the next boot; a fresh board has `None`. `Stored::record` lays
it out as 32 bytes with a checksum, and `Stored::from_record` takes back only a whole
record. The firmware appends records to a page of flash (`bark-fw/src/store.rs`); the
simulator keeps the last one across a `restart` and drops it at a `wipe`.

## 5. What the driver must simulate or implement

| Driver obligation | Simulator | Firmware |
|---|---|---|
| Clock | event-heap time | embassy monotonic |
| Wakeup when `now >= next_wakeup()` | heap entry per core, lazily invalidated | one embassy timer |
| `Listen` | update the per-channel listener set | standby, frequency and receive, each only if the chip does not hold it already (`bark-driver/src/sx1262.rs`) |
| `Sniff` | receives a frame only if a window of the cycle, started at the `Sniff` and again after each frame it takes, holds four symbols of its preamble; the time is counted | SetRxDutyCycle(240, 14 952), or the periods for a longer probed delay, from STDBY_RC with that delay, the receive timer stopped at the header and not at the preamble, the receiver programmed for the long preamble; started again after each packet; the chip woken by NSS before any other command and its whole configuration sent again after (`bark-driver/src/sx1262.rs`) |
| `Lbt` | deaf for the preparation, the CAD's sampling and the return; the CAD's result from the reception model, the reading the dB sum at the node `rssi_dwell_us` into the sampling | standby, frequency, CAD; `GetRssiInst` four symbol periods in, less the offset measured at boot; the result read and cleared; frequency and receive with the short oscillator delay (`bark-driver/src/sx1262.rs`) |
| `Rssi` | on another channel deaf for retune, `RSSI_SETTLE`, read and retune back, dB-sum at the node | `GetRssiInst`, on another channel `RSSI_SETTLE` after the retune |
| `Transmit` | deaf for airtime, deliveries to hearers via the reception model | standby, frequency and packet parameters if they differ, the payload, transmit; then packet parameters and receive |
| `Received` | reception model | `rx` with CRC check |
| Seed | per-run seed → per-node stream | nRF RNG at boot |
| Power (§4), after the boot wakeup and at each change | the power the placement gives a node, or `[power]`'s process (`bark-sim/src/power.rs`) | VBUS detection: `USBREGSTATUS.VBUSDETECT`, read again at each POWER interrupt (`bark-fw/src/power.rs`) |
| Battery (§4), on battery | `[battery]`'s cells, read once a minute (`bark-sim/src/battery.rs`) | the SAADC on AIN2 behind the board's 2:1 divider, eight samples averaged, its offset calibrated before each reading (`bark-fw/src/battery.rs`) |
| GPS (§4) | `Action::Locate` starts a search, one at or before now ends it; a fix after a hot or a cold start under the sky, none indoors for the share `[gps] no_sky`; `gps_tracks` read after each call, its time counted at the receiver's current, a walking relay handed a fix at each heartbeat while it holds; with `[gps] checks`, `fix_in_reach` asked of each fix (`bark-sim/src/gps.rs`, `sim.rs`) | `bark-driver` keeps the search's end and tells the application to rest, search or track after each call (`bark_driver::Gps`), ends a search at the first fix in reach, and asks `fix_in_reach` of each fix; the L76K's NMEA over UARTE1, its WAKEUP line high to run and low for standby, the UART off at rest (`bark-fw/src/gps.rs`) |
| Store (§4) | the last `Stored` per node, kept across `restart`, dropped at `wipe` | a log of records in the flash page `STORE`, written after the step that asked, through the MPSL's flash timeslots (`bark-fw/src/store.rs`, `bark-image/src/layout.rs`) |
| Update (§4) | the policy takes every image offered; a piece kept is counted, and a block decodes at its count of pieces; a piece's bytes are not written (`bark-sim/src/update.rs`) | `Holds` for the running image if the record's header is its version's and the slot hashes to it; the policy of `bark_image::policy`; the pieces in the external flash's store, a step at a time; a piece served coded from the slot or the decoded blocks, or not sent if not filled within 372.8 ms; the image checked and staged as from the phone, and a reboot at a quiet moment (`bark-fw/src/update/mesh.rs`, `bark-fw/README.md` "Updates") |
| Inbox (§4) | the application reads and confirms each message at once | the phone link reads the oldest and confirms it when the phone writes its ID back (`bark-fw/src/ble.rs`, spec "Phone link") |

## 6. Memory

`Relay` holds its tables inline (spec "Implementation notes"). Their sizes, checked by the
test in `tables/mod.rs`:

| Table | Entries × size | Total |
|---|---|---|
| Neighbours (spec "Neighbour table") | 64 × 120 B, and 4 × 16 B for the images their sections name | 7.6 KiB |
| Heard sleepy nodes (spec "Heard table") | 256 × 32 B | 8.0 KiB |
| Delivery queue (spec "Delivery queue") | 41 × 264 B | 10.6 KiB |
| Message records (spec "Message records") | 667 × 40 B | 26.1 KiB |
| Send queue (spec "Send queue") | (39 + 64) × 304 B | 30.6 KiB |
| Inbox (spec "Delivery queue", "Phone link") | 23 × 264 B | 5.9 KiB |
| Delivered window (spec "Delivery queue") | 41 × 16 B | 0.6 KiB |
| Duty budget (spec "Duty cycle cap") and busy estimate (spec "Busy estimate") | 16 B + 368 B | 0.4 KiB |
| **All** | | **89.9 KiB** |

The send queue's 39 regular places follow from the duty bucket, and its 254 hop places, one
for each hop count a message can be sent on with, keep overload from dropping messages
(spec "Send queue", roadmap D31); 64 of them can be held at once (roadmap D155). The records
hold the 564 messages a relay can handle in the 162.9 s a full send queue takes to drain at one
ban per place, plus the 103 the send queue
holds, whose records are never given up (spec "Message records"). The delivery queue's 41 places hold what
arrives in one delivery TTL for a full heard table (spec "Delivery queue"). The whole `Relay` is
95 016 B (92.8 KiB): the tables and, in under 1 KiB around them, the
medium access, the hand-over in flight, its owed reply with the DATA it takes (roadmap D93),
a neighbour's status asked or told, 16 B, with two marks in each neighbour's entry that fit
its padding (roadmap D157),
and its stored state as handed back and as last kept, 72 B, and beside them what it keeps
of the update, 1 888 B, with 16 B of each neighbour's entry for its heartbeats' section and
when it was heard, and the four images the sections name kept once in the table, 64 B (spec
"Updates over the mesh"); its application's message waits in the send queue. Every board runs it, since every node may be a relay (spec "Nodes"). A
`SleepyNode` is 76 000 B (74.2 KiB), without the heard table and the delivery
queue, 18.6 KiB (roadmap D58), for a driver whose node never switches, as the simulator's
tests can have. There is no fixed target;
the sizes follow from the design. The board's application has 255 KiB of RAM
(`bark-image/src/layout.rs`), 261 120 B. The node image leaves its call stack 112 008 B,
`bark-fw/scripts/uf2.py --stack` on the open release build of version 14 (roadmap D158), against a
floor of 24 576 B (`bark-fw/README.md`, "Tasks and the stack").
Checked by the tests in `tables/mod.rs` and `node/relay/tests.rs`.

That holds only if the relay is not built on the stack. `Relay::new` returns a relay by
value, and on the board's target building one took 174 KiB of stack in the
firmware's release build, more than the 88 KiB left beside a
relay in static memory. So `Relay::reset(config, seed)` sets up a relay in place, as
`Relay::new` would have, whatever it held before; it empties its large tables where they
lie, since a table built and assigned passes through the stack, and the four largest
together made a 28 KiB frame in the firmware's boot. A driver can keep the relay in a static
holding `Relay::UNSET`, a relay with empty tables built at compile time, or holding zero
bytes, which are a valid relay. The firmware does the second: a static of zero bytes is in
`.bss`, which takes no flash, where `Relay::UNSET`, whose empty tables are not all zeros,
would put the whole relay in flash, to be copied to RAM at boot. A constant in
`bark-fw/src/node.rs` stops the firmware building if zero bytes stop being a valid relay,
and a test in `bark-driver` checks that a relay reset in zeroed memory runs as a new one.
The simulator, on the host, uses `Relay::new`.

`Action::Transmit` carries a 255-byte frame by value: drivers hold one
action at a time, and the core never allocates.

## 7. Wire-format choices made here

- Position: 56-bit big-endian, latitude in the top 25 bits, longitude in the next 26, five
  spare bits zero (`position.rs`).
- Short position (spec "Positions on the wire", roadmap D14): the low 16 bits of the latitude count, then
  the low 16 bits of the longitude count, big-endian (`ShortName` in `position.rs`). The
  receiver rebuilds the full position from its own as the one candidate within half a
  period (32 768 counts) on each axis; both axes wrap as their wire fields do.
- Node ID (spec "Node identity"): six opaque bytes; the firmware takes the low 48 bits of the
  chip's FICR DEVICEID, all of DEVICEID[0] and the low 16 bits of DEVICEID[1]; the
  simulator gives node `i` the ID `(i + 1) × 0x9E3779B97F4B` modulo 2⁴⁸, or one drawn from
  the run's seed (`ids = "random"`).
- Message ID (spec "Packet formats", D15): 32-bit FNV-1a over `dst_pos || dst_id || payload`,
  `dst_pos` in its 7 wire bytes (`MsgId::of` in `ids.rs`; `Data::msg_id` on a decoded
  DATA).
- Everything else in spec "Packet formats" is encoded by the codecs in `wire.rs`. Choices made
  there:
  - Multi-byte integers (short positions, message IDs) are big-endian; a node ID is six
    opaque bytes in order, compared whole.
  - ACK `to` of a DATA is its `src_hop`.
  - A sleepy node's own DATA has `hops` zero. A relay sending for its own application
    sends `hops` 1, as if it had received the message from the application at 0: a
    receiver takes `hops` 0 for a sleepy node's own message (spec "Packet sender").
  - Application payload at most 227 bytes, what remains of 255 after the 28-byte DATA
    header; the decoder rejects anything longer.
  - A DATA's `aux` 15 is the update's piece, 16 and 17 a neighbour's status asked and
    told, and 13, 14 and 18 to 63 are rejected (spec "Packet formats"); a piece is always a
    full frame, 247 bytes after its 8-byte header, an ask 14 bytes and a status 30. A
    status's battery byte that is no percent, 101 to 254, is read as not read. A heartbeat of 30 bytes or more carries the update's section in bytes 13 to
    29, its versions big-endian (`wire::decode_section`).
  - The first byte's bit 6 marks an ACK, with the six bits under it `aux` and the top bit
    the seen flag (roadmap D76, D90). With bit 6 clear, the top bit marks a heartbeat,
    whose low four bits hold its channel, 0 to 12, and the two between are zero; the
    firmware's bench frames put 15 there, which nodes reject (`wire::BENCH_FIRST`). Both
    clear, the byte is DATA's, with `aux` in its six low bits.
  - The second byte is the format version (roadmap D77). The core writes it: 1, its only
    format, in DATA and the ACK, and in a heartbeat as the newest it speaks. It rejects a
    DATA or ACK of another format, reads every heartbeat whatever its format, and keeps
    each neighbour's in `Neighbour::format`. A driver neither sets nor reads it.
  - A heartbeat's third byte is its flags: bit 0 sleepy, bit 1 anchorless, bit 2 carried,
    bit 3 map point; the core sends the other bits zero and ignores them. The core sets
    the map point from its own position's source and keeps each neighbour's in
    `Neighbour::map_point`.
  - An ACK's `aux` is 0 when it takes the offer and the code of the wait the refuser
    asks for when it refuses, 1 to 63, 1.58 s to 89 min (spec "Bans", `budget::told`);
    a seen ACK (spec "The ACK") has
    the seen flag and `aux` 0, and one with the flag and a wait is rejected. Taken,
    refused or seen, an ACK is 18 bytes. A DATA's `aux` is the sender's home channel,
    and a DATA with either of the two high bits of `aux` set is rejected. A heartbeat is
    read from its first 13 bytes, and what follows is ignored. An ACK with trailing
    bytes is rejected.
