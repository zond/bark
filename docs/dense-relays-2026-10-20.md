# Dense relays

**Date:** 2026-10-20, roadmap D68. **Status:** measured; decided
2026-10-21 (roadmap D68): no rule for density, full power kept, the
hop-cap fix and candidate 4 adopted, candidate 4 kept when the neighbour
table filled by sector was measured and not adopted (roadmap D69); the proposal that a
node that moves sleeps is not adopted: a node that moves is to route but
hold no mail (roadmap D70). Spec references are to `protocol.md`. The base is main
`b1e1cfd` with this branch's simulator instruments, which repeat it to
the byte on S4 (`scenarios/s4-capacity.toml`).

The question: a node is to be a relay whenever it is on USB power. At
night most of S4's 20 000 people's nodes could be relays, beside the 269
on roofs and windowsills, and some relays move, on a power bank in a bag
or in a car. Does that just work, and if not, what rule makes it? The
proposal to test was that a relay with too many relays around it sends at
lower power.

## Summary

- **Static relays among the people need no rule for density: the more
  there are, the better the domain does,** but for about a thousand. Delivered of
  the texts sent in the settled hour, at the spec rate: S4 94.0 %, 1300
  relays 94.9 %, 5317 relays 97.1 %, 20 269 relays 97.8 %. At three times
  the rate, where S4 has 155 of its relays at the duty cap and delivers
  73.4 %, 5317 relays deliver 96.9 % and 20 269 relays 97.5 %. Every
  relay is a duty budget of its own, and texts spread over them. Heartbeats grow with the relays,
  from 97 to 5550 s an hour on each channel over the domain, but a relay
  hears only its own reach of them: its channels stay 2 % busy, as in
  S4, and nothing collapses.
- **What breaks is sparse indoor relays, about a thousand.** A relay
  indoors reaches the relays outdoors over links near the margin, and
  the ACK of a hand-over that went through is lost on its way back, on
  another channel with another fade: 81 % of the failed hand-overs of
  those relays had been taken. Each such loss splits a message in two,
  and the copies search on (spec "Why the search finds a path"): 3.8 % of
  the texts have a copy that runs past 100 hand-overs, and they spend 73 %
  of all frames.
  Texts to those relays' people and their sleepy nodes arrive 91 % of the
  time against 95.5 % for the rest. Denser, the indoor relays reach each
  other and it goes: 50 such texts at 5317 relays, none at 20 269.
- **What breaks among many relays is the owner rule against a full
  neighbour table.** A relay within 50 m of a destination owns it if no
  neighbour it keeps is closer (spec "Owner"). With every person's node a
  relay, 5.6 relays are within 50 m of each on average, and a full table
  turns newcomers away (roadmap D12), so a relay often does not keep the
  one next door. It then claims that relay's texts and queues them for a
  sleepy node that never beats: 0.9 % of the texts to relays at 5317
  relays, 2.4 % at 20 269, 2.6 % at three times the rate. A few arrive
  another way, one in five at 20 269 relays; the rest wait to the end.
- **A relay within 50 m always finds a place, and that is the rule to
  take** (spec "Neighbour table", proposal). When no entry is demoted or
  stale, such a newcomer takes the place of the least recently heard relay
  beyond 50 m. At 5317 relays the texts claimed fall from 41 to 5 and
  from 58 to 10 (seeds 1, 2), delivered 97.06 → 97.43 % and 97.14 →
  97.50 %, at three times the rate 96.86 → 97.11 %; at 20 269 relays
  97.81 → 99.59 %, the texts claimed 232 → 16, at the same airtime per
  text, and at three times the rate 97.54 → 99.41 %. S4, where few relays are that near, 94.04 → 94.20 % and 94.01 →
  93.89 %, at three times 73.37 → 73.40 %.
- **A core bug, fixed:** a message that reached its owner at the hop
  cap could not be delivered and was dropped at every try, a few
  milliseconds apart; one relay traced 1.3 million drops in an hour. It
  now goes at the cap (spec "Hop count").
