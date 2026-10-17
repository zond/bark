# Capacity of one collision domain

**Date:** 2026-10-15. Roadmap Phase 1 milestone 4, scenario S4
(`simulator-design.md` §6). Spec references are to `protocol.md`. The
protocol code is that of commit `e3dbaaa`, with one read-only addition
(`Relay::load`) for the simulator to sample.

## Summary

At the texting rate of spec "Capacity and sizing" the domain delivers about half of what is
sent. It delivers about 90 % up to half that rate. The first limit to
give way is the 10 % duty cycle cap of relays on windowsills in the
centre of the domain. It starts to bind at a quarter of the spec "Capacity and sizing" rate. The
channels never fill: measured at the relays, they stay 2 to 10 % busy.

The cap binds early because each text costs 5 to 6 times the airtime spec "Capacity and sizing"
assumes. Spec "Capacity and sizing" counts one DATA frame and one ACK per hop. The run measures
4 to 6 DATA frames per hand-over that succeeds. Most of the extra goes to
neighbours that do not answer. Fading lets a relay hear the short
heartbeat of a neighbour whose link is usually too weak for a 121-byte
DATA frame. Greedy forwarding prefers such a neighbour, because it is the
farthest and so the closest to the destination. Without fading the same
domain carries the spec "Capacity and sizing" rate at 93 %, and its cost per text falls from
4.2 s of airtime to 1.0 s.

Past its limit the domain does not level off. It collapses. At twice the
spec "Capacity and sizing" rate it delivers 8 %. A domain where every relay is on a roof, spec "Capacity and sizing"'s
own picture, fails even at a tenth of the rate. Each relay then hears
every other relay's acknowledgements of leaf keepalives, which keep
every channel about 40 % busy.

Per domain the run carries about 2.2 texts a second at 90 % delivery.
Over 14 domains that is about 31 texts a second, 9 texts per user per
day at 300 000 users. Spec "Capacity and sizing" predicts 80 texts a second and 23 per user per
day.

## Setup

`scenarios/s4-capacity.toml`, seed 1, run with `bark-sim` at commit
`3dcf7e5` and again with the faster `4fe2b11`, which gives identical
results. Each variant below changes that file only in what its name
says: the texting rate, the seed, no fading, no dark phones, or every
relay on a roof.

| What | Value | Source |
|---|---|---|
| Area | 28 km², a square of side 5291.5 m | Spec "Capacity and sizing": one collision domain |
| Relays | 269, uniform at random, 9.6 per km² | Spec "Capacity and sizing": indoor coverage density |
| Of which on roofs | 53, 1.9 per km² | Spec "Capacity and sizing" gives no share; this is its outdoor-only density, the relays a city would put up first |
| The rest | 216 on windowsills, modelled as outdoors | S2's choice. A relay behind a wall would reach an indoor leaf only 120 m away, and 9.6 per km² would leave most leaves without a relay (spec "Relay choice" counts on 300 m) |
| Leaves | 20 000, uniform at random, all indoors | Spec "Capacity and sizing": 300 000 users over 14 domains |
| Texts | Poisson, 23 per leaf per day, 100 bytes | Spec "Capacity and sizing" |
| Destination | any other leaf of the domain, all alike | see below |
| Phones going dark | 5 minutes in each hour on average | S2, spec "Delivery queue" |
| Path loss | exponent 3.6, 25.2 dB at 1 m | S2's fit to spec "Physical layer": 750 m to a handheld in the street with 129 dB of budget (10 dBm, spec "Regulatory limits", to −119 dBm, `reception-research-2026-10-15.md`) |
| Wall | 14.4 dB per indoor end | S2's fit: 300 m to a handheld indoors (spec "Physical layer") |
| Rooftop to rooftop | 21.4 dB less loss | 129 − 25.2 − 36 log10 3000: two rooftops hear each other at 3 km (spec "Physical layer", "Capacity and sizing"). A rooftop's link to anything below the roofs gets no bonus |
| Shadowing | 6 dB, fixed per pair | the scenarios' figure since S2; not measured |
| Fading | 4 dB per pair and channel, 60 s correlation | the scenarios' figure since S2; not measured |
| Duration | 4500 s; the network settles by 900 s | boot listen 150 s (195 s since 2026-10-16; spec "Channel at boot"), leaf choice within 195 s (spec "Relay choice"), first moves after 10 minutes (spec "Channel moves") |

