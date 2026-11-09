# An outdoor enclosure for a roof relay

2026-11-09. A printed ASA box for one T-Echo on a roof in Sweden, all
year, charged from a small solar panel over USB. The model is
`hardware/enclosure/enclosure.py`; how to run it and print it is in
`hardware/enclosure/README.md`. This note gives the measurements and
their sources, the conditions on the roof, the design that follows
from them, what is still open, and what the first print must show.

## The T-Echo as it goes in

The box holds the T-Echo in its own stock case, as the bench boards
are: the case already holds the board, cell and screen together, carries
the antenna jack, and has a published 3D model. The stock antenna
stays: the firmware's 10 dBm cap meets the 10 mW ERP limit only because
that antenna has no gain over a dipole (`bark-fw/README.md`).

Sizes below are read from the board maker's 3D model of the cased T-Echo,
`dimensions/t-echo_3d.stl` in its repository
(<https://github.com/Xinyuan-LilyGO/T-Echo>, fetched 2026-11-09), by
taking the extent of its triangles in each region. The model's axes are
kept as the box's: x along the case with the antenna end at x−, y from
the back (y−) to the screen (y+), z from the USB-C face (z−) to the
button face (z+), in mm.

| What | Where (mm) | How it was read |
|---|---|---|
| screen part | x −24.0 to 24.61, y 5.48 to 29.72, z −24.64 to 28.66 | extent of the main block |
| logo part, beside it | x −40.67 to −24.0, front face y 27.63, top z 6.36 | the narrower block that carries the jack |
| antenna jack | axis y 18.43, z −14.9; flange ring Ø16.1 out to x −42.39 | ring on the logo part's end face |
| reset and user buttons | z+ face, x −13.5 to −9.3 and 16.0 to 20.4, y 20.25 to 24.66, standing to z 29.65 | two squares proud of that face |
| USB-C opening | z− face, centre x 11.3, y 19.8, about 8.6 × 3.4 | slot in that face |
| screen window | y+ face, x about ±14.5, z about ±14 | unused here |

The maker's pin map image (`image/T-ECHO.jpg`) labels the side buttons
as reset (nearer the antenna end) and user (P1.10), and shows the
touch pad at the top of the case. The schematic (`schematic/T-Echo_Schematic.pdf`)
shows a TP4054 charger and a separate GPS antenna net; where the GPS
antenna sits inside the case is not in any drawing found.

Not in any source, so estimated and named as parameters in the script,
each to be measured on the parts in hand before the first print:

| Parameter | Estimate | Measure |
|---|---|---|
| `ANT_LEN`, `ANT_D` | 60 mm long, 16 mm at the base | the stock 433 MHz antenna, by caliper; the estimate is from the pin map photo's scale |
| `ADAPTER_REACH`, `ADAPTER_RISE` | 12 mm, 10 mm | the right-angle SMA adapter (below), from the jack's face to its upright axis, and from its horizontal axis to the face the antenna seats on |
| `PLUG_LEN` | 28 mm | the USB-C plug's body and strain relief, from the case face to where the cable is free |
| `CABLE_D` | 4.0 mm | the panel's USB cable |

## On a Swedish roof

**Temperature.** About −25 °C to +35 °C in the shade, more in sun on a
roof. The box makes no heat of its own worth counting: a relay draws
6.4 mA (D81), 24 mW, so the cell sits at the air's temperature by night
and above it in sun.

**The cell in the cold.** The T-Echo's cell is 850 mAh. Its maker's
data sheet was not found; a cell of the same size, Grepow
GRP704230 (835 mAh, specification approval sheet, rev. A), allows
charge only from 10 to 45 °C and discharge from −20 to 60 °C, and gives
at least 70 % of its capacity at −20 °C. The usual floor for charging
Li-ion is 0 °C; below it, charging plates lithium on the anode. So below
the cell's floor nothing may charge it. The board cannot see to that:
the TP4054 has no temperature input and its supply is VBUS, so the board
charges whenever USB power is there, at any temperature, and the
firmware has no line to stop it. The cut must come before the USB: the
panel's controller, or a switch in its output, must give no power while
the cell is below its floor or above 45 °C. Below −20 °C the cell is
outside its rating; at 6.4 mA, under a hundredth of its capacity an
hour, it will most likely still deliver, with less capacity.

**What this means in winter.** In a cold spell the relay lives on its
cell, whatever the sun does: 80 % of 850 mAh lasts 106 h at 6.4 mA
(D81), about 74 h at the 70 % the cell keeps at −20 °C. After that D81
turns it sleepy. A roof relay on this cell is down in any cold spell
longer than about three days. The panel cannot fix that; a larger cell
that is kept warm enough to charge can (open questions).

**Sun in December.** Stockholm is at 59.3° N. On 21 December the sun is
at most 90 − 59.3 − 23.4 = 7.2° above the horizon at noon and up for
under six hours. PVGIS (the EU's Joint Research Centre,
<https://re.jrc.ec.europa.eu/pvg_tools/>, SARAH-2 data 2010–2020,
59.33° N 18.07° E, monthly radiation, fetched 2026-11-09) gives for a
south-facing surface:

| kWh/m² a month | horizontal | 60° | 75° | 90° |
|---|---:|---:|---:|---:|
| December, mean | 5.5 | 20.3 | 21.4 | 21.2 |
| December, worst year | 2.7 | 5.8 | 5.9 | 5.7 |
| January, mean | 9.1 | 27.3 | 28.5 | 27.9 |
| February, mean | 25.6 | 54.6 | 55.0 | 52.1 |

A steep panel gets four times the horizontal in December, and
anything from 60 to 90° gets the same. 75° facing south is chosen: it
sheds snow, and the summer it gives up does not matter.

**Panel size.** A relay at 6.4 mA from a 3.7 V cell uses 0.57 Wh a day.
Between the panel and the cell: the panel's USB regulator, assumed
85 %; the TP4054, a linear charger from 5 V to about 3.9 V, 78 %; and
low light and dirt, assumed 80 %; 53 % in all. A panel rated P watts at
1000 W/m² gives P × H × 0.53 Wh a day, H the day's radiation in kWh/m².

| December at 75° | H, kWh/m² a day | panel for 6.4 mA | for 10 mA |
|---|---:|---:|---:|
| mean | 0.69 | 1.6 W | 2.4 W |
| worst year 2010–2020 | 0.19 | 5.7 W | 8.8 W |

D81's 6.4 mA is the radio's average in the simulator's busiest city
(S4); the rest of the board on battery has not been measured, so the
10 mA column is a margin, not a measurement. A 10 W panel at 75° covers
the worst December, when the cell is warm enough to take the charge.

Two more things follow for the firmware. Under D82 a relay on external
power runs its GPS all the time, 29 mA, and under D71 its LED and
crystal; a panel's USB looks like external power, so a solar relay would
spend its sun on the GPS. Giving the roof node a map point puts its GPS
in standby for good (D82). And the bench found that a USB-C-to-C cable
may not power the board (`bark-fw/README.md`, "Flash"), so the panel's
cable is USB-A to C.

**Snow and ice.** Snow can bury the box for weeks. Nothing on the node
depends on seeing it: the screen stays inside, unseen, and the node is
read through the phone and the mesh, which also removes the window's
sealing and the question of snow over it. The openings are all in the
lid, and there are several: two drain holes, four latch slots and the
cable slot, besides the joint itself. Ice may close some; it will
rarely close all. The lid should hang clear of the snow: mount the box
with its bottom well above the depth snow lies on that roof.

**UV and cold brittleness.** ASA is made for outdoor use: Prusa rates its
UV resistance high and "suitable for outdoor use"
(<https://help.prusa3d.com/article/asa_5078>). In the cold it loses
toughness: a moulding grade, Luran S MED 797S, falls from 40 kJ/m²
notched Charpy impact at 23 °C to 13 at 5 °C and 9 at −30 °C (INEOS
Styrolution product flyer, ISO 179/1eA). Printed ASA starts lower,
12 kJ/m² notched (Prusament ASA technical data sheet v1.1, 2024-05-20).
So the box has no thin parts on its outside, the latches flex only when
the lid goes on, and the box is shut indoors, warm.

**Wind.** Swedish reference wind speeds run to about 26 m/s
(Boverket's EKS, municipal table; Stockholm's own value to be read
there). At 26 m/s the basic pressure is ½ × 1.25 kg/m³ × 26² = 420 Pa;
at 20 m over open ground the gust factor raises it to about 1.2 kPa
(EN 1991-1-4, exposure factor about 2.8). The box with its pad shows
0.012 m² to the wind, so with a force coefficient of 2 it takes about
30 N. Two 12 mm stainless hose clamps hold that many times over. A
10 W panel, about 0.09 m², takes about 210 N and goes on its own
bracket, never on the box.

**Condensation.** The air in the box cools every night and its
moisture condenses on the walls. A box sealed tight still breathes,
through whatever leak it has, as the air inside swells and shrinks, and
it keeps what it draws in. So the box is not sealed: it breathes
through the lid's joint and openings, and what condenses runs down to
the drains and out.

## The design

**Shape and orientation.** One printed body, closed on every side but
the bottom; a lid that snaps into the bottom. The case stands with its
USB-C face down towards the lid, its back to the wall and its screen
facing out. The cavity has one outline from the lid up to the case, so
the case slides in from below with antenna and cable attached. Inside:
crush ribs on the front, back and right walls that press 0.15 mm into
the case, so it does not rattle; a shoulder over the logo part and three
stops under the ceiling that hold it from above, clear of the two
buttons, which the shell never presses; and two posts on the lid that
hold it up against them. The box is 96 × 30 mm and 118 mm tall, 124 mm
wide over its mounting ears and 38 mm deep with its back pad.

**Antennas inside.** Nothing goes through the walls, the antenna
included. With the USB-C face down, the antenna on the jack would lie
on its side, polarised across every other node's upright antenna; a
right-angle SMA adapter stands it upright, and it stands in a tower of
the body with 4 mm of air around it. ASA's permittivity and loss at
433 MHz and 1.575 GHz were not found. Taken anywhere in the range of
the styrene plastics, a relative permittivity up to 3.5 and a loss
tangent up to 0.02, a 2.4 mm wall absorbs under 0.01 dB at 433 MHz and
under 0.05 dB at 1.575 GHz, and reflects under 0.1 dB, since the wall is
a small fraction of a wavelength. The shell costs the signal nothing
that matters; what it may do is detune the antenna a little, and so
may a layer of wet snow or ice on it. That is a bench measurement:
the antenna's match in the box and out of it. Where the GPS antenna
sits in the case is unknown; nothing metal is between it and the sky
in any direction, and with a map point (above) the roof node hardly
needs it.

**Screen and buttons.** The screen is not exposed and not needed. The
buttons are not reachable from outside. The reset double-tap that the
bootloader needs is for the bench, before mounting: with the lid off,
the case slides out of the body. A board running this firmware enters
the bootloader from its serial port too (`bark-fw/README.md`, "Flash"),
and takes updates over the mesh, so a mounted node rarely needs it.

**The lid.** The lid is a plate recessed 1.5 mm into the bottom of the
body; the body's walls hang below it, so water running down them drips
off their edge and never reaches the joint. Its top falls 1.2 mm from
back to front, so water on it runs to the front, where the drains and
the cable slot are. Whatever the mounting, the lid is the bottom: every
mount below keeps the box upright.

**Snap fit.** Four latch beams lie along the lid's long edges, each
standing free of the floor over a slot 4 mm wide, with a tooth at its
free end that catches a groove in the body wall. The beams bend in the
plane of the print's layers, so the strain runs along the printed lines
and not across them: across layers printed ASA holds only 11 MPa, against
42 MPa along them (Prusament ASA data sheet). The strain at a beam's
root is 1.5 × thickness × deflection / length² (Bayer, "Snap-Fit Joints
for Plastics – A Design Guide", Table 1, constant section): 2.0 mm thick,
1.0 mm tooth, 22 to 25 mm long, 0.48 to 0.62 %. The guide lets an
amorphous plastic go to about 70 % of its yield strain for one snap and
60 % of that for snaps repeated; Prusament ASA yields at 3.4 %, so
1.4 %. The script halves that for the cold and checks every beam
against 0.71 %. A beam pushes back with about 2 to 2.5 N (modulus
1.7 GPa); through the 30° lead-in, with a friction coefficient of 0.5
assumed, the lid goes on with about 12 N. The catch is square; to open, push each beam in
through its slot with a small screwdriver. The slots are drains as
well.

**No gasket.** Water does not enter from below: rain falls on the top
and runs off the walls past the joint, and only wind-driven spray
reaches the lid from underneath, onto a recessed plate it cannot climb
past. A gasket would make the box tight but for its openings, and so
trap what the breathing draws in; at −25 °C a printed or rubber seal
also stiffens. The joint stays a plain 0.25 mm gap.

**Cable entry.** The cable comes up through a slot in the lid that runs
from under the USB-C port to the lid's front edge; the cable is slid in
from the side, before the lid goes on, and the slot, 0.3 mm narrower
than the cable, holds it lightly. Outside, the cable hangs down in a
drip loop below the box before it goes to the panel, so water running
along it drips off the bottom of the loop.

**Mounting.** A pad on the back carries four screw holes in ears beyond
the body, for a wall, and two pairs of tunnels for 12 mm stainless
hose clamps behind the body: one pair across, for a vertical pole, one
pair upright, for a horizontal railing. Steel clamps, not plastic ties,
which grow brittle in UV and cold.

**Material.** ASA, not PETG: ASA is made for UV, and it holds its shape
to 93 °C (heat deflection at 0.45 MPa, Prusament ASA data sheet)
against PETG's 68 °C (Prusament PETG data sheet v1.1, 2022-02-16), which a
dark box in summer sun on a roof can approach. Print it light: a light
box takes less sun in summer, when the cell must stay under 45 °C to
charge and 60 °C to run, and the cold is the cell's problem whatever
the colour.

**Walls, fit and print.** Walls 2.4 mm, six solid perimeters of 0.4 mm
lines and no infill, so no path for water runs between them. ASA
shrinks: 0.4 to 0.7 % for the moulding grade above (INEOS flyer), so
the script scales its STL by 0.5 % and the first print tunes that.
Clearances: 0.4 mm from the case to the walls, 0.25 mm each side between
lid and body. Prusament ASA prints at 260 ± 10 °C on a bed at
110 ± 5 °C with the fan at 30 % (data sheet), in an enclosure, since ASA
warps when its surroundings are cool (Prusa's ASA article). The body
prints standing on its open end, as mounted: the ceilings bridge the
box's 25 mm depth, the grooves' upper faces are at 45°, and nothing
needs support. The lid prints on its floor, the beams flexing in the
layer plane as above.

## Open questions

- **Winter power.** The T-Echo's own cell carries a relay about three
  days of cold. A year-round roof relay needs a larger cell kept warm
  enough to charge, or a charger that charges only when the cell is warm
  enough and a node that is down in the deepest cold. Which, and how a
  solar relay tells the mesh (O17, D81), is not settled.
- **The cold cut-off.** Which panel controller, or which switch at the
  cell's temperature, keeps charge off below the cell's floor. The
  nRF52840's own temperature sensor could tell the firmware when to
  report it, but cannot stop the charging on this board.
- **Sealing of the lid.** No gasket, for the reasons above. If the first
  winter shows water inside, a soft seal in the joint is the next step.
- **Insects.** The slots, 4 mm wide, let small insects in. A stainless
  mesh, or a breathing membrane vent in place of a drain, would close
  them; neither is in the model.
- **The drip loop.** How the cable is held below the box so the loop
  keeps its shape in wind and under snow: a clip on the pole is not
  modelled.
- **Hold-down of the cable.** The slot pinches the cable lightly; a hard
  pull on the cable pulls on the plug in the board.

## What the first print must verify

1. The case slides in from below with the adapter, antenna and cable on,
   the crush ribs grip without forcing it, and the buttons are not
   pressed; measure the estimated parameters first and rerun the script.
2. The lid snaps on by hand, holds, and comes off with a screwdriver
   through the slots, at room temperature and after a night in a freezer.
3. Shrinkage: the lid's fit in the body and the case's fit in the
   cavity; change `SHRINK` and the clearances to suit.
4. Watertightness: mounted as on the roof, an hour of water from a
   shower head over the top and sides leaves the inside dry; a spoonful
   of water poured into the box upright drains out completely.
5. The antenna's match at 433 MHz in the box and out of it, on the
   bench's analyser, and the link's level to a second board at the same
   place with and without the box.
6. A winter's log from the roof: the board's temperature, the cell's
   voltage, and whether any water stands in the lid.
