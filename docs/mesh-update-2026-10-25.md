# Firmware updates over the mesh (2026-10-25)

**Status:** design study, nothing built. Spec references are to
`protocol.md` by heading. Airtimes use the corrected SF5 figures being
brought into the spec at the same time: a 255-byte frame 138.6 ms, a
17-byte ACK 16.96 ms, an 11-byte heartbeat 14.40 ms.

## The question

Can a firmware image reach a planted relay that nobody visits and no
phone is paired to, over the mesh itself? The intended shape: the image
spreads as coded pieces at the lowest priority; a node that has the
whole image checks its signature against a public key it knows and
installs it. Sleepy nodes may take days. A node chooses whose signatures
it accepts, so that another implementation of the protocol is never
overwritten against its will, and may or may not pass on files it does
not want.

The mechanism comes later, after updates over the phone's BLE link. The
question now is what must be fixed before the first field deployment,
because a node in the field can only ever be changed by what its first
image already knows how to do.

## Facts

### The image today

Built 2026-10-25 from main `1654017`, `cargo build --release` in
`bark-fw/`, sizes by `llvm-size -A` on the ELF:

| Section | Node image | Console image |
|---|---|---|
| vector table | 256 B | 256 B |
| `.text` | 149 500 B | 79 684 B |
| `.rodata` | 16 200 B | 12 320 B |
| `.data` (copied from flash to RAM at boot) | 175 952 B | 2 120 B |
| **flash** | **341 908 B (334 KiB)** | **94 380 B (92 KiB)** |
| `.bss` | 5 556 B | 7 224 B |
| **RAM before the stack** | **181 508 B (177 KiB)** | 9 344 B |

`scripts/uf2.py` turns the node image into 1336 UF2 blocks, each 512
bytes carrying 256 bytes of flash, family `0xADA52840`, contiguous from
`0x26000` to `0x79700`: a 684 kB file holding 342 016 bytes of image.
The console image is 369 blocks.

173 122 of the 175 952 bytes of `.data` are zero: they are the relay's
empty tables (`Relay::UNSET`, `core-interface.md` Memory), stored in
flash so that they can be copied to RAM. The code and constants are
166 kB. The whole image compresses to 117 kB with `gzip -9` and to
103 kB with `xz -9e`; coding only the runs of zeros brings it to
170 kB. If the tables were placed in `.bss`, which the startup code
zeroes and which takes no flash, the image would be about 170 kB. This
note keeps the user's 250 kB as the size to plan for, since BLE (roadmap
D5) and the application are still to come.

### Flash

From `bark-fw/memory.x`, after the bootloader's own linker script and
the S140 6.1.1 SoftDevice specification:

| Range | Size | Holds |
|---|---|---|
| `0x00000`–`0x01000` | 4 KiB | MBR |
| `0x01000`–`0x26000` | 148 KiB | SoftDevice S140 6.1.1, kept, never enabled by us |
| `0x26000`–`0xF3000` | 820 KiB | the application |
| `0xF3000`–`0xF4000` | 4 KiB | the node's stored state (roadmap D73, `src/store.rs`) |
| `0xF4000`–`0xFD800` | 38 KiB | bootloader code |
| `0xFD800`–`0xFE000` | 2 KiB | bootloader config |
| `0xFE000`–`0xFF000` | 4 KiB | MBR parameter page |
| `0xFF000`–`0x100000` | 4 KiB | bootloader settings page |

Of the application's 820 KiB the image takes 334 KiB, so 486 KiB are
free: room for a second slot as large as the first, and more. Pages are
4 KiB, the erase unit; a page erase takes 85 ms and a word write 41 µs,
with the CPU stopped (nRF52840 Product Specification, NVMC; quoted in
`src/store.rs`).

### RAM

The application has 255 KiB (`memory.x`). The relay takes 173 840 B
(`core-interface.md` Memory, measured 2026-10-24), and the image as a
whole 177 KiB before the stack, which leaves 79 612 B (77.7 KiB) for the
stack, whose deepest single frame is 23 KiB (`bark-fw/README.md`), and
for BLE, under 5 KiB (`implementation-research-2026-10-14.md`). A
decoder that needs 10 to 20 kB fits; the one proposed below needs about
6 kB.