**Destinations.** Uniform over the domain gives a mean distance of
2.8 km and a median of 3 to 5 hand-overs. A city's texts are more local
than that, but texts across a city of 400 km² also cross several
domains, and each domain carries traffic passing through. The simulated
domain has edges, with no traffic from beyond them. Both effects are
left out. The second makes the figures per domain optimistic.

**What is counted.** Delivery is over messages sent from 900 s to
3900 s, so each had at least the delivery TTL of 10 minutes (spec "Delivery queue") to
arrive. "At the cap" means the relay's bucket held less than the reply
reserve and one full frame (242 + 138.6 ms, spec "Duty cycle cap", "Physical layer"), sampled every
second. So it could take no full frame of new work. Airtime per text
counts every frame that carried the message or answered it: DATA,
DELIVER and their ACKs, attempts that failed included. "Centre" is the
middle square of half the side, a quarter of the area. The rest is
"edge". Busy is the time a relay's own channel was 6 dB above the noise
floor (spec "Busy estimate"), as the simulator measures it at the relay.

## Results

### The rate sweep

| Rate × spec "Capacity and sizing" | 0.1 | 0.25 | 0.5 | 1 | 2 | 4 |
|---|---|---|---|---|---|---|
| Offered, texts/s | 0.49 | 1.22 | 2.42 | 4.80 | 9.43 | 17.1 |
| Delivered | 92.3 % | 91.1 % | 90.5 % | 52.5 % | 8.0 % | 0.4 % |
| Carried, texts/s | 0.45 | 1.11 | 2.19 | 2.52 | 0.75 | 0.08 |
| Latency median | 3.6 s | 5.0 s | 6.0 s | 36 s | 200 s | 489 s |
| Latency 90th percentile | 80 s | 127 s | 158 s | 464 s | 897 s | 1740 s |
| Airtime per delivered text, median | 1.25 s | 1.33 s | 1.37 s | 2.10 s | 3.43 s | |
| Airtime per delivered text, mean | 2.0 s | 5.2 s | 3.6 s | 4.2 s | 6.7 s | 13.8 s |
| Hand-overs per delivered text, median | 3 | 3 | 4 | 5 | 9 | 11 |
| DATA frames per hand-over that succeeded | 5.1 | 6.0 | 5.1 | 4.1 | 4.6 | 5.8 |
| Relay airtime, mean | 1.9 % | 3.3 % | 4.5 % | 6.9 % | 8.8 % | 9.4 % |
| Relay airtime, centre mean | 2.4 % | 4.4 % | 6.2 % | 8.6 % | 9.4 % | 9.5 % |
| Relay airtime, largest | 4.7 % | 7.1 % | 9.1 % | 9.5 % | 9.5 % | 9.5 % |
| Relays ever at the cap | 0 | 164 | 180 | 221 | 259 | 269 |
| Time at the cap, centre relays | 0 | 2.5 % | 6.8 % | 57 % | 89 % | 93 % |
| Time at the cap, edge relays | 0 | 0.8 % | 1.6 % | 22 % | 63 % | 88 % |
| Time at the cap, rooftop relays | 0 | 0.6 % | 1.7 % | 26 % | 72 % | 91 % |
| Time at the cap, windowsill relays | 0 | 1.3 % | 3.0 % | 30 % | 68 % | 88 % |
| Own channel busy at relays, mean | 1.7 % | 3.0 % | 4.2 % | 6.8 % | 8.9 % | 9.9 % |
| Own channel busy at rooftop relays, mean | 2.6 % | 5.5 % | 8.0 % | 13.1 % | 16.8 % | 18.3 % |
| Lost: sender found no relay to take it in 10 min | 8 | 23 | 50 | 3742 | 20 191 | 46 001 |
| Lost: delivery TTL ran out at the destination's relay | 106 | 298 | 629 | 2426 | 4005 | 1539 |
| Lost at relays, other reasons | 0 | 0 | 1 | 4 | 55 | 205 |
| Lost without a trace | 0 | 0 | 0 | 0 | 0 | 0 |

