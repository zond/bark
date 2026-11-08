# Firmware updates over the mesh: the transport (2026-11-07)

**Status:** research note, 2026-11-07. Builds the transport that
`mesh-update-2026-10-25.md` studied, as decided with the user on
2026-11-07; spec "Updates over the mesh" holds the rules, and roadmap
D146 the decision. Spec references are to `protocol.md` by section
name. Built: the frames, the core's side and the simulator's driver
side, without bytes. Not built: the firmware's side (the code that
makes and decodes the pieces, their flash, the policy, the signature
check and the install), and the sleepy nodes' side.

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

## What is left

- **The firmware's side**: the coder (Reed-Solomon over GF(2⁸), 64
  pieces a block), the pieces in the external flash's 22 sectors, the
  block table and the block hashes, the policy (keys, the version, given
  up images), the signature check, the install through the loader, and
  `Holds` at boot. The firmware's driver answers none of the update's
  actions yet.
- **Sleepy nodes**: the anchor's piece after its slot heartbeat while
  one of its sleepy nodes asked within the hour, the sleepy node's
  listen for it and its request riding a beat, and the anchor's
  announcement after its slot heartbeat. The core can express them (a
  sleepy node already listens for its anchor's heartbeat,
  spec "Relay choice"), but none is built: a sleepy node ignores every update frame,
  and takes an image when it is a relay or from a phone.
- **A chain.** Letting a node serve the blocks it has decoded would
  bring the time per generation from a whole image to a few blocks; it
  needs an announcement of what a node holds in part, and a rule for
  which partial holder a node asks.
- **Runs**: the district to the end and the city, which need the
  simulator to carry a piece to a channel's nodes more cheaply; more
  seeds.