- **Power by density makes it worse, in both forms.** At 5317 relays,
  target 16: delivered 97.1 → 95.6 %, airtime per delivered text 1.9 →
  9.8 s, median hop count 8 → 35. What a relay counts depends on its
  neighbours' power, not its own, so the relays on roofs and windowsills,
  which hear the most, step to the radio's lowest and stay there, and the
  long hops go. At 5317 relays the count settles (0.13 steps a relay an
  hour, the mean power flat); at 20 269 it does not: 5.2 steps a relay an
  hour, the mean power sliding from 1.8 to 0.3 dBm over the settled half
  hour, 6293 relays at the floor by its end, 28 % of DATA between relays
  sent to one that cannot hear it with margin, 91 relays no other keeps
  with margin, and a text takes 11.8 s of airtime against 1.75.
- **The sender's power in the heartbeat removes the asymmetry, not the
  cost.** With it, 2 % of DATA between relays goes to one that cannot hear
  it, against 15 % without, and delivery is 96.8 % at target 16, but the
  airtime per text is still 4.2 s against 1.9 at full power. Among 20 269
  relays the mean power settles near 5 dBm, though most relays step up and
  down every rotation or two, 6.4 steps an hour, and delivery is 98.8 %
  against 97.8 %, at 3.2 times the airtime per text: a smaller reach means
  fewer relays turned away by full tables, the fault found above, which
  candidate 4 mends at no airtime: 99.6 %. The heartbeat no longer has
  spare bits: since it carries the 4-byte short
  position (spec "Position and time") the power takes a tenth byte, 1.28
  ms on every heartbeat. With power lowered by nothing, the field has
  nothing to serve.
- **Standing down is neutral.** Where it settles at 5317 relays and
  N = 8, 1581 relays sleep: delivered 97.1 → 97.1 %, heartbeat airtime
  −29 %, data airtime +18 %, all airtime +4 %. Among 20 269 relays at
  N = 16, half stand down (10 431): 96.95 against 97.81 %, heartbeats
  −51 %, data +30 %, all airtime −20 %; the nodes that stood down
  take a sleepy node's dark spells in this model, which a node on USB
  power would not have. It saves what does not bind, and each switch
  would change the node's address.
- **A heartbeat interval that grows with the relays heard was not
  built:** heartbeats bind nothing at any density measured, and the
  fixed interval by ID is what a sleeping node wakes by for its anchor
  (roadmap D47).
- **A relay that moves does harm; a node that moves should sleep.** With
  a fifth of 5048 people's relays walking (1004), texts to them arrive 58
  % of the time and to the sleepy nodes anchored to them 60 %, and every
  other text spends three times the frames. Taking each fix instead of the
  first (roadmap D63) gives 79 % and 65 %. The same people with the
  walkers' nodes sleepy: texts to walkers 90 %, as to any walking sleepy
  node, everything else 98.3 %, airtime per text 1.84 s.
- **Recommendation:** no rule for density; keep full power (roadmap D40);
  keep the heartbeat as it is; keep a place in the neighbour table for
  every relay within 50 m (candidate 4); a node on USB power is a relay
  only while it stays where its first fix put it, and sleeps once it moves
  (spec "Hardware", proposal). The sparse indoor case is a weakness of
  marginal links that dense relays show and do not cause; it is a later
  item.

## What was changed

Kept on the branch:

| Commit | What |
|---|---|
| Simulator: a share of the people's nodes are relays, and some of them walk | `relay_share` on a placement of people's nodes by density: that share are relays, drawn apart from the positions. They send and receive texts, switch on in the people's window and may walk (`[movement] relay_fraction`) or ride (`[vehicles] relay_fraction`); they do not go dark. A walking relay's address is the position its core holds. `docs/simulator-design.md` §2, §3.7 |
| Core: a message that reaches its owner at the hop cap is delivered | the bug above; spec "Hop count"; a test in `tests/sleepy.rs` |
| Simulator: with relays among the people, who keeps whom | once a minute, the relays no other keeps in its neighbour table, with margin and at all; at the end, how many keep each |
| Candidate D68 4: a relay within the owner radius finds a place | the recommended rule: a full table with no demoted or stale entry gives a relay within 50 m the place of the least recently heard relay beyond 50 m (`tables/neighbours.rs` `near_victim`); spec "Neighbour table"; a test in the table's own tests |

Measured and reverted on top, each its own commit:

