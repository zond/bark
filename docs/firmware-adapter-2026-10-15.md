# The firmware's driver loop, and frames that arrive while it is busy

**Status:** 2026-10-15. Describes the loop that runs the protocol core on the board, as
built, and drafts a decision on what happens to received frames the loop cannot take at once
(section 2.5; decided as roadmap D37 on 2026-10-15). Nothing here has run on a board.

## 1. The loop

`bark-driver` holds the loop; `bark-fw/src/node.rs` runs it on the board. It does one thing
at a time:

1. Wait for the first of: the core's next wakeup, the radio, or a message from the
   application while the core accepts one.
2. Hand the core the event: `Wakeup` once the clock has reached `next_wakeup()` (a timer that
   fires early is waited out again), `Received` with the frame's bytes, channel, RSSI and SNR,
   or the application's `Send`.
3. Carry out what the core emits. `Listen` retunes at once. `Trace` and `App` go to the log.
   `Cad`, `Rssi` and `Transmit` are kept, and the next step performs the excursion, puts the
   radio back on the resting channel and hands the core its completion.

The radio, the clock and the application are traits. On the board they are the SX1262 through
`lora-phy`, the embassy clock and a log; in the tests, a fake radio and a clock that jumps to
the next moment anything waits for, so every wakeup and completion lands exactly when due.

Details that follow from the contract (`core-interface.md`):

- **Never early.** The board's clock counts 32 768 Hz ticks; a deadline is rounded up to the
  next tick, and the loop checks `now >= next_wakeup()` before it delivers a wakeup.
- **A check waits for a packet in progress** (§3 rule 8; spec "Busy estimate"). The radio reports a
  packet when it detects its preamble or header. A CAD, or an RSSI reading on another channel,
  asked for while one is arriving waits until it ends, at most one longest frame (138.6 ms).
  The frame reaches the core meanwhile, as in the simulator. A transmission never waits.
- **Every excursion completes.** If the radio fails, a CAD reports busy and an RSSI reading
  0 dBm, the top of the radio's scale, so the core does not transmit on a check it never got; a
  failed transmission still reports `TxDone`, and the retries of spec "Hand-over" cover the frame.
  The radio is set listening again on the next step.
- **Traces are log lines.** The firmware logs as text over USB because the board has no debug
  probe; it does not use defmt, which needs one or a decoder on the host.

The application is a log for now: a message for the node is printed, and the node sends none
of its own. A phone link (roadmap D5) will be another implementation of the same trait.

### 1.1 The relay must not be built on the stack

`Relay::new` returns 150 KiB by value. Built for the board, it needs 174 KiB of stack, and its
caller as much again to copy the result into a static; the board has 97 KiB of stack beside a
relay in static memory. The first build of this firmware would have overflowed its stack at
boot. The core now has `Relay::UNSET`, a relay with empty tables as a constant, and
`Relay::reset`, which sets one up in place (`core-interface.md` §6). The firmware keeps
`UNSET` in a static; the deepest stack frame left is 18 KiB, where `reset` replaces the small
tables.

The relay's empty tables are not all zero bytes, so the static is initialised data: 150 KiB
of flash copied to RAM at boot.

### 1.2 Sizes

Release builds, 2026-10-15:

| Build | Flash | RAM before the stack |
|---|---|---|
| Relay | 284 KiB, 150 KiB of it the relay's initial tables | 157 KiB |
| Leaf | 102 KiB | 8 KiB |
| Console | 89 KiB | 9 KiB |

The application has 824 KiB of flash and 255 KiB of RAM (`bark-fw/memory.x`). The relay
leaves 97 KiB for the stack.

### 1.3 The host tests

`bark-driver/tests/driver.rs` runs the loop against the fake radio. With a scripted core it
shows wakeups delivered at the instant asked for, also when the timer fires early; a frame on
the resting channel reaching the core with its bytes when it ends, and one on another channel
not; a transmission going out on its channel, `TxDone` one airtime later, and the radio back
on the resting channel; a CAD waiting for the packet in progress and running after it; and
that wait ending after one longest frame. With a real `Relay` for 20 simulated seconds it
shows the relay booting on its home channel, taking a neighbour's heartbeat into its table,
sweeping the channels with a 300 µs dwell, sending its own heartbeat, and delivering its
application's message to itself.

## 2. Frames that arrive while the loop is busy

### 2.1 What the radio does

The SX1262 receives one packet at a time. In continuous receive it writes each packet's
payload into its 256-byte buffer and raises RxDone; GetRxBufferStatus reports the start and
length of the last packet only, and GetPacketStatus its RSSI and SNR (SX1261/2 DS §13.5.2,
§13.5.3). After RxDone the radio goes on searching for the next preamble without the
processor. So the radio holds one frame for the processor: if a second packet ends before the
first is read, the first is lost, newest kept. In continuous receive each packet is written
after the previous one (DS chapter 7, Data Buffer), so the bytes of a long first frame are overwritten as the
second one's payload arrives, after the second's preamble, sync and header. Both are the
datasheet's account; neither has been seen on a board.

While the radio transmits, checks a channel or retunes, it receives nothing. The contract
says so (§3 rule 3), the simulator counts those losses, and the sender's retry is what
recovers them.

### 2.2 How close two frames can be

At SF5 and 125 kHz a symbol is 256 µs. The shortest frames the protocol sends, a heartbeat or
an ATTACH, take 11.84 ms; an ACK 14.40 ms; a full frame 138.6 ms (`radio::time_on_air`).
Another network on the same sync word could send a payload of 0 bytes, 31.25 symbols, 8.0 ms.

