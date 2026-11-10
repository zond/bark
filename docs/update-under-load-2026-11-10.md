# Rebooting into a new image at 64 times the design rate

**Status:** research note, 2026-11-10. Adds the simulator's model of the
reboot into an image gathered over the mesh (`bark-sim/src/install.rs`),
`scenarios/district-x64.toml` and `scenarios/district-x64-install.toml`.
Spec references are to `protocol.md` by section name; roadmap D170 is
the rule tested.

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