| Candidate | What |
|---|---|
| 1a: power by the relays heard | `Config::power_neighbours`: once a worst-case rotation, 195 s, a relay counts the relays it keeps with margin; above the target N it steps its power down 2 dB, below N / 1.29 up 2 dB, between −9 dBm and full power. 2 dB is the spread of the averaged RSSI under 4 dB of fading, 4 / √3; 1.29 is what one step changes the reached area by under the path-loss exponent 3.6, 10^(4/36). The core asks for its power with `Action::Power`, the driver applies it under its ceiling, the simulator adds it to every link from the node. The simulator counts each relay's steps and power, and DATA and failed hand-overs to a relay the sender hears with margin but whose own power does not reach with margin, before fading |
| 1b: the heartbeat's power byte | on 1a: a tenth heartbeat byte, how far below full power its sender sends. The path loss is the same both ways, so a relay counts, and takes as a next hop with margin, only neighbours it also reaches with margin |
| 2: standing down | a research model of where the rule settles: with `[stand_down] neighbours`, each people's relay that hears at least N relays still standing with margin both ways, in an order drawn from the seed, runs as a sleepy node from the start, its links kept |

## What was run

S4 at its spec rate (0.958 texts a person an hour) with a share of the
people's nodes as relays, seed 1, and the same at three times the rate.
Every number is of the texts sent in the settled window, 2700 s to the
end, those still on their way at the end counted as not delivered; this
is not the reachable delivery of the S4 rows of earlier notes, which
measure two settled hours. S4 and the densities to 5317 relays ran 6300
s, an hour settled; the 20 269 relays ran 4500 s, half an hour settled,
to keep it near 40 minutes of wall time (3.7 GB, the same as S4: a core
is the same size whatever its role). With every node a relay there are no
sleepy nodes, so no dark spells and no walkers: the comparison with the
others is not like for like there.

| Run | Relays | Wall time, up to eight runs at once on 32 cores |
|---|---|---|
| S4 | 269 | 2.5 min |
| 1300 relays | 269 + 1031 | 10 min |
| 5317 relays (`scenarios/dense.toml`) | 269 + 5048 | 22 min |
| 20 269 relays | 269 + 20 000 | 36 min |
| 5317 relays, a fifth of the people's relays walking (`scenarios/dense-movers.toml`) | 5317, 1004 walking | 20 to 32 min |

## Results

### Static relays, no rule

| | S4 | 1300 | 5317 | 20 269 |
|---|---|---|---|---|
| delivered, spec rate | 94.04 % | 94.91 % | 97.06 % | 97.81 % |
| latency p50 / p90 | 1.5 / 183 s | 4.3 / 161 s | 2.0 / 12 s | 4.0 / 11 s |
| highest hop count among a delivered text's frames, median (p90) | 4 (7) | 6 (12) | 8 (14) | 9 (16) |
| texts past 100 hand-overs; their share of frames | 23; 7.6 % | 663; 73 % | 50; 15 % | 0 |
| airtime per delivered text | 1.36 s | 5.67 s | 1.88 s | 1.75 s |
| airtime a channel an hour, domain: relay heartbeats / sleepy beats / data | 97 / 785 / 2082 s | 397 / 350 / 6560 s | 1597 / 230 / 3840 s | 5550 / 0 / 3690 s |
| busy at the 269 relays of S4, mean (most) | 1.8 (9.1) % | 4.3 (17.9) % | 2.2 (7.9) % | 2.4 (6.5) % |
| relays that reached the duty cap: of S4's / the people's | 3 | 62 / 15 | 26 / 11 | 23 / 5 |
| neighbour table full: S4's relays / the people's | 91 % | 93 % / 0.1 % | 97 % / 0.1 % | 98 % / 87 % |
| newcomers turned away a relay an hour: S4's / the people's | 900 | 776 / 0.1 | 1977 / 0.1 | 6968 / 271 |
| relays no other keeps with margin, at the end | not counted | 22 | 1 | 0 |
| texts claimed by a relay that does not keep their destination relay; of the texts to relays | none sent to relays | 1; 0.1 % | 41; 0.9 % | 232; 2.4 % |
| a people's relay kept by, mean: at all / with margin | | 10.7 / 4.0 | 26.5 / 9.9 | 59.8 / 30.4 |
| heard table, most | 238 | 206 | 112 | 0 |
| sleepy nodes anchored to a people's relay | 0 | 30 % | 64 % | |
| sleepy nodes on the busiest anchor | 161 | 101 | 43 | |
| anchor changes a sleepy node an hour | 0.46 | 0.48 | 0.56 | |
| a sleepy node's anchor not hearing it, sampled | 5.5 % | 5.5 % | 3.3 % | |
| sleepy node's current, mean (p90) | 0.633 (1.135) mA | 0.576 (1.001) mA | 0.536 (0.913) mA | |
| delivered, three times the rate | 73.37 % | 65.47 % | 96.86 % | 97.54 % |
| at three times: relays that reached the cap, S4's / the people's | 155 | 195 / 144 | 142 / 44 | 86 / 55 |
| at three times: texts claimed by the wrong owner | | | 153 | 759 |

