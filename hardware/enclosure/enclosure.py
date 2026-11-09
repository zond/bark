"""Outdoor enclosure for one T-Echo in its stock case, printed in ASA.

A sealed body with a snap-fit lid at the bottom. The lid carries the
drains and the USB cable's entry; nothing else goes through a wall.
docs/enclosure-2026-11-09.md gives the sources and the reasons.

Run:  hardware/.venv/bin/python hardware/enclosure/enclosure.py
Out:  hardware/enclosure/out/ (STL to print, STEP of the assembly, PNG views)

Coordinates are those of the board maker's 3D file of the cased T-Echo
(dimensions/t-echo_3d.stl), in mm, with z up as mounted: the case's USB-C
face (z-) points down at the lid, the antenna end (x-) is on the left,
the screen (y+) faces away from the wall.
"""

import math
from pathlib import Path

import cadquery as cq

# --- The cased T-Echo, measured from the maker's 3D file -----------------
MAIN_X = (-24.0, 24.61)      # the screen part, x
CASE_Y = (5.48, 29.72)       # back to screen bezel
LOGO_FRONT_Y = 27.63         # the logo part's front face
CASE_Z = (-24.64, 28.66)     # USB-C face to the button face
BUTTON_TOP_Z = 29.65         # reset and user buttons stand proud of the face
BUTTONS_X = ((-13.5, -9.3), (16.0, 20.4))
LOGO_X = (-40.67, -24.0)     # the narrower part that carries the antenna jack
LOGO_TOP_Z = 6.36
SMA_FACE_X = -42.39          # the jack's flange ring, Ø16.1
SMA_Y, SMA_Z = 18.43, -14.9  # the jack's axis
USB_X, USB_Y = 11.3, 19.8    # centre of the USB-C opening in the z- face

# --- Estimates: measure these on the parts in hand -----------------------
ADAPTER_REACH = 12.0  # ESTIMATE: right-angle SMA adapter, jack face to its upright axis
ADAPTER_RISE = 10.0   # ESTIMATE: its horizontal axis to the face the antenna seats on
ANT_LEN = 60.0        # ESTIMATE: stock 433 MHz antenna, from photo scale
ANT_D = 16.0          # ESTIMATE: its widest, at the base
PLUG_LEN = 28.0       # ESTIMATE: USB-C plug body and strain relief below the case
CABLE_D = 4.0         # ESTIMATE: the panel's USB-A-to-C cable

# --- Fit ------------------------------------------------------------------
CLEAR = 0.4           # case to wall: covers the file's tolerance and print error
RIB_BITE = 0.15       # crush ribs press this far into the case: no rattle
ANT_GAP = 4.0         # air around the antenna, so the shell barely detunes it
LID_CLEAR = 0.25      # lid to body, each side: a sliding fit
STOP_GAP = 0.2        # case to top stops and to lid posts
BUTTON_GAP = 0.8      # ceiling above the buttons: never pressed by the shell

# --- Walls and print --------------------------------------------------------
WALL = 2.4            # six 0.4 mm perimeters, solid
EDGE_R = 3.0          # outer vertical edges
SHRINK = 1.005        # ASA shrinks 0.4-0.7 % (INEOS Luran S data); tune after print 1

# --- Lid and snap fit -----------------------------------------------------------
LID_T = 2.4           # floor at its front edge
LID_FALL = 1.2        # floor top falls this much from back to front, to the drains
DRIP = 1.5            # body skirt hangs below the lid: water drips off before the joint
BEAM_T = 2.0          # latch beam thickness, flexing in the layer plane
BEAM_H = 8.0          # its height
UNDERCUT = 1.0        # tooth depth = deflection on snapping
TOOTH_FLAT = 2.0      # tooth's vertical catch length
LEAD_DEG = 30.0       # lead-in face from the insertion direction
SLOT_W = 4.0          # gap behind each beam: room to flex, a drain, a release
ROOT = 3.0            # floor kept at the beam's root end
POST_D = 6.0          # posts that hold the case up against the top stops
# Strain allowed at the beam root. Bayer's snap-fit guide: an amorphous
# plastic to about 70 % of its yield strain for one snap, 60 % of that for
# repeated ones; Prusament ASA yields at 3.4 %, so 1.4 %. Half of that for
# brittleness at -25 C (notched impact falls 40 -> 9 kJ/m2 at -30 C).
STRAIN_LIMIT = 0.5 * 0.6 * 0.7 * 0.034

