# Rebooting into a new image at 64 times the design rate

**Status:** research note, 2026-11-10. Adds the simulator's model of the
reboot into an image gathered over the mesh (`bark-sim/src/install.rs`),
`scenarios/district-x64.toml` and `scenarios/district-x64-install.toml`.
Spec references are to `protocol.md` by section name; roadmap D170 is
the rule tested. The additions at the end test D180, the tables kept
across the swap, D181, the leaving flag, and D186, the wait for a leaving
node.

## Summary

Every node of the district reboots into a new image within one hour
while the district carries 64 times the design rate. Every node
installs: 2 027 a run, none held back past the bound. Nothing a node
holds when it reboots is dropped by the reset: it is kept and offered
again. But the run is not lossless. Each run loses 77 to 93 more texts
in the network than the same run without the installs, under 0.2 % of
the texts the network took. Most of them die at a node that has just
come back from its swap. While the installs go on the district delivers
a third fewer texts.

Two thirds of the relays wait out the whole 600 s bound. What holds them
is mostly texts in the delivery queue for sleepy nodes that do not come
for them, not the send queue.

## What was run

The city at 64 times the rate cannot be simulated in useful time: at 8
times it takes two hours (`district-2026-11-04.md`). The district is a
tenth of the city at the same densities and is where every earlier run
at 64 times was made (`pushback-2026-11-03.md`, `waited-2026-11-05.md`).
`district-x64.toml` is `district.toml` at 64 times the rate, run for
5 400 s. `district-x64-install.toml` is the same file plus an
`[install]` section; a test checks that nothing else differs. Seeds 1 to
3, release build of this branch:

```
bark-sim scenarios/district-x64.toml --seed N --messages bN.msg.csv
bark-sim scenarios/district-x64-install.toml --seed N --installs iN.csv \
    --messages iN.msg.csv
```

**How a node installs** (simulator design, "Install"). The pieces are not
simulated. At the design rate the district's relays gather the image 13
to 15 hours after the first holder gets it, and the sweeps keep them
close: 50 % to 90 % hold it within 1.2 hours
(`mesh-update-2026-11-07.md`). At 64 times the rate a run cannot be that
long. So each node's image becomes whole at a time drawn uniformly
within the hour after the 900 s settle time. Sleepy nodes take days to
gather an image, but here they install in the same hour as the relays,
which is harder than a real spread. From that moment the simulator does
what `bark-fw/src/node.rs` does after each driver step:

- it asks the core to drain while `bark_image::policy::drains` says so;
- it reboots once `policy::quiet` says so;
- it writes what `Relay::held` lists into the carry slots
  (`bark_app_layer::carry`);
- the node is off for the swap, 46.6 s. That is 227 pages at 187.8 ms
  each, plus the hash and the clock start, from the loader's figures in
  `bark-fw/README.md`, "Updates";
- the node boots with a new core and its stored state;
- it offers the kept messages again one at a time, ahead of its
  application's.

The simulator has no phone links and its mailboxes write at once, so
links are always none and nothing is being written. With `[update]`
instead of `[install]`, a node stages its image when it decodes the last
block.

The figures below count the texts sent between 900 s and 5 400 s, from
the start of the installs to the end of the run.

## The numbers

| seed | | sent | delivered | still on its way at the end | given up at the sender | lost in the network | lost, not counting expired delivery TTL | without a trace | latency p50 / p90, s |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | without | 79 830 | 64 631 (81.0 %) | 10 939 | 3 959 | 301 | 137 | 0 | 222 / 956 |
| 1 | installs | 60 821 | 42 381 (69.7 %) | 12 871 | 5 189 | 379 | 214 | 1 | 376 / 1 409 |
| 2 | without | 77 887 | 63 967 (82.1 %) | 9 317 | 4 007 | 596 | 148 | 0 | 194 / 946 |
| 2 | installs | 61 994 | 42 896 (69.2 %) | 13 115 | 5 244 | 739 | 241 | 0 | 349 / 1 490 |
| 3 | without | 71 675 | 56 900 (79.4 %) | 9 828 | 4 555 | 392 | 143 | 0 | 263 / 1 100 |
| 3 | installs | 57 167 | 37 031 (64.8 %) | 13 832 | 5 786 | 517 | 231 | 1 | 459 / 1 611 |