The neighbour tables of S4's relays are full nine tenths of the time
already in S4, and turn newcomers away (D12): a relay keeps its first
64. Denser, they turn away more. Every relay is still kept by others, on
average by 11 to 60 of them, but the relay next door may be among those
turned away, which the owner rule cannot see (below). The heard tables
never reach their 256, so D61's replacement never acts.
Anchors move to the people's relays, nearer, and a sleepy node's current
falls with them. The duty cap binds where texts concentrate, on the
relays of S4, and less the more relays share the texts.

The 1300 relays: an indoor relay hears the windowsill relays 300 m away
near the margin. Its hand-overs fail 42 % of the time against 15 % for
S4's relays in the same run, and 81 % of those failures were taken by the
receiver: the DATA went through on the receiver's channel and the ACK, on the sender's
channel with another fade, did not. Each is a split; the copies search
on until they meet, arrive or reach the hop cap. That is where the hop
cap bug showed: copies reached the owner at the cap.

### Power by density and standing down, 5317 relays

| | full power | 1a, N = 16 | 1a, N = 32 | 1b, N = 16 | 1b, N = 32 | 2, N = 8 | 2, N = 16 |
|---|---|---|---|---|---|---|---|
| delivered | 97.06 % | 95.65 % | 96.00 % | 96.82 % | 96.70 % | 97.09 % | 96.95 % |
| latency p50 / p90 | 2.0 / 12 s | 17 / 77 s | 12 / 52 s | 5.6 / 20 s | 3.0 / 14 s | 2.0 / 15 s | 2.0 / 12 s |
| highest hop count of a delivered text, median | 8 | 35 | 24 | 19 | 10 | 7 | 8 |
| airtime per delivered text | 1.88 s | 9.80 s | 6.20 s | 4.17 s | 2.28 s | 2.10 s | 1.82 s |
| relay heartbeats / data, a channel an hour | 1597 / 3840 s | 1597 / 12698 s | 1597 / 7847 s | 1769 / 6913 s | 1769 / 4295 s | 1130 / 4530 s | 1590 / 3760 s |
| busy at S4's relays, mean | 2.2 % | 2.7 % | 2.2 % | 2.1 % | 2.0 % | 2.4 % | 2.2 % |
| relays that reached the cap | 37 | 197 | 56 | 70 | 49 | 68 | 33 |
| power steps a relay an hour; relays stepping more than twice an hour | | 0.13; 95 | 0.25; 218 | 0.63; 425 | 0.28; 232 | | |
| relays at −9 dBm at the end; at full power | | 269; 4994 | 113; 5057 | 0; 4996 | 0; 5070 | | |
| DATA between relays to one that cannot hear it with margin; of failed hand-overs | 0 | 15 %; 28 % | 14 %; 24 % | 2.0 %; 2.2 % | 2.4 %; 2.2 % | | |
| relays no other keeps with margin | 1 | 5 | 2 | 4 | 2 | 1 | 0 |
| relays that stood down | | | | | | 1581 | 34 |
| sleepy node's current, mean | 0.536 mA | 0.563 mA | 0.566 mA | 0.550 mA | 0.542 mA | 0.536 mA | 0.537 mA |

With every node a relay, 20 269 relays, target 16:

| | full power | 1a | 1b | 2 |
|---|---|---|---|---|
| delivered | 97.81 % | 97.10 % | 98.79 % | 96.95 % |
| latency p50 / p90 | 4.0 / 11 s | 32 / 72 s | 8.2 / 18 s | 3.6 / 12 s |
| highest hop count of a delivered text, median | 9 | 32 | 28 | 8 |
| airtime per delivered text | 1.75 s | 11.8 s | 5.68 s | 1.84 s |
| relay heartbeats / data, a channel an hour | 5550 / 3690 s | 5549 / 15115 s | 6150 / 7800 s | 2700 / 4810 s |
| power steps a relay an hour; relays stepping more than twice an hour | | 5.2; 14 030 | 6.4; 16 718 | |
| mean power over the settled half hour | 10 dBm | 1.8 → 0.3 dBm | 4.7 → 5.1 dBm | |
| relays at −9 dBm, from the settle time to the end | | 473 → 6293 | 0 → 3 | |
| DATA between relays to one that cannot hear it with margin; of failed hand-overs | 0 | 28 %; 35 % | 6.1 %; 6.1 % | |
| relays no other keeps with margin | 0 | 91 | 0 | 0 |
| relays that stood down | | | | 10 431 |