The mean airtime per text is far above the median because a few
searches wander. At half the spec "Capacity and sizing" rate the costliest tenth of the texts
used 62 % of the airtime spent on texts. Some reached the hop cap of 255.

**Seed.** Seed 2 gives the same picture. It delivers 87.3 % at half the
rate (2.11 texts/s carried) and 45.9 % at the spec "Capacity and sizing" rate (2.22 texts/s).
Its centre relays spend 19 % and 66 % of the time at the cap.

### Where it binds first

1. **The duty cycle cap of relays in the centre, at about a quarter of
   the spec "Capacity and sizing" rate.** At 0.25 times the rate, 164 relays reach the cap at
   some point. Centre relays spend 2.5 % of the time there, edge relays
   0.8 %. Windowsill relays spend 1.3 %, rooftop relays 0.6 %. At half
   the rate centre relays average 6.2 % airtime and spend 6.8 % of the
   time at the cap, and delivery holds at 90 %. At the spec "Capacity and sizing" rate they
   spend 57 % of the time at the cap and their send queues average 8.6
   frames (2.6 at the edge). Windowsill relays bind before rooftop ones
   because their links are weaker. A windowsill relay sends 6.0 DATA
   frames per hand-over that succeeds at a tenth of the rate, a rooftop
   relay 2.4.
2. **Then the leaves.** A relay at the cap refuses its leaves' texts
   with BUSY, and below its reply reserve it does not answer at all
   (spec "Duty cycle cap"). A refused leaf holds its text and offers it again, waiting up
   to 61 s between offers (spec section "Attach and polls" (before D46)). A leaf whose relay does not answer
   gives it up and looks for another. Either way the text waits at the
   sending leaf until the 10-minute TTL ends it. This is the main loss
   at and above the spec "Capacity and sizing" rate: 3742 of 6172 losses at ×1, 20 191 of
   24 251 at ×2. Leaves without a relay at the end grow from 2888 at a
   tenth of the rate to 6688 at the spec "Capacity and sizing" rate. Delivery queues never
   fill. The largest held 28 of their 41 places. No message was refused
   for a full delivery queue.
3. **The channels do not bind.** Mean busy time at a relay's own channel
   stays under 10 % at every rate. Rooftop relays, which hear farther,
   see about twice as much, 13 % at the spec "Capacity and sizing" rate. The busy time is
   spread evenly over the 13 channels: 6.3 to 7.3 % each at the spec "Capacity and sizing" rate,
   with 6 to 41 relays on each channel. Measured selection (spec "Home channel selection") does
   what it should.

### Without fading, without dark phones, all on roofs

| Variant | Offered/s | Delivered | Carried/s | Median | Air/text mean | DATA per hand-over | Relay air mean | Centre time at cap | Busy at relays |
|---|---|---|---|---|---|---|---|---|---|
| ×1 (the scenario) | 4.80 | 52.5 % | 2.52 | 36 s | 4.2 s | 4.1 | 6.9 % | 57 % | 6.8 % |
| ×1, no fading | 4.77 | 93.3 % | 4.45 | 1.8 s | 1.0 s | 2.4 | 3.0 % | 0.1 % | 2.4 % |
| ×2, no fading | 9.46 | 37.7 % | 3.57 | 37 s | 2.9 s | 3.4 | 7.4 % | 70 % | 6.9 % |
| ×4, no fading | 17.1 | 2.4 % | 0.41 | 566 s | 8.1 s | 4.2 | 9.3 % | 92 % | 8.7 % |
| ×1, no dark phones | 5.25 | 47.9 % | 2.52 | 43 s | 4.7 s | 4.2 | 7.3 % | 64 % | 7.2 % |
| ×0.1, all relays on roofs | 0.49 | 11.6 % | 0.06 | 11 s | 136 s | 15 | 6.9 % | 0 % | 38 % |
| ×1, all relays on roofs | 4.58 | 0.0 % | 0.00 | | | 27 | 8.4 % | 0.2 % | 47 % |