"Sent" counts the texts the people's applications handed over. A text
due while the previous one still waits is skipped and not counted, so
fewer are sent when relays refuse more. "Given up at the sender" counts
texts the sending node handed back with a wait longer than the delivery
TTL: they are refused at the sender and never taken by the network.

Network losses by cause, seed 1:

| | expired delivery TTL | no route | return failed | hop cap | banned | budget |
|---|---:|---:|---:|---:|---:|---:|
| without | 164 | 12 | 19 | 52 | 46 | 8 |
| installs | 165 | 75 | 34 | 61 | 36 | 8 |

The expired delivery TTL changes little in seed 1 and grows by 50 and 37
in seeds 2 and 3. The growth is in "no route" and "return failed".

**From staging to reboot**, per node, against the 600 s bound:

| seed | relays | min / median / p90 / max, s | at 600 s or more | sleepy nodes | min / median / p90 / max, s | at 600 s or more |
|---|---:|---|---:|---:|---|---:|
| 1 | 1 010 | 0 / 600.3 / 600.9 / 601.4 | 660 | 1 017 | 0 / 0 / 53.5 / 722.7 | 15 |
| 2 | 989 | 0 / 600.2 / 600.9 / 601.5 | 638 | 1 038 | 0 / 0 / 16.9 / 724.9 | 14 |
| 3 | 946 | 0 / 600.3 / 600.9 / 601.8 | 635 | 1 081 | 0 / 0 / 46.3 / 651.9 | 14 |

The relay counts differ from the 538 placed because people's nodes turn
relay or sleepy as their power changes.

**What was refused and kept:**

| seed | refused for draining | slots written | of them for a sleepy node or the node itself | messages kept | delivered | dropped | still on its way | without a trace |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 226 243 | 4 819 | 3 734 (77 %) | 4 672 | 1 624 | 202 | 2 845 | 1 |
| 2 | 212 444 | 4 376 | 3 184 (73 %) | 4 216 | 1 543 | 299 | 2 373 | 1 |
| 3 | 223 419 | 4 806 | 3 838 (80 %) | 4 665 | 1 535 | 285 | 2 845 | 0 |

A message kept by two nodes in turn is written twice and counted once
among the messages kept. The fates are at the end of the run, for
messages sent at any time.

Delivered texts per 10 minutes, seed 1, by when they arrived:

| minutes | 0–10 | 10–20 | 20–30 | 30–40 | 40–50 | 50–60 | 60–70 | 70–80 | 80–90 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| without | 8 903 | 9 036 | 9 471 | 9 534 | 10 192 | 9 504 | 8 786 | 9 006 | 9 485 |
| installs | 8 903 | 8 617 | 6 391 | 6 550 | 6 101 | 5 731 | 5 755 | 5 559 | 8 177 |
| relays draining or off, mean | 0 | 16 | 93 | 107 | 152 | 147 | 147 | 139 | 24 |

## What they mean

**Every node installs within the bound.** No relay is held back past
601.8 s. The 1.8 s over 600 s comes from two things: the bound counts
from the whole second after the drain began, and the policy is checked
after the node's next event. Sleepy nodes overshoot by up to 125 s.
Fourteen or fifteen a run hold their own text past the bound, and a
sleeping node next runs the check at its next wake-up, which may be a
beat away. The core's next wake-up does not include the bound. The same
holds in the firmware, which checks after each driver step.

**Most relays wait out the bound, and mostly for sleepy nodes.** About
two thirds of the relays still hold something at 600 s, and 70 to 80 %
of what they keep is in the delivery queue for their sleepy nodes. A
text there waits for a node that is dark, has moved to another relay,
or beats too seldom for the queue to empty. That is the delivery queue's
normal wait (spec "Delivery queue"), and the drain cannot shorten it. A
third of the relays empty quickly, half of them at once.

**Nothing is lost to the reset itself.** Every message a node held when
it rebooted was kept and offered again; none was lost by the reboot. Of
the kept messages a third were delivered by the end and about 60 % were
still on their way: the node offers them one at a time, each only once
the previous one has left, so 30 kept messages take a relay many
minutes. 5 to 7 % were dropped later. In seed 1 that was 67 by the
delivery TTL, for sleepy nodes that still did not come (the network
would have expired them anyway), 30 for no route, 3 at the hop cap and 1
for a failed return.

