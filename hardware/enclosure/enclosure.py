"""Outdoor enclosure for one T-Echo in its stock case, printed in ASA.

A sealed body with a snap-fit lid at the bottom. The lid carries the
drains and the USB cable's entry; nothing else goes through a wall.
docs/enclosure-2026-11-09.md gives the sources and the reasons.

Run:  hardware/.venv/bin/python hardware/enclosure/enclosure.py
Out:  hardware/enclosure/out/ (STL to print, STEP of the assembly, PNG views)

The case stands on end with its antenna jack up and the stock antenna
screwed straight on, so the antenna is upright like every other node's.
The USB-C port then faces sideways; a straight plug stays in it and the
cable turns down to the lid in a bay beside the case.

World axes, in mm: z up as mounted, y from the wall (y-) to the screen
(y+), x across. The case is measured in the axes of its maker's 3D file
(dimensions/t-echo_3d.stl), here called the file's: world (x, y, z) =
file (z, y, -x).
"""

import math
from pathlib import Path

import cadquery as cq

# --- The cased T-Echo, measured in the maker's 3D file (file axes) ---------
F_MAIN_X = (-24.0, 24.61)      # the screen part
F_CASE_Y = (5.48, 29.72)       # back to screen bezel
F_LOGO_FRONT_Y = 27.63         # the logo part's front face
F_CASE_Z = (-24.64, 28.66)     # USB-C face to the button face
F_BUTTON_TOP_Z = 29.65         # reset and user buttons stand proud of their face
F_BUTTONS_X = ((-13.5, -9.3), (16.0, 20.4))
F_BUTTONS_Y = (20.25, 24.66)
F_LOGO_X = (-40.67, -24.0)     # the narrower part that carries the antenna jack
F_LOGO_TOP_Z = 6.36
F_SMA_X = -42.39               # end face of the jack's flange ring, Ø16.1
F_SMA_Y, F_SMA_Z = 18.43, -14.9
F_USB_X, F_USB_Y = 11.3, 19.8  # centre of the USB-C opening in the z- face

# The same in world axes.
CASE_L, CASE_R = F_CASE_Z                      # left (USB-C) and right (button) faces
BUTTON_R = F_BUTTON_TOP_Z
BUTTONS_Z = tuple(sorted((-b, -a)) for a, b in F_BUTTONS_X)
CASE_Y = F_CASE_Y
CASE_BOT, SCREEN_TOP = -F_MAIN_X[1], -F_MAIN_X[0]
LOGO_TOP = -F_LOGO_X[0]
LOGO_R = F_LOGO_TOP_Z                          # the logo part spans x CASE_L..LOGO_R
SMA_TOP = -F_SMA_X
ANT_X, ANT_Y = F_SMA_Z, F_SMA_Y                # antenna axis
USB_Z, USB_Y = -F_USB_X, F_USB_Y

# --- Estimates: measure these on the parts in hand --------------------------
ANT_LEN = 60.0        # ESTIMATE: stock 433 MHz antenna above the jack, from photo scale
ANT_D = 16.0          # ESTIMATE: its widest, at the base
PLUG_LEN = 28.0       # ESTIMATE: straight USB-C plug, case face to where the cable is free
PLUG_W = 12.0         # ESTIMATE: plug body across the port's long side (vertical here)
PLUG_T = 7.0          # ESTIMATE: plug body across the short side
CABLE_D = 4.0         # ESTIMATE: the panel's USB-A-to-C cable
# Smallest radius the cable may turn through: a cable maker's figure for
# a 3.8 mm USB 2.0 cable is 38 mm (Lindy 32005 data sheet), ten diameters.
BEND_R = 38.0

# --- Fit ---------------------------------------------------------------------
CLEAR = 0.4           # case to wall: covers the file's tolerance and print error
RIB_BITE = 0.15       # crush ribs press this far into the case: no rattle
ANT_GAP = 4.0         # air around the antenna, so the shell barely detunes it
LID_CLEAR = 0.25      # lid to body, each side: a sliding fit
STOP_GAP = 0.2        # case to top stops and to lid posts
BUTTON_GAP = 0.8      # wall beside the buttons: never pressed by the shell

