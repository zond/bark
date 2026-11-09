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
stays, screwed straight onto the jack: the firmware's 10 dBm cap meets
the 10 mW ERP limit only because that antenna has no gain over a dipole
(`bark-fw/README.md`).

Sizes below are read from the board maker's 3D model of the cased T-Echo,
`dimensions/t-echo_3d.stl` in its repository
(<https://github.com/Xinyuan-LilyGO/T-Echo>, fetched 2026-11-09), by
taking the extent of its triangles in each region, in the model's own
axes: x along the case with the antenna end at x−, y from the back (y−)
to the screen (y+), z from the USB-C face (z−) to the button face (z+),
in mm.

| What | Where (mm) | How it was read |
|---|---|---|
| screen part | x −24.0 to 24.61, y 5.48 to 29.72, z −24.64 to 28.66 | extent of the main block |
| logo part, beside it | x −40.67 to −24.0, front face y 27.63, top z 6.36 | the narrower block that carries the jack |
| antenna jack | axis y 18.43, z −14.9; flange ring Ø16.1 out to x −42.39 | ring on the logo part's end face |
| reset and user buttons | z+ face, x −13.5 to −9.3 and 16.0 to 20.4, y 20.25 to 24.66, standing to z 29.65 | two squares proud of that face |
| USB-C opening | z− face, centre x 11.3, y 19.8, about 8.6 × 3.4 | slot in that face |
| screen window | y+ face, x about ±14.5, z about ±14 | unused here |

The maker's pin map image (`image/T-ECHO.jpg`) labels the side buttons
as reset (nearer the antenna end) and user (P1.10), shows the touch pad
at the top of the case, and marks a BME280 sensor as optional. The
schematic (`schematic/T-Echo_Schematic.pdf`) shows a TP4054 charger and
a separate GPS antenna net; where the GPS antenna sits inside the case
is not in any drawing found.

Not in any source, so estimated and named as parameters in the script,
each to be measured on the parts in hand before the first print:

| Parameter | Estimate | Measure |
|---|---|---|
| `ANT_LEN`, `ANT_D` | 60 mm above the jack, 16 mm at the base | the stock 433 MHz antenna, by caliper; the estimate is from the pin map photo's scale |
| `PLUG_LEN` | 28 mm | the straight USB-C plug from the case face to where the cable is free; one maker's plug body is 23.7 mm long (Lindy 32005 data sheet) |
| `PLUG_W`, `PLUG_T` | 12 × 7 mm | the plug body across the port's long and short sides; that maker's is 10.9 × 6.5 mm |
| `CABLE_D` | 4.0 mm | the panel's USB cable |
| `BEND_R` | 38 mm | the cable's smallest bend radius, from its maker if given (below) |

## On a Swedish roof

**Temperature.** About −25 °C to +35 °C in the shade, more in sun on a
roof. The box makes no heat of its own worth counting: a relay draws
6.4 mA (D81), 24 mW, so the case sits at the air's temperature by night
and above it in sun.

**What each part is rated for.**

| Part | Runs | Stored | Source |
|---|---|---|---|
| nRF52840 | −40 to 85 °C | | nRF52840 Product Specification, recommended operating conditions |
| SX1262 | −40 to 85 °C | | SX1261/2 data sheet, operating range |
| L76K GPS | −40 to 85 °C | −40 to 90 °C | Quectel L76K specification V1.4 (in the maker's repository) |
| e-paper | 0 to 50 °C | −25 to 70 °C | the panel is a GDEH0154D67 (`bark-fw/src/display.rs`); its sheet was not found, so these are from its sibling GDEY0154D67's Good Display data sheet |
| cell, discharge | −20 to 60 °C, at least 70 % of capacity at −20 °C | | Grepow GRP704230 (below) |
| cell, charge | 10 to 45 °C for that cell; 0 °C is the usual floor | | same |

The T-Echo's cell is 850 mAh; its maker's data sheet was not found, and
the figures are from a cell of the same size, Grepow GRP704230 (835 mAh,
specification approval sheet, rev. A). Below the floor, charging plates
lithium on the anode.

So on its cell the node runs down to the cell's rated −20 °C, and below
that most likely still, at a load under a hundredth of its capacity an
hour, with less capacity; the chips are rated to −40 °C. The screen is
the one part rated only from 0 °C. The roof node does not need it, and
the firmware draws it at least once an hour on battery
(`bark-fw/README.md`, "Display"); whether to stop drawing it in the
cold is open. On panel power the node runs only while the cell is warm
enough to charge, for the reason in the next paragraph: from 0 °C, or
10 °C for a cell like the one above.

**Nothing on the board stops the charge.** The TP4054 has no temperature
input and its supply is VBUS, so the board charges the cell whenever USB
power is there, at any temperature, and the firmware has no line to the
charger. The board can read a temperature: the nRF52840's die sensor, to
±5 °C in 0.25 °C steps over its operating range, that accuracy only
with the 32 MHz crystal running (nRF52840 Product Specification, TEMP),
which the firmware stops on battery (D71). It reads it already, for the
radio's oscillator (`bark-driver/src/tcxo.rs`). The optional BME280 is
not fitted on the bench boards: their factory firmware reports none
(`docs/bench-2026-10-30.md`). Either way the firmware could know it is
cold, and could not stop the charging. The cut must come before the USB:
the panel's controller, or a switch in its output, must give no power
while the cell is below its floor or above 45 °C. Since VBUS also runs
the board, that cut leaves the node on its cell.

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

A steep panel gets four times the horizontal in December, and anything
from 60 to 90° gets the same. 75° facing south is chosen: it sheds snow,
and the summer it gives up does not matter.

**The winter balance.** Between the panel and the cell: the panel's USB
regulator, assumed 85 %; the TP4054, a linear charger from 5 V to about
3.9 V, 78 %; low light and dirt, assumed 80 %; 53 % in all. A panel
rated P watts at 1000 W/m² gives P × H × 0.53 Wh a day, H the day's
radiation in kWh/m²: 0.69 in a mean December at 75°, 0.19 in the worst.
The loads, at 3.7 V: a relay's radio, 6.4 mA (D81), 0.57 Wh a day; and
with the GPS tracking as well, 6.4 + 29 = 35.4 mA (D82, L76K Hardware
Design), 3.14 Wh a day. D82 runs the GPS whenever the node is on
external power, and a panel's USB is that.

| Wh a day | 10 W, mean Dec. | 10 W, worst Dec. | 20 W, mean Dec. | 20 W, worst Dec. |
|---|---:|---:|---:|---:|
| from the panel | 3.66 | 1.01 | 7.31 | 2.01 |
| left, radio only (0.57) | +3.09 | +0.44 | +6.75 | +1.45 |
| left, radio and GPS (3.14) | +0.52 | −2.13 | +4.17 | −1.13 |

With no sun the cell carries the relay, from 80 % of 850 mAh (D81):

| Load | at room temperature | at −20 °C (70 %) |
|---|---:|---:|
| radio only, 6.4 mA | 106 h, 4.4 days | 74 h, 3.1 days |
| radio and GPS, 35.4 mA | 19 h, 0.8 days | 13 h, 0.6 days |

Without the panel's power D82 rests the GPS an hour after it goes, so a
dark day costs the radio's figure after that first hour. What the GPS
changes is the sunny side: it eats the 10 W panel's December surplus,
and in the worst December neither panel keeps up. Giving the roof node
a map point puts the GPS in standby for good (D82), which leaves the
radio column: the 10 W panel covers even the worst December, the 20 W
one with three times the margin. All of it holds only while the cell is
warm enough to charge; in a cold spell the panel gives nothing (above)
and the cell's days in the second table are all there is. D81's 6.4 mA
is the radio's average in the simulator's busiest city (S4); the rest of
the board on battery has not been measured.

The bench found that a USB-C-to-C cable may not power the board
(`bark-fw/README.md`, "Flash"), so the panel's cable is USB-A to C.

**Snow and ice.** Snow can bury the box for weeks. Nothing on the node
depends on seeing it: the screen stays inside, unseen, and the node is
read through the phone and the mesh, which also removes the window's
sealing and the question of snow over it. The openings are all in the
lid, and there are several: two drain holes, six latch slots and the
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
0.018 m² to the wind, so with a force coefficient of 2 it takes about
42 N. A 10 W panel, about 0.09 m², takes about 210 N; on its ball
joint about 0.1 m above a base, that is 21 N·m on the base's screws.
Where the panel goes is under "The panel" below.

**Condensation.** The air in the box cools every night and its
moisture condenses on the walls. A box sealed tight still breathes,
through whatever leak it has, as the air inside swells and shrinks, and
it keeps what it draws in. So the box is not sealed: it breathes
through the lid's joint and openings, and what condenses runs down to
the drains and out.

## The design

**Shape and orientation.** One printed body, closed on every side but
the bottom; a lid that snaps into the bottom. The case stands on end,
its antenna jack up, its back to the wall and its screen facing out, so
the stock antenna stands straight up on the jack, polarised like every
other node's. The USB-C port then faces sideways. Inside the body are
three spaces, each open down to the lid, so that the case slides in from
below with antenna and cable on: a column round the case, a tower over
the antenna, and a bay beside the port where the cable turns down. In
the column: crush ribs on the front, back and both side walls, clear of
the buttons, that press 0.15 mm into the case so it does not rattle; a
shoulder and a stop over the case's two upper faces that hold it from
above; and two posts on the lid that hold it up against them. The
shell never presses the buttons.

**The cable's bay.** A straight plug stays in the port, and the cable
turns 90° down to the lid beside it. Lindy's data sheet for a USB 2.0
Type-C cable (32005) gives a 3.8 mm jacket and a 38 mm minimum bend
radius, ten diameters; the rule of thumb of five diameters, 20 mm for a
4 mm cable, is half that. The model follows the maker: the bay reaches
28 mm of plug, 38 mm of bend and the cable's radius and a margin beyond
the case, and adds 68.7 mm to the box's width. At 20 mm it would add
50.7 mm. `BEND_R` takes the figure for the cable actually used.

**Size.** With a 38 mm bend: 131 mm wide, 30 mm deep, 164 mm tall over
the antenna's tower; without the bay the box would be 62 mm wide. With
the back pad and its ears it is 159 mm wide; 56 mm deep at the pole
saddle, and 96 mm from the saddle to the front of the panel pad. At 20 mm:
113 mm wide and 146 mm tall, the lid also coming up as the bend shortens.

**Antennas inside.** Nothing goes through the walls, the antenna
included: it stands in a tower of the body with 4 mm of air around it.
ASA's permittivity and loss at 433 MHz and 1.575 GHz were not found.
Taken anywhere in the range of the styrene plastics, a relative
permittivity up to 3.5 and a loss tangent up to 0.02, a 2.4 mm wall
absorbs under 0.01 dB at 433 MHz and under 0.05 dB at 1.575 GHz, and
reflects under 0.1 dB, since the wall is a small fraction of a
wavelength. The shell costs the signal nothing that matters; what it may
do is detune the antenna a little, and so may a layer of wet snow or ice
on it. That is a bench measurement: the antenna's match in the box and
out of it. Where the GPS antenna sits in the case is unknown; nothing
metal is between it and the sky in any direction, and with a map point
(above) the roof node hardly needs it.

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

**Snap fit.** Six latch beams lie along the lid's long edges, each
standing free of the floor over a slot 4 mm wide, with a tooth at its
free end that catches a groove in the body wall. The beams bend in the
plane of the print's layers, so the strain runs along the printed lines
and not across them: across layers printed ASA holds only 11 MPa, against
42 MPa along them (Prusament ASA data sheet). The strain at a beam's
root is 1.5 × thickness × deflection / length² (Bayer, "Snap-Fit Joints
for Plastics – A Design Guide", Table 1, constant section): 2.0 mm thick,
1.0 mm tooth, 25 mm long, 0.48 %. The guide lets an amorphous plastic go
to about 70 % of its yield strain for one snap and 60 % of that for
snaps repeated; Prusament ASA yields at 3.4 %, so 1.4 %. The script
halves that for the cold and checks every beam against 0.71 %. A beam
pushes back with about 1.7 N (modulus 1.7 GPa); through the 30° lead-in,
with a friction coefficient of 0.5 assumed, the lid goes on with about
16 N. The catch is square; to open, push each beam in through its slot
with a small screwdriver. The slots are drains as well.

**No gasket.** Water does not enter from below: rain falls on the top
and runs off the walls past the joint, and only wind-driven spray
reaches the lid from underneath, onto a recessed plate it cannot climb
past. A gasket would make the box tight but for its openings, and so
trap what the breathing draws in; at −25 °C a printed or rubber seal
also stiffens. The joint stays a plain 0.25 mm gap.

**Cable entry.** The cable comes down through a slot at the bay's outer
end that runs to the lid's front edge; the cable is slid in from the
side before the lid goes on, and the slot, 0.3 mm narrower than the
cable, holds it lightly. Outside, the cable hangs down in a drip loop
below the box before it goes to the panel, so water running along it
drips off the bottom of the loop.

**Mounting.** A pad on the back carries four screw holes in ears beyond
the body, for a wall, and a pair of upright tunnels for hose clamps
round a horizontal railing. For a pole, a saddle on the pad.

**The pole saddle.** Wooden broom handles in shops are 23 to 28 mm
across: Faithfull sells 23 mm, Hillbrush 23.5 and 28 mm. The saddle is
a 90° V, 26 mm open and 13 mm deep, down the back of the pad, and two
12 mm stainless hose clamps pull the pole into it through tunnels in the
pad, one near the top and one near the bottom. A 23 mm pole sits with
its centre 16.3 mm from the bottom of the V and stands 14.8 mm proud of
the saddle; a 28 mm pole, 19.8 and 20.8 mm. So the band wraps the pole
and not the saddle; larger poles ride on the V's edges and work as
well. One screwdriver closes
it. This and not a printed two-part clamp with one M4 bolt: the clamp's
arms would bend, across the print's weak layers and in the cold, and fit
one range of diameters; here the steel band takes the tension and the
plastic is only pressed. The band turns round 3 mm rounded edges of the
saddle. What it must hold: with a friction coefficient of 0.3 assumed
between ASA and wood, and the V pressing the pole 1.41 times as hard as
the band pulls, two clamps hold 1.7 times the band's tension; 42 N of
wind on the box needs 25 N in each band, and 250 N with a panel on the
box needs 150 N. A band tightened snug, about 300 N, presses the
rounded edge with about 12.5 MPa over a 12 × 2 mm contact, a strain of
0.7 % in compression, against a yield of 3.4 % and a strength of 40 MPa
in tension (Prusament ASA data sheet). Nothing in it is in tension or
bending, which is what the cold makes brittle; the cold raises the
modulus and lowers that strain. Steel clamps, not plastic ties, which
grow brittle in UV and cold.

**The panel.** The best place for the panel is on the pole, at least a
quarter wavelength from the antenna, 17 cm at 433 MHz (a rule of thumb
for keeping a conductor from detuning an antenna), and lower than the
antenna's tip, so the antenna sees over the panel's frame and cells,
which are metal and shade the radio toward the panel's side. Below the
box suits the cable too, which leaves the lid downward. Where there is
no room on the pole, a pad on the bay's top takes the panel's own base:
a 70 mm square plate (`PANEL_PAD`, an estimate for a base about 60 mm
across), 3 mm thick on 9 mm of ribs, with four M4 holes on a 45 mm
circle (`PANEL_PCD`, `PANEL_HOLES`, estimates: measure the base). The
holes take heat-set brass inserts, set with a soldering iron after the
print, and not printed threads or screws cut into the plastic: brass
threads hold the screws' preload where plastic threads creep, and take
the screws out and in again. The plate stands 40 mm beyond the front
wall on 45° ribs, so it prints without support. It sits beside the
tower, not on it: a panel there would put its metal at the antenna's
tip. Beside it, the panel's edge is a few centimetres from the antenna,
well inside that quarter wavelength, so this place costs the antenna
something; how much is a bench measurement. In a design gust the pad's
21 N·m pulls about 210 N out of the worst insert (pivoting on the base's
far edge); the inserts' pull-out in ASA was not found, and the first
print tests it.

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
  cell's temperature, keeps charge off below the cell's floor.
- **The screen in the cold.** Whether the firmware stops drawing the
  screen below its 0 °C rating, or on a roof node at all.
- **The bend.** The cable's real minimum bend radius sets a third of the
  box's width; a thin, flexible cable shrinks the bay.
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
- **The panel on the box.** What a panel beside the tower does to the
  antenna's match and pattern, and whether the inserts hold its gusts.

## What the first print must verify

1. The case slides in from below with the antenna, plug and cable on,
   the crush ribs grip without forcing it, and the buttons are not
   pressed; measure the estimated parameters first and rerun the script.
2. The cable turns in the bay without being forced tighter than its
   maker allows.
3. The lid snaps on by hand, holds, and comes off with a screwdriver
   through the slots, at room temperature and after a night in a freezer.
4. Shrinkage: the lid's fit in the body and the case's fit in the
   cavity; change `SHRINK` and the clearances to suit.
5. Watertightness: mounted as on the roof, an hour of water from a
   shower head over the top and sides leaves the inside dry; a spoonful
   of water poured into the box upright drains out completely.
6. The antenna's match at 433 MHz in the box and out of it, on the
   bench's analyser, and the link's level to a second board at the same
   place with and without the box.
7. The saddle on a 23 mm and a 28 mm handle: the box does not turn on
   it when pushed by hand with the clamps snug.
8. The inserts: an M4 screw in each, pulled to 400 N, twice the design
   gust's load, at room temperature and from the freezer.
9. A winter's log from the roof: the die temperature, the cell's
   voltage, and whether any water stands in the lid.
