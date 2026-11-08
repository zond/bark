# Firmware updates over the mesh: the transport (2026-11-07)

**Status:** research note, 2026-11-07. Builds the transport that
`mesh-update-2026-10-25.md` studied, as decided with the user on
2026-11-07; spec "Updates over the mesh" holds the rules, and roadmap
D146 the decision. Spec references are to `protocol.md` by section
name. Built: the frames, the core's side and the simulator's driver
side, without bytes; on 2026-11-08 the chain and the sleepy side
("The chain and the sleepy side", below), then heartbeat sections in
place of the announcement and the request, a sweep and a shared pace
("Heartbeat sections and a sweep", below, roadmap D150). Not built: the firmware's
side of the transport (the policy, the signature check and the
install, and the driver's answers; the code and the pieces store are
`bark-image`'s `code.rs` and `pieces.rs`, roadmap D149).

## What was built

**Frames** (spec "Packet formats"). DATA's `aux` 13 to 63, which no home
channel is, became frames from node to node, one value a kind, at the
user's choice the same day; 13 to 15 are the update's, 16 to 63 kept
for later kinds. They keep the first byte and the format byte, and lay
out the rest for their kind: none has a hop, a destination or a count,
and the piece needs every byte.

```
ANNOUNCE  first(1) format(1) from(6) home(1) version(4) blocks(1) last(1)
          key(1) image(32)                                    = 48, 32.3 ms
REQUEST   first(1) format(1) to(6) channel(1) version(4) image(4) block(1)
          lacks(1)                                            = 19, 18.2 ms
PIECE     first(1) format(1) image(4) block(1) index(1) piece(247)
                                                              = 255, 138.6 ms
```

The image is named by the SHA-256 of its signed header; the request and
the piece carry its first 4 bytes. Block 0 is the block table, one
piece up to 26 blocks of image; the image follows in blocks of 64
pieces of 247 bytes. Today's node image, 387 540 B (main `b6d14cc`,
`cargo build --release` in `bark-fw/`), is the table and 25 blocks, 24
of 64 pieces and one of 33: 1570 pieces. With this branch the image is
398 140 B, 10 600 B more, and its static memory 512 B more (`llvm-size`,
2026-11-08).

Against the note: the announcement carries no signatures, which travel
in the image's header and are checked by the firmware once the image is
whole; it carries the holder and its home channel, where a request to
it goes, and the last block's pieces, so that the core knows each
block's size. The request names one holder, the block and how many of
its pieces it lacks, since blocks are gathered in order; no bitmap.

**Pace and priority.** As the user chose: at most one update frame a
channel in 13.86 s (a full frame's 138.6 ms a hundred times, 1 % of a
channel), on at most two channels in any 13.86 s, last in the node's
radio jobs, paid from the bucket with the reply reserve left, as a
forward, and checked as a heartbeat is, up to three checks then a pace
lost. `bark_proto::update::PASS` = 9 is how many frames of the send
queue may go before an update frame that is due: the bucket's fill over
one pace, 9.83 full frames, rounded, less the update frame itself, so
that a node at its cap sends an update frame about every 14 s, as an
idle one does. It is counted on hand-overs that go while an update
frame is due (`relay/update.rs`, `update_passed`).

**What the core keeps** (`node/relay/update.rs`, `Spread`): the image
held; the image gathered, its holder, the first block lacked, and a
bitmap of the pieces handed for that block and the four after it
(5 × 32 B); per served channel (two) four blocks asked for, each with
its next piece and the pieces still to send, and the block begun; the
last two channels sent on and when; the announcement timers. 512 B; the
`Relay` is 188 128 B against 187 608 B (`core-interface.md`, Memory).

