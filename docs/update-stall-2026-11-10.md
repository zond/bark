# An hour with no pieces sent

2026-11-10. From 13:20 to 14:27 two nodes that held image version 24,
c367 and 3112, sent not one piece, while a neighbour gathered version 24
and lacked all but its first block. At 14:27 c367 was flashed with
version 25 and served at the normal pace within seconds. This note finds
why, from the four bench boards' USB logs (c367, 3112, f4f5 and bac7,
within a few metres of each other), and gives the rule that changed
(roadmap D191).

## What the logs show

Times are the host's, from each boot's `=== port open` and the uptime.
Each image is named by the first 4 bytes of its name, as the
`update: holds` and `update: offered` lines print them.

- 13:11:46. c367 is flashed with a build of version 24 named `1b7ce751`
  (`update: holds version=24 blocks=35 name=[1b, 7c, e7, 51]`).
- 13:13. f4f5, on a version 23 bench image, is offered it and takes it
  (`update: offered version=24 ... name=[1b, 7c, e7, 51]: taken`). c367
  streams to it (118 `update: serving` lines, blocks 0 and 1), and f4f5
  decodes block 0 at 13:15 (`block 0 decoded and written, 1 of 35`).
- 13:19:58. c367 is flashed again, with a later build of main, also
  version 24 but named `671c74b5`. 3112, still on 23, hears it at 13:20:03
  (`update: offered version=24 ... name=[67, 1c, 74, b5]: not taken`).
- 13:20:36. 3112 is flashed with that build
  (`update: holds version=24 blocks=35 name=[67, 1c, 74, b5]`).
- 13:20 to 14:27. Every 10 s line of both holders reads
  `update=holds:24 serving:0:0`, 400 lines each, and neither logs a piece.
  Every line of f4f5 reads `update=gathering:24 blocks:1/35`. f4f5 skipped
  5 heartbeats in that time, so its section was heard; it logs no offer of
  `671c74b5`.
- 14:27. c367 is flashed with version 25. f4f5, 3112 and bac7 take it
  within seconds, and each decodes block 0 within minutes.

## Why nothing was sent

The holders and the gatherer named two different images of one version.

A holder streams to a neighbour only while that neighbour's section says
it gathers the image the holder holds (spec "Updates over the mesh",
Who sends what, rule 3; `node/relay/update.rs`, `gatherer`, which takes a
section only if `Section::gathers` the holder's tag). f4f5's section
named `1b7ce751`; the holders held `671c74b5`. So `needs` found no channel
that lacked anything, and each line read `serving:0:0`.

A gatherer is offered a neighbour's image only if its version is above
the one it runs and the one it gathers (rule 1; `section_heard`, the
`image.version <= newest` test). The holders' 24 was not above f4f5's 24,
so f4f5 was never offered it, and nobody held the image it gathered.
Neither side could move until a higher version appeared, and when 25 did,
everything ran at once.

This is not a mix of old and new code. The heartbeat's section has the
same layout at the commit the version 23 images were built from (main of
2026-11-09 evening) as on main, the update code in `bark-proto` changed
by one test line since, and both sides followed rules 1 and 3 as main
has them. Two nodes of one build reach the same state whenever their
gatherer took another build of the version they hold, and on the bench
that is common: c367's log alone shows five builds of version 23 today.
A release, whose version CI takes from its tag, never carries two
images of one version.

## The other candidates

- The helper choice with two holders. Never reached: a holder chooses
  among gatherers of its own image, and there was none.
- A holder with no stored place. c367 booted at 0, 0 at 13:19:58. The
  only rule that uses distance (`streams_to`, for a gatherer off the
  holder's home channel) is asked only after `gatherer` has found a
  gatherer of the holder's image, so it was never asked. At 14:27 c367
  served from the same code.
- The lease against a clock that starts before boot. Likewise only asked
  of a gatherer of the holder's image; at 14:27 the same code served
  within seconds of the first rotation's end, as before.
- A section read differently across builds. The section's encoding is
  unchanged; 25, announced by the new code, was taken by the version 23
  nodes and served to them.
- The leaving flag and the wait for a leaving node. Set only while a node
  drains for a reboot or to turn sleepy; none did in the hour.
- Block 0. f4f5 already held it.

## bac7

bac7's own log of the hour was lost (`log: 21849 records dropped`), and
by the person's reading of its screen it too stood at 1 of 35 blocks of
24, which fits it having taken `1b7ce751` at 13:13 as f4f5 did. Its log
from 14:25 shows another fault: from then until the log ends at 14:42,
every heartbeat it tried was skipped for a busy channel
(`HeartbeatSkipped`, each channel in turn, never more than 14 s apart,
100 lines), so no node heard its section. c367's boot at 14:27 counted
two relays where its boot at 13:20 counted three. bac7 still gathered 25:
it decoded block 0 at 14:30:23 and block 1 at 14:36:04, 3112 at 14:30:30
and 14:35:50, so it took the pieces c367 streamed to 3112, on a channel
the two seem to have shared. Why every check of bac7's found the channel
busy is open; on the bench its console's `lbt <k>` prints whether the
CAD or the reading said busy, and the reading against the node's busy
threshold would tell a reading that failed from a noise floor that is
off.

## What changed

Rule 1 also offers an image of the version a node gathers under another
name, once, when the neighbour that names it holds its first block and
no relay the node hears holds the block the node lacks of its own
(`node/relay/update.rs`, `stranded_for`; roadmap D191). Taken, it
replaces the image gathered, as a higher version does, and the blocks
held of the first are lost. A gatherer that still hears a holder of its
own image keeps it.

`tests/update.rs`, `a_gatherer_whose_image_nobody_holds_takes_another_of_its_version`,
plays the bench: three relays, one holding an image, two gathering it
and decoding its table, then both others told they hold another image of
the same version. Without the change the third is never offered it;
with it, it is offered within a rotation and comes to hold it.
`a_gatherer_with_a_holder_keeps_its_image` checks that a gatherer that
hears a holder of its image is offered no other of its version.

Resolved otherwise the same day (roadmap D198): a version names one
image, as a release's does, so the rule above and its tests were removed,
and a node now only logs a neighbour that names another image of its
version, so that a bench sees two builds under one number at once.