**What is lost, and why.** Seeds 1 to 3 lose 77, 93 and 88 more texts in
the network than without the installs, leaving out the expired delivery
TTL. Nearly all are "no route" and "return failed", and most die at a
node that came back from its swap less than a minute before. A trace of
seed 1 shows three mechanisms:

1. *A kept message at a node just back has no parent.* The node offers
   it as its own, with count 0, as D170 says. The node has just booted
   and knows few neighbours: relay 364 knew one 8.5 s after its return.
   The search runs out at once and ends at this node as "no route".
   Before the reboot it would have gone back to its parent or waited out
   the TTL. This is the cost D170 accepted, and the empty neighbour
   table after a boot makes it larger: 30 of seed 1's extra losses.
2. *A sender's way on depends on a draining relay.* The relay refuses,
   so the message waits at its sender. Then the relay goes silent for
   the swap. The sender's tries meet silence, its search runs out of
   neighbours, and the message ends as "no route" or "return failed".
   The trace of `ring.toml` with installs added shows it most clearly,
   since there each relay is the only way on: it loses 126 texts in the
   network against 14 without.
   Refusing pushes the message back as D170 means it to, but the wait
   then ends in a minute of silence, not in a node that takes it.
3. *Returns fail more often*: 34 against 19 in seed 1, 19 of them
   within 600 s of the dropping node's return. These were not traced.
   A node that has rebooted has lost its message records, which a
   return to it relies on.

Two texts in the three runs were lost without a trace, both kept
messages. In seed 1, text 56562 had been queued at node 807 for its
destination. Node 807 rebooted, kept the text, and offered it again
after the swap. Relay 45 took the copy, with an ACK. It had searched
for the same message 1 165 s before, and it absorbed the copy without
handing it on. That is the case of two copies of one message (simulator
design, "Accounting"), here made by the carry.

**What the installs cost the district.** While they go on, 100 to 150
of about 1 000 relays are draining or off at any moment, and the
district delivers a third fewer texts. Median latency is 1.7 to 1.8
times longer. A fifth to a quarter fewer texts are sent, since nodes that drain refuse
their own applications too. Delivery recovers once the installs end, at
86 % of the baseline in the last 10 minutes, with the kept messages
still being offered. Sleepy nodes gather an image over days, so a real
spread puts fewer nodes in the same hour, but at that rate the relays
would cost the same.

## Tests

`bark-sim/tests/scenarios.rs`:
`the_install_run_is_its_baseline_with_installs` checks that the two
files differ only by `[install]` and that `district-x64.toml` is
`district.toml` at 64 times the rate.
`nodes_reboot_into_a_staged_image_within_the_bound` has every node of
`mesh-spread.toml` install over its settled half hour. Each must reboot
within 601 s of its staging and come back after the swap, no kept
message may be dropped or lost without a trace, and every text to a
reachable destination must be accounted for.

## Addition, 2026-11-10: the tables kept across the swap

Roadmap D180 has a node keep its neighbour table and its message
records across the swap, aged by the swap's length. The same three
seeds were run again with it. Two things changed at once, so a third
run of each seed separates them:

- *The swap is longer.* The simulator now takes the swap's length from
  the loader's figures in `bark_image::boot::time`, with the external
  flash's bus at the 1 Mbit/s measured on the bench, not the 2 to
  3 Mbit/s of its instructions, and the hash of an image that fills the
  slot: 59.0 s, not 46.6 s. The bench measured 55 s from the reboot to
  the port back.
- *The tables come back.* At its reboot the node writes its tables as
  the firmware does (`bark_app_layer::tables`), and its new core takes
  them back before its first event.

"Swap 59.0 s" is this branch with the tables not given back; "tables"
is this branch.