# --- Mounting ---------------------------------------------------------------------
PAD_T = 8.0           # back pad: bands pass through it, behind the body
EAR_W = 14.0          # ears beyond the body for wall screws
SCREW_D = 4.5         # M4 or 4 mm stainless wood screw
SCREW_HEAD_D = 8.5
BAND_W, BAND_T = 14.0, 3.5  # tunnels for 12 mm stainless hose clamps

OUT = Path(__file__).resolve().parent / "out"


def box(x0, x1, y0, y1, z0, z1):
    return cq.Workplane("XY").box(x1 - x0, y1 - y0, z1 - z0, centered=False).translate((x0, y0, z0))


def rbox(x0, x1, y0, y1, z0, z1, r):
    return box(x0, x1, y0, y1, z0, z1).edges("|Z").fillet(r)


def prism_x(x0, x1, pts_yz):
    """A prism along x from a polygon in the (y, z) plane."""
    return cq.Workplane("YZ").workplane(offset=x0).polyline(pts_yz).close().extrude(x1 - x0)


# --- Derived layout ---------------------------------------------------------------
ANT_X = SMA_FACE_X - ADAPTER_REACH                 # antenna axis
ANT_BASE_Z = SMA_Z + ADAPTER_RISE
ANT_TOP_Z = ANT_BASE_Z + ANT_LEN
ANT_R = ANT_D / 2 + ANT_GAP

# The cavity has one footprint from the lid up to the case, so the case,
# adapter and antenna slide in from below as one piece.
CX0 = ANT_X - ANT_R
CX1 = MAIN_X[1] + CLEAR
CY0 = min(CASE_Y[0] - CLEAR, SMA_Y - ANT_R)
CY1 = max(CASE_Y[1] + CLEAR, SMA_Y + ANT_R)
CEIL_Z = BUTTON_TOP_Z + BUTTON_GAP                 # over the screen part
TOWER_CEIL_Z = ANT_TOP_Z + ANT_GAP                 # over the antenna
SHOULDER_Z = LOGO_TOP_Z + STOP_GAP                 # solid above the logo part
FLOOR_Z = CASE_Z[0] - PLUG_LEN                     # lid floor top at its front
LID_BOT_Z = FLOOR_Z - LID_T
BODY_BOT_Z = LID_BOT_Z - DRIP

OX0, OX1 = CX0 - WALL, CX1 + WALL
OY0, OY1 = CY0 - WALL, CY1 + WALL
TOWER_X1 = LOGO_X[0] - CLEAR + WALL

# Lid footprint and latches.
LX0, LX1 = CX0 + LID_CLEAR, CX1 - LID_CLEAR
LY0, LY1 = CY0 + LID_CLEAR, CY1 - LID_CLEAR
TOOTH_Z = LID_BOT_Z + BEAM_H - TOOTH_FLAT - UNDERCUT / math.tan(math.radians(LEAD_DEG)) - 0.5
# (root x, free-end x, side); the front right one stops short of the cable slot.
BEAMS = (
    (LX0 + ROOT, LX0 + ROOT + 25.0, +1),
    (-18.0, 4.0, +1),
    (LX0 + ROOT, LX0 + ROOT + 25.0, -1),
    (LX1 - ROOT, LX1 - ROOT - 25.0, -1),
)
POSTS = ((-32.0, (CASE_Y[0] + CASE_Y[1]) / 2), (22.0, (CASE_Y[0] + CASE_Y[1]) / 2))


def beam_strain(length):
    # Cantilever of constant section, Bayer guide Table 1: e = 1.5 h y / l^2.
    return 1.5 * BEAM_T * UNDERCUT / length ** 2


