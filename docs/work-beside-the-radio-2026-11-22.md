# Work beside the radio: what the bench measured (2026-11-14 to 2026-11-17)

The section "Work beside the radio" of `bark-fw/README.md` as it stood on
2026-11-22, with the measurements that led to each rule. The README now
states the rules; this note keeps how they were found.

Every task runs on one executor, so a task that does not await holds up
all the others, the radio's among them. Two moments of the radio's task
cannot wait long (Radio driver, below): the reading inside a
listen-before-talk's CAD is due 1 024 µs into it and must be made before
its sampling ends at 2 048 µs, or the check counts as busy and a
heartbeat is skipped; and the receive after a CAD must start within
1 ms of it for the oscillator to restart warm, or it is a cold start, the
oscillator's longer delay.

No other task may run longer between two awaits than a quarter of the
restart's window, `radio::STRETCH`, 250 µs: the radio's wake can then
wait out four such stretches and still be in time. The reading inside a
CAD waits for no task at all (below).

The external flash's software bus moves a byte in 8 µs (roadmap, Phase
2, "The external flash's bus"), so a read of a 256-byte slot took 2 ms;
a block's decode was a run of them, and 82 of 88 checks that got no
reading in four hours on a board that gathered an image fell within 20 s
before one of its blocks was written (2026-11-14). Now every
asynchronous user of the chip moves at most 16 bytes between two awaits,
`bark_image::ext_flash::CHUNK`: a read of them takes 187 µs and a
program's start 184 µs, which the build checks against `STRETCH`. A page
is programmed in parts of 16 bytes, each waited out with a timer and
looked at every 50 µs, since the datasheet gives a page's time alone.
Before each part the user waits with a timer for any program or erase on
the chip, whoever started it, which a read would otherwise wait out on
the processor, up to 12 ms. The update over the mesh's store, the
serving of a piece, the hash of an image staged from the mesh or the
phone, the mailbox's writes and the phone's staging all go so. A read
of the store's and the sums it goes into are a step each; the code's
points are weighed 4 rows a step, about 0.12 ms; a piece served from the
running image is summed 64 bytes at a time; and a hash takes one block of
SHA-256 a step, block 1's header and block 0's table among them, the
running image's two hashes in turns. The update's task awaits after each
part of its pass, a piece served, a block's line, an offer taken, so that
none runs on into the next step within one poll.

The parts cost time: a block decoded from none of its data pieces reads
for about 12 s instead of 8, a slot is written in 16 programs, and the
hash of a staged image takes about 6 s instead of 3. Until a board
measures how long a short program takes, a phone's update may take up
to twice its 20 s.

The parts were not enough for the reading: in four hours on a board
gathering an image with them, 23 checks got no reading, 14 of them
within 20 s before a block was written (2026-11-14). The reading's wake
waited out what ran at two awaits, the command's wait for BUSY and the
reading's own timer, and the two together had the 1 024 µs, not each:
in 17 of the 23 the CAD's result came less than its 2 ms after the start
the driver saw, so the driver saw the start late and read after the CAD
had ended, the two waits 1 ms or more together; in 5 the reading's wake
alone was 1.4 to 2.3 ms late, longer than any poll of the update or the
phone link measured that minute, so something not then timed held the
processor. So from
`SetCAD` to the reading the radio's task keeps the processor, waiting
for BUSY and for the reading's time without an await
(`sx1262::Chip::hold`, `bark_driver::Clock::hold`): about 1.1 ms a
check, more by the oscillator's delay when the CAD starts it, a few a
minute. Only an interrupt can make the reading late now. In the next
2 h 12 min on three boards, one serving and two gathering, no check went
without a reading (2026-11-14, evening).

Timed with the interrupts taken out (Bench build, below), that run also
showed what else held the processor longer than the bound. The GPS's
task parsed a second's sentences in one poll, 1.5 to 1.9 ms every
minute on every board; it now parses 64 bytes between awaits
(`gps::PARSED_AT_ONCE`). The store's check hashed a decoded piece a
step, 0.5 to 0.7 ms; it now hashes one 64-byte block of SHA-256 a step.
A piece served from the running image measured 0.55 ms at its longest
poll, the update's offer 0.64 ms, each with a log line in the poll; the
bench build now counts a log line's time at its own place, `log`.

A log line holds its caller's poll about 0.17 ms with one value (the
night's polls that did little but log `uptime` or the watchdog's line,
0.17 to 0.18 ms), and some 30 µs more for each value it formats: the
GPS's fix line, twelve, 0.6 to 0.8 ms. Nearly all of it is the
formatting, which runs in the caller before the logger sees a byte; the
logger then copies the line into its ring, a few hundred instructions.
So nothing in the logger bounds a line: each line goes in a poll of its
own, and a long one is formatted in parts with an await between and then
logged whole, as the fix's is (`bark_driver::gnss::FixLine`).

