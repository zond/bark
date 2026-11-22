# Keeping versions compatible after the first deployment (2026-11-22)

**Status:** thoughts for a decision not yet due. Nothing here is a rule
yet: the rule in force is `AGENTS.md`, "Versions before the first
deployment". Its last paragraph says that after the first deployment a
change an older node or app cannot follow goes out in two releases. This
note says what that takes in each place two versions meet, what it costs,
and what the protocol lacks for it. Each proposal below that changes a
protocol rule needs the user's approval before it is built.

## Where two versions meet

| Where | What says the version | How old and new live together today |
|---|---|---|
| Radio frames | the format byte of every packet; a heartbeat's is the newest its sender speaks (spec "Packet formats") | a node speaks every format from 1 to its newest and sends a neighbour the lower of the two (spec "Hand-over", Format) |
| Phone link | Node's and Recipient's `version` and `oldest` (spec "Versions") | each side serves the other if its version is at least the other's `oldest`; values the node writes only grow at the end; a change of meaning takes a new service UUID |
| Message payload | the format byte's high four bits; a card and a mailbox summary say the newest version their side reads (spec "The payload") | a sender writes no newer than its reader is known to read |
| The node's stored state | the layout in each page's head (spec "Implementation notes") | layout 3 reads a page of layout 2 once and writes it again as its own (roadmap D241) |
| The app's stored files | a version in each file | `bark.json` version 4 reads version 3; an identity file's messages likewise |
| Firmware over the mesh | the image header's format, the key a signature names, the block table (spec "Updates over the mesh") | none: there has been one form |

## What each costs

The app can carry every old form it ever had: a phone has the room, and
its owner updates it when they please, so it meets the longest tail.

The firmware cannot grow without bound. The flash slot, 908 KiB, is not
what binds: an image travels over the mesh in at most 48 blocks, 47 of
image, 742 976 bytes packed, and CI fails a build past 40 blocks, about
616 KB (`bark-fw/scripts/image.py`, `BUDGET_BLOCKS`). On 2026-11-22 the
node image is about 571 KB in 38 blocks: about 45 KB under the budget and
about 170 KB under the limit.

What an old form costs the firmware:

- **A stored layout's reader:** tens to a few hundred bytes. Layout 2's
  is about forty lines (`bark-fw/logic/src/store.rs`, `scan_before`).
- **A phone link value with fields added at its end:** nothing. A change
  of meaning takes a second service table beside the first, kilobytes.
- **A radio format's decoder:** small. A format that changes a rule, not
  only a field, costs far more than its bytes: the node carries both
  rules, and every test of the one needs its twin for the other.

So bytes are not the first limit. The number of rule sets a node must
keep right at once is.

## What differs between nodes and phones

Nodes update themselves over the mesh; a phone updates when its owner
does. So a node meets an older neighbour only while a release spreads,
and for the nodes that were switched off meanwhile. The firmware needs a
window of old forms, not all of them. The app is the side to carry the
long tail.

A node that was off for a year takes many releases in one update. Its
stored state then comes from a layout several back. Losing it means a
person goes to the node and pairs again, and for a node on a roof that
is a climb. So the readers of old stored layouts are the ones to keep
longest, and they are the cheapest.

## What the protocol lacks

**The radio has no floor.** Spec "Packet formats" says a node speaks
every format from 1 to its newest. As written, no format can ever be
dropped. The phone link has `oldest` for this; the radio has nothing.

*Proposal: a window, with no new field.* A node speaks the formats from
its newest less N to its newest. A heartbeat already says its sender's
newest, so a neighbour's floor follows from it. Two nodes can exchange
messages when their newest formats differ by at most N, in the lower of
the two, as now. One rule is new in the core: a neighbour whose newest
format is more than N from this node's is no next hop. Without it a node
would offer frames the other rejects and count the silence as failures.

Three things to settle with it:

1. *The window counts formats, not releases.* The wire carries the
   format, and formats will change far less often than releases.
2. *N is fixed once nodes are out.* An old node assumes the old N of
   every neighbour. A later release that narrows the window makes old
   nodes offer frames the new ones reject: airtime wasted, nothing
   broken, but it does not mend itself. Choose N generously, once. An
   explicit floor in the heartbeat would not have this fault and costs a
   byte, which is a block of symbols, 1.28 ms a heartbeat.
3. *The update's path must outlive the window.* A node outside the
   window can come back only by gathering a new image, for which it must
   still read the heartbeat's section and the piece frame. A heartbeat's
   first 13 bytes are the same in every format; the section after them
   and the piece frame are not promised. A format that changed either
   would strand every node outside the window for good. So: the window is
   for messages; the section and the piece frame stay as they are, or
   change only by two releases with a window much longer than N.

**Firmware over the mesh has one form.** The new image must be one the
old firmware takes: the same header format, a signature by a key it
holds, the same blocks and code. Any change to those is a two-release
change by its nature: the first release teaches nodes the new form while
still travelling in the old. A new signing key already goes this way
(`bark-fw/README.md`, "Updates", Keys).

**No number says how far back a reader is kept.** "Reads the one before"
loses the state of a node two layouts behind. Once nodes are with people
the project does not know, the guide should say how far back each kind
of reader is kept. A first suggestion:

| Reader | Kept |
|---|---|
| the node's stored layouts | every layout a released image wrote, until the image's budget forces a choice |
| the app's stored files and identity files | every version a released app wrote |
| payload versions, in the app | every version |
| phone link versions, in the node | the window the app's own releases need: `oldest` raised only when no released app below it is in use that the project knows of |
| radio formats | newest less N |

## What a change is, by cost

When a change is planned, say which kind it is:

1. **A field added where the form allows it** (the end of a value the
   node writes, a new flag bit a receiver ignores, a new record type in a
   payload): free, one release.
2. **A new form of stored data:** one release, with the reader of the
   form before. Test it as D241's was: the old form written by the old
   code, read by the new, with the power cut at every write of the
   change-over.
3. **A new form on the air or the link that an older side cannot
   follow:** two releases. The first reads the new form and does not
   send it; the second sends it once the first has spread.
4. **A changed rule:** as 3, and the node carries both rules for the
   window. The expensive kind; avoid it where a new field would do.

## Not supported, on purpose

A node flashed back to an older image does not see what a newer layout
wrote since. An older image cannot know a newer layout, and going back
is a bench act.

## Tested so far

The version rule itself is tested by a build that claims a later version
(roadmap D208). The one real change-over so far, the node's stored layout
2 to 3, ran on a board on 2026-11-22: board f4f5 on release v46 with one
paired phone and a stored home channel took the new build and kept both.