| seed | | sent | delivered | still on its way at the end | given up at the sender | lost in the network | lost, not counting expired delivery TTL | without a trace | latency p50 / p90, s |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | without | 79 830 | 64 631 (81.0 %) | 10 939 | 3 959 | 301 | 137 | 0 | 222 / 956 |
| 1 | installs, swap 46.6 s | 60 821 | 42 381 (69.7 %) | 12 871 | 5 189 | 379 | 214 | 1 | 376 / 1 409 |
| 1 | installs, swap 59.0 s | 62 244 | 42 315 (68.0 %) | 14 049 | 5 405 | 474 | 319 | 1 | 413 / 1 430 |
| 1 | installs, tables | 61 686 | 44 845 (72.7 %) | 11 559 | 5 062 | 220 | 157 | 0 | 414 / 1 407 |
| 2 | without | 77 887 | 63 967 (82.1 %) | 9 317 | 4 007 | 596 | 148 | 0 | 194 / 946 |
| 2 | installs, swap 46.6 s | 61 994 | 42 896 (69.2 %) | 13 115 | 5 244 | 739 | 241 | 0 | 349 / 1 490 |
| 2 | installs, swap 59.0 s | 59 146 | 40 816 (69.0 %) | 12 384 | 5 262 | 684 | 322 | 0 | 349 / 1 510 |
| 2 | installs, tables | 60 455 | 44 173 (73.1 %) | 10 979 | 4 915 | 388 | 189 | 0 | 351 / 1 480 |
| 3 | without | 71 675 | 56 900 (79.4 %) | 9 828 | 4 555 | 392 | 143 | 0 | 263 / 1 100 |
| 3 | installs, swap 46.6 s | 57 167 | 37 031 (64.8 %) | 13 832 | 5 786 | 517 | 231 | 1 | 459 / 1 611 |
| 3 | installs, swap 59.0 s | 58 052 | 37 672 (64.9 %) | 13 993 | 5 704 | 683 | 339 | 0 | 454 / 1 587 |
| 3 | installs, tables | 57 333 | 39 830 (69.5 %) | 11 751 | 5 421 | 331 | 164 | 0 | 463 / 1 567 |

Network losses by cause, and of "no route" and "return failed" those
at a node that came back from its swap less than 60 s before:

| seed | | expired delivery TTL | no route | of them just back | return failed | of them just back | hop cap | banned | budget | other |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | without | 164 | 12 | | 19 | | 52 | 46 | 8 | 0 |
| 1 | swap 46.6 s | 165 | 75 | 33 | 34 | 18 | 61 | 36 | 8 | 0 |
| 1 | swap 59.0 s | 155 | 109 | 88 | 74 | 17 | 91 | 36 | 7 | 2 |
| 1 | tables | 63 | 18 | 1 | 21 | 0 | 78 | 34 | 6 | 0 |
| 2 | without | 448 | 27 | | 20 | | 38 | 53 | 9 | 1 |
| 2 | swap 46.6 s | 498 | 53 | 22 | 65 | 39 | 74 | 42 | 3 | 4 |
| 2 | swap 59.0 s | 362 | 150 | 124 | 59 | 27 | 74 | 33 | 5 | 1 |
| 2 | tables | 199 | 18 | 3 | 43 | 0 | 79 | 47 | 2 | 0 |
| 3 | without | 249 | 32 | | 24 | | 35 | 49 | 3 | 0 |
| 3 | swap 46.6 s | 286 | 85 | 43 | 60 | 29 | 37 | 45 | 4 | 0 |
| 3 | swap 59.0 s | 344 | 189 | 146 | 57 | 30 | 38 | 48 | 6 | 1 |
| 3 | tables | 167 | 27 | 0 | 23 | 0 | 48 | 59 | 5 | 2 |

"Just back" counts a drop at a node whose swap ended less than 60 s
before it, from the drop's time and node in the messages file and the
nodes' return in the installs file.

What was kept and what came back, with the tables:

| seed | neighbour entries kept | taken back | records kept | taken back | messages kept | dropped later |
|---|---:|---:|---:|---:|---:|---:|
| 1 | 68 739 | 62 320 (91 %) | 43 177 | 36 833 (85 %) | 3 720 | 40 |
| 2 | 67 975 | 61 508 (90 %) | 41 075 | 35 254 (86 %) | 3 725 | 155 |
| 3 | 64 017 | 57 850 (90 %) | 41 220 | 34 917 (85 %) | 4 203 | 170 |

An entry is not taken back when it expired during the swap; most of
those are a sleepy node's, whose entries are old since it hears only in
its windows, or a relay's record last active nearly 600 s before.

### What it means

**The losses at a node just back are gone.** Without the tables, and
with the longer swap, 88 to 146 texts a run end as "no route" at a node
less than a minute back, and 17 to 30 as "return failed"; with them, 0
to 3 and none. Mechanisms 1 and 3 above were the empty neighbour table
and the lost records, as traced.

