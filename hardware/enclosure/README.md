# Outdoor enclosure

A printed ASA box for one T-Echo, in its stock case, on a roof: a
sealed body and a snap-fit lid at the bottom that carries the drains and
the USB cable's entry. The reasons, sources and open questions are in
`docs/enclosure-2026-11-09.md`.

## Run

From the repository root, once:

```
python3 -m venv hardware/.venv
hardware/.venv/bin/pip install cadquery
```

Then:

```
hardware/.venv/bin/python hardware/enclosure/enclosure.py
```

It checks the model and writes to `hardware/enclosure/out/`:

- `body.stl`, `lid.stl`: to print, already standing as they print and
  scaled by `SHRINK` for the plastic's shrinkage.
- `assembly.step`: body, lid and the cased T-Echo as mounted, unscaled.
- `view-*.png`: views for review, among them a section through the
  antenna's axis with the T-Echo inside.

The checks: each part is one valid solid and its STL is watertight;
the lid does not touch the body; nothing but the crush ribs reaches
into the case, antenna, plug or the cable's curve; every latch beam's
strain is under the limit. The parameters at the top of the script
marked ESTIMATE are not in any source: measure them on the parts in
hand and rerun before printing.

## Parts

- The T-Echo in its stock case, with its stock 433 MHz antenna screwed
  straight onto the jack.
- A USB-A to USB-C cable with a straight plug, from the panel's
  controller; its minimum bend radius goes in `BEND_R`.
- Two 12 mm stainless hose clamps for a pole (a broom handle of 23 to
  28 mm or larger, in the saddle) or a railing.
- The panel goes on the pole, a quarter wavelength (17 cm) or more below
  the antenna; the note says why. Where the pole has no room, its round
  foot, `PANEL_FOOT_D` across, is screwed to the flat square front of
  the bay with its own self-tapping screws, of about 3 mm, that end
  within the 6 mm wall.

## Print

ASA, in a light colour. From the Prusament ASA data sheet and Prusa's
ASA guidance:

| Setting | Value |
|---|---|
| nozzle | 260 °C (±10) |
| bed | 110 °C (105 °C first layer) |
| part fan | 30 %, up to 50 for bridges |
| enclosure | closed, no draught |
| walls | 6 perimeters of 0.4 mm, so the 2.4 mm walls are solid |
| top and bottom | solid, at least 5 layers |
| bay's front wall | 100 % infill, by a modifier over it |
| supports | none |

The body stands on its open end, as mounted; the lid on its floor. Keep
the first layer from spreading (elephant's foot) on the body: the lid
fits inside the body's bottom edge with 0.25 mm a side.

## Assembly

1. On the bench, flash and test the board; the reset button cannot be
   reached once the case is in the body.
2. Screw the antenna onto the jack.
3. Plug the cable into the USB-C port, turn it down in an even curve no
   tighter than its bend radius, and slide it into the lid's slot from
   the front edge.
4. Push the case, antenna first, up into the body from below until it
   stops, the cable's curve in the bay beside it; then press the lid in
   until all six latches click.
5. Mount the box upright with the lid at the bottom, its bottom clear of
   the snow: on a pole, set the pole in the saddle's V and run each hose
   clamp through its tunnel and round the pole, snug, not hard. Lead
   the cable down in a loop below the box before it goes to the panel.

To open: push each latch beam inward through its slot in the lid with
a small screwdriver and pull the lid down.

## What to check on the first print

`docs/enclosure-2026-11-09.md`, "What the first print must verify".