# --- Walls and print -----------------------------------------------------------
WALL = 2.4            # six 0.4 mm perimeters, solid
EDGE_R = 3.0          # outer vertical edges
SHRINK = 1.005        # ASA shrinks 0.4-0.7 % (INEOS Luran S data); tune after print 1

# --- Lid and snap fit --------------------------------------------------------------
LID_T = 2.4           # floor at its front edge
LID_FALL = 1.2        # floor top falls this much from back to front, to the drains
DRIP = 1.5            # body skirt hangs below the lid: water drips off before the joint
BEAM_T = 2.0          # latch beam thickness, flexing in the layer plane
BEAM_H = 8.0          # its height
UNDERCUT = 1.0        # tooth depth = deflection on snapping
TOOTH_FLAT = 2.0      # tooth's vertical catch length
LEAD_DEG = 30.0       # lead-in face from the insertion direction
SLOT_W = 4.0          # gap behind each beam: room to flex, a drain, a release
ROOT = 3.0            # full-height block at the beam's root end
POST_D = 6.0          # posts that hold the case up against the top stops
# Strain allowed at the beam root. Bayer's snap-fit guide: an amorphous
# plastic to about 70 % of its yield strain for one snap, 60 % of that for
# repeated ones; Prusament ASA yields at 3.4 %, so 1.4 %. Half of that for
# brittleness at -25 C (notched impact falls 40 -> 9 kJ/m2 at -30 C).
STRAIN_LIMIT = 0.5 * 0.6 * 0.7 * 0.034

# --- Mounting ------------------------------------------------------------------------
PAD_T = 8.0           # back pad: bands pass through it, behind the body
EAR_W = 14.0          # ears beyond the body for wall screws
SCREW_D = 4.5         # M4 or 4 mm stainless wood screw
SCREW_HEAD_D = 8.5
BAND_W, BAND_T = 14.0, 3.5  # tunnels for 12 mm stainless hose clamps
# Pole saddle: a 90 deg V that seats broom handles of 23 to 28 mm (the
# shop sizes) and larger poles, the hose clamps pulling the pole into it.
POLE_D = (23.0, 28.0)
SADDLE_W = 40.0       # across
SADDLE_D = 18.0       # out from the pad
V_DEPTH = 13.0        # of the V, 26 mm open
SADDLE_R = 3.0        # round edges where the band turns
# Panel pad on the bay's top, for a small panel's own base.
PANEL_PAD = 70.0      # ESTIMATE: square, for a base about 60 mm across
PANEL_T = 3.0         # plate
PANEL_RISE = 9.0      # ribs and bosses under the plate, room for the inserts
PANEL_PCD = 45.0      # ESTIMATE: the base's screw holes lie on this circle
PANEL_HOLES = 4       # ESTIMATE: three or four
INSERT_HOLE = 5.6     # ESTIMATE: hole for an M4 heat-set brass insert, from its maker
INSERT_L = 8.1        # ESTIMATE: that insert's length
RIB = 3.0             # rib thickness under the plate

OUT = Path(__file__).resolve().parent / "out"


def box(x0, x1, y0, y1, z0, z1):
    return cq.Workplane("XY").box(x1 - x0, y1 - y0, z1 - z0, centered=False).translate((x0, y0, z0))


def rbox(x0, x1, y0, y1, z0, z1, r):
    return box(x0, x1, y0, y1, z0, z1).edges("|Z").fillet(r)


def prism_x(x0, x1, pts_yz):
    """A prism along x from a polygon in the (y, z) plane."""
    return cq.Workplane("YZ").workplane(offset=x0).polyline(pts_yz).close().extrude(x1 - x0)


def cyl_z(x, y, z0, z1, d):
    return cq.Workplane("XY").center(x, y).circle(d / 2).extrude(z1 - z0).translate((0, 0, z0))


# --- Derived layout ------------------------------------------------------------------
ANT_TOP = SMA_TOP + ANT_LEN
ANT_R = ANT_D / 2 + ANT_GAP
PLUG_END = CASE_L - PLUG_LEN                   # where the cable leaves the plug
CABLE_X = PLUG_END - BEND_R                    # the cable's run down to the lid
BEND_BOT = USB_Z - BEND_R                      # where that run starts

