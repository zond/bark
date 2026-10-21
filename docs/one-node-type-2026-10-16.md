# One node type, one setting: measured

**Date:** 2026-10-16 to 17. Roadmap D46, adopted 2026-10-17. Branch
`research/one-node-type`, first based on main `8cd70a4` and measured
against it; then rebased onto main `994e1ce`, which carries D44, and
measured again ("After the rebase onto D44", at the end). The sections
before that one describe the prototype as first measured, and cite the
spec as it was then; the spec now describes one node type
(`protocol.md` "Sleepy nodes").

The question: if relays and leaves become one node type with one
setting, sleepy, what does the network lose?

## Summary

- **With sleepy nodes that listen all the time, as the simulated ones
  do, one node type costs nothing measurable at the rate of
  spec "Capacity and sizing"** and little at twice it: S4 delivers 96.3 % against
  main's 96.7 % at that rate (−0.4 ± 0.0 points, seeds 1 and 2) and
  93.7 % against 96.0 % at twice it (−2.2 ± 1.0); the standard and
  loaded suites deliver what main does, and loaded rings and corridors
  carry from 5 % less to 4 % more once every sleepy node has its anchor.
- **Sending to a sleepy node only on its beat, what a node that sleeps
  needs, costs three points and two minutes**: S4 93.2 % at the rate
  (−3.4 ± 0.1), a median latency of 130 s instead of 1 s. Flows to one
  node faster than its window after a beat carries, the rings, the
  corridors at a text a second and the burst, lose a third to a half of
  what they carry.
- **Presence costs less airtime and collides less**: a sleepy node's
  beats take 212 ms an hour against 292 ms for a leaf's polls and
  answers, and 7 to 8 % of the DATA lost at the relay it was sent to
  had a sleepy beat on the air there, against 15 % with a poll or its
  answer on main. **A node back from the dark is heard later**: after
  114 s at the median against 58 s.
- **Three rules of the brief did not hold as given**, each measured and
  changed (below): a sleepy node's own message handed to the neighbour
  nearest its destination, as any node's is, collapsed the domain at
  twice the rate (62 and 36 % delivered); sent as the origin of its
  search, it took hop places and cost the ring at its cap a third of
  what it carried; and a node back from the dark changed anchor at the
  first heartbeat it heard, two and a half times as often as main's
  leaves change relay. Each now does what a leaf did. The fourth, sleepy
  nodes as the last resort of the search, measured the same as the
  stricter rule the branch keeps.
- **Awake sleepy nodes alone deliver where main delivers nothing**: a
  village of 60 every text, a square kilometre of S4's 714 sleepy nodes
  98 %. The simulator never sleeps a node, and a sleepy node without an
  anchor does not sleep (adopted with D46), so these are awake sleepy
  nodes; a mesh of sleepers without relays was not measured.
- **After the rebase onto D44** the figures hold: at the rate 96.2 %
  listening and 93.1 % on the beat against main's 96.6 %.
- **What delivery does not show**: a sleepy node carries a full node's
  tables, 160 KiB of RAM against a leaf's 0.8 KiB; it listens 9.25 s
  after each beat, 0.24 mA on average against 11 µA for a sleeping
  leaf's poll window; and it is heard only on its own home channel, so
  sleepy nodes route among themselves only where they share one.

## What was built

Every node is the relay of today (`Relay`) with a 4-byte node ID, and
`Config::sleepy` changes what it does. The commits, each with its tests:

| Commit | Story |
|---|---|
| `1f06ba7` | Every node has a 4-byte ID; DATA `dst_hop` and `src_hop`, ACK `from` and `to` carry it; an address is a position and an ID; the heartbeat carries the ID and the position's 4-byte short form; the neighbour table is keyed by ID |
| `8683ad1` | The sleepy heartbeat type, sleepy entries in the neighbour table, the heard table |
| `7d7ce74` | One node type: ATTACH, ATTACHED, DELIVER, the leaf table, the polls and the leaf are gone; sleepy behaviour; the simulator runs nodes placed as leaves sleepy; firmware feature `leaf` becomes `sleepy` |
| `dc7c2da` | The simulator's summary carries the frames lost at the node they were sent to |
| `d0d80ac` | A sleepy node back from the dark keeps its anchor |
| `ef92c48`, `2d4f6f7` | A sleepy node is a next hop only where no other kind is in reach (its message blames the collapse at twice the rate on the search through sleepy nodes; that was wrong, the next commit found the cause) |
| `94c6af9` | A sleepy node hands its own message to its anchor, and tries it again a fade later |