- **Fading is what makes the cap bind early.** With steady links a text
  costs 1.0 s of airtime and the domain carries the spec "Capacity and sizing" rate. The knee
  then lies between 1 and 2 times the rate, close to spec "Capacity and sizing"'s estimate.
  With fading the same texts cost 4.2 s. A trace shows the mechanism.
  A relay offers a text three times each to two or three neighbours
  that never answer before one takes it. Those neighbours are in its
  table because one heartbeat of 8 bytes reached it during a fade-up.
  They are the ones greedy forwarding prefers: the farthest, so the
  closest to the destination. A neighbour that fails twice is demoted
  (spec "Neighbour table"). It is promoted again on its next ACK or after 390 s, and each
  other direction finds its own such neighbours. The relays demoted
  2438 neighbours in the hour at a tenth of the rate and 56 054 at the
  spec "Capacity and sizing" rate.
- **Dark phones are not the cause.** Without them the result is the
  same.
- **All on roofs, spec "Capacity and sizing"'s picture of 270 relays that all hear each other,
  fails at any rate.** Each relay acknowledges its leaves' keepalives:
  20 000 leaves every 60 s with a 14.4 ms ACK is 4.8 s of airtime per
  second, 37 % of each channel when every relay hears every ACK. The
  channels are 38 % busy at a tenth of the rate. The leaves' ATTACHes,
  which get no rooftop bonus, are lost under the relays' ACKs. 18 159 of
  20 000 leaves had no relay at the end. Spec "Capacity and sizing"'s 2.5 % counts heartbeats
  only. The heartbeats alone do come to 2.4 % of each channel here.

### Heartbeats and keepalives

| Per channel, summed over the domain, ×0.1 | Share of time |
|---|---|
| Heartbeats | 2.44 % |
| Leaf ATTACHes (keepalives and their retries) | 31 to 46 % |
| ACKs (most of them of keepalives) | 23 to 36 % |
| DATA | 5 to 7 % |
| DELIVER | 0.3 to 0.5 % |

These are sums over every transmitter in the domain. A relay hears only
part of them; its own channel was 1.7 % busy on average at this rate.
Heartbeats match spec "Heartbeat" and "Capacity and sizing": 2.4 % of each channel if every relay
hears every other. Keepalives cost more than heartbeats at every relay.
Acknowledging 66 leaves on average takes 1.5 % of a relay's airtime,
15 % of its duty budget, before it forwards a single text. Spec section "Attach and polls" (before D46) counts
this load against the channel, about 2 %, but not against the relay's
budget.

### Neighbour tables at rooftop relays (O4)

| Run | Table full, share of time | Mean entries | Evictions per relay-hour | Newcomers rejected per relay-hour |
|---|---|---|---|---|
| ×0.1 | 66 % | 60.5 | 174 | 34 |
| ×0.5 | 47 % | 59.0 | 110 | 9 |
| ×1 | 23 % | 54.6 | 48 | 4 |
| all on roofs, ×0.1 | 40 % | 52.3 | 135 | 672 |
| all on roofs, ×1 | 19 % | 47.7 | 61 | 185 |

Windowsill relays never filled their tables. They averaged 26 to 35
entries, with a largest of 62. With 53 rooftops of 269, a rooftop
relay's table is full much of the time and replaces about three entries
a minute under D12. Under load fewer tables are full, because more
entries are demoted and expire. Whether a set spread around the relay
would route better (O4) is not tested here.

### Queues, refusals and moves

| Rate × spec "Capacity and sizing" | 0.1 | 0.25 | 0.5 | 1 | 2 | 4 |
|---|---|---|---|---|---|---|
| Send queue, centre mean | 0.0 | 0.2 | 0.5 | 8.6 | 17.8 | 22.5 |
| Send queue, largest | 3 | 8 | 14 | 40 | 42 | 46 |
| Delivery queue, largest of 41 | 3 | 5 | 11 | 26 | 28 | 17 |
| Refused for the bucket (BUSY) | 0 | 360 | 2526 | 106 185 | 407 473 | 758 375 |
| Refusals received by relays (bans) | 0 | 58 | 603 | 36 929 | 108 853 | 127 691 |
| Heartbeats skipped, channel busy | 131 | 598 | 1144 | 2241 | 3018 | 4048 |
| Home channel moves after boot | 198 | 398 | 561 | 767 | 835 | 908 |

Home channel moves come to 0.6 per relay per hour at a tenth of the
rate and grow with load. Spec "Channel moves" meant them to be rare.

### Leaves