# Three spaces, each open down to the lid so that the case slides in from
# below with antenna and cable on: the column round the case, the tower
# over the antenna, and the bay beside the port where the cable turns down.
CX0 = CABLE_X - CABLE_D / 2 - 3.0              # bay's outer wall, a solid lid beyond the slot
MX0 = min(CASE_L - CLEAR, ANT_X - ANT_R)       # column's left wall, above the bay
CX1 = BUTTON_R + BUTTON_GAP
CY0 = min(CASE_Y[0] - CLEAR, ANT_Y - ANT_R)
CY1 = max(CASE_Y[1] + CLEAR, ANT_Y + ANT_R)
CEIL_Z = LOGO_TOP + 0.8                        # over the case; the jack's ring sits in the tower
TOWER_CEIL_Z = ANT_TOP + ANT_GAP
TX0, TX1 = ANT_X - ANT_R, ANT_X + ANT_R
BAY_TOP = USB_Z + PLUG_W / 2 + 1.0
# The lid floor sits below the latch beams' tops under the case and below
# the cable's bend.
FLOOR_Z = min(CASE_BOT - STOP_GAP - (BEAM_H - LID_T) - 1.0, BEND_BOT - LID_FALL - 1.0)
LID_BOT_Z = FLOOR_Z - LID_T
BODY_BOT_Z = LID_BOT_Z - DRIP

OX0, OX1 = CX0 - WALL, CX1 + WALL
OY0, OY1 = CY0 - WALL, CY1 + WALL
SADDLE_X = (OX0 + OX1) / 2                     # under the middle of the box

# Lid footprint and latches.
LX0, LX1 = CX0 + LID_CLEAR, CX1 - LID_CLEAR
LY0, LY1 = CY0 + LID_CLEAR, CY1 - LID_CLEAR
TOOTH_Z = LID_BOT_Z + BEAM_H - TOOTH_FLAT - UNDERCUT / math.tan(math.radians(LEAD_DEG)) - 0.5
BEAM_L = 25.0
# (root x, free-end x, side +1 front / -1 back), spread along the lid and
# clear of the cable slot at the bay's outer end.
BEAMS = (
    (LX1 - ROOT, LX1 - ROOT - BEAM_L, +1),
    (LX1 - ROOT - BEAM_L - 8.0, LX1 - ROOT - 2 * BEAM_L - 8.0, +1),
    (CABLE_X + 12.0, CABLE_X + 12.0 + BEAM_L, +1),
    (LX1 - ROOT, LX1 - ROOT - BEAM_L, -1),
    (LX1 - ROOT - BEAM_L - 8.0, LX1 - ROOT - 2 * BEAM_L - 8.0, -1),
    (LX0 + ROOT, LX0 + ROOT + BEAM_L, -1),
)
POST_Y = (CASE_Y[0] + CASE_Y[1]) / 2
POSTS = ((-15.0, POST_Y), (20.0, POST_Y))
DRAINS_X = (CABLE_X + 6.0, 0.0)


def beam_strain(length):
    # Cantilever of constant section, Bayer guide Table 1: e = 1.5 h y / l^2.
    return 1.5 * BEAM_T * UNDERCUT / length ** 2


def tooth_x(x_root, x_free):
    return (x_free - 6.0, x_free - 1.0) if x_free > x_root else (x_free + 1.0, x_free + 6.0)


def tooth_profile(side):
    """Tooth on a beam's outer face, in (y, z)."""
    face = LY1 if side > 0 else LY0
    rise = UNDERCUT / math.tan(math.radians(LEAD_DEG))
    s = side
    return [
        (face - s * 0.01, TOOTH_Z),
        (face + s * UNDERCUT, TOOTH_Z),
        (face + s * UNDERCUT, TOOTH_Z + TOOTH_FLAT),
        (face - s * 0.01, TOOTH_Z + TOOTH_FLAT + rise),
    ]