**Interface** (`core-interface.md`, Application). From the driver:
`Holds(image)` at boot and after a change, `Gather { image, from }` as
its policy's answer, `Block { index, ok }` when a block decodes or
fails its hash. To the driver: `Offered(image)`, `StorePiece(piece)`
and `Serve(frame)`. The driver holds the bytes: the core sends a piece
frame with its header written and the driver fills the 247 bytes when
it transmits, so the core never copies a piece it serves and a coded
piece is made only when it goes. Pieces received are handed by value.
The firmware's driver answers nothing yet: it never says it holds an
image, so its core never serves, and it drops what is offered.

**Rules as built** differ from the design of the morning in five
places, each found in the simulator (below, "How it was tuned"):

- a holder sends one block until the pieces bought for it are spent,
  then the highest block asked for, not the lowest missing block;
- a request buys 1.55 times the pieces it says it lacks;
- a node takes the pieces of the first block it lacks and of the four
  after it, not of the first alone: 320 pieces at most, within the 352
  of the flash;
- the announcements and the requests go before the pieces;
- a holder that hears another's piece of its image on its home channel
  sends none there for two paces, whatever the block, and a node keeps
  a holder of its own channel; the design said only that holders hold
  back from the same block.

## How it was tuned

Each change came from a district run in which no relay but the first
came to hold the image (`bark-sim`, `district.toml` with `[update]`):

1. **Requests at once.** Every relay that heard the first announcement
   asked in the same instant, and the requests collided. A request now
   waits a random time within a pace; so does the announcement of a
   node that has just come to hold the image, since the nodes of a
   channel finish together.
2. **The block of the latest request.** On one channel some nodes
   always lag, losing more frames, and others join late: people's nodes
   that turn relay on USB, and nodes that move their home. Followed
   request by request, the holder turned back to block 0 every few
   minutes, sent its first pieces again each time, and in 35 simulated
   hours no node finished. Every piece of a block once per request was
   255 pieces for a block of 64. A request now buys 1.55 times what it
   lacks, a block is sent until those are spent, and a block asked for
   again starts at a random piece.
3. **The lowest block first.** The late nodes held everyone at the first
   blocks: the nodes ahead stopped at block 18 of 26 for ten simulated
   hours. The highest block first brings the nodes nearest to holding
   the image to hold it, and each serves two channels more.
4. **One block at a time.** A node behind could use nothing sent for
   the nodes ahead. It now takes pieces of five blocks, as many as the
   flash holds whole.
5. **Announcements behind pieces.** Pieces were first in the node's
   own order and always due on the two channels a holder serves, so the
   hourly announcements never went: in 18 simulated hours the first
   holder announced three times, and only the 30 relays that heard the
   first one ever gathered. Then announcements and requests that are
   due blocked every piece behind them, and a holder never served its
   second channel. Now they go first, and the pieces go on whichever
   channel the pace frees first. The core's wakeup and its start chose
   the next frame differently for a while, and the simulator spun; the
   choice now depends on the present only through the times the frames
   want.
6. **Several holders on a channel.** Each holder keeps its own pace, so
   once 70 relays held the image the update's frames took 7 % of each
   channel's time, past where the network collapses
   (`collapse-research-2026-10-16.md`: about three times the design
   rate, 3 to 4 %). One holder a channel at a time keeps it near 1 %.

The test medium found none of this: its four relays all hear each
other, lose nothing, and none joins late.

## Results

Runs of the core as committed in `291cf59`, seed 1, 36 hours after the
image is given. The district is `scenarios/district.toml`, 2.8 km², about 1080 relays of which 27 are
placed as relays and the rest people's nodes on USB, and about 950
sleepy nodes; the city is `scenarios/dense-power.toml`, ten times as
large, from which the district is cut (`district-2026-11-04.md`).
Each is run with `[update]` appended, which gives the image to the relay
placed as a relay nearest the centre once the run has settled, at the
design rate and at twice it (copies made as `district-2026-11-04.md`,
"The scenario", says), and without it for the texts' comparison:

```
k=1; r=$(echo "0.9583333*$k" | bc -l | sed 's/^\./0./')
sed -e "s/^name = \"district\"/name = \"district-x$k\"/" \
    -e "s/^per_leaf_per_hour = 0.9583333/per_leaf_per_hour = $r/" \
    scenarios/district.toml > district-x$k.toml
(cat district-x$k.toml; printf '\n[update]\n') > district-x$k-update.toml
bark-sim district-x$k-update.toml --duration-s 129600 \
    --update district-x$k-update.upd.csv --messages district-x$k-update.msgs.csv \
    --json district-x$k-update.json
```

The city's copies are made the same way from `dense-power.toml`.

### How far it spread

Relays that hold the image, of those that are relays at the time, from
the moment the first relay was given it (`--update` series; the
district has 1076 to 1085 relays at a time, 27 of them placed as
relays):

| hours | ×1 relays | ×1 placed | ×2 relays | ×2 placed |
|---|---|---|---|---|
| 8 | 1 | 1 | 1 | 1 |
| 10 | 6 | 3 | 7 | 3 |
| 20 | 12 | 4 | 15 | 3 |
| 24 | 20 | 6 | 24 | 4 |
| 30 | 30 | 7 | 31 | 6 |
| 34 | 37 | 10 | 56 | 9 |
| 35.75, the end | 44 (4.1 %) | 10 | 65 (6.0 %) | 10 |

Neither run reached 50 % of the relays, so the times to 50 %, 90 % and
all are not measured. The holders come in generations about ten hours
apart, the time one channel's nodes take to gather the whole image at
the pace (9.4 hours, spec "Updates over the mesh", What it costs), and
their number grew by about half every six hours from the second
generation on. At that rate 50 % of the district's relays would hold
the image after about 70 hours, three days, and nearly all a day or so
later; that is an extrapolation. Longer runs were started and stopped:
the simulator slows as holders multiply, since every piece reaches
every node on its channel, and 48 simulated hours had not finished
after 80 minutes where 36 took 26.

At twice the rate the image spread somewhat further, 65 relays against
44; with one seed each, that is within what a seed changes, not an
effect of the rate.

### What it cost the channels and the texts

| | ×1 without | ×1 with | ×2 without | ×2 with |
|---|---|---|---|---|
| channel busy, mean of the relays' channels over the run (summary `channels`) | 1.27 % | 1.50 % | 1.69 % | 1.92 % |
| the busiest channel | 1.62 % | 2.07 % | 2.53 % | 2.57 % |
| the relays' own busy estimate of their home, mean at the end (`busy_home_mean`) | | 0.50 % | | 0.62 % |
| the update's piece airtime in the last 10 minutes, whole district | | 268 s | | 355 s |
| texts delivered, 0–12 h / 12–24 h / 24–36 h | 99.68 / 99.83 / 99.75 % | 99.65 / 99.84 / 99.68 % | 99.66 / 99.81 / 99.73 % | 99.66 / 99.78 / 99.67 % |
| latency p50 / p90, 24–36 h | 1.0 / 16.1 s | 1.0 / 16.1 s | 1.0 / 16.0 s | 1.0 / 16.1 s |
| latency p99, 24–36 h | 608 s | 609 s | 626 s | 629 s |
| refused at the gate | 0 | 0 | 0 | 0 |
| texts whose application waited for its node (pushback) | 43 | 44 | 172 | 220 |
| the busiest node's airtime | 0.88 % | 2.64 % | 1.59 % | 3.05 % |

The busy share of a channel rises by 0.23 points at both rates, a
fifth of what the design rate puts there, while 4 to 6 % of the relays
hold the image. The network collapses near three times the design rate
(`collapse-research-2026-10-16.md`), 3 to 4 %, and the channels stay
well clear of it. The texts do not notice: delivery and latency are within the run's
own spread, and nothing is refused. More applications waited for their
node at twice the rate (220 against 172), the nine-frame rule letting
an update frame in at nodes at their cap. The busiest node's airtime
triples: a holder that serves two channels sends about 2 % of the time.