The heartbeat is 9 bytes: the first byte (type 0x1 or 0x2 as today, 0x8
from a sleepy node; `aux` the home channel), the ID (4) and the short
position (4). At SF5 it is 20 payload symbols like today's 8 bytes,
11.84 ms on air, so naming the sender costs nothing. DATA (21 bytes and
the payload) and ACK (13) keep their sizes. Types 0x4, 0x6 and 0x7 are
unused.

A node that is not sleepy does what a relay does today. A sleepy node:

- **Home and address.** A worst-case rotation (195 s) after it first
  hears a node that is not sleepy, by when every such node in range has
  visited its channel, it takes the strongest as its anchor and listens
  on the anchor's home channel; its address is the anchor's position and
  its own ID. It moves to another only when that one has been heard over
  two heartbeats and is 8 dB stronger (spec "Relay choice"), and follows
  its anchor to a new home channel at the anchor's next heartbeat it
  hears. Its averaged RSSI is the neighbour table's, each new frame
  counting half, which varies less than the leaf's mean of two
  heartbeats (σ²/3 against σ²/2), so 8 dB is a little over two standard
  deviations.
- **Beat.** On its home channel only, at a random time in the second
  half of each beat period. The period is half the delivery TTL, 300 s,
  as the poll period of spec section "Attach and polls" (before D46): a holder keeps a sleepy
  node it has heard for two periods, a TTL, so one lost beat does not
  lose it and a node that has gone is kept no longer than its messages.
  The spread keeps two nodes whose beats met once from meeting again.
  After it chooses or changes its anchor it beats at once and once more
  within a heartbeat interval, so that one lost beat does not hide it
  from its new holder for a period.
- **Delivery.** A message that reaches the owner of its position (spec
  "Owner") and is for another node waits in the delivery queue and goes
  to that node as DATA to its ID, answered by the ordinary ACK, while
  the node listens: 9.25 s from each beat, renewed by each message it
  takes. The window is the one of spec section "Leaf receive mode" (before D46) for one
  message, a full send queue of full frames with their ACKs (6.12 s) and
  one hand-over with all its attempts (3.13 s); the node is not told how
  many messages wait, so it listens as long again after each. The hold
  keeps its own time, the delivery TTL, as the owner's delivery queue
  does; on this base no message carries a time of its own.