def groove_profile(side):
    """Groove in the body wall that takes a tooth: flat catch below, 45 deg above
    so that it prints without support with the body on its open end."""
    wall = CY1 if side > 0 else CY0
    d = UNDERCUT + 0.3
    z0 = TOOTH_Z - 0.2
    z1 = TOOTH_Z + TOOTH_FLAT + 0.4
    s = side
    return [(wall - s * 0.01, z0), (wall + s * d, z0), (wall + s * d, z1), (wall - s * 0.01, z1 + d)]


def make_body(ribs=True):
    outer = rbox(MX0 - WALL, OX1, OY0, OY1, BODY_BOT_Z, CEIL_Z + WALL, EDGE_R)
    outer = outer.union(rbox(TX0 - WALL, TX1 + WALL, OY0, OY1, BODY_BOT_Z, TOWER_CEIL_Z + WALL, EDGE_R))
    outer = outer.union(rbox(OX0, MX0 + EDGE_R, OY0, OY1, BODY_BOT_Z, BAY_TOP + WALL, EDGE_R))

    cavity = box(MX0, CX1, CY0, CY1, BODY_BOT_Z - 1, CEIL_Z)
    cavity = cavity.union(box(TX0, TX1, CY0, CY1, BODY_BOT_Z - 1, TOWER_CEIL_Z))
    cavity = cavity.union(box(CX0, MX0 + 0.01, CY0, CY1, BODY_BOT_Z - 1, BAY_TOP))
    body = outer.cut(cavity)

    # Shoulder over the screen part beside the logo part, and a stop over
    # the logo part beside the tower: they hold the case from above.
    body = body.union(box(LOGO_R + CLEAR, CX1, CY0, CY1, SCREEN_TOP + STOP_GAP, CEIL_Z))
    body = body.union(box(TX1, LOGO_R + CLEAR + 0.01, CY0, CY1, LOGO_TOP + STOP_GAP, CEIL_Z))

    # Crush ribs: vertical, chamfered at the bottom so the case slides in.
    def rib(x, y_wall, y_case, side, z0, z1):
        reach = abs(y_wall - y_case) + RIB_BITE
        y_tip = y_wall - side * reach
        pts = [(y_wall + side * 0.01, z0 - 3.0), (y_wall + side * 0.01, z1), (y_tip, z1), (y_tip, z0 - 3.0 + reach * 2)]
        return prism_x(x - 0.8, x + 0.8, pts)

    if ribs:
        for x in (-15.0, 0.0, 15.0):
            body = body.union(rib(x, CY1, CASE_Y[1], +1, CASE_BOT, SCREEN_TOP))
        for x in (-15.0, 0.0):
            body = body.union(rib(x, CY0, CASE_Y[0], -1, CASE_BOT, LOGO_TOP))
        body = body.union(rib(15.0, CY0, CASE_Y[0], -1, CASE_BOT, SCREEN_TOP))
        # On the button side, below the buttons' band; on the port side,
        # above the bay.
        for y in (9.0, 15.0):
            reach = CX1 - CASE_R + RIB_BITE
            body = body.union(box(CX1 - reach, CX1 + 0.01, y - 0.8, y + 0.8, CASE_BOT, SCREEN_TOP))
        for y in (9.0, 26.0):
            reach = CASE_L - MX0 + RIB_BITE
            body = body.union(box(MX0 - 0.01, MX0 + reach, y - 0.8, y + 0.8, BAY_TOP + 1.0, LOGO_TOP))

    # Latch grooves. The front wall is thickened outside each so it stays WALL
    # thick; at the back the pad does that.
    for x_root, x_free, side in BEAMS:
        tx0, tx1 = tooth_x(x_root, x_free)
        if side > 0:
            body = body.union(box(tx0 - 2, tx1 + 2, OY1 - 0.01, OY1 + UNDERCUT + 0.3,
                                  TOOTH_Z - 2, TOOTH_Z + TOOTH_FLAT + 4))
        body = body.cut(prism_x(tx0 - 1.0, tx1 + 1.0, groove_profile(side)))

    # Back pad: wall ears with screw holes, upright tunnels for bands round
    # a horizontal rail. (The XZ workplane faces -y, so its offsets are minus y.)
    py0, py1 = OY0 - PAD_T, OY0 + 0.01
    pz0, pz1 = BODY_BOT_Z, CEIL_Z + WALL
    pad = rbox(OX0 - EAR_W, OX1 + EAR_W, py0, py1, pz0, pz1, 2.0)
    mid_y = (py0 + OY0) / 2
    h = pz1 - pz0
    for xc in (OX0 + 15.0, OX1 - 15.0):
        pad = pad.cut(box(xc - BAND_W / 2, xc + BAND_W / 2, mid_y - BAND_T / 2, mid_y + BAND_T / 2,
                          pz0 - 1, pz1 + 1))
    for xc in (OX0 - EAR_W / 2, OX1 + EAR_W / 2):
        for zc in (pz0 + 8.0, pz1 - 8.0):
            hole = cq.Workplane("XZ").workplane(offset=-py1 - 1).center(xc, zc).circle(SCREW_D / 2).extrude(PAD_T + 2)
            head = cq.Workplane("XZ").workplane(offset=-py1 - 0.01).center(xc, zc).circle(SCREW_HEAD_D / 2).extrude(3.0)
            pad = pad.cut(hole).cut(head)

    # Pole saddle behind the pad. The bands run across through the pad
    # behind it, come out past its sides and round the pole, so the
    # plastic is only pressed, never bent.
    sx0, sx1 = SADDLE_X - SADDLE_W / 2, SADDLE_X + SADDLE_W / 2
    sy = py0 - SADDLE_D
    saddle = rbox(sx0, sx1, sy, py0 + 0.01, pz0, pz1, SADDLE_R)
    v = [(SADDLE_X - V_DEPTH, sy - 0.01), (SADDLE_X + V_DEPTH, sy - 0.01), (SADDLE_X, sy + V_DEPTH)]
    saddle = saddle.cut(cq.Workplane("XY").polyline(v).close().extrude(h + 2).translate((0, 0, pz0 - 1)))
    pad = pad.union(saddle)
    for zc in (pz0 + h * 0.2, pz0 + h * 0.8):
        z0, z1 = zc - BAND_W / 2, zc + BAND_W / 2
        pad = pad.cut(box(sx0 - BAND_T - 1, sx1 + BAND_T + 1, mid_y - BAND_T / 2, mid_y + BAND_T / 2, z0, z1))
        for xe in (sx0 - BAND_T - 1, sx1 + 1):
            pad = pad.cut(box(xe, xe + BAND_T, py0 - 1, mid_y + BAND_T / 2, z0, z1))
    body = body.union(pad)
    return body.union(panel_pad())