**The extra losses fall by about three quarters.** Leaving out the
expired delivery TTL, the runs lose 20, 41 and 21 texts more than
without the installs, against 77, 93 and 88 with the shorter swap and
no tables, and 182, 174 and 196 with the longer swap and no tables.
More texts are delivered while the installs go on: 72.7 %, 73.1 % and
69.5 %, against 68.0 %, 69.0 % and 64.9 %. Expired delivery TTL falls
too, by 92 to 177 a run: kept messages for sleepy nodes find their
nodes sooner, and 40 to 170 kept messages are dropped later, against
124 to 354. Median latency does not change; the drain before the
reboot sets it.

**The swap's length matters.** With no tables, 12.4 s more off the air
adds 81 to 108 losses a run, nearly all "no route" at a node just back:
in a longer swap more of its neighbours' entries go unheard, and more
of its senders give up on it. The longer figure is the right one: it
matches the bench.

**What is left** is mechanism 2: the hop cap, 48 to 79 against 35 to
52 without the installs, and "return failed" away from a returning
node in seed 2, 43 against 20. These come from searches that run long
around relays that are draining or off, not from a node's state after
its boot. Nothing was tuned to these runs. No text was lost without a
trace in the three runs with the tables.

## Addition, 2026-11-10: the leaving flag (roadmap D181)

D181 has a node's heartbeats say that it is about to leave the air while
it drains, and its neighbours send round it while they have another way
(spec "Next hop"). It is aimed at the second mechanism above: a sender
whose way on drains and then falls silent for the swap. The runs below
are the same files and seeds, release build of the D181 branch on main
`c9d2a58`, without D180, which had not merged. They include the
exception that a leaving node is not gone round for a message that may
end there, addressed to it or to a place within 50 m of it; seed 1 run
without that exception is given where it differs.

| seed | | build | sent | delivered | still on its way at the end | given up at the sender | lost in the network | lost, not counting expired delivery TTL | without a trace | latency p50 / p90, s |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | without | before | 79 830 | 64 631 (81.0 %) | 10 939 | 3 959 | 301 | 137 | 0 | 222 / 956 |
| 1 | without | D181 | 79 798 | 65 150 (81.6 %) | 10 500 | 3 846 | 302 | 153 | 0 | 223 / 941 |
| 1 | installs | before | 60 821 | 42 381 (69.7 %) | 12 871 | 5 189 | 379 | 214 | 1 | 376 / 1 409 |
| 1 | installs | D181 | 64 372 | 45 075 (70.0 %) | 13 534 | 5 312 | 451 | 254 | 0 | 360 / 1 268 |
| 2 | without | before | 77 887 | 63 967 (82.1 %) | 9 317 | 4 007 | 596 | 148 | 0 | 194 / 946 |
| 2 | without | D181 | 75 670 | 61 997 (81.9 %) | 9 214 | 4 015 | 444 | 142 | 0 | 180 / 1 003 |
| 2 | installs | before | 61 994 | 42 896 (69.2 %) | 13 115 | 5 244 | 739 | 241 | 0 | 349 / 1 490 |
| 2 | installs | D181 | 63 449 | 45 273 (71.4 %) | 12 351 | 5 098 | 727 | 246 | 0 | 312 / 1 342 |
| 3 | without | before | 71 675 | 56 900 (79.4 %) | 9 828 | 4 555 | 392 | 143 | 0 | 263 / 1 100 |
| 3 | without | D181 | 73 568 | 58 487 (79.5 %) | 10 375 | 4 266 | 440 | 164 | 0 | 257 / 1 071 |
| 3 | installs | before | 57 167 | 37 031 (64.8 %) | 13 832 | 5 786 | 517 | 231 | 1 | 459 / 1 611 |
| 3 | installs | D181 | 57 634 | 38 927 (67.5 %) | 12 338 | 5 688 | 681 | 193 | 0 | 397 / 1 441 |

The runs without installs change too, since a relay that flushes to turn
sleepy sets the flag, but within what one seed differs from another.

**Losses do not fall.** Not counting the expired delivery TTL, the
installs lose 101, 104 and 29 more texts in the network than their own
runs without, against 77, 93 and 88 before: 693 in the three runs
against 686. The expired delivery TTL moves by tens to hundreds between
runs of one seed, with or without installs, and is left out here as
above. What does improve is delivery while the installs go on: 0.3 to
2.7 points more of the texts sent, more texts sent, and a median latency 16 to 62 s shorter. In the ring of the
first trace (`ring.toml` with installs), where each relay is the only way
on, the network loses 50 texts against 127, and delivers 1 865 of 2 110
sent against 1 278 of 1 730.