### The bootloader the board ships with

The T-Echo ships the Adafruit nRF52 bootloader; `INFO_UF2.TXT` is
expected to report version 0.6.1 with S140 6.1.1 (`bark-fw/README.md`,
roadmap D6). From its repository (github.com/adafruit/Adafruit_nRF52_Bootloader,
read 2026-10-25 at `c67f0bc`):

- **Signatures.** Version 0.6.1 verifies none: a UF2 copy, and its
  serial and BLE update paths, check at most a CRC. Signature checking
  (ECDSA over P-256 with SHA-256, `SIGNED_FW=1`) was added on 2026-01-15
  and first released in 0.10.0; it is off by default, a build-time
  option with the key compiled in, and it switches UF2 off, since "UF2
  format does NOT support signing keys" (README, "Signed firmware
  support"). It checks only images that arrive through its own serial
  or BLE path.
- **Two slots.** Also added 2026-01-15, in 0.10.0, off by default
  (`DUALBANK_FW=1`): the upload goes to a second bank and is copied
  over the first only once complete, so "in case the firmware upload
  fails, the bootloader will revert to the last working firmware
  version" (README, "Dual bank firmware support"). It protects the
  upload, not the boot: nothing reverts an image that was installed
  whole and then fails to run. There is no path by which the running
  application hands it a staged image.
- **Self-replacement.** Since 0.4.0 it replaces itself from a UF2 file
  copied to its drive (`update-{board}.uf2`, UF2 family `0xd663823c`;
  changelog 0.4.0): it writes the new bootloader into application flash
  and has the MBR copy it into place (`SD_MBR_COMMAND_COPY_BL`,
  `src/usb/msc_uf2.c`). No probe is needed. The same MBR command is open
  to the application, and nothing in the bootloader write-protects its
  own region, so the application can also replace it.
- **Booting the application.** After a UF2 copy it records the
  application as valid with a CRC of 0, which means no CRC is checked
  (`bootloader_dfu_update_process` in `bootloader.c`, 0.6.1), and from
  then on jumps to `0x26000` whenever the first two words there are not
  erased. So whatever the application later writes above its own first
  page does not stop the bootloader from starting it.
- **BLE update.** Its BLE path needs the SoftDevice and is entered only
  by a button combination at reset or by the application setting
  `GPREGRET` to `0xA8` before a soft reset (README). Our firmware does
  neither, so the path is closed to the air.

### Other bootloaders that verify

- **MCUboot** (docs.mcuboot.com, "Design"): two slots with a swap that
  can be undone ("test" images that must confirm themselves, else
  reverted at the next boot), signatures by RSA, ECDSA or Ed25519,
  downgrade prevention by version or by a security counter. In Nordic's
  SDK its default partition on the nRF52840 is 48 KiB (0xC000; Nordic,
  "Device Firmware Update (DFU) with MCUboot bootloader", DevZone). It
  is C, normally built with Zephyr or Nordic's SDK, and expects to be
  the first thing the chip runs, in place of the MBR and the board's
  bootloader.
- **embassy-boot** (embassy.dev book, "Bootloader"; source read at
  embassy `002a412`, 2026-10-25): Rust, about 8 kB, on the nRF52 with or
  without the SoftDevice. Four regions: the loader, ACTIVE (written only
  by the loader), DFU (written by the application, one page larger than
  ACTIVE) and a state page. On request it swaps ACTIVE and DFU page by
  page in a way that survives a power cut, and if the new image does
  not mark itself booted before the next reset it swaps back.
  Signatures are Ed25519 (RFC 8032), checked by the application before
  it asks for the swap (`verify_and_mark_updated`), and on main since
  2026 optionally by the loader as well (`verify_update`). The nRF part
  can run the watchdog while it copies (`WatchdogFlash`).

## The bootloader: the recommendation

**Keep the Adafruit bootloader untouched, and put a second-stage loader
at the bottom of the application region.** Roadmap D6 stands.

