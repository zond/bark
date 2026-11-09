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

The checks: each part is one valid solid; the lid does not touch the
body; nothing but the crush ribs reaches into the case, adapter, antenna
or plug; every latch beam's strain is under the limit. The parameters at
the top of the script marked ESTIMATE are not in any source: measure
them on the parts in hand and rerun before printing.

## Parts

- The T-Echo in its stock case, with its stock 433 MHz antenna.
- A right-angle SMA adapter, male to female, which stands the antenna
  upright.
- A USB-A to USB-C cable from the panel's controller.
- Two 12 mm stainless hose clamps for a pole or railing, or four 4 mm
  stainless screws for a wall.

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
| supports | none |

The body stands on its open end, as mounted; the lid on its floor. Keep
the first layer from spreading (elephant's foot) on the body: the lid
fits inside the body's bottom edge with 0.25 mm a side.

## Assembly

1. On the bench, flash and test the board; the reset button cannot be
   reached once the case is in the body.
2. Screw the adapter onto the jack and the antenna onto the adapter,
   standing up beside the case.
3. Plug the cable into the USB-C port and slide it into the lid's slot
   from the front edge.
4. Push the case, antenna first, up into the body from below until it
   stops; then press the lid in until all four latches click.
5. Mount the box upright with the lid at the bottom, its bottom clear of
   the snow, and lead the cable down in a loop below the box before it
   goes to the panel.

To open: push each latch beam inward through its slot in the lid with
a small screwdriver and pull the lid down.

## What to check on the first print

`docs/enclosure-2026-11-09.md`, "What the first print must verify".