Network losses by cause, seed 1:

| | expired delivery TTL | no route | return failed | hop cap | banned | budget | other |
|---|---:|---:|---:|---:|---:|---:|---:|
| before | 165 | 75 | 34 | 61 | 36 | 8 | 0 |
| D181 | 197 | 60 | 40 | 109 | 39 | 6 | 0 |
| D181 without the exception | 188 | 68 | 41 | 114 | 28 | 9 | 4 |

"No route" falls by 15 and the hop cap rises by 48; in seeds 2 and 3 the
failed returns fall, 65 to 16 and 60 to 22, and the hop cap rises, 74 to
114 and 37 to 55.

**The losses traced.** Seed 1 was run again with the trace, before and
with D181, and each text lost in the network, not counting the expired
delivery TTL, put in the first class that fits it, from the installs'
log and the frames sent of it:

| | before | D181 |
|---|---:|---:|
| kept by a node across its swap, lost after | 65 | 68 |
| the dropping node had sent it to a node draining or off | 29 | 26 |
| an earlier hop had sent it to a node draining or off | 60 | 80 |
| of these, at the hop cap | 31 | 65 |
| dropped at a node back from its swap within 600 s | 18 | 21 |
| passed through such a node | 29 | 40 |
| none of these | 13 | 19 |
| all | 214 | 254 |

The class D181 aims at, the sender whose way on drains and then falls
silent, is the second row: 29 before and 26 with it, so the flag
removes few of these. Where the way on drains, the drain's refusal
already sent the text round it (spec "Bans"), so going round before the
offer saves frames, not texts.

**Why the hop cap rises.** Every text lost at the hop cap, before and
with D181, met refusals for draining on its way: 3 915 refusals for 61
texts before, 3 022 for 109 with D181. With D181, 1 966 of them came
from the node where the text would end, the destination or the relay
that holds mail for it, against 1 152 before. Most of these texts were
sent before or during that node's drain and dropped after it came back.
The text can end only there, the node refuses it for draining, and its
neighbours send it round the refuser as for any refusal, to another of
them, which offers it again, until the count reaches 255
(spec "Hop count"). Before D181 fewer texts got that far while it drained: the
draining relays on the way refused them first, and they waited behind.
With D181 they go round those relays and reach the neighbourhood of the
draining destination, where they wander. The exception for a node a
message may end at changes little, 114 to 109 texts at the hop cap, as
the destination still refuses them.

So the loss that remains is the drain's refusal of a text that can end
only at the draining node: sent round it, the text has nowhere to go.
Waiting for that node, at the neighbour that holds the text, until it
is back would keep it; this needs a rule of its own, D186, the next
addition.

## Addition, 2026-11-10: the wait for a leaving node (roadmap D186)

D186 has a text refused by a leaving neighbour where it may end wait in
the delivery queue of the node that holds it, and go on once the
neighbour's heartbeat no longer says leaving (spec "Delivery queue"). The
runs below are the same files and seeds on main `a64f625`, which has
D180, release builds:

- *D180*: the "tables" runs of the D180 addition, main before D181;
- *D181*: main with D181 merged, commit `9cbd9c8`;
- *D186*: that with D186, commit `03d34d2`.

"Without" is the D186 build without installs; the run of the first
addition, "before", is given beside it as the runs without installs
move with the build too. Lost texts are counted as above.