At a tenth of the rate 2888 of the 20 000 leaves had no relay at the
end of the run. That includes the dark ones, about 8 %, and those no
relay reaches. Leaves changed relay 35 000 times in the hour; the run
does not record why. A leaf that has moved leaves its old address with
senders. Of the 106 texts whose delivery TTL ran out at a tenth of the
rate, 39 waited at the relay their leaf was on at the end of the run;
the rest waited at a relay their leaf had left, or for a leaf with no
relay. This, with phones dark for longer than the TTL, is the 8 % lost
at light load. It does not depend on capacity. The design expects the
sender to retry (spec "Delivery queue"), which the simulated applications do not do. At
light load 56 % of texts arrive within 5 s and 79 % within a minute.

## Against spec "Capacity and sizing"

| | Spec "Capacity and sizing" | This run, one domain | Scaled to 14 domains |
|---|---|---|---|
| Airtime per text | 0.67 s over 8 hops | 1.25 s median, 3.6 s mean over 3 to 4 hops (×0.5) | |
| Binding limit | channel time, 35 % usable | relay duty cap in the centre | |
| Texts/s at 90 % delivery | ≈ 80 city-wide | ≈ 2.2 | ≈ 31 |
| Texts per user per day | ≈ 23 | ≈ 9 at 20 000 users | ≈ 9 at 300 000 users |
| Heartbeat load per channel | ≈ 2.5 % | 2.4 % summed over the domain | |
| Keepalive load | 2 % of a channel at 80 leaves (spec section "Attach and polls" (before D46)) | 1.5 % of each relay's airtime; 37 % of a channel when all relays hear each other | |

Where spec "Capacity and sizing" is wrong, and why:

1. **Cost per hop.** Spec "Capacity and sizing" counts one DATA and one ACK per hop. Fading
   puts neighbours in the table that cannot take a DATA frame, and
   greedy forwarding tries them first. The measured cost is 4 to 6 DATA
   frames per hand-over that succeeds, 2.4 without fading.
2. **The binding limit.** Spec "Capacity and sizing" puts the limit at the channel. The
   channels stay under 10 % busy, and the relays' 10 % duty cap binds
   first. With uniform destinations the centre relays carry the through
   traffic and reach the cap at a quarter to half of the spec "Capacity and sizing" rate. Spec "Duty cycle cap"
   says the cap binds first in the centre. This confirms it. Spec "Capacity and sizing"'s table
   does not take it into account.
3. **Keepalives.** Spec "Capacity and sizing" counts heartbeats against the channel. It counts
   neither the leaves' keepalives nor the relays' ACKs of them. These are
   the largest load in the domain. They take 15 % of a relay's budget,
   and they make a domain where every relay hears every other unusable.
4. **Overload.** Spec "Capacity and sizing"'s figures read as a ceiling. The run shows a peak
   and then a collapse. Past the knee each text costs more, from longer
   searches, refusals and bans, so less gets through.

The 31 texts a second scaled to the city is an upper figure. Texts
crossing domains, and traffic passing through each domain, are not in
the run.

## Run time and memory

S4 at the spec "Capacity and sizing" rate, 4500 simulated seconds, took 377 s on the
development machine (32 cores, one used) and 358 MB. The 269 busy-time
observers and the per-second load samples are included. Eleven runs in
parallel took 5 to 18 minutes each. Profiling found the packet error
curve, 31 exponentials per evaluation, the hottest spot. It was being
evaluated again for signals that changed nothing at the receiver.
Skipping those (`4fe2b11`) saves 16 % and changes no result. The rest is
spread over the fading steps (12 %), the error curve (25 % with the
exponentials), the busy observers (7 %) and the event loop.
`simulator-design.md` §7 aims at a few minutes for one simulated hour:
the run takes about six.

## What this does not show

- The links are a model. Shadowing (6 dB) and fading (4 dB, 60 s) are
  the scenarios' figures, not measurements, and the result depends on
  fading more than on anything else. Bench and field measurements of how
  often a link that carries a heartbeat fails a DATA frame would settle
  it.
- One seed at most rates, two at ×0.5 and ×1.
- Traffic across domain borders and transit traffic are not modelled.
- Senders do not retry a lost text.
- The leaf core does not report refusals it receives, so how often
  leaves were told BUSY is not counted.