- The Adafruit bootloader stays what it is now: the USB recovery path.
  A board whose application is broken is double-tapped and given a
  UF2, as today. It is not rebuilt, and its signature option is not
  used: that option guards only images that arrive through its own
  paths, none of which the mesh uses, and it costs the UF2 path.
- At `0x26000`, where the Adafruit bootloader jumps, sits a small loader
  of our own built on embassy-boot. It swaps a staged image into place
  and swaps it back when the new image does not confirm itself. The
  firmware image and the loader go to the board together as one UF2,
  so bring-up does not change: one copy to `TECHOBOOT`.
- The signature is checked by the running application before it asks
  for the swap, never by the loader. The loader then holds no key, and
  the keys live in the image, where an update can change them. A key
  compiled into the one part that is never updated could never be
  replaced or withdrawn. The loader checks only that the staged image
  matches the length and hash the application recorded, against flash
  that went bad.
- Before it jumps to the application, the loader write-protects its own
  pages and the Adafruit bootloader's until the next reset (nRF52840
  Product Specification, ACL), so that a faulty image cannot erase
  either.

*Why not replace the bootloader.* Replacing it with MCUboot, or with a
rebuilt Adafruit bootloader that verifies, gains nothing for the mesh,
whose images come in through the application anyway. It risks a board
that only a probe on its SWD pads can recover, and either loses the UF2
path or keeps it open, which defeats the signature. A loader in
application flash gives the same swap and revert, can be replaced by
UF2 at any visit, and leaves every board recoverable by a double tap.

*Why embassy-boot and not MCUboot chained the same way.* MCUboot could
also be placed at `0x26000`, but it is a C build of 48 KiB with its own
image header, in a Rust repository whose firmware already uses Embassy.
embassy-boot is a sixth of the size and builds with the firmware. MCUboot
is far more widely deployed; the swap is the part that must not fail,
and it is tested on the bench below whichever is chosen.

*What it costs at bring-up.* The layout below, the loader, and the
packing of loader and image into one UF2. Bring-up step 1 waits for none
of it; it must be in the first image that goes to the field.

*What it risks.* A loader bug that survives the bench bricks every
board until someone double-taps it. The tests: power cut at random
points of 100 swaps and 100 reverts, and a new image that hangs, that
panics, and that never confirms.

### Flash layout

The application region holds 205 pages:

| Pages | Size | Holds |
|---|---|---|
| 4 | 16 KiB | the loader, at `0x26000` |
| 1 | 4 KiB | the loader's state |
| 88 | 352 KiB | ACTIVE: the running image |
| 89 | 356 KiB | DFU: the staged image, one page larger (embassy-boot's swap needs it) |
| 23 | 92 KiB | staging: the update's own state and coded pieces waiting to be decoded |

D73's page at `0xF3000` stays where it is. Today's 334 KiB image fits in
ACTIVE; moving the relay's tables to `.bss` would leave it half full,
with room for BLE. The SoftDevice's 148 KiB stay reserved; the Adafruit
bootloader uses them only for its BLE path, which we do not use, but
whether it boots the application without them is not verified, and
they are not needed.

A swap moves each page three times, about 3 × (85 + 42) = 381 ms a page
by the NVMC times above, so 34 s for ACTIVE, and as long again to swap
back. At three erases a page per update, the 10 000 erase cycles of the
flash allow over 3000 updates.

## The transport

### Where it lives

The sending and the priority touch the bucket, the send queue and the
heartbeat schedule, so they belong in the protocol core, and the
simulator runs them (AGENTS.md: what cannot be expressed in the core
does not go in the firmware). The core does no I/O: it asks the driver
to store each piece it accepts and is told when a block decodes,
through new actions and events in `core-interface.md`. The decoder, the
flash and the signature check sit in the driver, in a `no_std` crate
the host tests too. In the simulator the pieces need not carry bytes:
twenty thousand nodes with an image each would not fit beside the
5 to 8 GB a run already takes.

### Frames

The spec reserves DATA `aux` values 13 to 63, which every node today
rejects (spec "Packet formats"). The update takes one of them, 13, with
a second byte for the kind. A node that does not know a kind ignores it.

```
SYMBOL    first(1) kind(1) file(4) block(1) index(1) symbol(247)            = 255
          kind = 1; 138.6 ms
MANIFEST  first(1) kind(1) file(4) format(1) target(2) version(4) length(3)
          stage(1) table(32) [key(1) signature(64)] × 1..3                  = 114..244
          kind = 0; 133.4 ms with three signatures
WANT      first(1) kind(1) file(4) holder(6) channel(1) missing(1..32)      = 14..45
          kind = 2; 16.96 ms with 17 blocks
first     = 0x0D: the DATA shape, aux 13
file      = the first 4 bytes of `table`
```

- **SYMBOL** carries one coded piece: block `block` (up to 256 blocks of
  64 × 247 bytes, 4 MB, more than the flash holds), piece `index` 0 to
  254 of that block, 247 bytes of it.
- **MANIFEST** says what the file is and who signed it. `format` is
  this layout's version; `target` the board and flash layout the image
  is for, so that a T-Echo image never lands on a RAK4631; `version` the
  signed version number; `length` the image's bytes; `stage` the
  roll-out share (Policy). `table` is the SHA-256 of the file's
  **block table**: the image's SHA-256 and each block's SHA-256 cut to
  8 bytes. The table travels as block 0 of the file, coded like any
  other, so a receiver checks each block as it decodes it. Each
  signature is Ed25519 (RFC 8032) over the bytes from `format` to
  `table`, `key` naming which of the receiver's accepted keys made it.
- **WANT** asks `holder` to send on `channel` the blocks whose bits are
  set in `missing`.

`file` is a hash of the content, so a new manifest for the same image,
with a larger `stage`, names the same file, and nothing is sent again
but the manifest.

*Why 4 bytes of file ID.* Two files in one neighbourhood collide with
odds of 2⁻³² a pair. A forger gains nothing from a collision: he can
send false pieces under any ID he hears anyway, and the block hashes
catch them.

*Why block hashes.* Nothing on the air is authenticated
(spec "Phone link"), so anyone can send pieces that decode to garbage.
A block that fails its hash is dropped and gathered again; a forger can delay an
update but never get anything installed, since the image hash and the
signature are checked before the swap.

### The code

A block is 64 pieces of the image. Pieces 0 to 63 are the block itself;
pieces 64 to 254 are combinations of them, coded with a Reed-Solomon
code over GF(2⁸) built on a Cauchy matrix. Any 64 distinct pieces of a
block, in any mix, give back the block, and no others are needed. A
receiver that hears more than 64 has slack, and one that misses some
waits for any others, which is what a broadcast to many receivers that
each lose different frames needs: every piece beyond the first 64 is
useful to every receiver still short of that block.

Compared:

| | Reed-Solomon over GF(2⁸), 64-piece blocks | LT codes | RaptorQ (RFC 6330) |
|---|---|---|---|
| pieces needed | exactly 64 per block | well above k for blocks this small | k, with a failure chance of about 1 % at k, falling by about 100× with each extra piece (RFC 6330) |
| pieces per block | at most 255 | unbounded | unbounded |
| decoding memory with pieces in flash | a 64 × 64 matrix and short strips, about 6 kB (below) | the block's pieces in RAM as they are combined | the block's intermediate pieces in RAM; the RFC lets the sender cut blocks to the receiver's working memory |
| patents | the code dates from 1960; nothing to license | the first LT patent, US 6,307,487, filed 1999-02-05, is listed as expired (Google Patents) | Qualcomm's IETF declaration 2554 (2015-03-19) promises not to assert its patents against devices that implement no wireless wide-area standard, but only for implementations of RFC 6330 in full |

RaptorQ earns its complexity on blocks of thousands of pieces, where
its linear decoding time matters; at 64 pieces a block, the size the
chip's RAM allows, it is a heavier way to the same result, and a
reduced version would fall outside the patent promise. LT codes need a
large overhead at that block size. Reed-Solomon is exact, simple and
free.

*Why 64.* The node's RAM sets the block and the field sets the pieces:
255 pieces a block are 4 times 64, room for 20 % loss across 74
receivers, which needs about 1.55 times 64 (Reaching the nodes), and for
two holders sending the same block. A block of 128 would save about 7 %
of the frames at 10 % loss and quadruple the matrix.

**Decoding with the pieces in flash.** Each byte position of a piece is
decoded on its own by the same 64 × 64 matrix. So:

1. Pieces 0 to 63 that arrive are written straight to their place in
   DFU; the others go to the staging pages. Nothing is ever rewritten.
2. When a block has 64 distinct pieces, the node inverts the 64 × 64
   matrix of the pieces it holds (4 kB, 64³ field operations, a few ms).
3. It reads the 64 pieces in strips of 16 bytes, 1 kB in RAM, multiplies
   each strip by the inverse, and writes the missing pieces' strips into
   their still-erased places in DFU, a word at a time.
4. It checks the block's hash; then the staging pieces of that block
   are free.

RAM: about 6 kB, held only while a block decodes. The staging pages
hold 368 pieces of 256 bytes (piece and index), nearly six blocks' worth
of coded pieces, more than a node gathers at once since holders send
the lowest block anyone wants. Received bitmaps, 32 bytes a block, sit
in the staging pages too, so a reboot keeps what was gathered.

**Holders encode.** A node that holds a decoded block, whether it runs
the image or only staged it, can send any of its 255 pieces: piece j is
the sum of the block's 64 pieces times row j of the matrix, 64 reads and
16 000 field multiplications, about a millisecond. It sends pieces 0 to
63 first, which most receivers mostly take, then pieces from 64 up from
a random start, so that two holders seldom send the same piece. It can
serve block b as soon as it has decoded b, so a file moves through a
chain of relays one block behind each other rather than one whole file
behind.

### Priority

A node sends a file frame only when:

- its bucket is full, at its depth of 6 s (spec "Duty cycle cap");
- its send queue and delivery queue have nothing to send now, and no
  hand-over or reply is under way;
- it has not sent a file frame on that channel in the last 13.9 s, and
  has sent file frames on at most two channels in that time.

*Why a full bucket makes texts pay almost nothing in budget.* A full
bucket discards its fill, so the frame spends budget that would
otherwise be lost. After it the bucket holds 6 − 0.139 = 5.86 s and is
full again 1.41 s later unless the node sends meanwhile. A text waits
for the frame's sake only if the node, from then on, sends at its cap
until the 5.86 s are gone, a minute of it; then one text waits 1.41 s
longer, once.

*Why a cap on the channel all the same.* A full bucket says nothing
about the channel. Unthrottled, every idle relay would send at its
9.83 %, 269 relays together twice a channel's time on each of the 13,
and the network's real limit is the channel: in the simulator the
busy share of a relay's channel at the design rate is 1.0 to 1.4 %, at
twice the rate 2.0 to 2.8 %, and at three times the network collapses
(`collapse-research-2026-10-16.md`, `bandwidth-research-2026-10-16.md`).
File frames at 1 % of a channel's time, one 138.6 ms frame every
13.9 s, add less than the design rate itself and keep the network below
twice it.

*Why two channels at most.* A node sending on another channel is deaf
on its own, as for a heartbeat, and a hand-over to it fails. Two
channels at 1 % each cost it 2 % of its hand-overs, below the 3.1 % of
its time the average relay spends sending at the design rate (S4 at
×1, 34 457 s of airtime in the settled hour, less the sleepy nodes'
beats, over 269 relays; `answer-packet-2026-10-24.md`).

These two numbers are starting points for the simulator, not results.

### Who wants what

1. A holder sends the MANIFEST of the newest file it holds on its home
   channel once an hour, unless it heard that manifest there in the
   hour, and once an hour on one other channel, stepping through them.
   Anchors send it right after a slot heartbeat on their home channel,
   where their sleepy nodes listen (below).
2. A node that hears a manifest checks it against its policy (Policy):
   a key it accepts, enough signatures, its own `target`, a version
   above its own and not one it gave up on. If it wants the file, it
   keeps the manifest and sends a WANT to that holder with every block
   missing; else it ignores it, and stores and sends nothing.
3. A holder that hears a WANT sends pieces of the lowest block missing
   on that channel, at the pace above, for as long as wants keep coming.
   Other holders on the channel that hear the pieces do not start on
   the same block.
4. The node sends a new WANT when a block completes, or after 10
   minutes of no pieces.

Nothing is sent while nobody wants anything but one manifest per
channel per hour, 133 ms in 3600 s.

A node may also carry files it does not install, for a signer it lists
as carried but not trusted; by default it carries none. A node of
another implementation that does not take part ignores code 13 and is
never asked.

## Reaching the nodes

### Relays

The plan size: 250 000 bytes are 1013 pieces of 247 bytes, 16 blocks
(15 of 64 and one of 53), and the block table, 160 bytes, one piece in
a block of its own. A receiver that loses each frame with chance
p needs more than 64 frames for 64 distinct pieces; for every receiver
on a channel to finish with 99 % odds, by the binomial distribution:

| loss | 1 receiver | 20 receivers | 74 receivers |
|---|---|---|---|
| 5 % | 72 (1.13×) | 75 (1.17×) | 76 (1.19×) |
| 10 % | 78 (1.22×) | 82 (1.28×) | 84 (1.31×) |
| 20 % | 91 (1.42×) | 97 (1.52×) | 99 (1.55×) |

A channel in a 28 km² district holds about 269 / 13 ≈ 21 relays at
home on it. At 10 % loss: 1013 × 1.28 = 1297 frames, 180 s of airtime.
At 1 % of a channel's time that is 5.0 h for the whole image across one
hop, 19 minutes a block.

Because a relay sends a block as soon as it has decoded it, a chain of
H hops takes about (16 + H − 1) blocks' time, not 16 × H:

| From one relay to | hops H | time |
|---|---|---|
| a district, started in its middle | 4 | 6.0 h |
| a district, started at an edge | 8 | 7.3 h |
| the city, 14 districts | about 40 | 17 h |

A relay that holds the file serves two channels at once, so the first
hops are slower until holders sit on every channel; say 6 to 10 hours
for a district, a day for the city.

The ceiling, for comparison. By spec "Capacity and sizing" a channel
carries about 640 B/s with listen-before-talk, and the design rate puts
about 80 kB/s over 14 districts and 13 channels, 440 B/s each, leaving
200 B/s: the image would cross a hop in about half an hour if updates
took the whole free share. They must not: that is the load at which the
simulator's network collapses.

### Sleepy nodes

A sleepy node receives duty-cycled between its windows, and would hear
nothing with the ordinary preamble
(spec "Duty-cycled receive and the long preamble"). It does wake once a
rotation, about 130 s, for its anchor's slot heartbeat on its home
channel (spec "Relay choice", the paragraph *Asleep*). So:

- An anchor that holds a file sends one SYMBOL after its slot heartbeat
  on its home channel, after the usual listen-before-talk, while it has
  heard a WANT from one of its sleepy nodes in the last hour. The
  manifest goes in the same place, once an hour.
- A sleepy node that wants the file stays awake after that heartbeat
  for the frame, at most a backoff and a frame, 200 + 138.6 ms, and
  sends its WANT to its anchor after a beat, at most once an hour.
- All the sleepy nodes of an anchor share its home channel and wake for
  the same heartbeat, so one frame reaches all of them: the anchor's 74
  sleepy nodes (20 000 / 269) take the image as one broadcast, at
  1013 × 1.31 = 1327 frames, not 74 × 1013.

At one frame a rotation, 1327 frames take 1327 × 130 s = 2.0 days. The
anchor spends 138.6 ms in 130 s, 0.11 % of its channel; about nine such
anchors in hearing range on one channel reach the 1 % above.

Battery: listening costs 5.8 mA
(spec "Duty-cycled receive and the long preamble"). 1327 extra listens of at most 0.34 s are 0.73 mAh, 0.09 %
of the 850 mAh battery; the frames alone are 0.30 mAh. The WANTs, the
flash writes and the swap add less. A node in a pocket or a basement
keeps what it gathered and goes on where it left off, from whatever
anchor it has; over days or weeks, as the user put it, it gets there.

Anchorless nodes receive only the long preamble, and get the image when
they next have an anchor, or over BLE.

## Policy

### Keys

The image carries its acceptance rule: a list of up to 8 public keys
and how many of them must sign, as in "2 of 3". A node installs a file
only if the manifest carries that many valid signatures by keys on its
list. An update can change the list, under the rule of the image that
receives it. The app may narrow the rule of a node it is linked to: no
updates, or only its owner's key. A planted relay keeps the rule it was
planted with.

*What a key holder can do to a city, plainly.* Whoever holds, steals or
is forced to use enough signing keys can, within a day, replace the
firmware of every node in a city that accepts them. Such an image could
switch the network off in the hour it is needed, jam the band, or make
every pocket node broadcast its carrier's position from its GPS. The
end-to-end encryption in the app keeps message contents safe from the
node, but not who talks when, nor where the node is. A single project
key makes one person, or whoever coerces them, able to do this.

So the first image should accept **2 of 3 keys**, held by different
people in different places, offline. One lost or stolen key then does
nothing on its own, and the other two can roll out an image that drops
it. Each image should be built reproducibly and published with its
source before it is signed, so that anyone can check that the signed
bytes are the published code; the staged roll-out below gives them the
time.

### Versions

`version` is a 32-bit number the signers raise with every image. A node
accepts only a version above both the one it runs and any it has given
up on, and keeps the highest it has confirmed in the update state, so
that a signed old image replayed from a recording is refused.

### Revert

A new image boots in test state. It confirms itself only when it runs
the core, its radio works, and, if the node knew neighbours before the
update, it has heard a valid frame from one, within 10 minutes, two
worst-case rotations and a margin. The loader starts the watchdog before
the jump; a hang or a crash resets the node, and a reset before the
confirmation swaps the old image back. The node then records the file
as given up: it never wants it again and never serves it.

A node installs only when its send queue and delivery queue are empty,
since a reboot loses what they hold in RAM. Its stored state (D73)
brings it back on its home channel at once.

*What a city of reverting nodes does.* Each node is out for a swap,
up to 10 minutes of test, and a swap back: about 11 minutes. The nodes
finish the file at different times, hours apart, so the outage passes
through the city like a wave, never all at once; the sleepy nodes of an
anchor that is out miss at most a few slot heartbeats, short of the two
rotations after which they look for another (spec "Relay choice").

The worse case is an image that passes the test and fails later: it
routes badly, drains batteries, or stops passing files on. Nothing
reverts that by itself. Two defences:

- The previous image stays in DFU until the next update, so a node can
  swap back on its own later, if its own code notices, or on a newer
  manifest that names the bad version as given up.
- The update path is one hop at a time and needs no routing, so an image
  with broken routing can still receive the fix, as long as its update
  code works.

### Staged roll-out

`stage` is a share of nodes in 256ths. A node installs only if the first
byte of SHA-256(node ID ‖ file) is below `stage`; every node still
gathers, holds and serves the file. The signers start with a few nodes,
1/64 say, and raise `stage` with new manifests for the same file, each
133 ms per channel, as no reverts come in and nobody finds fault.

## What must be fixed before the first field deployment

A node in the field can be changed later only by what its first image
can do. So the first image must hold:

1. **The loader and the flash layout** above, sent with the image as one
   UF2. Changing them later means rewriting the loader in the field,
   which a power cut turns into a visit. The same loader serves the BLE
   updates of pocket nodes, so it is needed first anyway.
2. **The reserved code.** DATA `aux` 13 for files, with its kind byte,
   is set aside in the spec, and every node rejects it, which the spec
   already requires of `aux` above 12.
3. **The manifest and the frames**, with `format`, `target`, `version`,
   `stage`, the block table, the code (the field's polynomial, the
   Cauchy matrix), the piece and block sizes, Ed25519 and SHA-256. Any
   later format reaches the nodes only through this one, so this one
   must be able to carry its own replacement: a node takes an image in
   format 1 whatever formats that image then speaks.
4. **The keys and the rule**, 2 of 3, and who holds them.
5. **The version and given-up state** in flash.
6. **For planted relays that nobody will visit: the receiving side**,
   manifests, wants, gathering, decoding and installing. The sending
   side need not be there: it can come in the first update, from nodes
   that took it over BLE or USB. A relay planted without the receiving
   side can be updated only by a visit, by BLE from a phone in reach or
   by USB.

What can wait: the sending side, the sleepy path, the tuning of the
1 % and the two channels, carrying other signers' files, the BLE path
itself for pocket nodes (it needs only the loader and the manifest
check).

## Cost

Weeks of work, for one person who knows the code:

| Part | Work | Weeks |
|---|---|---|
| Image | relay tables to `.bss`, so the image is about 170 kB | 0.5 |
| Bootloader and staging | loader on embassy-boot, layout, one UF2, confirmation, watchdog, swap back, write protection; bench: power cuts in 100 swaps and 100 reverts, hung and crashing images | 3 |
| Policy | manifest, host signing tool with 2 of 3 keys, key handling, version and given-up state, staged roll-out | 2 |
| BLE path | GATT writes into DFU, the same checks (after BLE exists) | 1 to 2 |
| Mesh, receiving | frames in the core, wants, the decoder in strips, block checks, staging | 3 |
| Mesh, sending | priority in the core, holders encoding, the sleepy path, simulator scenarios and the measured cost to texts | 3 to 4 |
| **All** | | **12.5 to 14.5** |

## Recommendation

Keep the Adafruit bootloader as the USB recovery path and change
nothing in it; add a 16 KiB loader of our own on embassy-boot at
`0x26000` that swaps a staged image in and swaps it back when the new
image does not confirm itself, with the signature checked by the
application, against keys that live in the image and can therefore be
changed. Do this, the layout and the policy, 2 of 3 keys, monotonic
versions and staged roll-out, before any node goes to the field: about
six weeks, and the BLE updates for pocket nodes need all of it first.

Reserve DATA `aux` 13 for files now. Use Reed-Solomon over GF(2⁸) in
blocks of 64 pieces, decoded in about 6 kB of RAM with the pieces in
flash: exact, free of patents, and as good as a fountain code at this
size. At 1 % of a channel's time a 250 kB image crosses a district in
6 to 10 hours and a city in about a day; sleepy nodes take it in about
two days from one frame after each of their anchor's slot heartbeats, at
0.09 % of their battery. Build the receiving side before planting any
relay that will not be visited; the sending side can come later, as an
update.

## Sources

- nRF52840 Product Specification: Memory, NVMC (page size, erase and
  write times, endurance), ACL, WDT, as already cited in `memory.x` and
  `src/store.rs`.
- Adafruit nRF52 Bootloader, github.com/adafruit/Adafruit_nRF52_Bootloader,
  read 2026-10-25 at `c67f0bc`: README ("Signed firmware support",
  "Dual bank firmware support"), changelog 0.4.0, commits `c8af92c` and
  `f20bfe9` (2026-01-15, first in tag 0.10.0), `src/usb/msc_uf2.c`,
  `src/usb/uf2/ghostfat.c`, `bootloader.c` at tag 0.6.1.
- embassy-boot: embassy.dev book, "Bootloader"; github.com/embassy-rs/embassy
  at `002a412` (2026-10-25), `embassy-boot/src/boot_loader.rs`,
  `embassy-boot-nrf/src/lib.rs`, `examples/boot/bootloader/nrf/memory-s140.x`.
- MCUboot design, docs.mcuboot.com/design.html; Nordic DevZone, "Device
  Firmware Update (DFU) with MCUboot bootloader" (default partition
  0xC000 on the nRF52840).
- RFC 6330, RaptorQ Forward Error Correction Scheme for Object Delivery;
  IETF IPR disclosure 2554 (Qualcomm, 2015-03-19),
  datatracker.ietf.org/ipr/2554.
- US 6,307,487, "Information additive code generator and decoder for
  communication systems", filed 1999-02-05, status "Expired - Lifetime"
  on Google Patents.
- RFC 8032, Edwards-Curve Digital Signature Algorithm (Ed25519).
- I. S. Reed and G. Solomon, "Polynomial codes over certain finite
  fields", J. SIAM 8(2), 1960.