| seed | | sent | delivered | still on its way at the end | given up at the sender | lost in the network | lost, not counting expired delivery TTL | of them at the hop cap | without a trace | latency p50 / p90, s |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | without, before | 79 830 | 64 631 (81.0 %) | 10 939 | 3 959 | 301 | 137 | 52 | 0 | 222 / 956 |
| 1 | without, D186 | 79 105 | 64 542 (81.6 %) | 10 597 | 3 746 | 220 | 127 | 38 | 0 | 203 / 902 |
| 1 | installs, D180 | 61 686 | 44 845 (72.7 %) | 11 559 | 5 062 | 220 | 157 | 78 | 0 | 414 / 1 407 |
| 1 | installs, D181 | 62 028 | 46 224 (74.5 %) | 10 415 | 5 151 | 237 | 198 | 105 | 1 | 361 / 1 267 |
| 1 | installs, D186 | 66 221 | 51 987 (78.5 %) | 9 348 | 4 642 | 242 | 150 | 52 | 2 | 315 / 1 174 |
| 2 | without, before | 77 887 | 63 967 (82.1 %) | 9 317 | 4 007 | 596 | 148 | 38 | 0 | 194 / 946 |
| 2 | without, D186 | 74 424 | 61 261 (82.3 %) | 8 689 | 4 074 | 400 | 142 | 35 | 0 | 181 / 1 011 |
| 2 | installs, D180 | 60 455 | 44 173 (73.1 %) | 10 979 | 4 915 | 388 | 189 | 79 | 0 | 351 / 1 480 |
| 2 | installs, D181 | 63 827 | 47 576 (74.5 %) | 10 868 | 5 021 | 362 | 206 | 98 | 0 | 360 / 1 366 |
| 2 | installs, D186 | 72 962 | 56 553 (77.5 %) | 11 236 | 4 567 | 606 | 199 | 65 | 0 | 317 / 1 243 |
| 3 | without, before | 71 675 | 56 900 (79.4 %) | 9 828 | 4 555 | 392 | 143 | 35 | 0 | 263 / 1 100 |
| 3 | without, D186 | 73 335 | 57 634 (78.6 %) | 10 870 | 4 459 | 372 | 167 | 45 | 0 | 250 / 1 082 |
| 3 | installs, D180 | 57 333 | 39 830 (69.5 %) | 11 751 | 5 421 | 331 | 164 | 48 | 0 | 463 / 1 567 |
| 3 | installs, D181 | 57 172 | 41 048 (71.8 %) | 10 347 | 5 432 | 344 | 175 | 93 | 1 | 428 / 1 508 |
| 3 | installs, D186 | 64 042 | 47 347 (73.9 %) | 11 215 | 5 081 | 399 | 133 | 37 | 0 | 360 / 1 313 |

**The hop cap falls back.** With D181 on D180, 105, 98 and 93 texts a
run die at the hop cap; with D186, 52, 65 and 37, against 38, 35 and 45
without the installs. Losses not counting the expired delivery TTL fall
from 579 in the three runs with D181 and 510 with D180 alone to 482,
against 436 without the installs: the installs now cost 23, 57 and −34
texts a run. Seed 2 keeps the most, 30 more at the hop cap than its own
run without; these were not traced.

**More is delivered, sooner.** While the installs go on, 78.5, 77.5 and
73.9 % of the texts sent arrive, against 74.5, 74.5 and 71.8 % with D181
and 72.7, 73.1 and 69.5 % with D180, and more texts are sent, as fewer
wait at their origins: 66 221, 72 962 and 64 042 against 57 172 to
63 827. The median latency falls 43 to 68 s from D181. Refusals for
draining fall from 197 944 with D180 and 169 859 with D181 to 112 634 in
seed 2, as a text refused once by a draining node now waits instead of
being offered to it again by each of its neighbours.

**What it does not change.** The relays still wait out the bound,
median 600.2 s from staging to reboot, held mostly by texts for sleepy
nodes, as the first addition found. "No route" rises with D181, 27 to
35 a run against 18 to 27 with D180, and stays there with D186, 32 to
40. The expired delivery TTL moves by hundreds between runs of a seed,
as above. A text that waits for a leaving node and expires does so at a
node that did not queue it for a sleepy node of its own; such drops
count 2, 3 and 6 a run, against 0 to 6 in the runs before D186. Two texts in seed 1 were lost without a
trace: one a kept message, the case described above, the other not
traced.

The ring of the first trace, `ring.toml` with every node installing
within 600 s of the settle time, all texts of the run counted:

| | sent | delivered | lost in the network |
|---|---:|---:|---:|
| without installs, D186 | 3 670 | 3 519 (95.9 %) | 14 |
| installs, D180 | 2 211 | 2 088 (94.4 %) | 31 |
| installs, D181 | 1 876 | 1 543 (82.2 %) | 34 |
| installs, D186 | 2 157 | 1 866 (86.5 %) | 26 |

In the ring each relay is the only way on and no text dies at the hop
cap, so D186 has little to act on there: the losses are "no route" and
failed returns. That the ring delivers less with D181 than with D180
alone, more of its texts still on their way at the end, 243 against 55,
is one run of one seed and was not traced.