A packet whose preamble began during the previous one can still be received if enough of its
preamble is left after the previous one ends. In the worst case none is needed, so the next
RxDone can come one 0-byte frame after the previous minus its 12-symbol preamble: 19.25
symbols, **4.9 ms**. The next packet's payload begins to arrive after its sync word and
header, 14.25 symbols, **3.6 ms** after the previous RxDone at the earliest. For the
protocol's own frames the gap is at least 11.84 ms less the preamble, 8.8 ms.

So the loop keeps every frame if it reads each one within 3.6 ms of its end.

### 2.3 How long the loop takes per frame

Reading a frame is five commands over SPI at 8 MHz, the longest moving 255 bytes in 255 µs;
about 0.3 ms with the commands' own overhead. Then the core handles it.

The core's time per event was measured on the host with `bark-driver/examples/handle_time.rs`:
a relay with a full neighbour table (64) hears full-length DATA frames back to back for 600
simulated seconds, each for a destination 20 km away, with a neighbour's heartbeat every
eighth frame. Nobody answers its hand-overs, so its send queue stays full, and about 4 300
messages pass through its 1 695 records. That is 32 058 events. Instructions per event were
counted by running the deterministic run under `perf stat -e instructions:u` to just before
and just after the event, accurate to about ±1000 and including the loop around the call:
for 600 events at random after the first 1 500, the median is 1 900 and the 99th percentile
7 000; the highest seen in those and all of the first 1 500 is 7 600. The whole run averages
2 460 an event. On the host (AMD Ryzen 9 9955HX3D) a received frame takes 0.35 µs on average
and at most 1.5 µs, the shortest of five runs.

The nRF52840 runs at 64 MHz. Two factors make an estimate for it, neither measured: the
Cortex-M4's Thumb-2 code takes up to 1.5 times as many instructions as x86-64 for the same
work, and executing from flash with the instruction cache off (embassy-nrf does not turn it
on), loads and taken branches average up to 3 cycles an instruction. That is 4.5 cycles per
host instruction: the highest seen, 7 600, is **0.53 ms**; the median 0.13 ms. Formatting a
trace for the log adds an unmeasured few tens of microseconds for each the event emits.

A frame therefore holds the loop for about 1 ms at most, against the 3.6 ms by which the
next one could start to overwrite it, and the 4.9 ms by which the next one could replace it.
The firmware logs its longest `handle` and the longest stretch it spent away from a listening
radio (`longest_handle_us`, `longest_away_us`) every 10 s, so the boards will say whether this
holds.

### 2.4 What the simulator assumes

The simulator hands a frame to the core the instant it ends on the air, the core takes no
time, and the actions it emits start at once. A node misses frames only while it transmits,
checks a channel, retunes, or holds a packet another one corrupts. It has no notion of a
processor that is busy, and no buffer. That is the firmware with a loop of zero duration and
no queue.

### 2.5 Draft decision (D37)

| # | Decision | Why |
|---|---|---|
| D37 (as drafted here; the roadmap has the decision) | **No queue of received frames.** The radio's one frame is the only buffer: the loop reads it when it is next free, and a frame that ends while the previous one is unread replaces it. The loop must be back at the radio within 3.6 ms of any frame's end; the firmware measures how long it is away (`longest_away_us`), and a board that exceeds it has a bug to fix, not a protocol to change. A frame lost anyway is recovered as any frame lost on the air: DATA, ATTACH and DELIVER by the sender's three attempts (spec "Hand-over"), an ACK by the sender's retry, which the receiver answers again without acting on it twice (D35), a heartbeat by the next rotation. | The loop is estimated at 1 ms per frame against 3.6 ms; it keeps every frame with room to spare. The simulator assumes no queue and no processing time; with no queue the firmware differs from it only by that 1 ms, well inside the 100 ms ACK timeout. A queue would also deliver frames late, and the core stamps a frame with the time it is handed over: a frame that waited would make an ACK look later than it was and put a busy reading in the wrong place (spec "Busy estimate"). Not chosen: a queue of k frames filled by a higher-priority task, needed only if something stalls the loop for more than 3.6 ms (a BLE connection event, a flash write, an e-paper refresh), with depth the longest stall over 4.9 ms and the newest frame dropped when full; if that work comes, it should not stall the loop, and if it must, the queue comes with an arrival time in `Event::Received`, a contract change. |

## 3. Not verified

- Everything about the radio in 2.1: one frame held, the newest kept, the overwrite in
  continuous receive. The boards show it with two transmitters a few milliseconds apart.
- The loop's time on the board (2.3 is an estimate). `longest_away_us` measures it.
- The read and clear of the radio's interrupt flags are two commands; a flag the radio raises
  between them is cleared unseen. The window is tens of microseconds and the next frame's
  RxDone is at least 4.9 ms after the one being read, so only a preamble notice can be lost
  this way, which costs at most an unheld check. Clearing only the flags read would close it;
  the driver clears them all.
- The radio keeps its oscillator running between operations, so a retune or a transmission
  should cost about what the simulator assumes, 0.1 ms. A CAD still turns the oscillator off at
  its end, and restarting it leaves the radio deaf for an assumed 5 ms after every CAD, which
  the simulator does not model (`bark-fw/README.md`, Radio driver and Risks). Neither has been
  measured.