The night's four hours on three boards (2026-11-15) showed what else was
over the bound, each poll's longest place: the fix's line, every minute;
the update's weighing of 16 rows, 0.39 to 0.47 ms; a read of the store's
and its sums in one step, 0.28 to 0.34 ms (`sum`); block 0's table
hashed in one step, 0.75 ms, once an image; a piece served, its last
weighing and a piece's sums in one poll, up to 0.95 ms, or the data
pieces it did not want passed over with its line, 0.72 ms; the offer's
two lines in one poll, 0.66 ms; and at boot the running image's hash,
eight blocks of SHA-256 a poll, up to 2.7 ms. Each now goes as above.

Six hours on the same boards with these changes (2026-11-15) brought the
longest poll of a minute near the bound, not under it: the GPS task's
0.24 to 0.26 ms at the median and 0.29 ms at most, the update task's 0.21
to 0.34 ms at the median and 0.36 ms at the 95th percentile. No channel
check went without its reading.

The processor's instruction cache, off after reset, is on from the first
line of `main` in every build (roadmap D227). Forty minutes on three
boards with it on and off (2026-11-17) took the GPS task's longest poll
of a minute from 220–250 µs to 176–189 µs at the median and from at most
295 µs to at most 204 µs, and the update task's from 319–358 µs to
253–285 µs at the 95th percentile. Writes to the internal flash need
nothing for it: while the NVMC is in write or erase mode the cache is
invalidated and stays so (Product Specification, NVMC, `CONFIG`), so
code runs then as it did without the cache.

This bounds what the update does, not every task. Known stretches over
the bound, left as they are (2026-11-15; measured where it says so, else
worked out from the bus and the datasheets):

| Where | How long | Why it stays |
|---|---|---|
| `sign::verify`, a header's Ed25519 signature: once a boot for the running image, at each image staged | 45.6 ms at boot, measured | one call into the library; once a boot and once an update |
| the phone link's start, its host's key pair | 286 ms, measured, 1.4 s after boot, the radio up | once a boot |
| the phone link's host while a phone is linked | 1.4 to 1.75 ms a poll, measured | not yet known what |
| a phone link's slot while a phone is linked | up to 9 ms a poll, measured | not yet known what |
| the screen's pictures at boot | 0.9 and 3.3 ms, measured | the first two of a boot; no other was drawn in 2 h |
| the end of the running image's hash at boot (`update/mesh.rs`, `finish`): the last block, the table's own entry, the firmware's hash | some ten blocks of SHA-256, up to 2.7 ms, worked out | once a boot |
| the GPS's line on a first fix whose satellites give no C/N0 | about 0.3 ms, worked out | once a search, only on a sky that gives none |
| the MPSL's interrupt (RTC0) | 10.6 ms, measured, once as a board finished its image | most likely an erase part of the internal flash (below) |
| the mailbox's reads (`mailbox.rs`, `Ring::read`), from the phone link's task, `home_now` | 4 ms for an item, up to 63 ms after a page erase; a read during a write waits it out, up to 12 ms | the ring reads blocking through `bark-app-layer`; parts there are a larger change |
| the same reads from the radio's own task between its steps | as above | they delay its next step, not its reading inside a CAD |
| the MPSL's flash timeslots for the internal flash (`store.rs`) | 41 µs a word written, 328 µs a record; an erase in nine parts of 10 ms, a length fixed in `nrf_mpsl::Flash` | the processor stops while the internal flash writes, inside the MPSL's interrupt: no task can shorten it, and it can make a reading inside a CAD late |
| the phone link's pairing, P-256 in software | tens of ms | once a pairing |
| the core's `handle` on a relay whose send queue and neighbour table are full and every neighbour banned: every queued message routed over every live neighbour, at each event while no hand-over is under way and nothing may go | 48 µs on the host with 39 + 64 queued and 64 neighbours, so some 24 ms here, where a call takes about 500 times as long; 6 µs, some 3 ms, with 3 neighbours. A received DATA takes at most 2.9 µs, some 1.5 ms, and the choice when one may go 1.5 µs, some 0.75 ms (2026-11-17, roadmap D231, `bark-proto/src/node/relay/handle_time.rs`) | to know that none may go, the choice routes every message; nothing kept yet says it without routing |

A wholly bounded radio would run its task on an interrupt executor above
thread mode, so that no task could delay any of its wakes, not only the
reading's. That costs more than it looks: what the radio's task shares
with the others, the update's store and requests, the mailbox, the phone
link's notes, is held in `ThreadModeRawMutex` cells today, which an
interrupt cannot take, and each would need a critical section or a
channel; the MPSL's flash, whose users must stay in thread mode; and the
external flash, which both the radio's task and the others read. It is
the answer if the bench shows the restart or a frame's service late.

The bench build times every task's polls and every interrupt, once a
minute and over each check without a reading (Bench build, below).