def tooth_profile(side):
    """Tooth on a beam's outer face, in (y, z); side +1 front, -1 back."""
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
    top = CEIL_Z + WALL
    outer = rbox(OX0, OX1, OY0, OY1, BODY_BOT_Z, top, EDGE_R)
    outer = outer.union(rbox(OX0, TOWER_X1, OY0, OY1, BODY_BOT_Z, TOWER_CEIL_Z + WALL, EDGE_R))

    cavity = box(CX0, CX1, CY0, CY1, BODY_BOT_Z - 1, CEIL_Z)
    cavity = cavity.union(box(CX0, LOGO_X[0] - CLEAR, CY0, CY1, BODY_BOT_Z - 1, TOWER_CEIL_Z))
    body = outer.cut(cavity)
    # Shoulder over the logo part: locates the case in x and from above.
    body = body.union(box(LOGO_X[0] - CLEAR, MAIN_X[0] - CLEAR, CY0, CY1, SHOULDER_Z, CEIL_Z))

    # Top stops clear of the two buttons.
    for x in (-21.0, 0.0, 22.0):
        body = body.union(box(x - 1.5, x + 1.5, CY0, CY1, CASE_Z[1] + STOP_GAP, CEIL_Z))

    # Crush ribs: vertical, chamfered at the bottom so the case slides in.
    def rib(x, y_wall, y_case, side):
        reach = abs(y_wall - y_case) + RIB_BITE
        z0, z1 = CASE_Z[0] - 3.0, CASE_Z[1]
        y_tip = y_wall - side * reach
        pts = [(y_wall + side * 0.01, z0), (y_wall + side * 0.01, z1), (y_tip, z1), (y_tip, z0 + reach * 2)]
        return prism_x(x - 0.8, x + 0.8, pts)

    for x in (-15.0, 0.0, 15.0) if ribs else ():
        body = body.union(rib(x, CY1, CASE_Y[1], +1))
        body = body.union(rib(x, CY0, CASE_Y[0], -1))
    if ribs:
        body = body.union(rib(-32.0, CY0, CASE_Y[0], -1))
        body = body.union(rib(-32.0, CY1, LOGO_FRONT_Y, +1).intersect(box(-40, -24, CY0, CY1, -40, LOGO_TOP_Z)))
    for y in (11.0, 24.0) if ribs else ():
        reach = CX1 - MAIN_X[1] + RIB_BITE
        body = body.union(box(CX1 - reach, CX1 + 0.01, y - 0.8, y + 0.8, CASE_Z[0], CASE_Z[1]))

    # Latch grooves. The front wall is thickened outside each so it stays WALL
    # thick; at the back the pad does that.
    for x_root, x_free, side in BEAMS:
        tx0, tx1 = (x_free - 6.0, x_free - 1.0) if x_free > x_root else (x_free + 1.0, x_free + 6.0)
        if side > 0:
            body = body.union(box(tx0 - 2, tx1 + 2, OY1 - 0.01, OY1 + UNDERCUT + 0.3,
                                  TOOTH_Z - 2, TOOTH_Z + TOOTH_FLAT + 4))
        body = body.cut(prism_x(tx0 - 1.0, tx1 + 1.0, groove_profile(side)))

    # Back pad: wall ears with screw holes, tunnels for bands round a pole or a rail.
    # (The XZ workplane faces -y, so its offsets are minus y.)
    py0, py1 = OY0 - PAD_T, OY0 + 0.01
    pz0, pz1 = BODY_BOT_Z, CEIL_Z + WALL
    pad = rbox(OX0 - EAR_W, OX1 + EAR_W, py0, py1, pz0, pz1, 2.0)
    mid_y = (py0 + OY0) / 2
    h = pz1 - pz0
    for zc in (pz0 + h * 0.25, pz0 + h * 0.75):
        pad = pad.cut(box(OX0 - EAR_W - 1, OX1 + EAR_W + 1, mid_y - BAND_T / 2, mid_y + BAND_T / 2,
                          zc - BAND_W / 2, zc + BAND_W / 2))
    w = OX1 - OX0
    for xc in (OX0 + w * 0.25, OX0 + w * 0.75):
        pad = pad.cut(box(xc - BAND_W / 2, xc + BAND_W / 2, mid_y - BAND_T / 2, mid_y + BAND_T / 2,
                          pz0 - 1, pz1 + 1))
    for xc in (OX0 - EAR_W / 2, OX1 + EAR_W / 2):
        for zc in (pz0 + 8.0, pz1 - 8.0):
            hole = cq.Workplane("XZ").workplane(offset=-py1 - 1).center(xc, zc).circle(SCREW_D / 2).extrude(PAD_T + 2)
            head = cq.Workplane("XZ").workplane(offset=-py1 - 0.01).center(xc, zc).circle(SCREW_HEAD_D / 2).extrude(3.0)
            pad = pad.cut(hole).cut(head)
    return body.union(pad)


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
        end_pad = 1.0 if x_free > x_root else -1.0
        sx = sorted((x_root, x_free + end_pad))
        lid = lid.cut(box(sx[0], sx[1], strip[0], strip[1], LID_BOT_Z - 1, FLOOR_Z + LID_FALL + 1))
        lid = lid.union(box(xa, xb, beam_y[0], beam_y[1], LID_BOT_Z, LID_BOT_Z + BEAM_H))
        # A full-height block at the root, so the beam bends along its length
        # and not by twisting the thin floor it stands on.
        rx = sorted((x_root, x_root - ROOT * (1 if x_free > x_root else -1)))
        lid = lid.union(box(rx[0], rx[1], strip[0], strip[1], LID_BOT_Z, LID_BOT_Z + BEAM_H))
        tx0, tx1 = (x_free - 6.0, x_free - 1.0) if x_free > x_root else (x_free + 1.0, x_free + 6.0)
        lid = lid.union(prism_x(tx0, tx1, tooth_profile(side)))

    # Cable slot, open to the front edge: the cable is slid in from the side.
    w = CABLE_D - 0.3  # a slight pinch holds the cable
    lid = lid.cut(box(USB_X - w / 2, USB_X + w / 2, USB_Y, LY1 + 1, LID_BOT_Z - 1, FLOOR_Z + LID_FALL + 1))
    lid = lid.cut(cq.Workplane("XY").center(USB_X, USB_Y).circle(w / 2).extrude(20).translate((0, 0, LID_BOT_Z - 5)))

    # Two more drains at the front, away from the latches.
    for x in (-30.0, 18.0):
        lid = lid.cut(cq.Workplane("XY").center(x, LY1 - 3.0).circle(2.0).extrude(20).translate((0, 0, LID_BOT_Z - 5)))

    # Posts hold the case up against the top stops once the lid is in.
    for x, y in POSTS:
        top = CASE_Z[0] - STOP_GAP
        lid = lid.union(cq.Workplane("XY").center(x, y).circle(POST_D / 2).extrude(top - FLOOR_Z + 1).translate((0, 0, FLOOR_Z - 1)))
    return lid