def panel_pad():
    """Flat plate on the bay's top for a small panel's base: ribs under it,
    bosses for M4 heat-set inserts, gussets under the part beyond the front."""
    zb = BAY_TOP + WALL                      # the bay's outer top
    zp = zb + PANEL_RISE                     # underside of the plate
    x1 = TX0 - WALL                          # stop at the tower
    x0 = x1 - PANEL_PAD
    y0 = OY0                                 # in front of the back pad
    y1 = y0 + PANEL_PAD
    xc, yc = (x0 + x1) / 2, (y0 + y1) / 2
    plate = box(x0, x1, y0, y1, zp, zp + PANEL_T).edges("|Z").fillet(3.0)
    drop = y1 - OY1                          # overhang beyond the front wall
    # Upright ribs at the plate's ends and under the screw holes.
    hole_xs = sorted({round(xc + PANEL_PCD / 2 * math.cos(2 * math.pi * (i + 0.5) / PANEL_HOLES), 6)
                      for i in range(PANEL_HOLES)})
    for x in [x0 + RIB / 2, *hole_xs, x1 - RIB / 2]:
        pts = [(y0, zb - 0.01), (OY1 - 0.01, zb - 0.01), (OY1 - 0.01, zp - drop), (y1, zp + 0.01), (y0, zp + 0.01)]
        plate = plate.union(prism_x(x - RIB / 2, x + RIB / 2, pts))
    for y in (y0 + RIB / 2, OY1 - RIB / 2):
        plate = plate.union(box(x0, x1, y - RIB / 2, y + RIB / 2, zb - 0.01, zp + 0.01))
    for i in range(PANEL_HOLES):
        a = 2 * math.pi * (i + 0.5) / PANEL_HOLES
        hx, hy = xc + PANEL_PCD / 2 * math.cos(a), yc + PANEL_PCD / 2 * math.sin(a)
        if OY0 < hy < OY1:
            plate = plate.union(cyl_z(hx, hy, zb - 0.01, zp + 0.01, 11.0))
        else:
            # Beyond the wall the boss hangs from the plate; a 45 deg cone
            # under it, down onto its rib, prints without support.
            bot = zp - INSERT_L - 2.0
            plate = plate.union(cyl_z(hx, hy, bot, zp + 0.01, 11.0))
            plate = plate.union(cq.Workplane("XY").center(hx, hy).circle(RIB / 2 - 0.2).workplane(offset=4.0)
                                .circle(5.5).loft().translate((0, 0, bot - 4.0)))
        plate = plate.cut(cyl_z(hx, hy, zp + PANEL_T - INSERT_L - 1.0, zp + PANEL_T + 1, INSERT_HOLE))
    return plate


