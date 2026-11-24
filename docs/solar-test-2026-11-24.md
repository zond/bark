# Solar unit test plan, 2026-11-24

A relay on the roof, fed over USB by a solar unit with its own battery, ran on its
internal cell all night and never saw USB power. This plan finds out why, and whether the
unit can carry a relay through winter.

## The unit

A 9 W, 5 V panel with a battery built in behind it, sold for charging battery cameras
(seller's page, read 2026-11-24): rated 15 600 mAh, which is about 58 Wh at a lithium
cell's 3.7 V if the rating holds; one cable out, ending in micro-USB, USB-C and a barrel
plug; IP66. The panel is wired to the battery inside, so its yield cannot be metered
alone: only what comes out of the cable can.

## What we want to know

1. Does the unit keep its USB output on with only a T-Echo as load?
2. How much does the relay draw from the unit, on average?
3. How much energy does the unit's battery really give, warm and cold?
4. How much does the panel put back on a winter day?

The node stays up if a day's panel yield covers a day's draw, or if the battery, cold,
covers the longest dark stretch.

## Rules for every run

- Change one thing per run.
- Start with the unit's battery full and the T-Echo's own cell full, so no run pays for
  charging the cell.
- A run ends when USB power stops, not when the node dies: the node runs on for about two
  days on its own cell. The time is read from a neighbour's log, which asks the node
  every ten minutes and notes `usb` or a battery percent (`bark ask`).
- The other nodes stay on, so the relay hears the same traffic in every run.
- The inline meter draws current itself and may keep an output awake that would
  otherwise switch off. A result with the meter does not prove the same without it.

## Steps

Each step says what to do, what to write down, and what decides the next step.

### 0. Prepare

Dry the unit. Charge its battery full. Charge the T-Echo's cell full from a wall
charger.

### 1. Does the output stay on? (1 hour, no meter)

Connect the T-Echo to the unit, indoors, panel covered. Watch the log for an hour.

- `usb` throughout: go to step 2.
- Falls to battery: the unit switches off under a small load, or its output is faulty.
  Stop here. The unit needs a setting that keeps the output on, a larger steady load, or
  replacing; the later steps measure nothing until this is solved.

### 2. Does the meter read? (10 minutes)

Put the meter between the unit and the T-Echo. It should show a steady, non-zero current.
A reading of zero means the meter cannot see a load this small: get another meter.

### 3. Draw (24 hours)

Same setup, panel covered. Reset the meter's counter, read it after 24 hours.

Write down: mAh and Wh over the day. Average draw = that, divided by 24 h.
Predicted life = rated capacity ÷ average draw.

### 4. Run down, warm (to the end)

Leave it running until the log shows USB power gone.

Write down: hours from full to USB off, and the meter's total.
Real capacity = the meter's total. If it is far below the rated capacity, the difference
is what the unit's own electronics use, or a worn battery.

### 5. Run down, cold (to the end)

Charge the unit full at room temperature. Put unit and T-Echo in the freezer in a sealed
bag with a desiccant, the meter in the cable outside if it reaches. Run until USB power
stops. Let the bag reach room temperature before opening it.

Never charge the unit or the T-Echo while they are below 0 °C: charging a frozen lithium
cell damages it. Check whether the unit refuses to charge when cold; on a roof in winter
it must.

Write down: hours to USB off, the meter's total. Cold capacity ÷ warm capacity is what
winter leaves.

### 6. Panel yield (3 days or more)

Run the unit down until USB power stops (as step 4 ends), then put it outdoors under
open sky, where the roof's light is; a window understates it. Leave the T-Echo
unplugged for three days, so that everything the panel gives is stored. Then plug the
T-Echo in with the meter, indoors, panel covered, and run to USB off again: the meter's
total is three days' yield. An empty start matters: a full battery takes no more, and the
yield would read as zero.

Write down: the weather of each day in a word, and the total. Yield per day = the total
÷ 3, to set against the draw of step 3.

## Results

| Step | Date | Result |
|---|---|---|
| 1. Output stays on | | |
| 2. Meter reads | | |
| 3. Draw over 24 h | | |
| 4. Warm: hours, total | | |
| 5. Cold: hours, total | | |
| 6. Yield per day | | |