Before the rule of one holder a channel (`roadmap.md` D146; the run on
`6a41de2`), the same district at the design rate had 73 relays holding
the image after 35 hours, twice as many, while the update's pieces took
567 s of every 600 in the district, 7 % of each channel's time; the
texts still showed no change (99.64 / 99.82 / 99.67 % delivered).

### The city

The city runs, `dense-power.toml` with `[update]` at the design rate and
at twice it, did not finish: the simulator slows as holders multiply,
since every piece reaches every node on its channel, and 36 simulated
hours of the city had not finished after four hours. From the district,
the city's first generation comes at the same ten hours, and the city
has ten times the relays, so it takes about three generations more than
the district, about 30 hours more, to the same share.

### Sleepy nodes

Not measured: they take no part yet.

## Against the note of 2026-10-25

The note estimated 6 to 10 hours for a district and about a day for the
city. In the simulator 4 % of the district's relays hold it after 36
hours, and half of them would after about three days. The difference has
four causes, in order of weight:

1. **No chain.** The note let a relay serve a block as soon as it had
   decoded it, so a chain of H hops took about (blocks + H − 1) block
   times. Here only a node that holds the whole image announces and
   serves, so every generation of holders costs the whole image's time
   on one channel: 2430 pieces at the pace, 9.4 hours, and in the runs
   the first relays besides the first holder held it after 9.6 hours.
2. **A larger image and more pieces per block.** Today's image is
   387 540 B, not the 250 kB the note planned for: 26 blocks, not 17. A
   request buys 1.55 times its pieces, the note's worst case, where the
   note sent 1.28 times for 20 receivers at a tenth lost.
3. **Few senders.** A holder serves two channels and, since the rule of
   one holder a channel, holders at home on one channel take turns, so
   the number of channels being served grows only as fast as holders
   appear on new channels; a channel learns of a holder elsewhere from
   its announcement on one other channel an hour, stepping through
   twelve.
4. **Nodes that begin late.** A fifth of the people's nodes switch
   between USB and battery, so relays keep turning up that lack the
   whole image, and the nodes a holder serves are never all at one
   block.

The pace is the user's choice and is kept: the update never takes more
than 1 % of a channel per holder, busy or not.

## The chain and the sleepy side (2026-11-08)

The user decided on 2026-11-08 to build both before D146 is merged.

**What changed** (spec "Updates over the mesh"):

- The announcement carries how many blocks its sender holds, from the
  first (`held`, one byte: 49 bytes, 33.6 ms). A relay serves every
  block it has decoded and whose hash passed, before it holds the whole
  image. It announces on its home channel a random time within a pace
  after its count grows, and from its first block hourly on one other
  channel in turn.
- A node asks a holder that holds the block it lacks, one at home on
  its own channel first, then the one heard last. A holder asked for a
  block it does not hold is silent; the node asks when it hears that
  one, or another, holds more. A block that fails its hash is gathered
  again and not counted held.
- The request's channel byte carries the asker's sleep in its top bit.
  A sleepy node with an anchor asks its anchor alone, at once. The
  anchor sends one frame right after each slot heartbeat on its home
  channel: a piece while sleepy nodes asked, else its announcement once
  an hour. A sleepy node listens 143.6 ms after its anchor's slot
  heartbeat while it gathers, 38.6 ms otherwise.
- The silence of a channel where another holder sends holds for every
  holder, partial ones too.

**How it was tuned.**

7. **Collisions after growth.** The two relays behind a first holder
   finished each block together and asked for the next a pace after
   their own growth announcements, in the same instant, and again ten
   minutes later, every time: the requests collided and never reached
   the holder. Growth announcements and quiet re-requests now wait a
   random time within a pace.