### Moving relays, 5317 relays, 1004 of them walking

A walking relay takes a fix at each heartbeat, through the driver's
checks (spec "Position and time"). Keeping the first fix is roadmap D63:
past 50 m from it every fix is refused, and the relay heartbeats where it
was. Taking each fix lets it through when it is within 50 m of the last.
The third run is a city of the same make with the walkers' nodes sleepy:
as many relays fewer and walking sleepy nodes more, drawn afresh (relay
share 0.216, walking share of the sleepy nodes 0.253).

| | keep the first fix | take each fix | moving nodes sleep |
|---|---|---|---|
| delivered, all | 91.12 % | 92.94 % | 96.55 % |
| to a moving relay | 58.1 % | 79.4 % | |
| to a sleepy node anchored to a moving relay | 59.9 % | 64.8 % | |
| to a walking sleepy node | 89.2 % | 88.9 % | 89.9 % |
| from a moving relay | 96.3 % | 96.9 % | |
| every other text | 96.9 % | 97.4 % | 98.3 % |
| airtime per delivered text | 7.22 s | 4.50 s | 1.84 s |
| texts past 100 hand-overs | 604 | 304 | 47 |
| anchor changes a sleepy node an hour | 0.76 | 0.95 | 0.67 |

### A relay within 50 m always finds a place (candidate 4)

| | full power, D12 | candidate 4 |
|---|---|---|
| S4, seed 1 / 2: delivered | 94.04 / 94.01 % | 94.20 / 93.89 % |
| S4 at three times: delivered | 73.37 % | 73.40 % |
| 5317 relays, seed 1 / 2: delivered | 97.06 / 97.14 % | 97.43 / 97.50 % |
| texts claimed by a relay that does not keep their destination | 41 / 58 | 5 / 10 |
| texts to relays delivered | 98.88 / 98.69 % | 99.52 / 99.58 % |
| airtime per delivered text | 1.88 / 1.86 s | 1.82 / 1.76 s |
| 5317 relays at three times: delivered; claimed | 96.86 %; 153 | 97.11 %; 34 |
| 20 269 relays: delivered; claimed | 97.81 %; 232 | 99.59 %; 16 |
| 20 269 relays at three times: delivered; claimed | 97.54 %; 759 | 99.41 %; 103 |

### A second seed

Seed 2, a different city of the same densities, the same runs at the
spec rate:

| | seed 1 | seed 2 |
|---|---|---|
| S4: delivered; airtime per delivered text | 94.04 %; 1.36 s | 94.01 %; 1.43 s |
| 1300 relays | 94.91 %; 5.67 s | 95.11 %; 5.36 s |
| 5317 relays | 97.06 %; 1.88 s | 97.14 %; 1.86 s |
| 5317 relays, 1a at 16 | 95.65 %; 9.80 s | 96.04 %; 10.1 s |
| 5317 relays, 1b at 16 | 96.82 %; 4.17 s | 96.90 %; 4.12 s |
| 5317 relays, standing down at 8 | 97.09 %; 2.10 s | 97.22 %; 2.26 s |
| moving relays keep the first fix: to them; to their sleepy nodes; all | 58.1 %; 59.9 %; 91.12 % | 55.7 %; 61.1 %; 91.74 % |
| moving relays take each fix | 79.4 %; 64.8 %; 92.94 % | 78.9 %; 66.7 %; 93.52 % |
| moving nodes sleep: to walking sleepy nodes; all | 89.9 %; 96.55 % | 91.0 %; 96.79 % |

What the core does today with a relay that moves: its driver keeps the
first fix (D63), so it heartbeats that position wherever it is. Its
neighbours keep it with that position, where it is not. Texts for it go
to the position, where no relay within 50 m owns them, and search the
domain. The sleepy nodes anchored to it keep their address, its old
position, until two of its rotations go unheard, then choose again and
change address; texts sent to the old address wait at no owner. Taking
each fix moves its address with it, so senders hold a stale one, and its
sleepy nodes' addresses go stale as it walks off.