def make_techo(cable=True):
    """The cased T-Echo with adapter, antenna and plug, as blocks: for the fit check and the views."""
    t = box(MAIN_X[0], MAIN_X[1], CASE_Y[0], CASE_Y[1], CASE_Z[0], CASE_Z[1])
    t = t.union(box(LOGO_X[0], LOGO_X[1] + 0.5, CASE_Y[0], LOGO_FRONT_Y, CASE_Z[0], LOGO_TOP_Z))
    for x0, x1 in BUTTONS_X:
        t = t.union(box(x0, x1, 20.25, 24.66, CASE_Z[1] - 0.1, BUTTON_TOP_Z))
    t = t.union(cq.Workplane("YZ").workplane(offset=SMA_FACE_X).center(SMA_Y, SMA_Z).circle(8.05).extrude(LOGO_X[0] - SMA_FACE_X + 0.1))
    t = t.union(cq.Workplane("YZ").workplane(offset=ANT_X).center(SMA_Y, SMA_Z).circle(4.5).extrude(SMA_FACE_X - ANT_X + 0.1))
    t = t.union(cq.Workplane("XY").center(ANT_X, SMA_Y).circle(4.5).extrude(ANT_BASE_Z - SMA_Z + 4.5).translate((0, 0, SMA_Z - 4.5)))
    t = t.union(cq.Workplane("XY").center(ANT_X, SMA_Y).circle(ANT_D / 2).workplane(offset=ANT_LEN).circle(ANT_D / 2 - 3).loft().translate((0, 0, ANT_BASE_Z)))
    t = t.union(box(USB_X - 6.25, USB_X + 6.25, USB_Y - 3.5, USB_Y + 3.5, CASE_Z[0] - PLUG_LEN + 2, CASE_Z[0] + 0.1))
    if cable:
        t = t.union(cq.Workplane("XY").center(USB_X, USB_Y).circle(CABLE_D / 2).extrude(PLUG_LEN + 10).translate((0, 0, BODY_BOT_Z - 10)))
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
    # Nothing but the crush ribs may reach into the case, adapter, antenna or
    # plug, and nothing but the slot's pinch into the cable.
    for name, part in (("body without ribs", bare), ("lid", lid)):
        v = volume(part.intersect(make_techo(cable=False)))
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
          f"({TOWER_CEIL_Z + WALL - BODY_BOT_Z:.1f} mm tall), pad {OX1 - OX0 + 2 * EAR_W:.1f} mm wide")
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
    cut = box(OX0 - 30, OX1 + 30, OY0 - 30, SMA_Y, BODY_BOT_Z - 5, TOWER_CEIL_Z + 10)
    png([(body.cut(cut), shell), (lid.cut(cut), cover), (techo.cut(cut), board)],
        OUT / "view-section.png", (0, 1, 0), "Section through the antenna axis, seen from behind")
    png([(lid, cover)], OUT / "view-lid.png", (0.5, -1, -0.9), "Lid: latch beams, posts, drains")
    png([(body, shell)], OUT / "view-back.png", (-0.5, 1, -0.3), "Back: wall ears and band tunnels")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