8. **An announcement after every slot heartbeat.** Every anchor that
   held a block announced after each of its slot heartbeats, 8 190 s of
   announcements in the district's first 12 hours. Now once an hour.
9. **Pieces after every slot heartbeat.** Anchors sent their sleepy
   nodes a piece after every slot heartbeat whatever their channel
   carried; the frame now keeps the channel's silence.

**Results.** The district at the design rate, seed 1, 36 hours, on
`6cea6c1`, with and
without the update, the image packed at 387 796 B (26 blocks):

| hours | relays holding | gathering (relays and sleepy) | first block they lack, mean / most |
|---|---|---|---|
| 10 | 3 | 2024 | 3.4 / 19 |
| 20 | 4 | 2023 | 6.6 / 23 |
| 30 | 9 | 2018 | 9.9 / 25 |
| 35 | 14 (1.3 %) | 2013 | 11.2 / 25 |

No sleepy node came to hold the image; at one piece a rotation of its
anchor it needs 3.7 days (spec, "What it costs a sleepy node").

| | without | with the chain |
|---|---|---|
| channel busy, mean over the run (summary `channels`) | 1.27 % | 2.04 % |
| the busiest channel | 1.61 % | 2.29 % |
| the update's pieces, whole district, a 10-minute sample from 6 h on | | 900 to 1 080 s |
| texts delivered, 0–12 / 12–24 / 24–36 h | 99.62 / 99.84 / 99.68 % | 99.60 / 99.79 / 99.67 % |
| latency p50 / p90 / p99, 24–36 h | 0.9 / 15.9 / 620 s | 1.0 / 16.2 / 628 s |
| refused at the gate | 0 | 0 |
| texts whose application waited for its node | 37 | 39 |
| the busiest node's airtime | 0.88 % | 1.65 % |

```
bark-sim district-x1-update.toml --duration-s 129600 --update c-upd.upd.csv \
    --messages c-upd.msgs.csv --json c-upd.json
bark-sim district-x1.toml --duration-s 129600 --messages c-base.msgs.csv --json c-base.json
```

**What it says.** The chain spreads blocks to every relay at once
rather than the whole image to a few: after 35 hours every relay that
gathers holds 11 blocks on average, and 14 relays hold all 26, where
without the chain 44 held all and the rest held almost none. The mass
gains about a third of a block an hour, so half the relays would hold
the image after about 80 hours; that is an extrapolation, and no worse
than the three days estimated without the chain. The cost is the
channels': the update's pieces come to about 1.6 s of air a second
across the district, from about a thousand partial holders, and the
channels the relays sit on are busy 2.04 % of the time, 0.77 points
more than without, where the rule without the chain added 0.23. That
is still below the 3 to 4 % at which the network collapses
(`collapse-research-2026-10-16.md`), and the texts do not notice.

Two variants, 12 hours each, the same district, not committed:

| variant | pieces in 12 h | relays holding at 10 h | gathering, first block lacked, mean |
|---|---|---|---|
| as committed | 55 078 s | 3 | 3.7 of 2024 |
| sleepy nodes take no part | 39 168 s | 2 | 6.9 of 1086 |
| a partial holder serves its own channel only, and sleepy nodes only whole holders | 9 352 s | 2 | 1.3 of 2017 |

Sleepy nodes gathering a block a rotation behind their anchors cost
the relays a third of the update's air and half their progress. A
partial holder that serves only its own channel keeps the air to a
sixth, but behind a sender on the same channel it is silent, so the
chain hardly moves. Neither changes how soon the first relays hold the
image, about ten hours, a whole image at the pace on one channel.

**The image on the merged branch** (2026-11-08, `llvm-size`, the
node image as `bark-fw/README.md` builds it): main `ea1141f`, 420 552
B; this branch merged with it, 434 392 B, 13 840 B more, and 552 B
more static memory. Packed behind its header it is 420 808 B, 1 704
pieces in 27 blocks, the last of 40, and its table of 248 bytes is two
pieces; 28 blocks in all. The runs above used the 387 796 B packed
of 2026-11-07.