## Why

**Density.** The design binds first on a relay's 10 % duty cycle, at the
relays where routes meet (spec "Duty cycle cap", "Capacity and sizing").
More relays are more budget where people are, and they shorten the
links: a text to a person goes to their own relay or one indoors near
them. The channels were not the limit at S4's density and are not now: a
relay hears the heartbeats of those within its reach, and at 20 269
relays that is two in a hundred of its seconds.

**Power by density.** D40 found that lowering power costs where the duty
budget binds, since shorter hops cost the same airtime a hop and there
are more of them, and buys only where every relay hears every other and
the channel binds. Density does not change that: the channels stay 2 %
busy, so there is no channel time to win. It adds a flaw of the rule
itself. A relay's power changes what others hear of it, not what it hears,
so its count answers to its neighbours' steps. The relays that hear most
are on roofs and windowsills, heard by everyone; they turn down and stay
down whatever they do, and they were the long hops. Among 20 269 relays
the loop does not settle: one relay's step changes the counts of hundreds,
each of which steps a rotation later.

**The power byte** makes the count depend on the relay's own power, so
it settles, and takes the one-way links out of routing. It cannot give
back the long hops, which lowering power takes away.

**Standing down** takes relays away where they are many. Delivery does
not need them, but the texts take longer routes through those left, and
the heartbeats it saves bind nothing.

**The owner rule** decides from the table, which was sized for spec
"Capacity and sizing"'s 9.6 relays per km², where a relay rarely has
another within 50 m (0.08 on average). A relay that is not kept cannot be
seen to be closer, so its neighbour within 50 m owns its texts and queues
them for a beat that never comes. Keeping the near ones is cheap where
they are few and fits where they are many, and it makes the table answer
the question the owner rule asks of it.

**Moving relays.** A relay's position is its neighbours' route to it and
its sleepy nodes' address. Kept, it is wrong once the relay walks off;
followed, every address learnt from it goes stale as it moves. A sleepy
node's address is its anchor's position, which does not move, so a
walking person is reached through whichever relay they are near; that is
what the design has for carried nodes (spec "Sleepy nodes", roadmap D46).

## What this does not show

- Two seeds for the comparisons at 5317 relays and for S4, one for the
  rest. The differences that decide are large, several points or several
  times the airtime, and both seeds give them; the small ones, such as
  standing down against full power, are within the spread of the two.
- The people's relays never go dark: a node on USB power at home is not
  in a pocket or a basement. A relay on a power bank would be, and that
  would cost more than these runs show.
- Standing down and the moving nodes that sleep are measured where they
  settle, set up before the run. The switching is not: each switch
  changes a node's address and leaves its queued mail and its anchored
  nodes to be handed on, which is the runtime switch of a node's role
  this work leaves for later.
- Moving relays walk; none drives. A relay in a car moves 50 m in five
  seconds and its fixes fail the 50 m check either way.
- Candidate 4 makes way for a near relay by the least recently heard one
  beyond 50 m, which may be a long hop; the hop counts and airtime say it
  costs nothing measurable here, but no run looked at which entries went.
- The sparse indoor case is one placement of 1031 relays, all indoors;
  how many indoor relays a district has before their links stop being
  marginal depends on walls the model has as one number.

## Recommendation

1. **No rule for density.** Static relays among the people raise
   delivery and capacity at every density measured but about a thousand,
   and nothing taken is dropped or collapses.
2. **Keep a place for every relay within 50 m** (candidate 4, spec
   "Neighbour table", proposal), so that the owner rule reads a table that
   holds the relays next door: it takes the texts claimed by the wrong
   owner from 0.9 % of the texts to relays to 0.1 to 0.2 % at 5317 relays,
   and changes S4 within its spread.
3. **Keep full power** (roadmap D40) and the 9-byte heartbeat: both
   forms of power by density cost several times the airtime per text,
   and the field earns nothing without them.
4. **A node is a relay only while it stays put** (spec "Hardware",
   proposal): on USB power and within 50 m of its first fix as a relay;
   once a fix is farther it sleeps, as a carried node does. It is the
   cheapest form that does no harm, measured at 96.6 % against 91.1 and
   92.9 % for a relay that moves, and what the runtime switch of a node's
   role must do; that switch also has to hand on the mail it holds.
5. **Later item:** the split copies of a hand-over whose ACK fades on the
   way back, which dense indoor relays show at about a thousand relays.