- **Its own message** goes to its anchor with count 0, as a leaf's did:
  the anchor is the origin of its search, takes it only into its regular
  room and only while not all its neighbours are banned (roadmap D31,
  D34), and counts the sleepy node as tried. Unanswered, it is tried
  again a fade (60 s) later, as a leaf tried its relay. With no live
  neighbour at all, the node keeps it for the delivery TTL and then
  drops it (`NoNeighbour`, the leaf's `NoRelay`).
- **Routing.** A sleepy node is a next hop only for a node that has no
  live neighbour that is not sleepy: sleepy nodes alone, or a pocket the
  others do not reach. The brief's rule, a sleepy node after every
  other, with a message waiting for a banned neighbour that is not
  sleepy rather than go to a sleepy one, delivers the same (below); the
  stricter one needs no rule about bans.
- **Tables.** A heartbeat fills the neighbour table, 64 entries, where a
  sleepy entry expires after two beat periods, gives way to any newcomer
  that is not sleepy and takes only a free or stale sleepy entry's slot;
  and a sleepy heartbeat also fills the heard table, 256 entries (the
  leaf table's size), which keeps the node's home channel, until when it
  listens, and whether a delivery to it went unanswered, and lets the
  weakest go for a stronger newcomer.
- **Channel.** It does not move by the busy estimate, and sweeps the
  channels only while it boots, for its noise floor and the quietest
  channel: asleep it could not. A sleepy node that hears no other kind
  takes the quietest channel, ties to the lowest, so that the sleepy
  nodes of a place without one meet; the others spread ties at random.

A research switch, `Config::sleepy_listens`, has holders send to a
sleepy node whenever they hold a message for it, as if it listened all
the time, which the simulated ones do; it separates what sending on the
beat costs from the rest.

In the firmware the node ID is the low word of the chip's factory
device ID (nRF52840 FICR DEVICEID), the same at every boot; the image
builds as a node, with `--features sleepy` and with `--features
console`. The `sizes` test: a node is 163 840 B, where a relay was
169 160 B (the heard table, 8 KiB, replaces the leaf table, 14 KiB) and
a leaf 776 B. The simulator gives node `i` the ID `(i + 1) × 0x9E3779B1`,
a bijection on 32 bits, so IDs are unique and spread as random ones are.

## What was run

Three simulators, paired by seed: **main** (`8cd70a4`), **beat** (the
prototype as described) and **listen** (the prototype with
`sleepy_listens`). Main's simulator was given the same count of lost
frames with a presence frame on the air, there ATTACH or ATTACHED, which
changes nothing it does. S4 (`scenarios/s4-capacity.toml`) at one, two
and three times the spec "Capacity and sizing" rate, 125 kHz; S4 at the rate with
every tenth relay (27) restarting at 1800 s, to 2700 s; the standard
suite and the loaded suites of D42; a village of 60 sleepy nodes over a
square kilometre outdoors with no other kind; and S4's sleepy nodes
alone, over the whole domain and over a square kilometre of it.

Delivered counts the reachable texts originated from the settle time to
the end less a delivery TTL, for every scenario, so that a text waiting
for a beat at the end is not counted lost. Hand-overs per text count the
first and the last: for main the leaf's DATA, the relays' and the
DELIVER, for the prototype the DATA up to the one to the sleepy node.

## S4

| | main | beat | listen |
|---|---|---|---|
| ×1, delivered, seeds 1, 2 | 96.8, 96.5 % | 93.5, 93.0 % | 96.4, 96.1 % |
| ×1, − main, points | | −3.4 ± 0.1 | −0.4 ± 0.0 |
| ×1, latency p50, p90 | 1.0 s, 65 s | 130 s, 327 s | 1.0 s, 120 s |
| ×1, airtime a run | 24 372 s | 20 616 s | 22 678 s |
| ×1, frames per text | 17.1 | 15.4 | 17.1 |
| ×1, hand-overs per delivered text | 5.93 | 5.89 | 5.96 |
| ×1, not delivered | QueueExpired 871, NoRelay 98 | QueueExpired 1919 | QueueExpired 1044 |
| ×2, delivered, seeds 1, 2 | 96.1, 95.9 % | 89.7, 90.1 % | 92.8, 94.7 % |
| ×2, − main, points | | −6.1 ± 0.3 | −2.2 ± 1.0 |
| ×2, latency p50, p90 | 2.7 s, 84 s | 143 s, 385 s | 4.0 s, 193 s |
| ×2, airtime a run | 51 310 s | 45 629 s | 59 353 s |
| ×2, frames per text | 22.6 | 20.9 | 26.0 |
| ×2, not delivered | QueueExpired 1995, NoRelay 320 | QueueExpired 5710 | QueueExpired 3022, in flight 572 |
| ×3, delivered, seed 1 | 37.7 % | 25.9 % | 32.9 % |
| Anchor or relay changes, ×1 | 8 195 | 7 517 | 8 124 |
| Without anchor or relay at the end, ×1 | 205 | 5 | 5 |

At the rate, what the beat loses is messages waiting at the holder for
a beat that does not come within their TTL: their node was dark, or had
moved to another anchor, whose holder does not hear it. A listening
node takes the same messages at once; its longer tail, 120 s at the 90th
percentile against 65 s, is most likely its own texts that waited a
fade for an anchor that did not answer, where the ban's ladder of D42
offers them again within seconds; that was not measured apart. At twice the rate the listening
prototype's last two 600 s bins fall (seed 1: 0.91 and 0.72 against
main's 0.96 and 0.94), and its frames per text rise by 3.4: the cliff of
the collapse note comes a little sooner. The beat variant does not show
it: its holders send less. At three times the rate all three collapse.

**Back from the dark.** A dark phone or sleepy node is heard again, by
its relay on main and by its anchor here:

| ×1 | main | beat | listen |
|---|---|---|---|
| Median | 58 s | 114 s | 114 s |
| 90th percentile | 220 s | 254 s | 254 s |
| Heard before it went dark again or the run ended, seed 1 | 95.5 % | 92.6 % | 92.4 % |

A sleepy node does not know it was dark: nothing answers its beat. It
is heard at its next beat, at most a period later, where a leaf that
missed its poll attaches again a heartbeat and up to a minute after it
is back (D41).

**A restart.** Every tenth relay (27) restarting at 1800 s, seed 1:

| | main | beat | listen |
|---|---|---|---|
| Delivered | 96.4 % | 92.4 % | 96.3 % |
| Restarted nodes' tables back to half what they held, median and p90 | 420 s, 600 s | 420 s, 480 s | 420 s, 480 s |
| The same to 90 % | 1500 s, 2700 s | 600 s, 2700 s | 540 s, 2700 s |
| Without a relay or anchor, 1740 s and the worst minute after | 226, 336 | 55, 55 | 54, 54 |
| Lost without a trace | 16 | 80 | 19 |

The tables are main's leaf table and the prototype's heard table. A
sleepy node keeps its anchor through the restart and is heard again at
its next beat, so the heard tables refill within a beat period; leaves
on main notice at their next poll and attach again. The beat variant
loses more without a trace: messages waiting in a restarted holder's
delivery queue for a beat.

**Hidden terminals.** Of the DATA lost at the relay it was sent to,
the share with a presence frame on the air at that relay (main ATTACH or
ATTACHED, the prototype a sleepy beat), and with a relay's heartbeat:

| | main | beat | listen |
|---|---|---|---|
| ×1, presence frame | 15.0 % | 8.3 % | 8.0 % |
| ×1, heartbeat | 5.4 % | 5.3 % | 5.2 % |
| ×2, presence frame | 15.7 % | 7.3 % | 7.0 % |
| ×3, presence frame | 18.3 % | 5.9 % | 6.0 % |

A sleepy node beats at a time it chooses, as the leaf-driven keepalive
that D41 rejected did (28 % of lost DATA had one of its ATTACHes on the
air), and about as often; but a beat is one frame that nothing answers,
where a poll is two, and its beats meet lost DATA half as often as
main's polls and answers do. The variant of beating right after a
heartbeat of its anchor was not needed and not built.

## Airtime of presence

Per node and hour, measured at the rate (the analytic figure in brackets):

| | Airtime |
|---|---|
| A sleepy node's beats | 212 ms (16 beats of 11.84 ms at a mean interval of 225 s, 189 ms, and the beats after an anchor change) |
| A leaf's poll exchange on main, ATTACHED and its answer | 292 ms (12 × (10.56 + 11.84) ms = 269 ms, and the ATTACHes after a missed poll) |
| A relay's or a node's heartbeats, not sleepy | 4 063 ms (360 × 11.84 ms = 4 262 ms, less those skipped) |

Summed over S4's 20 000 sleepy nodes the beats take 1.18 s of airtime a
second, 9 % of one channel over the thirteen, against 1.62 s and 12 % for
the polls and answers; the relays' share falls from 127 ms an hour per
leaf to nothing. The listening is the other side: 9.25 s after each beat
is 148 s an hour, 4 % of the time, 0.24 mA on average at the 5.8 mA of
listening, where a sleeping leaf listens 0.58 s per 300 s for its poll,
11 µA. An answer to the beat that said how many messages wait would let
a node with none go back to sleep at once; that is the poll's count
again, from the holder.

## The suites

Delivered of the window, prototype − main paired by seed, and latency at
the 90th percentile:

| Scenario | Seeds | main | beat − main | listen − main | p90 main, beat, listen |
|---|---|---|---|---|---|
| S1 | 10 | 100 % | 0 | 0 | 0.2, 207, 0.2 s |
| S1b | 60 | 100 % | 0 | 0 | 1.2, 215, 1.2 s |
| S2 | 60 | 96.0 % | −3.9 ± 0.7 | −1.7 ± 0.8 | 117, 328, 137 s |
| Corridor | 60 | 100 % | 0 | 0 | 1.5, 221, 1.4 s |
| Mesh, burst | 10 | 100 % | 0 | 0 | 4.6, 206, 4.3 s |
| Mesh, spread | 10 | 100 % | −0.2 ± 0.2 | −0.2 ± 0.2 | 1.1, 218, 1.2 s |
| Ring | 10 | 100 % | −5.8 ± 2.3 | 0 | 5.4, 370, 11 s |
| Corridor at 1 s | 30 | 100 % | 0 | 0 | 7.4, 110, 6.7 s |
| Corridor at 0.5 s | 30 | 100 % | 0 | 0 | 22, 177, 19 s |
| Ring at 1 s | 30 | 100 % | −10.9 ± 1.3 | −0.1 ± 0.1 | 19, 499, 46 s |
| Ring at 2 s | 30 | 100 % | −6.1 ± 1.0 | 0 | 5.8, 393, 7.6 s |
| Mesh, burst ×64 | 30 | 95.8 % | −19.4 ± 2.1 | −1.2 ± 0.6 | 59, 347, 71 s |

What the loaded scenarios carry once every sleepy node has its anchor,
from the settle time plus 300 s, texts a second, prototype against main:

| Scenario | main | beat | listen |
|---|---|---|---|
| Ring | 2.83 | −36.9 % | −3.6 ± 1.8 % |
| Ring at 1 s | 2.59 | −52.8 % | −5.1 ± 2.0 % |
| Ring at 2 s | 2.83 | −33.3 % | −0.9 ± 0.7 % |
| Corridor at 1 s | 1.42 | −9.6 % | +3.8 ± 0.8 % |
| Corridor at 0.5 s | 1.40 | −21.6 % | +3.2 ± 1.4 % |
| Mesh, burst ×64 | 2.33 | −34.5 % | −3.2 ± 1.2 % |

In the window that starts at the settle time the prototype carries 12
to 15 % less in the rings, because a sleepy node chooses its anchor a
rotation after it first hears one, which is up to 600 s into a run whose
relays boot with it, where a leaf attaches a rotation after the boot
listen; texts to a node without an address are not sent. Main's ring
flows send a text every 1 or 2 s to one leaf; on the beat a node takes
what its holder holds when it beats, at most the delivery queue's 41, and
those flows bring 150 to 300 a period. The listening prototype's last
hop is DATA, 12 bytes longer than DELIVER, 6.4 ms more on a 40-byte text
of about 190 ms of airtime, 3 %, which is what the rings at their cap
carry less.

**Awake sleepy nodes alone.** These nodes have no anchor and so, by the
rule adopted with D46, do not sleep; the simulator never sleeps a node
either. The results below show awake sleepy nodes routing among
themselves, not a mesh of sleepers without relays. The village, 60 sleepy nodes outdoors over a
square kilometre, texting at the spec "Capacity and sizing" rate, ten seeds: every text
delivered in both variants, 1.34 hand-overs and 2.9 frames per text,
25 s of airtime a run, mostly straight to the destination, which every
node hears. Main delivers nothing here. S4's 20 000 sleepy nodes alone,
indoors, where two of them hear each other only within about 120 m, did
not finish in two hours of computing (the search among them is slow to
simulate) and are not reported. A square kilometre of them, 714 indoor
sleepy nodes alone, three seeds: 98.0 % delivered on the beat, 98.2 %
listening, 4.5 hand-overs and 15 frames per text, 806 s of airtime a
run, latency 1.1 s at the median and 125 s at the 90th percentile;
what is lost waited for a beat that did not come. The hops between them
are taken as if they listened (see "What this does not show").

## Rules of the brief that did not hold

Each was built as given, measured, and changed; the beat variant at twice
the rate in S4, seeds 1 and 2, main delivering 96.1 and 95.9 %:

| Form | Delivered |
|---|---|
| Own message to the neighbour nearest the destination; sleepy nodes the last resort of every search | 62.1, 35.9 % |
| The same with sleepy nodes only where no other kind is live | 62.6 % (seed 1) |
| Own message to the anchor, retried on the ban's ladder of D42 | 91.1, 90.6 % |
| Own message to the anchor, retried a fade later (the design above) | 89.7, 90.1 % |
| The design with the brief's last resort instead | 89.7, 90.1 % |

The listening variant went from 52.7 and 34.0 % to 90.9 and 92.3 %, then
92.8 and 94.7 %, and with the brief's last resort the same. With the
first hop to the nearest neighbour it was often a faint, far relay:
sleepy nodes sent six times main's leaves' frames (2.8 million against
0.47 million, seed 1), most lost at a relay that was listening and
free, the relays reached their cap from 2400 s, and the domain
collapsed. Which rule decides when a sleepy node is a next hop changed
nothing there: in S4 the brief's rule and the stricter one give the
same runs frame for frame at one and two times the rate, and in the
rings and S2 the same within noise (ring 94.4 against 94.2 % on the
beat, 100 % listening; ring at 1 s 89.3 against 89.1 %; S2 the same
92.2 %).

Two more, at the spec "Capacity and sizing" rate:

- **Sent as the origin of its search with count 1**, as a relay's own
  message is, a sleepy node's text could take hop place 1 at its first
  hop, which overload must not give to fresh work (D31): the ring at its
  cap, listening, seed 1, delivered 96.2 % and carried a third less
  (2 618 texts in the settled phase against main's 3 945); with count 0,
  98.5 %, and with the sender counted as tried, so that its first hop
  does not offer the message back to it, 99.6 % (before the later
  changes).
- **Back from the dark** a sleepy node took the first node it heard as
  its anchor, its own having left the neighbour table while it was dark:
  20 000 anchor changes a run against 8 200 relay changes on main,
  messages to the old address waiting out their TTL, 90.5 % delivered
  (beat) and 94.7 % (listen). Waiting a rotation for its anchor, as at
  boot: 7 600 and 8 300 changes, 93.2 and 96.6 %.

## Costs

- **RAM.** A sleepy node is a full node: 160 KiB of the board's 255 KiB
  for the application, where a leaf needs 0.8 KiB. The same chip has it,
  but nothing else fits beside it.
- **Battery.** 9.25 s of listening after each beat, 0.24 mA on average,
  where a sleeping leaf's poll window costs 11 µA. Telling the node that
  nothing waits needs a frame from the holder.
- **Latency.** On the beat a message waits for its node a mean of about
  two minutes, at most a period.
- **One channel.** A sleepy node beats only on its home, its anchor's,
  so only nodes there hear it. Sleepy nodes route among themselves only
  where they share a channel: where they hear no other kind, all of them
  take the lowest quiet one.
- **Delivery throughput to one node.** On the beat, a node takes what
  its holder holds: at most 41 messages a beat period.

## What this does not show

- No simulated node sleeps: a sleepy node listens all the time, and the
  holder's sending on the beat is what makes the beat variant a sleeper's.
  A sleepy node that forwards, in a place without other nodes, would in
  truth be reached only in its windows; the village delivers straight to
  most destinations, and its latency is that of a listening node.
- A sleepy node's tables in truth fill only while it listens; here it
  hears every heartbeat on its channel. A sleeping one would need a
  listening rotation now and then, as the hybrid leaf of
  spec section "Leaf receive mode" (before D46).
- Main's leaves listen all the time and take a DELIVER at once, which a
  sleeping leaf would not: main's figures are those of the listening
  variant's kind, and its latency is not what a sleeping leaf would see.
- Two seeds of S4 at one and two times the rate, one at three and one of
  the restart. The rejected forms at twice the rate were measured with
  the beat variant; the listening one where said.
- Main moved meanwhile to `994e1ce`: DATA carries the time a message has
  left. The branch was later rebased onto it ("After the rebase onto
  D44").

## Recommendation

One node type is cheap where the sleepy node listens, and that is the
honest comparison with main, whose simulated leaves listen too. What it
costs is in what a sleeper needs: on the beat the network loses three
points at the spec "Capacity and sizing" rate and two minutes of latency, a node is heard
back from the dark a minute later, and it pays 160 KiB of RAM and twenty
times the listening of a leaf. The simplification is worth adopting only
together with an answer to a beat, a frame from the holder that says
whether anything waits and lets the node sleep at once, which brings the
poll back in another form. Without it, keep the two roles. If it is
adopted, the three rules above that did not hold as given are part of it.

## After the rebase onto D44

**2026-10-17.** D46 was adopted. The branch was rebased onto main
`994e1ce`, which carries D44, the spec was rewritten for one node type,
and the measurements above were repeated against `994e1ce`.

What the rebase and the review changed:

- **Time left.** A sleepy node's own message starts with the TTL when
  its application gives it and, like any held message, carries what is
  left at each transmission, so its waits for its anchor and the fade
  retry count against it; one alone waits for a neighbour until its time
  runs out (`Expired`), where it was kept a TTL and dropped as
  `NoNeighbour`. The holder's delivery queue keeps its own 600 s, and the
  DATA it finally sends to the node carries the time the message had
  when it arrived. DATA's header is 22 bytes, as on main.
- **A sleepy node sleeps only while it has an anchor.** One that hears
  no relay has nobody to hold its messages, so it listens all the time
  and acts as a relay: it heartbeats at a relay's interval on the
  rotating channel, still with the sleepy type so that no other sleepy
  node takes it as its anchor, sweeps, and chooses and moves its home
  by the busy estimate (spec "Sleepy nodes alone"). Built first with the
  home kept on the lowest quiet channel, as sleepy nodes alone had it,
  the village's sixty nodes heartbeating as relays on one channel
  overloaded it: split copies of messages circulated until their time
  ran out, 1 300 frames per text against 3. Spread over the channels as
  relays are, the village delivers every text again.
- **Position.** A node's position is its GPS fix unless the fix fails
  the plausibility checks or the app sets a map point; a relay's no
  longer freezes at its first fix (spec "Position").

Measured as before, three simulators paired by seed: **main**
(`994e1ce`, with the same count of lost frames that had a presence frame
on the air), **beat** and **listen**.

**S4**, seeds 1 and 2:

| | main | beat | listen |
|---|---|---|---|
| ×1, delivered, seeds 1, 2 | 96.7, 96.5 % | 93.3, 92.8 % | 96.2, 96.2 % |
| ×1, − main, points | | −3.5 ± 0.1 | −0.4 ± 0.1 |
| ×1, latency p50, p90 | 1.0 s, 65 s | 129 s, 323 s | 1.0 s, 122 s |
| ×1, airtime a run | 24 449 s | 26 944 s | 28 909 s |
| ×1, frames per text | 17.1 | 15.3 | 17.0 |
| ×1, not delivered | QueueExpired 892, NoRelay 94 | QueueExpired 1924, Expired 77 | QueueExpired 1025, Expired 72 |
| ×2, delivered, seeds 1, 2 | 95.8, 95.7 % | 90.5, 89.2 % | 94.1, 90.6 % |
| ×2, − main, points | | −5.9 ± 0.6 | −3.4 ± 1.7 |
| ×2, latency p50, p90 | 2.9 s, 95 s | 143 s, 384 s | 4.8 s, 215 s |
| ×2, not delivered | QueueExpired 2069, NoRelay 335, Expired 20 | QueueExpired 5588, Expired 222 | QueueExpired 3293, Expired 966 |
| Back from the dark, ×1, p50, p90 | 58 s, 219 s | 104 s, 248 s | 103 s, 249 s |
| Lost DATA with a presence frame on the air, ×1 | 15.0 % | 8.9 % | 8.6 % |
| Presence airtime per sleepy node or leaf, ×1 | 292 ms/h | 467 ms/h | 467 ms/h |

At the spec "Capacity and sizing" rate the figures are those measured on `8cd70a4`
within a tenth of a point. At twice the rate the listening variant's
seed 2 falls further, 90.6 %, with 966 messages whose time ran out on
the way: past the cliff the messages that wander end by time, as D44
intends. The presence airtime has more than doubled, and so has the
run's airtime against main's: every sleepy node now heartbeats as a
relay from its boot until it has an anchor, a boot listen and a
rotation, and all 20 000 boot together in S4. After that its beats cost
what they did. A city's phones do not start at once, so the steady
figure, 212 ms an hour, is the one that matters; a mass start, after a
blackout for example, costs a few minutes of heavy heartbeat load.

**The suites**, delivered of the window, prototype − main paired by
seed, and latency at the 90th percentile:

| Scenario | Seeds | main | beat − main | listen − main | p90 main, beat, listen |
|---|---|---|---|---|---|
| S1 | 10 | 100 % | 0 | 0 | 0.2, 215, 0.2 s |
| S1b | 60 | 100 % | 0 | 0 | 1.2, 218, 1.3 s |
| S2 | 60 | 96.0 % | −4.7 ± 0.7 | −0.8 ± 0.6 | 117, 325, 155 s |
| Corridor | 60 | 100 % | 0 | 0 | 1.5, 217, 1.4 s |
| Mesh, burst | 10 | 100 % | 0 | 0 | 4.6, 201, 4.8 s |
| Mesh, spread | 10 | 100 % | −0.5 ± 0.3 | −0.2 ± 0.2 | 1.1, 212, 1.2 s |
| Ring | 10 | 100 % | −9.7 ± 2.6 | 0 | 5.4, 393, 5.7 s |
| Corridor at 1 s | 30 | 100 % | −0.4 ± 0.4 | 0 | 7.4, 139, 7.1 s |
| Corridor at 0.5 s | 30 | 100 % | 0 | 0 | 22, 166, 19 s |
| Ring at 1 s | 30 | 100 % | −10.1 ± 1.3 | 0 | 19, 513, 19 s |
| Ring at 2 s | 30 | 100 % | −7.6 ± 1.3 | 0 | 5.8, 385, 5.9 s |
| Mesh, burst ×64 | 30 | 95.3 % | −17.4 ± 1.8 | −0.9 ± 0.7 | 46, 339, 51 s |

What the loaded scenarios carry once every sleepy node has its anchor,
texts a second, against main:

| Scenario | main | beat | listen |
|---|---|---|---|
| Ring | 2.83 | −42.5 % | −2.1 ± 0.5 % |
| Ring at 1 s | 2.59 | −54.6 % | −1.0 ± 0.7 % |
| Ring at 2 s | 2.83 | −35.6 % | −0.6 ± 0.4 % |
| Corridor at 1 s | 1.42 | −9.5 % | +2.8 ± 0.8 % |
| Corridor at 0.5 s | 1.40 | −18.8 % | +2.9 ± 1.3 % |
| Mesh, burst ×64 | 2.33 | −31.4 % | −2.5 ± 1.3 % |

As before: listening, the suites deliver what main does; on the beat,
flows to one node faster than its window after a beat carries lose a
third to a half of what they carry.

**Awake sleepy nodes alone.** The village, 60 sleepy nodes with no
relay, ten seeds, delivers every text in both variants, 2.3 hand-overs
and 3.1 frames a text, 316 s of airtime a run; a square kilometre of
S4's indoor sleepy nodes alone, 714 of them, three seeds, delivers 98.0 %
on the beat and 98.2 % listening. These are awake sleepy nodes: none of
them has an anchor, so by the rule above none sleeps, and the simulator
never sleeps a node anyway. They show that sleepy nodes without relays
route among themselves when awake, not that a mesh of sleepers without
relays works; that was not measured.

**Firmware.** The image, relay or sleepy, is 301 KiB of flash and uses
171 KiB of RAM before the stack, a 162.3 KiB node; its deepest stack
frame is 23 KiB (`bark-fw/README.md`).

**Decided 2026-10-17: adopted** (roadmap D46). The next step is a bit in
DATA that says more follows, so that a node after its beat listens only
briefly unless told more is coming, and holders that send a node that
has just beaten its messages first.