def make_lid():
    # Floor whose top falls from back to front, so water runs to the front drains.
    pts = [(LY0, LID_BOT_Z), (LY1, LID_BOT_Z), (LY1, FLOOR_Z), (LY0, FLOOR_Z + LID_FALL)]
    lid = prism_x(LX0, LX1, pts).edges("|Z").fillet(1.0)

    for x_root, x_free, side in BEAMS:
        xa, xb = sorted((x_root, x_free))
        # The slot frees the beam from the floor; open below, it is a drain and
        # lets a small screwdriver push the beam in to release the lid.
        if side > 0:
            strip = (LY1 - BEAM_T - SLOT_W, LY1)
            beam_y = (LY1 - BEAM_T, LY1)
        else:
            strip = (LY0, LY0 + BEAM_T + SLOT_W)
            beam_y = (LY0, LY0 + BEAM_T)
        toward = 1.0 if x_free > x_root else -1.0
        sx = sorted((x_root, x_free + toward))
        lid = lid.cut(box(sx[0], sx[1], strip[0], strip[1], LID_BOT_Z - 1, FLOOR_Z + LID_FALL + 1))
        lid = lid.union(box(xa, xb, beam_y[0], beam_y[1], LID_BOT_Z, LID_BOT_Z + BEAM_H))
        # A full-height block at the root, so the beam bends along its length
        # and not by twisting the thin floor it stands on.
        rx = sorted((x_root, x_root - ROOT * toward))
        lid = lid.union(box(rx[0], rx[1], strip[0], strip[1], LID_BOT_Z, LID_BOT_Z + BEAM_H))
        lid = lid.union(prism_x(*tooth_x(x_root, x_free), tooth_profile(side)))

    # Cable slot, open to the front edge: the cable is slid in from the side.
    w = CABLE_D - 0.3  # a slight pinch holds the cable
    lid = lid.cut(box(CABLE_X - w / 2, CABLE_X + w / 2, USB_Y, LY1 + 1, LID_BOT_Z - 1, FLOOR_Z + LID_FALL + 1))
    lid = lid.cut(cyl_z(CABLE_X, USB_Y, LID_BOT_Z - 1, FLOOR_Z + LID_FALL + 1, w))

    # Two more drains, just behind the front latches.
    for x in DRAINS_X:
        lid = lid.cut(cyl_z(x, LY1 - BEAM_T - SLOT_W - 3.0, LID_BOT_Z - 1, FLOOR_Z + LID_FALL + 1, 4.0))

    # Posts hold the case up against the stops once the lid is in.
    for x, y in POSTS:
        lid = lid.union(cyl_z(x, y, FLOOR_Z - 1, CASE_BOT - STOP_GAP, POST_D))
    return lid