## Heartbeat sections and a sweep (2026-11-08)

The user changed the shape on 2026-11-08, in three steps the same day:
the holder streams and every block is kept (roadmap D150's store); then
the announcement and the request go, and the heartbeat tells instead;
then the stream sweeps the blocks in order, and the senders on a
channel share the pace instead of one keeping it.

**What changed** (spec "Updates over the mesh", "Packet formats"):

- A heartbeat may carry a 17-byte section after its first 13: the
  version the node runs, the newest image it holds or gathers (version,
  first 4 bytes of the name, key, blocks, the last block's pieces), the
  blocks it holds from the first and the pieces it holds of the next.
  A relay's heartbeats carry it for the rotation after the version it
  runs, its newest image or its blocks held change, and one rotation an
  hour; a sleepy node's beats for one beat after a change and one an
  hour. The neighbour table
  keeps each neighbour's (24 B an entry, 104 → 128 B). The announcement
  and the request are gone, and `aux` 13 and 14 are free.
- The brief asked for 15 bytes. The core needs the image's blocks and
  its last block's pieces before the first piece, to know how many
  pieces give back each block and to set up the firmware's lanes, so
  the section is 17 bytes: 30 bytes and 23.36 ms a heartbeat, one block
  of symbols, 1.28 ms, more than 15 would take.
- A relay streams where a neighbour's section lacks a block it holds,
  on at most two channels, its home first. On another channel it
  streams only the blocks past those a relay at home there holds, and
  to a gatherer only if it is the best holder it hears for it (below,
  11 and 14). The stream sweeps the blocks
  in order: of each, what the neighbour there furthest behind on it
  lacks, over 1 less the measured loss, then the next block a neighbour
  lacks, and from the first again while any lacks something.
- On its home channel a sender that heard k pieces from others since
  its last there sends its next k + 1 paces after its last.
- A sleepy node's beats carry its section; its anchor's frame after
  each slot heartbeat is the next piece of its stream on that channel,
  and the node listens for it while its anchor's section says it holds
  a block the node lacks. The 38.6 ms listen for an hourly announcement
  is gone: the slot heartbeat it wakes for carries the anchor's
  section.
- The core keeps 1 856 B (`Spread`); the `Relay` is 191 008 B.

**How it was tuned.**

10. **The loss margin, first.** The simulator counts, for each piece
    sent, the neighbours of its sender whose section said they lacked
    it, and how many of them heard it (`bark-sim/src/update.rs`,
    `piece_sent`, `piece_heard`). In the district at the design rate
    over the first 6 hours of an update, with every relay streaming on
    two channels and the section in every heartbeat, relays missed
    40.4 % of the pieces sent to them (921 421 of 1 545 703 heard).
    The margin was set to 40 % for the steps below, then measured again
    (15).
11. **Streams from elsewhere.** A node hears pieces only on its home
    channel, so it can share the pace only there. On the second channel
    it streams on, every relay sent a piece a pace unheard by the
    others: in the first 6 hours pieces were 55 % of all the district's
    airtime, 92 400 s, and the channels were busy 4.2 % of the time on
    average, where the network collapses at 3 to 4 %
    (`collapse-research-2026-10-16.md`). With the relays at home on a
    channel serving its blocks first, and the others only the blocks
    past those, the pieces took 69 000 s and the channels 3.5 %, the
    relays as far along (first block lacked 4.9 against 4.6 on average
    at 5.25 h). That run had the 40 % margin and the one before 20 %,
    so the cut is if anything larger than it shows.
12. **Spin.** The core asked to be woken for a slot frame held back by
    a listen-before-talk wait, and for a channel whose only neighbour
    there held a block's pieces but had not yet said it decoded it; the
    test medium's harness caught both as a wakeup that never moved on.
13. **The first sweep began at block 1.** A sweep's state started at
    block 0 with nothing to send, so the first block chosen was the
    one after it; it now starts before the first.

14. **The best holder for a foreign channel.** As the user put it: a
    relay hears only its home channel, so only the relays at home on a
    channel can share its pace, and the sweep there is theirs. Elsewhere
    a relay streams to a gatherer only if it is the best holder it
    hears for it, the most blocks from the first, then the lowest ID.
    Taken as it stands, the rule stalled a chain in the tests: a relay
    stood back for a better holder that did not reach the gatherer
    behind it. The relay now stands back only for a better holder no
    farther from the gatherer than itself, by the positions in its
    table. In the 6-hour run the pieces took 56 900 s and the channels
    3.15 %, the relays a little behind (first block lacked 3.8 on
    average at 5.25 h). Foreign senders still overlap: of the pieces
    from another channel than the sender's home to a relay that lacked
    them, 64.0 % came within a pace of one from another such sender to
    the same relay. Two holders that each think the other is out of
    the gatherer's reach both send.
15. **Sections on change.** A relay's section rides the rotation after
    what it tells changes, and one an hour; the count of pieces of the
    next block goes as it stands, and the margin covers it being
    stale. The sections' airtime in 6 hours fell from 22 430 s to
    1 505 s, the channels from 3.15 to 2.80 %, and the relays went
    faster (first block lacked 5.4 on average at 5.25 h, the furthest
    11), the air freed carrying more pieces. Relays then missed 20.5 %
    of the pieces sent to them that they lacked (499 902 of 628 456
    heard), and sleepy nodes 12.8 % of the pieces their anchor sent
    right after its slot heartbeat (22 064 of 25 299), the only ones
    they listen for. `PIECE_LOSS_PERCENT` is 20: a sweep sends 1.25
    times what the furthest behind lacks, 2 133 paces, 8.2 hours, for
    today's image. With no loss the neighbours' sections end each block
    early, and the test medium's hop takes about as many paces as the
    image has pieces. Foreign senders overlapped on 47.6 % of their
    pieces.
16. **A gate on measured occupancy**, no piece on a channel whose busy
    estimate is above 2 %, was built and dropped the same day at the
    user's word: even a saturated mesh must update, so pieces yield to
    texts only by the bucket's reply reserve, their place last among
    the radio jobs after nine frames of the send queue, and
    listen-before-talk with a pace's wait.

The 6-hour runs, the district at the design rate, seed 1:

| | pieces' airtime | of all airtime | sections' airtime | channels busy, mean | texts delivered | first block lacked, mean / most at 5.25 h |
|---|---|---|---|---|---|---|
| every relay on two channels (10) | 92 400 s | 55 % | 22 400 s | 4.2 % | 99.0 % | 4.6 / 8 |
| a channel's blocks to its own (11) | 69 000 s | 48 % | 22 400 s | 3.5 % | 99.2 % | 4.9 / 10 |
| the best holder (14) | 56 900 s | 43 % | 22 400 s | 3.15 % | 99.1 % | 3.8 / 9 |
| sections on change (15) | 60 600 s | 53 % | 1 500 s | 2.80 % | 99.0 % | 5.4 / 11 |

Texts delivered here are of all originated in the 6 hours, the last
minutes' still in flight counted as not delivered.

**Results.** The district at the design rate, seed 1, 36 hours, on
`d78ecaa`, with the update and without it, the image packed at
420 808 B (28 blocks):

```
bark-sim district-x1-update.toml --duration-s 129600 --update z36.upd.csv \
    --messages z36.msgs.csv
bark-sim district-x1.toml --duration-s 129600 --messages b36.msgs.csv
```

Relays that hold the image, from the moment the first relay was given
it: 50 % after 13.3 hours, 90 % after 14.5 hours, all 1 083 after 28.2
hours; the 27 placed as relays all after 13.2 hours. The sweeps keep
the relays in step, so they finish nearly together: at 12.3 hours the
gathering relays lacked block 12 on average, and an hour later 932 of
1 078 held the whole image. The last ones are people's nodes that
turned relay late.

| | without | with the update |
|---|---|---|
| channel busy, mean over the run (summary `channels`) | 1.28 % | 2.13 % |
| the busiest channel | 1.6 % | 2.5 % |
| the pieces' airtime, and their share of all | | 228 400 s, 42 % |
| the sections' airtime | | 6 430 s |
| texts delivered, 0–12 / 12–24 / 24–36 h | 99.64 / 99.83 / 99.75 % | 99.57 / 99.79 / 99.68 % |
| latency p50 / p90 / p99, 0–12 h | 1.0 / 16.6 / 740 s | 1.2 / 16.9 / 773 s |
| latency p50 / p90 / p99, 24–36 h | 1.0 / 16.1 / 608 s | 1.0 / 16.1 / 598 s |
| refused at the gate | 0 | 0 |
| texts whose application waited for its node | 43 | 49 |
| the busiest node's airtime | 0.88 % | 1.31 % |

Over the whole run relays missed 18.1 % of the pieces sent to them that
they lacked, sleepy nodes 9.7 % of their anchor's pieces after its
slot, and 63.4 % of the pieces from another channel than the sender's
home came within a pace of one from another such sender to the same
relay.

*Sleepy nodes.* Of about 945, 5 to 12 held the image at a time after
the relays were done, and those that gathered lacked block 1.75 on
average after 34 hours: about two blocks, where one piece a rotation of
its anchor would have given each about seven since the relays finished.
Their anchors sent few pieces after their slot heartbeats: 217 000
went in 36 hours to nodes that lacked them, about 230 a node. The
likely cause is the shared pace: an anchor that heard k other pieces
on its home channel since its last waits k + 1 paces, and the slot
piece goes only if the pace allows it at that moment, so on a channel
with several anchors each sends after only some of its slot
heartbeats. Not yet measured further.

*96 hours.* The same district, 96 hours, on `d78ecaa`, against the run
without the update and a run on `a3a78c6` (before the best holder,
sections on change and the 20 % margin):

| | without | `a3a78c6` | `d78ecaa` |
|---|---|---|---|
| relays holding, 50 / 90 / 100 % | | 16.5 / 17.8 / 32.0 h | 13.3 / 14.5 / 28.2 h |
| channel busy, mean over the run | 1.28 % | 2.05 % | 1.71 % |
| the pieces' airtime, and their share of all | | 405 700 s, 28 % | 313 500 s, 28 % |
| the sections' airtime | | 235 500 s | 14 500 s |
| texts delivered | 99.8 % | 99.8 % | 99.8 % |
| latency p50 / p90 | 1.0 / 16.2 s | 1.0 / 16.2 s | 1.0 / 16.2 s |
| sleepy nodes holding the image at 90 h | | 58 of 945 | 55 of 945 |
| sleepy nodes gathering, first block lacked, mean at 90 h | | 4.0 | 2.9 |

The sleepy nodes gain about a block a day. Fewer sections and a stricter
foreign rule slowed them by a block over the run: their anchors' slot
pieces are the same pace's.


## What is left

- **The firmware's side**: the driver's answers to the update's
  actions over `bark-image`'s `pieces.rs` and `code.rs` (D149), the
  policy (keys, the version, given up images), the signature check, the
  install through the loader, and `Holds` at boot. The firmware's driver
  answers none of the update's actions yet.
- **Foreign senders.** Two thirds of the pieces from another channel
  than the sender's home still overlap with another such sender's; a
  holder cannot tell whether a better one reaches the gatherer.
- **Sleepy nodes.** Their anchors send few pieces after the slot
  heartbeat once the pace is shared ("Heartbeat sections and a sweep",
  Results).
- **Runs**: the city, twice the design rate, more seeds.