def make_techo(through_lid=True):
    """The cased T-Echo with antenna, plug and cable, as blocks: for the fit
    check and the views. through_lid: the cable's run down through the lid."""
    t = box(CASE_L, CASE_R, CASE_Y[0], CASE_Y[1], CASE_BOT, SCREEN_TOP)
    t = t.union(box(CASE_L, LOGO_R, CASE_Y[0], F_LOGO_FRONT_Y, SCREEN_TOP - 0.5, LOGO_TOP))
    for z0, z1 in BUTTONS_Z:
        t = t.union(box(CASE_R - 0.1, BUTTON_R, F_BUTTONS_Y[0], F_BUTTONS_Y[1], z0, z1))
    t = t.union(cyl_z(ANT_X, ANT_Y, LOGO_TOP - 0.1, SMA_TOP, 16.1))
    t = t.union(cq.Workplane("XY").center(ANT_X, ANT_Y).circle(ANT_D / 2).workplane(offset=ANT_LEN)
                .circle(ANT_D / 2 - 3).loft().translate((0, 0, SMA_TOP)))
    t = t.union(box(PLUG_END, CASE_L + 0.1, USB_Y - PLUG_T / 2, USB_Y + PLUG_T / 2,
                    USB_Z - PLUG_W / 2, USB_Z + PLUG_W / 2))
    # The cable's turn: a quarter circle from the plug's end down to its run.
    start = cq.Vector(PLUG_END + 0.1, USB_Y, USB_Z)
    centre = cq.Vector(PLUG_END, USB_Y, BEND_BOT)
    mid = centre + cq.Vector(-BEND_R * math.sqrt(0.5), 0, BEND_R * math.sqrt(0.5))
    end = cq.Vector(CABLE_X, USB_Y, BEND_BOT)
    path = cq.Wire.assembleEdges([cq.Edge.makeLine(start, cq.Vector(PLUG_END, USB_Y, USB_Z)),
                                  cq.Edge.makeThreePointArc(cq.Vector(PLUG_END, USB_Y, USB_Z), mid, end)])
    plane = cq.Plane(origin=start, xDir=(0, 1, 0), normal=(-1, 0, 0))
    t = t.union(cq.Workplane(plane).circle(CABLE_D / 2).sweep(cq.Workplane().add(path), transition="round"))
    if through_lid:
        t = t.union(cyl_z(CABLE_X, USB_Y, BODY_BOT_Z - 10, BEND_BOT + 0.1, CABLE_D))
    return t


def volume(wp):
    return sum(s.Volume() for s in wp.solids().vals())


def check(body, bare, lid, techo):
    for name, part in (("body", body), ("lid", lid)):
        solids = part.solids().vals()
        assert len(solids) == 1, f"{name}: {len(solids)} solids"
        assert solids[0].isValid(), f"{name}: invalid solid"
    clash = volume(body.intersect(lid))
    assert clash < 1e-3, f"lid clashes with body: {clash:.3f} mm3"
    # Nothing but the crush ribs may reach into the case, antenna, plug or
    # the cable's turn, and nothing but the slot's pinch into its run down.
    for name, part in (("body without ribs", bare), ("lid", lid)):
        v = volume(part.intersect(make_techo(through_lid=False)))
        assert v < 1e-3, f"{name} clashes with the T-Echo: {v:.3f} mm3"
    print(f"  crush ribs bite {volume(body.intersect(techo)):.0f} mm3 of the case")
    for x_root, x_free, _ in BEAMS:
        e = beam_strain(abs(x_free - x_root))
        assert e <= STRAIN_LIMIT, f"beam strain {e:.4f} over {STRAIN_LIMIT:.4f}"
    print(f"  latch strain {max(beam_strain(abs(b - a)) for a, b, _ in BEAMS):.2%}, limit {STRAIN_LIMIT:.2%}")


def png(parts, path, direction, title):
    """Shaded parallel view of (workplane, rgb) pairs, looking along direction."""
    import vtk

    ren = vtk.vtkRenderer()
    ren.SetBackground(1, 1, 1)
    for wp, rgb in parts:
        for solid in wp.solids().vals():
            data = solid.toVtkPolyData(0.05, 0.2)
            mapper = vtk.vtkPolyDataMapper()
            mapper.SetInputData(data)
            actor = vtk.vtkActor()
            actor.SetMapper(mapper)
            actor.GetProperty().SetColor(*rgb)
            ren.AddActor(actor)
            edges = vtk.vtkFeatureEdges()
            edges.SetInputData(data)
            edges.BoundaryEdgesOff()
            edges.ManifoldEdgesOff()
            edges.NonManifoldEdgesOff()
            edges.SetFeatureAngle(30)
            emap = vtk.vtkPolyDataMapper()
            emap.SetInputConnection(edges.GetOutputPort())
            emap.ScalarVisibilityOff()
            eact = vtk.vtkActor()
            eact.SetMapper(emap)
            eact.GetProperty().SetColor(0.15, 0.15, 0.15)
            ren.AddActor(eact)
    text = vtk.vtkTextActor()
    text.SetInput(title)
    text.GetTextProperty().SetColor(0, 0, 0)
    text.GetTextProperty().SetFontSize(18)
    text.SetPosition(10, 10)
    ren.AddViewProp(text)
    cam = ren.GetActiveCamera()
    cam.ParallelProjectionOn()
    cam.SetFocalPoint(0, 0, 0)
    cam.SetPosition(*(-c for c in direction))
    cam.SetViewUp(0, 0, 1)
    ren.ResetCamera()
    win = vtk.vtkRenderWindow()
    win.SetOffScreenRendering(1)
    win.AddRenderer(ren)
    win.SetSize(1000, 900)
    win.Render()
    grab = vtk.vtkWindowToImageFilter()
    grab.SetInput(win)
    grab.Update()
    out = vtk.vtkPNGWriter()
    out.SetFileName(str(path))
    out.SetInputConnection(grab.GetOutputPort())
    out.Write()


def main():
    OUT.mkdir(exist_ok=True)
    body, lid, techo = make_body(), make_lid(), make_techo()
    print(f"outside: {OX1 - OX0:.1f} x {OY1 - OY0:.1f} mm, {BODY_BOT_Z:.1f} to {TOWER_CEIL_Z + WALL:.1f} "
          f"({TOWER_CEIL_Z + WALL - BODY_BOT_Z:.1f} mm tall); the bay adds {MX0 - CX0:.1f} mm; "
          f"pad {OX1 - OX0 + 2 * EAR_W:.1f} x {CEIL_Z + WALL - BODY_BOT_Z:.1f} mm")
    check(body, make_body(ribs=False), lid, techo)

    # Both print as mounted, standing on the bed: the body on its open end,
    # the lid on its floor.
    for name, part in (("body", body), ("lid", lid)):
        solid = part.val().scale(SHRINK)
        solid = solid.translate(cq.Vector(0, 0, -solid.BoundingBox().zmin))
        assert solid.isValid()
        cq.exporters.export(cq.Workplane().add(solid), str(OUT / f"{name}.stl"), tolerance=0.02, angularTolerance=0.1)

    asm = cq.Assembly()
    asm.add(body, name="body", color=cq.Color(0.85, 0.85, 0.82))
    asm.add(lid, name="lid", color=cq.Color(0.6, 0.6, 0.6))
    asm.add(techo, name="techo", color=cq.Color(0.2, 0.4, 0.8))
    asm.export(str(OUT / "assembly.step"))

    # Review views. The section is through the antenna axis.
    shell, cover, board = (0.85, 0.85, 0.8), (0.55, 0.6, 0.65), (0.25, 0.45, 0.85)
    png([(body, shell), (lid, cover)], OUT / "view-front.png", (0.6, -1, -0.5), "Front, as mounted")
    png([(body, shell), (lid, cover)], OUT / "view-below.png", (0.4, -1, 0.9), "From below: lid, drains, cable slot")
    cut = box(OX0 - 30, OX1 + 30, OY0 - 30, ANT_Y, BODY_BOT_Z - 5, TOWER_CEIL_Z + 10)
    png([(body.cut(cut), shell), (lid.cut(cut), cover), (techo.cut(cut), board)],
        OUT / "view-section.png", (0, 1, 0), "Section through the antenna axis, seen from behind (x+ to the right)")
    png([(lid, cover)], OUT / "view-lid.png", (0.5, -1, -0.9), "Lid: latch beams, posts, drains")
    png([(body, shell)], OUT / "view-back.png", (-0.5, 1, -0.3), "Back: wall ears, pole saddle, band tunnels")
    png([(body, shell), (lid, cover)], OUT / "view-top.png", (0.5, -0.8, -1), "From above: the panel pad on the bay")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
