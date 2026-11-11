# Phase 3: V-Carve, Inlay, and Photo V-Carve — Functional Spec

**Status:** draft

## Goal

A hobbyist can V-carve a sign from SVG outlines, cut a two-board inlay that
fits, and carve a photograph as V-bit lines, all from the command line, for
any of the six controllers. V-Carve drives a V-bit along the medial lines of
closed shapes so the cut width follows the shape; a flat depth gives wide
areas a flat floor, which an end-mill Pocket can clear first. Inlay makes
the matched female pocket and male plug of that carve, mirrored and set at
the depths the technique needs. Photo V-Carve carves a grayscale image as
parallel V-bit lines whose depth follows the pixels. A Drill can countersink
with a V-bit. `dreamcarve image-prep` cleans an SVG for inlay work before
`init`, and `init` itself reports what in a drawing will trouble an inlay.
An Inlay can take a closed shape the user has drawn as the outer edge of
its male's band. Every program passes the phase 1 and 2 safety rules and
the new ones below.

Where VCarve Pro and PureCutCNC differ in how a feature is presented, this
spec follows VCarve's vocabulary and option set and PureCutCNC's algorithm,
as phase 2 did. Inlay has no PureCutCNC reference: its method and parameters
follow Corbin's Workshop's documentation of its Fusion add-in
([https://docs.corbinsworkshop.com/](https://docs.corbinsworkshop.com/)), with VCarve's names where the two
agree. Photo V-Carve has no PureCutCNC reference either: its behaviour and
test cases follow RasterCarve (`../rastercarve`, GPLv2+), whose code is not
read for porting and not copied; only its documented behaviour, its
command-line defaults and its outputs on DreamCarve's own fixtures are used,
as the ordering research did with FreeCAD. Sources are cited per capability.

## Users and scenarios

- **Sam** draws a sign in Inkscape: a rectangle, an oval inside it, and
the words "DreamCarve" set on a curve inside the oval, converted to
paths. `dreamcarve init sign.svg` writes a `.dcv` with one Profile. Sam
adds a V-Carve of the rectangle (the oval nests inside it, so it stands
as an island) with a 90° V-bit, a 2.5 mm flat depth and a 6.35 mm end
mill listed under `clearing`, so the end mill clears the flat background
first, in a program of its own, and the V-bit carves only the
slope round the oval and the corners the end mill cannot reach. Then a
second V-Carve, of the lettering alone with no flat depth, cuts the
letters **inset** into the raised oval: the bit follows each letter's
medial lines, so every serif and sharp corner of the lettering is cut as
drawn, to a point, which no end mill could do. `dreamcarve check
--preview sign-preview.svg` shows the floor the end mill clears, the
slope round the oval and the V-bit's lines running into every serif. Sam
exports, cuts, and the oval stands proud with a clean sloped edge and
the lettering reads crisply inside it.
- **Sam, later,** adds two hanging holes: a Drill with a 3.175 mm end mill,
then a second Drill on the same circles with the V-bit and
`countersink: { "diameter_mm": 6 }`. The V-bit plunges exactly deep
enough to open a 6 mm mouth.
- **Alex** makes a cutting-board inlay as in Corbin's Workshop's Fusion
tutorial: a two-letter monogram, walnut into maple. They make one
project from the SVG with one Inlay operation, a 60° V-bit,
`depth_mm: 4`, `glue_gap_mm: 1`, `plug_depth_mm: 2`, the maple as the
stock and the walnut as the Inlay's male board. `export` writes the
female board's programs and the male board's, and says the male's
design is mirrored. Alex cuts both, test fits, glues, clamps, saws off
the plug's backing and planes it flush. The
top shows no gap; a test piece cut in half shows the 1 mm glue gap under
the plug.
- **Alex, after a test fit** that bottoms out before the top closes,
raises `glue_gap_mm` to 1.5 and re-exports only the male board
(`export --board male`), as Corbin's fit procedure says: change one
number, cut again.
- **Maya** has line art of an eagle, a trace drawn as strokes. `init`
imports the strokes as bare centrelines, which no Inlay can use, and
`init` says so, and says that `dreamcarve image-prep` would fix it. At
50 mm tall the ink is 0.35 mm wide at the median; at 200 mm, 1.41 mm, and
four fifths of it is still narrower than 1.5 mm. She runs `image-prep
eagle.svg --regions`: the spaces between the lines become the shapes to
inlay, the lines stay as the board between the pockets, and the report
says the narrowest ridge between two pockets is 0.56 % of the design's
height, 1.7 mm at 300 mm tall. She scales the result, runs `init`, adds an
Inlay and draws an oval round the eagle as its `boundary`, so the male
board's plug band is the oval she drew. She cuts the pair, and the eagle's
feathers show as dark wood in light, separated by thin ridges of the
light board. (The eagle is the maintainer's own art. It motivates this
section and is never committed; the tests use synthetic drawings.)
- **Jo** carves a photo of their dog. `dreamcarve init dog.jpg --width 100`
writes a project holding the picture, placed at the origin 100 mm wide
with its height from the picture's proportions, and one Photo V-Carve
operation. Jo sets a 30° V-bit, a 1.5 mm maximum depth and a 22.5° line
angle, exports, and gets a program of parallel grooves, deeper where the
photo is dark. On a dark wood they set `invert: true` so the light parts
of the picture are cut instead.
- **Jo, later,** runs `dreamcarve image add plaque.dcv dog.png --width 60 --at 20,30` to put the picture on the phase 2 plaque and carves it beside
the pocket, in the same project.

## Capabilities

Numbering continues from phase 2's capability 37, which still holds with
phase 1's nine. Phase 4's capabilities continue from 49 (its spec was
written first), so the sub-phase 3.4 capabilities and acceptance tests
below carry letters: 49a to 49g, 61a to 61s, as capability 13a and 50a
do. Vocabulary is `CONTEXT.md`'s; this phase adds **Flat
Depth**, **Start Depth**, **Clearing**, **Inlay**, **Glue Gap**,
**Plug Depth**, **Photo V-Carve**, **Bitmap**, **Countersink**, **Image
Prep**, **Ink**, **Piece** and **Boundary**, defined
in the capabilities that introduce them; `CONTEXT.md` gains them before the
tag (exit criteria). "The user can" means by editing the `.dcv` or running the CLI; there
is no GUI in this phase.

### V-Carve

38. **V-Carve** (`v_carve`; VCarve "V-Carve / Engraving Toolpath",
 [https://docs.vectric.com/docs/V12.0/VCarvePro/ENU/Help/form/VCarve%20Toolpath%20Creator](https://docs.vectric.com/docs/V12.0/VCarvePro/ENU/Help/form/VCarve%20Toolpath%20Creator);
 PureCutCNC `v_carve_medial`). An operation that drives a V-bit along
 the **medial lines** of each closed target, the lines equidistant from
 the shape's two nearest walls, at the depth where the bit's two flanks
 touch both walls: for a bit of included angle `α`, a point whose walls
 are `w` apart is cut `w / (2 · tan(α/2))` deep. A 60° bit carves a
 5 mm stroke 4.330 mm deep; a 90° bit carves it 2.5 mm deep. So the cut
 width follows the shape's width, the bit rises to the surface at every
 sharp convex corner, and a serif ends in a point. Outlines nested
 inside a target are islands and outlines inside those are carved
 again (even-odd, as Pocket nests): the letter O carves the ring, and a
 rectangle with letters inside it carves the background and leaves the
 letters standing. Targets are closed outlines; an open path is skipped
 with `v_carve_target_open`; overlapping outlines are an error, as for
 Pocket. The tool must be a V-bit (`cutter: { "v_bit": { … } }`) with an
 included angle above 0° and below 180°; any other cutter is the error
 `v_carve_needs_v_bit`. The bit is an ideal point: no tip flat is
 modelled (see Out of scope). Within an operation, shapes are visited
 nearest-neighbour from the first in drawing order, and a shape is cut
 before any shape enclosing it, as capability 11 says; `machining_order`
 applies as to Pocket. Between the lines of one shape the bit stays
 down where the link runs through material the carve has already
 removed, and retracts to safe Z otherwise (PureCutCNC's chain links).
39. **Flat depth** (`flat_depth_mm`; VCarve "Flat Depth (F)", "flat
 bottomed carving"; `null` for none). Where a shape is wider than
 `2 · F · tan(α/2)` the bit would go deeper than `F`; instead the carve
 stops at `F`: the bit runs along the **floor boundary**, the shape
 inset by `F · tan(α/2)`, at depth `F`, so the walls are V-slopes from
 the outline down to that boundary, and the floor inside it is flat at
 `F`. Narrower parts of the same shape, and the corners, are carved as
 in 38. The floor is cleared by the carve's **clearing** tools (40)
 where the user lists any, and what they cannot reach (a band one
 end-mill radius wide along the floor boundary, and the floor's
 corners), or the whole floor when there is none, is cleared by the
 V-bit itself at depth `F` with the tool's step-over as a fraction of
 its cut width at `F` (`step_over_fraction`, from the tool by default;
 VCarve: "If no clearance tool is set and Flat Depth is on, the
 V-carving tool clears flat areas"). A V-Carve with no flat depth whose
 deepest point would be deeper than the bit's cone height, the depth at
 which the flanks reach the tool's full diameter
 (`diameter_mm / (2 · tan(α/2))`), is the error `v_bit_cone_exceeded`
 naming the shape and the depth, because below that the shank cuts;
 with a flat depth the same check applies to `F`. PureCutCNC's
 `maxCarveDepth` clamps depth the same way but has no clearing; the
 clearing follows VCarve's "Use Clearance Tools" and Corbin's pocket
 setup, where the end mill's radial stock to leave is
 `tan(α/2) · depth` ([https://docs.corbinsworkshop.com/pocket-setup-2d.md](https://docs.corbinsworkshop.com/pocket-setup-2d.md)).
40. **Clearing** (`clearing` on V-Carve and Inlay: a list of **clearing
 passes**; VCarve "Use Clearance Tools", Corbin "Clearance" toolpaths).
 Each pass names a flat end mill and three settings, `strategy`
 (`offset`, or `raster` with its angle), `direction` and `entry`, as a
 Pocket's; step-down and step-over come from the tool. The carve
 generates one Pocket-like run per pass before its own V-bit run, in
 list order, each clearing the carve's floor region (the carved area
 inset by `F · tan(α/2)`, at the start depth plus `F`) and each later
 pass only what the earlier passes' tools could not reach, worked out
 from their tools as rest machining is (capability 13, S14), so a
 6.35 mm then a 3.175 mm end mill leave the V-bit only the corners.
 Export writes each pass as a run of its own tool, so a carve with two
 clearing tools is three tool runs, named `op2 (clearing t1)`, `op2
 (clearing t2)` and `op2` in programs, summaries and the preview. A
 carve with no flat depth may not have clearing passes (error
 `clearing_needs_flat_depth`); a pass whose tool is not a flat end
 mill is `clearing_needs_end_mill`; a pass whose tool is wider than the
 one before warns `clearing_not_descending` (it is not refused, but a
 wider end mill reaches nothing a narrower one before it left, so it also
 warns `clearing_no_room`; #406); a pass that can reach nothing warns
 `clearing_no_room` and has no run of its own.
 An empty list is the default: the V-bit clears the whole floor itself.
 The carve never trusts a clearing run's toolpath: it subtracts what
 each pass's tool can reach, so removing a pass simply gives the V-bit
 more floor. (changed 2026-10-28, the maintainer's decision: the draft
 had a separate Pocket operation naming the carve with `clear_for`,
 which cost the user fifteen lines of Pocket JSON per tool and five
 validation rules; one operation with several tool runs is what VCarve
 and Corbin's add-in present, and export already groups programs by
 tool.)
41. **Start depth** (`start_depth_mm`, default 0; VCarve "Start Depth
 (D)": "when cutting directly into the surface of a job the Start Depth
 will usually be 0.0"; for a pocket floor, enter the pocket's depth).
 Every depth of the carve is measured from the **start plane**, that far
 below the stock top: the outline is where the bit's flanks meet the
 start plane, and the flat depth counts from it. The user cuts the
 pocket first with a Pocket of the same depth. `check` warns
 `start_depth_mismatch`, naming the Pocket, when an enabled earlier
 Pocket's region contains the targets but its depth is not the start
 depth: the mistake is visible and the cut is in material the project
 knows. When no such Pocket exists and the start depth is above 0,
 export is refused with `start_depth_uncleared` unless the operation
 says `start_plane_cleared_externally: true`, the user's statement that
 another program cut the pocket; then the bit's **flank engagement**
 counts from the stock top, not the start plane, so the cone-height
 check (39) refuses a start depth the bit cannot reach without its
 shank, whatever the carve's own depth (changed after the security
 review of 2026-10-28, decided by the maintainer: the draft warned and
 exported a cut whose material the project could not see, and counted
 the cone from the start plane). The bit rapids down to the entry
 clearance above the stock top and plunges from there at the plunge
 rate (S2 unchanged). On V-Carve, Photo V-Carve and a countersink (44)
 in this phase; an Inlay has no start depth setting (the male's start
 plane is the plug's own design, 45).
42. **One pass.** A carve is cut in one pass whatever its depth, as
 PureCutCNC cuts; VCarve's Z-level passes ("Multiple Depths" in
 Corbin's add-in) are out of scope and may come later. (decided
 2026-10-26: the draft had `step_down_mm` passes, each the carve capped
 at `k · step_down_mm`; the maintainer chose single pass.)
43. **Offset strategy** (`strategy: "medial"` default, or `"offset"`;
 PureCutCNC `v_carve`, "V-Carve offset"). With `offset` the carve is
 cut as concentric rings stepping in from the outline by `step_over_mm`
 (a distance, as PureCutCNC's `stepover`), each ring at the depth where
 the bit's flank touches the outline, `inset / tan(α/2)`, until the
 rings vanish or reach the flat depth; `direction` (climb or
 conventional) sets the way round each ring. The result is the same
 V-slope cut in lighter passes, for hard material or a deep carve. Its
 corners are rounded by the step-over, not run to a point; the chapter
 says so. The medial strategy has no `direction` and ignores
 `step_over_mm`.

### Drill

44. **Countersink** (`cycle: { "countersink": { "diameter_mm": …,
 "start_depth_mm": … } }` on Drill; PureCutCNC drill type
 `countersink`, issue #489, which measures from each target's own top
 face; VCarve has no countersink cycle, Fusion's "Countersink" uses a
 countersink tool). The tool must be a V-bit (error
 `countersink_needs_v_bit`). The **hole's top** is the start plane,
 `start_depth_mm` below the stock top (default 0; the pocket's depth for
 a hole in a pocket floor, with the `start_depth_mismatch` check of 41).
 At each circular target the bit plunges at the centre, at the plunge
 rate, to the depth below the hole's top that opens a mouth of
 `diameter_mm` there, `diameter_mm / (2 · tan(α/2))`: a 90° bit opens
 6 mm at 3 mm; an 82° bit at 3.451 mm. The mouth may not be wider than the bit's diameter
 (error `countersink_mouth_exceeds_tool`), and a target circle at least
 as wide as the mouth is skipped with
 `countersink_mouth_not_wider_than_hole`, since there is nothing to
 cut. Drill's rule that skips a hole narrower than its tool does not
 apply: a countersink's bit is wider than its hole by design (changed
 in spec sync 1). Always plain moves, never a canned cycle, even with
 `use_canned_cycles` on: the motion is a simple plunge, but the depth is
 derived, and a controller's cycle cannot say so. `dwell_s` applies as
 for `simple`; `retract_mm` is measured from the stock top, not the
 hole's top, so the rapid down to it never ends in the stock (changed
 after the security review: measured from a start plane it did); there
 is no pecking (a countersink is one plunge). With a start depth, the
 flank engagement for the cone check is `start_depth_mm` plus the
 countersink's depth unless an earlier Pocket cleared the start plane
 (41). Usually
 a second Drill on the circles an earlier Drill made; the order is the
 user's, as always.

### Inlay

45. **Inlay** (`inlay`; VCarve's inlay method with its "V-Carve Inlay"
 vocabulary, Corbin's Workshop add-in for the method and the parameters:
 [https://docs.corbinsworkshop.com/terminology.md](https://docs.corbinsworkshop.com/terminology.md),
 [https://docs.corbinsworkshop.com/vbit\_inlay\_feature\_tab.md](https://docs.corbinsworkshop.com/vbit_inlay_feature_tab.md),
 [https://docs.corbinsworkshop.com/plug-setup-2d.md](https://docs.corbinsworkshop.com/plug-setup-2d.md),
 [https://docs.corbinsworkshop.com/inlay\_fit\_adjustment.md](https://docs.corbinsworkshop.com/inlay_fit_adjustment.md)). An
 operation that cuts both boards of a V-bit inlay of its targets from one
 project: the **female**, a pocket in the project's stock, and the
 **male**, a plug standing on the Inlay's **male board**, so the plug's
 V-sloped walls seat in the pocket's. One set of values describes both
 boards, so they cannot drift apart (changed 2026-11-02 in #498, the
 maintainer's decision from the phase 4 UI review, #460: until format
 25 an Inlay cut one `side`, and a pair was two project files kept in
 step by hand):
    - `male_board` (format 25, #498): the board the male is cut from,
    `{ "width_mm", "height_mm", "thickness_mm" }`. Width and height
    `null` are the stock's; the thickness is the male board's own, and
    until it is set the male is not cut: only the female exports, and
    the generator warns `inlay_male_board_unset`. A width or height set
    and not above 0 is `inlay_male_board_size`, a thickness set and not
    above 0 `inlay_male_board_thickness`. The project's stock is the
    female board, which can carry other operations (a Pocket, an
    outline Profile). The male board shares the stock's bottom-left
    corner, so the design is mirrored in place on it, and the project's
    origin preset applies to it as to the stock (bottom-left of that
    board, centre of that board; Z at that board's top or bottom). Every
    rule that reads the stock for the male reads the male board: its
    thickness, top, bottom and edges. Every Inlay of a project cuts its
    male on one male board: a later Inlay naming a different one is
    `inlay_male_boards_differ`.
    - `depth_mm` (Corbin "Inlay Depth" `inlay_depth`; VCarve's female
     "Flat Depth"): how deep the female pocket is. Corbin: about 2.5 mm
     for a superficial inlay, 6 mm for a cutting board.
    - `glue_gap_mm` (Corbin "Inlay Fit Adjustment" `inlay_fit_adjustment`,
    "the glue gap under the plug"): the gap left under the seated plug.
    The male's start depth is `depth_mm − glue_gap_mm` below the male
    board's top (VCarve's male "Start Depth"). 0 is a plug that bottoms exactly; must be below
    `depth_mm` (error `inlay_glue_gap_not_below_depth`). Corbin starts at
    0 or 0.25 mm and adjusts in 0.25 mm steps; the known-good values on
    the parameter list run from 0 to 2.3 mm depending on the bit.
    - `plug_depth_mm` (Corbin `plug_depth = plug_surface_gap + inlay_fit_adjustment`; VCarve's male "Flat Depth"): how far below the
    male's start plane the plug's surround is cut flat, so the plug stands
    that much above its base when the walls are mated and the base clears
    the female's surface. Corbin uses a surface gap of 1.3 to 1.8 mm plus
    the fit adjustment.
    - `margin_mm` (Corbin "Plug Stock Offset", 5 to 7.5 mm for clamping;
    VCarve's drawn boundary): how far beyond the design's outer outlines
    the male's surround is carved. Used by the male, and required once
    it is cut; the female ignores it.
    - `outline_rounding_mm` (format 24, added 2026-10-31 in #448): `null`
    or a radius above 0 (`inlay_outline_rounding` otherwise) the male's
    band is rounded by, so its outer edge need not follow the plug's
    shape: the band is the design grown by `margin_mm` and **closed** by
    the radius (grown by `margin_mm` plus it, then shrunk by it), so its
    inward curves are no tighter than the radius and notches and gaps
    narrower than about twice it fill. Used by the male; the female
    ignores it.
    A user-drawn boundary, as VCarve's and Corbin's, is capability 49f
    (sub-phase 3.4, format 26).
     On the stock the female is a V-Carve of its targets with
     `flat_depth_mm: depth_mm` and start depth 0, move for move. On the
     male board the targets are **mirrored** left to right in place,
     about the vertical centre line of their joint bounding box, because
     the plug is turned over onto the pocket; `export` and `check` say so
     once per Inlay that cuts its male (`inlay_male_mirrored`). The male then
     V-carves the **surround**: the mirrored outlines offset outward by
     `margin_mm` and rounded, minus the mirrored design, so the band round
     the design and every hole in the design (a circle inside a star) is
     carved and the design stands. The band's **outer edge** (changed in
     #448 after the maintainer's preview of a lettered male): when a
     clearing pass's end mill fits the band's flat floor (`margin_mm −
     plug_depth_mm · tan(α/2)` at least its diameter; the first that
     does, the edge pass) and the margin leaves room for the V-bit's flank
     at the plug's foot (`margin_mm ≥ (depth_mm − glue_gap_mm + 2 ·
     plug_depth_mm) · tan(α/2)`), the end mill clears the floor out to
     the edge and cuts it upright, the band rounded by at least the end
     mill's radius, and the V-bit carves only the walls against the design
     and the floor no end mill reaches, its cone never crossing the edge
     below the male board's top. Otherwise the V-bit cuts the edge as a V wall,
     and with clearing passes `inlay_band_edge_v_wall` names the margin
     that would open it. Only the walls against the design mate with the
     female; past the cut-out everything is waste. Where the band runs
     past the board's edge it is **clipped to the board** (#495,
     maintainer decision 2026-11-02: the band only gives the plug room,
     and nothing outside it matters), and the generator warns
     `inlay_band_clipped`, naming the edges and the narrowest band left.
     With an open edge the end mill clears the floor to the board's edge,
     its cutting edge on it and never past it, and the board's edge is
     the plug's rim there; a V wall is cut the V-bit's radius inside the
     board, since the stock-boundary rule (S7) holds a V-bit to its whole
     radius. A band that stays on the board is unchanged. The design's
     own cut is never clipped: where an edge clips the band, the design
     must stand far enough from it for the V-bit along the plug's foot
     (`plug_depth_mm · tan(α/2)` plus the V-bit's radius), the edge
     pass's floor ring and the V-bit's flank (open edge), and the
     cut-out's floor ring; else `check` refuses the male
     (`InlayTooNearEdge`), naming the edge, the room needed, how far to
     move the design and the board that fits it centred. Its start plane is `depth_mm − glue_gap_mm` below
     the male board's top and its flat depth `plug_depth_mm`, so the surround's
     floor is `depth_mm − glue_gap_mm + plug_depth_mm` deep. At the start
     plane the plug's walls pass exactly through the mirrored outline;
     above it the plug narrows by `z · tan(α/2)` at height `z`, which is
     the female wall's inset at depth `z`, so turned over, the plug's walls
     meet the pocket's walls along their whole height and the plug's top
     stops `glue_gap_mm` short of the floor. The tool is a V-bit, with the
     same checks as V-Carve, except that the cone-height check counts the
     flank's whole engagement from its board's top: `depth_mm` on the
     female, and `depth_mm − glue_gap_mm + plug_depth_mm` on the male,
     whose board above its start plane is the plug and stays (changed
     after the security review: the draft counted the male's
     `plug_depth_mm` alone, and with the bench's 90° bit, cone height
     3.175 mm, Corbin's usual 2.5, 0.25 and 1.5 mm engage 3.75 mm). Both
     boards are cut with the one V-bit, so their angles always agree.
     `safe_z_mm` and
     `machining_order` are as V-Carve's, and each board is one pass (42); both take
     the `clearing` passes (40). The male board must leave a base under the
     surround's floor: less than 6 mm warns `inlay_plug_base_thin` (Corbin:
     "Plug Base Thickness" at least 6 mm, the backing that holds the plug
     together until the glue dries; read from the male board's thickness).
     The male may **cut its plug out**
     (`cut_out`, format 23, added 2026-10-30 in #443; Corbin's optional
     "Plug Cutout", a contour toolpath "with or without tabs"): a flat end
     mill (`tool`; `cutter_not_flat` otherwise) runs On line round the
     mirrored design's outer outlines along the outer edge of the band's
     flat floor, which runs from the design grown by `plug_depth_mm ·
     tan(α/2)` out to the band's edge when the edge is open, or to the
     V wall's foot, `plug_depth_mm · tan(α/2)` inside the edge, with a V
     wall: its centre runs on the band's edge (rounded, #448) inset by
     the tool's radius, or by the foot plus the radius (changed in #461,
     2026-11-01, from the middle of the floor), so above the floor it
     cuts nothing the carve left, and with a V wall the freed piece has
     no knife edge the board's full height round it. Its
     passes start at the floor and go through the board to its own
     `through_cut_allowance_mm`, with `tabs` as a Profile's tab request
     (heights from the male board's bottom; `null` for none). It is the
     male's last tool run, after its V-bit's; the female ignores it. Design holes are not
     cut out. Where an open band is clipped to the board (#495), the
     stretches of that path the radius inside the board's edge are
     dropped, as the board's own edge frees the plug there: the rest are
     open paths from the board's edge to the board's edge, their ends the
     radius inside it, so the kerf reaches the edge and the tool never
     leaves the board, and the tabs go only on them. A floor narrower than the tool is `inlay_cut_out_no_room`,
     naming the margin needed. Freed, the piece is the plug on its
     backing with a rim of floor round it, `plug_depth_mm` below the start
     plane, which clears the female's surface. With `cut_out: null` the
     plug is sawn free inside the band. Cutting the backing off after
     gluing stays the user's: a saw.
     **Parts that nearly touch** (added 2026-11-02 in #479, the
     maintainer's decision): two parts of the design closer than 0.05 mm
     are cut as one shape on both boards, so the female cuts one pocket with no
     knife-edge wall between them and the male one plug with no groove,
     and `inlay_parts_merged` names the two shapes, the gap and where it
     is. A V-bit cannot carve such a gap: the female left the wall to
     the stock top, the male no groove, and the plug stood up to
     `depth_mm − glue_gap_mm` proud on it. Plain V-Carve and Pocket cut
     their parts as drawn. One outline that comes that near itself across
     the outside is merged across the gap the same way (#515). A hole of
     the design narrower than 0.05 mm all
     along is filled on both boards the same way, and `inlay_hole_filled`
     names it, its width and where it is (#511).
     **Programs** (#498): `export` writes the stock's programs, then the
     male board's, each board split by tool runs as any project's, named
     `<stem>-female-<k>-T<n>.<ext>` and `<stem>-male-<k>-T<n>.<ext>`
     (`k` counting from 1 within each board; a project with no Inlay
     names its programs as before), and prints them grouped under
     **Female board** and **Male board**, each board's first program
     setting its own work zero. `export --board female|male` writes one
     board only (re-cutting the male after a fit change); it is not
     saved. The safety pass holds each program to its own board. A
     format 24 project migrates: a female side gets an unset male board
     and exports what it did; a male side gets a male board the size of
     its stock, exports its male programs byte for byte under the new
     names, and now also cuts the female on the stock, which `check` and
     `export` say in a note.

### Photo V-Carve

46. **Bitmap** (a picture placed on the stock; `init` and `image add`).
 `dreamcarve init photo.png --width <mm>` (or `--height <mm>`; exactly
 one, the other follows the picture's proportions, as RasterCarve's
 `--width`/`--height` do) writes a project holding the picture as a
 **bitmap** placed with its bottom-left corner at the origin, and one
 Photo V-Carve operation (47) on it, where `init part.svg` writes one
 Profile. `dreamcarve image add <dcv> <file> --width|--height <mm>  [--at <x>,<y>]` puts a picture into an existing project at a position
 in project millimetres (default the origin); `image list <dcv>` prints
 each bitmap's id, name, pixel size, placement and footprint. PNG and
 JPEG are read; a colour picture is converted to grayscale by luminance
 and `image add` says so; transparency counts as white. The picture
 appears on the stock as it appears on screen: its top row at the top,
 highest Y. A bitmap is an instance in the project like a shape, with a
 name, a position and a size, and it keeps `visible` and `locked` for
 the GUI; it cannot be rotated in this phase. The project **embeds**
 the picture's pixels, as it embeds its tools (ADR-0002), so a `.dcv`
 stays one file that exports anywhere; the chapter says the file grows
 by the picture's size and that 4 to 8 pixels per millimetre of
 footprint is plenty, since the carve samples every `sample_mm`.
47. **Photo V-Carve** (`photo_v_carve`; VCarve "Photo V-Carve Toolpath",
 [https://docs.vectric.com/docs/V12.0/VCarvePro/ENU/Help/form/PhotoVCarveStrategy/index.html](https://docs.vectric.com/docs/V12.0/VCarvePro/ENU/Help/form/PhotoVCarveStrategy/index.html);
 RasterCarve's engraving parameters, `rastercarve --help`). An
 operation on one bitmap (`image: "<instance id>"`) with a V-bit that
 carves the picture as parallel grooves whose depth follows the
 pixels, as Vectric's PhotoVCarve does:
    - `max_depth_mm` (VCarve "Max Carving Depth", RasterCarve
     `MAX_DEPTH`): the depth of a black pixel, at most the bit's cone
     height (`v_bit_cone_exceeded`). White is the surface
     (the bit skims at the start plane); a grey pixel is carved in
     proportion to its darkness, within the sample tolerance below.
     `invert: true` swaps black and white, for a dark wood whose cuts
     show light. Dark is deep by default because on a light wood the
     groove reads dark, as RasterCarve inverts its image and
     PhotoVCarve carves dark areas deeper.
    - `line_angle_deg` (VCarve "Line Angle": 0 horizontal, 90 vertical;
    RasterCarve `LINE_ANGLE`, default 22.5): the grooves' direction,
    0 to 180 exclusive, counterclockwise from the X axis.
    - `line_spacing_fraction` (VCarve "Line Spacing", a percentage of the
    tool width at maximum carving depth; RasterCarve `STEPOVER`, default
    110 %): the spacing between grooves as a fraction of the groove's
    width at `max_depth_mm`, `2 · max_depth_mm · tan(α/2)`. 1.0 leaves a
    knife-edge ridge between full-depth grooves; more leaves a flat
    ridge; less than 1.0 overlaps and warns `photo_lines_overlap`.
    - `strategy: "raster"` (default) or `"hatch"` (VCarve "Raster",
    "Hatch"; RasterCarve `carve_strategy`): hatch adds a second set of
    grooves at right angles to the first, after it, for a denser picture
    at about twice the time.
    - `sample_mm` (RasterCarve `LINEAR_RESOLUTION`, default 0.01 in; here
    0.25 mm by default): the distance along a groove between the points
    where the depth is read, so each groove is straight moves between
    them. The depth at a point is the picture's brightness there,
    interpolated between pixels.
    - `start_depth_mm` as 41, `safe_z_mm`, and `stepover_retract_mm`
    (VCarve "Stepover Retract", default 1 mm): between one groove and
    the next the bit lifts this far above the start plane and moves
    over at the feed rate, not at a rapid, which S2 confines to safe Z.
     The grooves are spaced `line_spacing_fraction` times the full-depth
     width apart, measured perpendicular to them, with one groove through
     the picture's top-left corner (its origin, where RasterCarve lays
     its lines from; changed in the design spec from the bottom-left so
     the goldens compare groove for groove), clipped to the picture's
     footprint, cut alternately in opposite directions, so nothing is cut
     outside the picture and nothing inside it is missed. Each groove
     starts with a
     plunge to its first sample's depth and ends with the lift. The
     picture's area, groove count and the full-depth width are printed by
     `check`. Arc fitting (capability 24) does not apply to grooves: their
     points are not on arcs, and the chapter says so.

### Output

48. **Warnings and errors.** Every new code above is a stable snake\_case
 code with named parameters, printed as capability 25 says, and listed
 in the Warnings chapter: `v_carve_target_open`, `v_carve_needs_v_bit`,
 `v_bit_cone_exceeded`, `clearing_needs_flat_depth`,
 `clearing_needs_end_mill`, `clearing_no_room`, `clearing_not_descending`,
 `countersink_needs_v_bit`, `countersink_mouth_exceeds_tool`,
 `countersink_mouth_not_wider_than_hole`,
 `inlay_glue_gap_not_below_depth`, `inlay_plug_base_thin`,
 `inlay_male_mirrored`, `inlay_band_clipped` (#495),
 `inlay_parts_merged` (#479), `inlay_hole_filled` (#511), `photo_lines_overlap`, `image_colour_converted`,
 `start_depth_mismatch`, `start_depth_uncleared`,
 `countersink_ignores_tool_width`. An error stops export
 and names the operation, shape, tool or field; a warning never does.
49. **Golden parity.** The V-Carve goldens are captured from PureCutCNC's
 `v_carve_medial` and `v_carve` on fixtures the design spec lists
 (rectangle, square, circle, ring, L, outline text, triangle; a square
 in offset mode), with the parity tool as in phases 1 and 2, and
 compared at the tolerance each fixture's README states, 0.02 mm unless
 the README says otherwise. The Photo V-Carve goldens are RasterCarve's
 own output, from its command line on DreamCarve's own fixture pictures,
 captured by the parity tool the same way; its G-code is Y-up like
 PureCutCNC's exported programs, so it is compared directly. Nothing
 from RasterCarve but those programs enters the repository.

### Image prep and the drawn boundary (sub-phase 3.4)

Preparing artwork is the hardest part of making an inlay (#462). A drawing
that looks right still trips the Inlay: letters overlap, a hole is a
separate path, line art is drawn as strokes, and hidden layers, specks and
hairlines ride along. This sub-phase adds checks that tell the user (49a),
an opt-in command that fixes it (49b to 49e), and the drawn boundary that
lets the user shape the male's band (49f). It changes no machine output
by itself: the Inlay cuts what it always cut from whatever the drawing
says. The sources are PureCutCNC's import (`src/import/svg.ts`, whose
paint-intent and content warnings the checks extend, and `classifier.ts`,
whose nesting and ambiguity rules are `init`'s already) and the public
documentation of VCarve's and Corbin's inlay workflows for the boundary.
PureCutCNC has no stroke-to-outline, speck or smoothing step: those are
designed here.

49a. **Import checks** (`init` and `check`; #462, slice 1; always on,
 changing nothing). Reading an SVG, `init` reports, one warning per kind,
 each saying what it found, where, and what to do:
    - `shapes_overlap`: outlines of different paths cross. It names the
    first pair by id and where they meet, and the count. (An outline that
    crosses itself is already refused, and overlapping shapes are already
    an error where an operation targets them; this tells the user at
    `init`, before an operation exists.)
    - `svg_hole_as_separate_path`: a filled closed path lies wholly
    inside another filled path. They are two shapes in the project, and
    the inner one is a hole only in an operation that targets both, as
    "Shapes, holes, and islands" says of every nested outline; a Pocket of
    the outer path alone would cut across it. The warning names both paths
    and says `image-prep` makes them one path with a hole. (An unfilled
    frame drawn round a design is not reported.)
    - `svg_stroke_only_path`: a path has a stroke and no fill. `init`
    imports its centreline, an open path no Inlay or V-Carve can cut; the
    warning gives the stroke's width and says that `image-prep` turns
    strokes into shapes.
    - `svg_hidden_object`: an object that is not drawn (`display:none`,
    `visibility:hidden`, zero opacity, or neither filled nor stroked).
    `init` imports geometry whatever its paint, so a hidden layer is
    imported and would be cut; the warning names it and says `image-prep`
    skips such objects.
    - `svg_object_not_convertible`, the existing text, image, clip-path
    and mask warnings with a fix added to each (convert text
    to paths, expand clips and masks, trace images in Inkscape first):
    live text, images, clip paths and masks cannot become shapes, and
    `image-prep` skips them too, with counts.
    - `feature_below_size`: pieces or holes whose widest point is under
    the default `--min-detail` (49d) of the design's height, as a count
    and the smallest. A speck a few hundredths of a millimetre across is
    never worth carving, and traced art is full of them.

 Each message that `image-prep` would fix names `dreamcarve image-prep`.
 `init` prints them as `warning: <code>: <message>`, as `check` and
 `export` print theirs, and its exit status and the project it writes are
 exactly what they were without the checks. `check` repeats the two that
 still apply to a project, `shapes_overlap` and `feature_below_size`,
 read from the project's shapes; the others need the drawing's paint and
 paths and are `init`'s alone. A clean drawing prints none of them.
49b. **`image-prep`** (`dreamcarve image-prep design.svg [-o
 design-inlay.svg] [--regions] [--min-detail <pct>] [--smooth <pct>]`;
 #462). Reads one SVG and writes one SVG that is easy to inlay, and
 prints a report (49e). The decisions that shape it, agreed with the
 maintainer on 2026-11-01:
    - **Opt-in.** Nothing requires it: `init` takes any SVG, and a clean
    CAD drawing needs none of it. Cleanup changes the art (it merges
    shapes, drops specks, converts line art), so it is the user's visible
    choice, and the generators must handle any valid design whether or
    not it was prepared.
    - **A clean SVG only.** The output takes no tool, stock, depth or
    final size, and the command has no option for any. Every threshold is
    a percentage of the **design's height**, the height of the visible
    geometry's bounding box, strokes included; the report prints that
    height. The user scales the result, in a drawing program or with the
    `init` margin they already use, and runs `init` as today.
    - **SVG in, SVG out.** PNG and JPEG are refused with a message that
    says to trace in Inkscape ("Trace Bitmap") first. Tracing a picture is
    deferred, and any future tracer must be permissively licensed (Potrace
    is GPL).
    - **Where the output goes.** `-o` names it; by default the file is
    the input's name with `-inlay` added, in the same folder. An existing
    file is never overwritten (as `init` never overwrites a `.dcv`).
    - **At the input's size.** The page keeps its physical size and the
    shapes their place on it, so the output lies over the input. It
    declares millimetres.
    - **The output** is filled shapes only: one path per piece with its
    holes. No strokes, text, images, clip paths, masks, gradients or
    hidden objects remain. Curves are written as straight segments within
    a tolerance of 0.005 % of the design's height (0.01 mm at 200 mm tall),
    so a circle in the input is a many-sided polygon in the output; the
    chapter says so.
49c. **What it does**, in this order (design spec, "Image prep"). A
 **piece** is one connected area of the result with its holes; **ink** is
 everything the drawing paints, after step 2.
    1. *Visible geometry only.* Hidden layers, hidden or unpainted
    objects, zero-opacity objects and guides are skipped and counted.
    Text, images, clip paths and masks cannot be converted and are
    reported as such, with the fix.
    2. *Strokes become areas.* A stroked path becomes the filled outline
    of its stroke, using the stroke's own width, caps, joins, miter limit
    and dashes, under the path's transform. (`init` imports such a path
    as a bare centreline.)
    3. *Fills are resolved.* Each path's own fill rule (nonzero or
    even-odd) decides what it fills; paths that cross are merged; a path
    wholly inside another path's area is a hole of it, the rule `init`
    applies to every nested outline, so a ring drawn as two paths comes
    out as one piece with a hole. What remains is disjoint pieces, each
    with its holes. Paint colour does not decide what is a shape.
    4. *The mode.* By default the filled shapes are the inlay: lettering
    and logos. With `--regions` the **enclosed spaces between the lines**
    are the inlay instead, which is the line-art case: the art's outline
    (the ink's outer boundary, its holes filled) minus the ink. The ink
    stays as the board between the pockets, and the existing Inlay cuts
    the result unchanged: one female pocket per space, and one male
    carrying all the plugs. An ink line that closes off no space leaves
    nothing to cut there, and the report says how many ink components do
    not (`prep_regions_open_ink`). The report suggests `--regions`
    (`prep_regions_suggested`) when the ink is mostly thin line work,
    its median width under 2 % of the design's height.
    5. *Clean up* (49d), relative to the design's height.
    6. *Write* the result (49b).
49d. **Thresholds** (`--min-detail <pct>`, `--smooth <pct>`; both a
 percentage of the design's height, from 0 to 5, 0 turning the step off,
 and a value outside the range an error, `prep_threshold_range`).
    - `--min-detail` (default **0.2**) drops a piece, and fills a hole,
    whose widest point, the diameter of the largest circle that fits in
    it, is under the threshold: a speck, or a hairline that is nowhere
    wider. It never thins or widens anything above it; narrow places in
    larger pieces are reported (49e), not changed.
    - `--smooth` (default **0.05**) replaces runs of tiny segments with a
    straight one where the result stays within the threshold of the
    original outline, which takes the staircase out of traced edges. It
    never rounds a corner: a corner between two long segments is
    kept, and the result never crosses itself or another piece (a piece
    where it would is left unsmoothed, `prep_smooth_skipped`). A
    `--smooth` above half of a non-zero `--min-detail` warns
    `prep_smooth_exceeds_detail`: smoothing that large could remove what
    the other step keeps.

 The defaults are proposals, chosen from synthetic fixtures and traced
 art as the design spec's calibration slice describes, and the maintainer
 confirms them in review (open questions 16 and 17).
49e. **The report** (printed by `image-prep`; informational, nothing in
 it is enforced). It states: what was read and skipped, with counts and
 reasons (hidden, unpainted, zero opacity, text, images, clip paths,
 masks); how many strokes became areas and their widths; the design's
 height; the number of pieces and holes; what was dropped, filled or
 smoothed and by what threshold in millimetres; **the thinnest piece**
 (its typical width, the median of its widths along its length, so a
 tapering tip does not count) and **the narrowest gap** between pieces,
 each in millimetres and as a share of the height, where in `--regions`
 mode the narrowest gap is the board ridge between two pockets; what
 those two widths become at 100, 200 and 300 mm tall; and the height at
 which each reaches 1.5 mm (below it the feature is narrower than that).
 Example, in
 `--regions` mode:

 ```text
 Prepared eagle.svg → eagle-inlay.svg (regions)
   Read: 214 paths, of them 3 hidden and 12 unpainted skipped; 0 text, 0 images, 0 clip paths, 0 masks
   Strokes made into areas: 187, 0.35 to 1.20 mm wide
   Design height: 98.40 mm
   Result: 41 pieces, 6 holes
   Dropped: 17 pieces below 0.20 % of the height (0.197 mm), 3 holes filled; smoothed within 0.05 % (0.049 mm)
   Thinnest piece: 3.10 mm (3.15 % of the height)
   Narrowest gap, the board ridge between pockets: 0.55 mm (0.56 %)
   At 100 / 200 / 300 mm tall: thinnest piece 3.15 / 6.30 / 9.45 mm; narrowest gap 0.56 / 1.12 / 1.68 mm
   The narrowest gap reaches 1.5 mm at 268 mm tall.
 ```

 The 1.5 mm figure is a guide for what a V-bit inlay holds well, not a
 limit. Warnings print as `warning: <code>: <message>` after it.
49f. **Drawn boundary** (`boundary` on Inlay, format 26; #449; the
 maintainer: "should not be complicated"). VCarve's inlay method and
 Corbin's add-in have the user draw a boundary vector round the plug, and
 the maintainer's first test design already had an oval drawn round its
 letters; their public documentation is the behaviour reference, and
 nothing learned by inspecting either product's output or files is used.
 An Inlay's `boundary` lists instances, by id, whose closed outlines
 together draw the **outer edge of the male's band**, in place of the
 computed edge (the design grown by `margin_mm` and rounded by
 `outline_rounding_mm`). The default is empty: the computed band, unchanged.
    - **The band** is the area inside the boundary less the mirrored
    design, as the computed band is: every hole of the design is carved
    and the design stands. A hole in a boundary shape is ignored
    (`inlay_boundary_hole_ignored`), since a hole would leave board
    standing in the band and hold the plug off the pocket (#484).
    - **It replaces `margin_mm` and `outline_rounding_mm`.** With a
    boundary they are ignored and need not be set (a set one warns
    `inlay_margin_ignored`). What the band's rules read from the margin
    is the boundary's **narrowest band**, the least distance from the
    design to the boundary: whether an end mill fits the band's floor
    to cut its edge upright, whether the V-bit's flank stays inside, and
    the cut-out's room. The end mill rounds a convex corner of the
    boundary by its own radius, as it always rounds the band's.
    - **It is mirrored with the design.** The user draws it round the
    design as it appears; the male uses it mirrored about the same
    vertical line as the design (the centre of the targets' bounding box),
    so it always lies round the plug the way it was drawn, and the room to
    the boundary on each side swaps sides with the design's. `check` and
    `export` say so with the same note (`inlay_male_mirrored`).
    - **The female ignores it.**
    - **Checks**, errors unless marked, each naming the operation and
    `operations[i].boundary`: an id that is no instance
    (`inlay_boundary_unknown`); an outline that is open
    (`inlay_boundary_open`); an instance that is also a target
    (`inlay_boundary_is_target`); a boundary that crosses or touches the
    design, so some part of the design is not strictly inside it
    (`inlay_boundary_crosses_design`, naming the nearest point and the
    distance); a boundary shape that holds no part of the design
    (warning `inlay_boundary_empty`). The boundary may run past the male
    board: the band is clipped to the board as 45 says
    (`inlay_band_clipped`), and the design must still stand clear of the
    board's edge (`InlayTooNearEdge`). It need not lie inside the stock.
    - **Several.** Every listed shape counts: the band's edge is their
    union, and each part of the design must lie inside that union, not
    necessarily one shape. Use one oval round everything, or one shape
    round each separate group.
    - **The cut-out runs inside it**, exactly as inside the computed
    band: with an open edge the tool's centre is one radius inside the
    boundary, so its edge runs along the end mill's wall, and the freed
    piece's outline is the boundary inset by the tool's diameter (3.175 mm
    for a 1/8 in end mill; more with a V wall, where the floor ends at
    the slope's foot). The user draws the boundary that much larger when
    the finished plug's size matters, and the chapter says so. A narrow
    band gives the same `inlay_cut_out_no_room` and
    `inlay_band_edge_v_wall` as a narrow margin, from the narrowest band.
    - The boundary is an ordinary instance, so another operation may cut
    it (a Profile of the oval on the female board, say); `init` writes
    the drawing's shapes as instances as always, and the user names the
    one to use.
49g. **Warnings and errors** of 49a to 49f, stable snake\_case codes with
 named parameters, printed as 25 says and listed in the Warnings chapter:
 `shapes_overlap`, `svg_hole_as_separate_path`, `svg_stroke_only_path`,
 `svg_hidden_object`, `svg_object_not_convertible`, `feature_below_size`
 (`init` and `check`); `prep_regions_suggested`, `prep_regions_open_ink`,
 `prep_background_shape` (one filled shape covers nearly all of the
 drawing, probably a page background: delete it in the drawing program),
 `prep_smooth_skipped`, `prep_smooth_exceeds_detail`,
 `prep_stroke_failed` (a stroke the stroker could not turn into an area,
 skipped), `prep_speck_dropped`, `prep_hole_filled` (`image-prep`);
 errors `prep_threshold_range`, `prep_nothing_left` (nothing visible, or
 everything dropped), `prep_not_svg` (a picture was given),
 `prep_output_unreadable` (the written SVG did not read back, which would
 be a defect, so nothing is written); `inlay_boundary_unknown`,
 `inlay_boundary_open`, `inlay_boundary_is_target`,
 `inlay_boundary_crosses_design` (errors), `inlay_boundary_empty`,
 `inlay_boundary_hole_ignored`, `inlay_margin_ignored` (warnings).

## Out of scope

The GUI (phase 4). Text: letters come in as SVG outlines (phase 5 draws
them). V-carving open paths, which Engrave does at a fixed depth. A V-bit's
tip flat: the bit is an ideal point, as PureCutCNC models it, and the glue
gap absorbs the difference for an inlay (Corbin's fit adjustment does the
same); tip-width modelling waits for a reason to model it. Clearing with a
ball-nose or engraving cutter (VCarve allows them; a clearing pass is a
Pocket inside, so end mills only); a pass's wall pass, stock to leave, corner
relief or rest-of chain (a pass takes its tool's step-down and
step-over and nothing else); and VCarve's "Corner Sharpen". Projecting a
carve onto a 3D model. VCarve's "Use Vector Start Points" and "Use Vector
Selection Order" (capability 11's rules order the shapes). 3D inlays on
curved surfaces (Corbin's 3D mode). Inlays whose female and male are
cut side by side in one board; the Inlay's male board (#498), or two
instances the user places, cover it. PhotoVCarve's "Selected Vectors" strategy and RasterCarve's
experimental dots mode. Picture editing: contrast, gamma, borders, cropping
and rotation are done in an image editor first, as Vectric's "Edit Picture"
does before the toolpath. Heightmap carving, where brightness is a surface
rather than a groove depth (phase 7, Surface operations). 16-bit and TIFF
pictures (read as 8-bit; TIFF in phase 7 with heightmaps). Canned cycles
for a countersink. Depth passes on a carve: one pass whatever the depth
(42). Fluting, texture and prism toolpaths (post-1.0). In 3.4:
tracing a PNG or JPEG into outlines (deferred; Inkscape's Trace Bitmap does
it today, and a tracer DreamCarve adds must be permissively licensed, since
Potrace is GPL); colour separation into several woods; closing gaps in line
art so that nearly closed spaces count; widening or thickening thin
features (the report says where they are, and nothing is enforced);
repairing live text, clip paths or masks (convert and expand them in the
drawing program); recovering arcs in prepared output; changing what `init`
imports (the checks only report); an `image-prep` option for any machining
value; and a boundary that is not an existing instance (drawing it is
phase 5).

## Acceptance tests

Numbering continues from phase 2's 47.

48. **Medial depth.** A 20 × 5 mm rectangle V-carved with a 60°, 6.35 mm
 V-bit (cone height 5.5 mm): the cut runs along the rectangle's long centreline 4.330 mm deep (within
 0.01 mm) and, at each end, up the two corner bisectors to the corners,
 reaching the surface (Z within 0.01 mm of 0) at each corner; every
 cutting point's depth equals its distance to the nearest wall divided
 by tan 30° within 0.02 mm. A 6 mm square with a 90°, 6.35 mm V-bit
 (cone height 3.175 mm) carves four diagonals meeting at the centre 3 mm
 deep. A 6 mm circle collapses to one plunge at its centre, 5.196 mm deep
 with the 60° bit. (All V-bits in these tests are 6.35 mm, the bench's
 1/4 in set: 15°, 30°, 60° and 90°.)
49. **Nesting and corners.** A 24 mm square with a 12 mm square hole, with
 the 90° bit and `flat_depth_mm: 3` (where a corridor turns, the disc
 touching both outer walls and the hole's corner is 6(2 − √2) = 3.51 mm,
 past the bit's 3.175 mm cone; changed in #404): the carve runs along the
 ring's centreline, 3 mm from each wall and 3 mm deep, and rises
 to the surface at the four outer corners; no cutting point is closer to
 any wall than its depth times tan(α/2) allows. The outline text "Rag"
 from PureCutCNC's fixture: the carve reaches every sharp corner tip at
 the surface, no cut crosses an outline, and the toolpath matches the
 `v_carve_medial` golden within 0.02 mm. A triangle's carve meets at
 its incentre at depth `inradius / tan(α/2)`.
50. **Flat depth and V-bit floor.** A 30 × 10 mm rectangle, 90° V-bit,
 `flat_depth_mm: 2`: no cutting point is below Z −2; the bit traces the
 26 × 6 mm floor boundary at Z −2; from each floor corner the carve
 rises along the bisector to the rectangle's corner at the surface; the
 union of the bit's 4 mm wide footprint at Z −2 covers the floor region
 within 0.05 mm (the V-bit clears the floor itself at the tool's
 step-over fraction of 4 mm).
51. **Clearing passes.** The same rectangle with `clearing: [ { "tool":
 "t2", … } ]`, a 3.175 mm end mill: export writes two runs, `op1
 (clearing t2)` then `op1`; the clearing run's footprint stays inside
 the 26 × 6 mm floor region and its depth is 2 mm; the V-Carve run's
 floor cutting at Z −2 (its floor rings and boundary trace; the corners'
 medial lines keep their stretch at Z −2 to the junction) lies only where
 the end mill could not reach (within 1.5875 mm of the floor boundary)
 and, at the recommended `step_over_fraction` 0.1, is under a third of
 the length it had in 50 (at 0.4 the trace alone is 40 % of it); the
 union of both runs' footprints at Z −2 still covers the floor within
 0.05 mm. A second pass with a 1.5 mm end mill after the first clears
 only the corners the first left; the two passes the other way round
 warn `clearing_not_descending`, and the 3.175 mm pass, which then
 reaches nothing, `clearing_no_room` (amended in #406). A pass
 on a V-Carve with `flat_depth_mm: null` is `clearing_needs_flat_depth`;
 a pass naming the V-bit is `clearing_needs_end_mill`. Removing the
 list gives the V-Carve the whole floor again, as in 50. (changed
 2026-10-28 with capability 40.)
52. **Cone and tool checks.** A 20 mm square with a 90°, 6.35 mm V-bit
 (cone height 3.175 mm) and no flat depth is the error
 `v_bit_cone_exceeded` naming the shape and 10 mm; with
 `flat_depth_mm: 3` it exports. An end mill on a V-Carve is
 `v_carve_needs_v_bit`. An open polyline among the targets is skipped
 with `v_carve_target_open` and the closed targets are carved.
53. **Start depth.** The 20 × 5 mm rectangle with `start_depth_mm: 4`
 inside a 60 × 30 mm Pocket 4 mm deep that runs before it: every cutting
 Z is 4 mm lower than in 48, the descent from the entry clearance above
 the stock top to the first cut is at the plunge rate, the program
 passes S1 and S2, and `check` is quiet. With the Pocket 3 mm deep,
 `check` warns `start_depth_mismatch` naming it (the rectangle given a
 1 mm flat depth: a mismatched plane counts the flank from the stock
 top, and 4 mm plus the 4.33 mm carve exceeds the 60° bit's 5.5 mm
 cone; #409). With the Pocket
 disabled, or the rectangle outside it, export is refused with
 `start_depth_uncleared`; with `start_plane_cleared_externally: true` as
 well it exports, and the same project with the 90° bit (cone height
 3.175 mm) is instead refused with `v_bit_cone_exceeded`, since 4 mm of
 start depth engages the flank past the cone (changed after the security
 review). The carve is one pass whatever its depth: the 20 × 5 mm
 rectangle plunges once per chain and never re-traces a line at a
 shallower Z, and its first plunge ends at the surface at a corner, not
 at the centreline's depth.
54. **Offset strategy.** The 6 mm square with `strategy: "offset"`,
 `step_over_mm: 0.5`, 90° bit: rings inset 0.5, 1, … 2.5 mm from the
 outline at Z −0.5, −1, … −2.5, each ring's depth equal to its inset
 within 0.01 mm; climb and conventional reverse every ring; the toolpath
 matches PureCutCNC's `v_carve` golden within 0.02 mm.
55. **Countersink.** Four 3.175 mm circles with a 90°, 6.35 mm V-bit and
 `countersink: { "diameter_mm": 6 }`: each hole is one plunge at the
 centre to Z −3 at the plunge rate and a retract, as plain moves on
 LinuxCNC with canned cycles on (no `G81`); an 82° bit plunges to
 −3.451; `diameter_mm: 7` is `countersink_mouth_exceeds_tool`;
 `diameter_mm: 3` skips every hole with
 `countersink_mouth_not_wider_than_hole`; a drill bit is
 `countersink_needs_v_bit`; `dwell_s: 0.5` writes a `G4` at the bottom.
 The same four circles inside a 40 × 40 mm Pocket 4 mm deep that runs
 before them, with `start_depth_mm: 4`: each plunge ends at Z −7 (the
 6 mm mouth opens at the pocket floor), the rapid before it stops at
 the stock top plus `retract_mm`, the descent from there to Z −7 is at
 the plunge rate, and `check` is quiet; with `start_depth_mm: 0` it warns
 `start_depth_mismatch` naming the Pocket and the mouth would open 3 mm
 above the floor, so the chapter shows this case; with the Pocket
 disabled it is refused with `start_depth_uncleared` (changed after the
 security review).
56. **Inlay female.** The star fixture (a four-point star 60 mm tip to tip
 with a 12 mm circle at its centre), an Inlay with a 60° V-bit,
 `depth_mm: 4`: the female's toolpath on the stock equals a V-Carve of
 the same targets with `flat_depth_mm: 4`, move for move. With
 `male_board.thickness_mm: null` it is the only board cut, and the
 generator warns `inlay_male_board_unset`.
57. **Inlay male.** The same Inlay with `glue_gap_mm: 1`,
 `plug_depth_mm: 2`, `margin_mm: 8` and a male board the stock's size
 (`male_board.thickness_mm` set): the female's runs come first, then
 the male's, each tagged with its board; `check` prints
 `inlay_male_mirrored`; every cutting point lies in the band from the
 mirrored star out to 8 mm beyond it or inside the mirrored circle; no
 cutting point is below Z −5; the bit's flank passes through every
 mirrored outline point at Z −3 within 0.02 mm (the uncut cross-section
 at Z −3 is the mirrored design; at Z 0 it is the design inset by
 1.732 mm); for each sample point on the star's outline, the female
 wall's inset at depth `z` and the plug's inset at height `z` above
 its start plane agree within 0.02 mm for `z` from 0 to 3. A
 `glue_gap_mm` of 4 is `inlay_glue_gap_not_below_depth`; on a 10 mm male
 board the male warns `inlay_plug_base_thin` (5 mm left) and on 12 mm it
 does not, whatever the stock's thickness. With `clearing: [t2]` (a 3.175 mm end mill) the male's first run
 clears the surround's floor at Z −5 and its V-bit floor cutting shrinks
 as in 51. With a `cut_out` by the 3.175 mm end mill (4 tabs, #443) the
     male's last run is the cut-out, `op1 (cut-out t2)`: every point of its
 tool-centre path is `1.155 + 1.5875` = 2.742 mm inside the band's
 edge, the mirrored star grown by 8 mm, within 0.01 mm (#461; the floor
 ring runs from 1.155 to 6.845 mm out, so its cutter stays at least its
 radius inside both edges); no lap runs above Z −5, and its
 last pass reaches Z −12 less the allowance; it never runs round the
 circle; and its passes below the tabs' top rise over all four. With
 `margin_mm: 4` the floor is 1.69 mm wide: `inlay_cut_out_no_room`.
 With `clearing: [t2]` the band's edge is open (#448): no V-bit cut
 comes within its cone's radius at the male board's top of the band's edge,
 and none within 1 mm; the end mill's outermost path at Z −5 is the
 edge less its radius; without clearing the V wall is cut; with
 `margin_mm: 4` the pass does not fit and `inlay_band_edge_v_wall`
 names 4.33 mm. With `clearing: [t2]`, with and without
 `outline_rounding_mm: 6`, and the cut-out, every point of the cut-out's
 path is its radius, 1.5875 mm, inside the band's edge (the star grown
 by 8 mm and closed by `max(R, 1.5875)`), within 0.01 mm (#461), and the
 walls still mate within 0.02 mm (with `outline_rounding_mm: 12`).
 With the star 2 mm from the stock's left edge (#495), a 3.175 mm 60°
 V-bit, a 1.5 mm end mill's clearing pass, `depth_mm: 2`,
 `glue_gap_mm: 0.5` and `plug_depth_mm: 0.6`, the male's band runs 6 mm
 past the edge and is clipped to it: `inlay_band_clipped` names the left
 edge and 2 mm; no tool reaches past the stock; the end mill's centre
 runs its radius inside the edge; a cut-out is open paths whose ends are
 its radius inside the edge, its tabs on them; and the pair seats within
 0.05 mm. With the bench's 6.35 mm bits it is refused, the V-bit along
 the plug's foot needing 4.33 mm there.
58. **Bitmap and placement.** `init fixtures/photo/gradient.png --width  100` (a 64 × 32 pixel picture) writes a project with one bitmap, 100 ×
 50 mm at the origin, and one Photo V-Carve naming it; `--height 50`
 gives the same; both flags, or neither, is an error. `image add  plaque.dcv gradient.png --width 60 --at 20,30` adds a bitmap whose
 footprint is 20..80 × 30..60 mm; `image list` shows it; a colour JPEG
 adds with `image_colour_converted`. Copying only the `.dcv` elsewhere
 still exports the same program. A footprint past the stock edge fails
 S7 naming the operation and the overhang.
59. **Photo grooves.** The gradient picture (black left edge to white
 right edge, with one pure black and one pure white 8 × 8 block) 100 mm
 wide, 30° V-bit, `max_depth_mm: 2`, `line_angle_deg: 22.5`,
 `line_spacing_fraction: 1.1`: every cutting move lies inside the
 100 × 50 mm footprint; the grooves are straight, at 22.5° within
 0.01°, 1.179 mm apart (1.1 × 1.072 mm) within 0.001 mm, one through
 the top-left corner (changed in the design spec), and alternate in
 direction; samples over the
 black block are at Z −2.000 and over the white block at Z 0.000
 within 0.01 mm, and over 50 % grey at Z −1.00 within 0.02 mm; with
 `invert: true` the black and white depths swap; with `line_angle_deg:  0` every groove is parallel to X; with `strategy: "hatch"` a second
 set at 112.5° follows the first; between grooves the bit rises to Z
 +1 and crosses at the feed rate; `line_spacing_fraction: 0.9` warns
 `photo_lines_overlap`.
60. **Photo parity.** For each photo golden case the design spec lists
 (RasterCarve's defaults on the gradient picture at 0.1 in wide, the
 same in millimetres at 100 mm wide, and a hatch case), DreamCarve's
 grooves match RasterCarve's program groove for groove, each sample
 within 0.01 mm in XY and 0.05 mm in Z (2.5 % of the 2.03 mm maximum
 depth, the room sampling leaves), after mapping RasterCarve's picture
 origin to DreamCarve's as the fixture README states.
61. **Sign end to end.** The sign `.dcv` (see Test parts) exports its
 programs in tool-run order, `check --preview` draws the end mill's
 floor rings round the oval, the V-bit's lines into every serif of the
 inset lettering, the two countersinks and the tabbed outline, and each
 program passes S1–S19.

Sub-phase 3.4 (lettered, as the capabilities are; every fixture is a
synthetic drawing under `tests/fixtures/prep/`, drawn by a script, and no
artwork of the maintainer's is committed):

61a. **Import checks.** `messy.svg`, a 100 mm tall page holding two
 filled 10 mm squares that overlap by 4 mm, a filled 20 mm square with a
 filled 10 mm square inside it as a separate path, a 1.2 mm stroke-only
 polyline, a layer with `display:none` holding a square, one `<text>`, one
 `<image>`, a clip-path group and a 0.1 mm filled dot. `init` prints one
 warning for each of `shapes_overlap` (naming the two squares' ids and the
 overlap), `svg_hole_as_separate_path` (naming the nested pair),
 `svg_stroke_only_path` (1.200 mm), `svg_hidden_object` (naming the
 layer), the text, image and clip-path warnings each with its fix, and
 `feature_below_size` (one piece under 0.200 mm, 0.2 % of the design's
 100 mm, the fixture's shapes spanning the page's height); each that
 `image-prep` would fix names `dreamcarve image-prep`.
 The `.dcv` it writes and its exit status equal what the same build wrote
 before the checks, byte for byte (the checks change nothing). `clean.svg`
 (the monogram "DC", one compound path per letter) prints none. `check` on
 `messy.dcv` prints `shapes_overlap` and `feature_below_size` and none of
 the SVG-only codes.
61b. **Visible geometry only.** `image-prep messy.svg` reports the hidden
 layer (1 object), the text, the image and the clip path as skipped or
 not convertible, each with its fix, and the output holds none of them:
 the hidden square's area is not in it.
61c. **Strokes become areas.** A 2 mm wide, 20 mm long line prepares to
 one piece of area 43.14 mm² (2 × 20 + π, within 0.05) with round caps,
 40.00 with butt caps and 44.00 with square caps (within 0.01); a
 polyline of two 20 mm legs at a right angle, 2 mm wide, butt caps,
 prepares to 80.00 mm² with a miter join and 79.79 mm² (within 0.05) with
 a round join; a dashed 2 mm line gives one piece per dash, each as long
 as the dash; a stroke under a non-uniform scale is as wide as the
 transform makes it along each axis.
61d. **Fills are resolved.** Two 10 mm squares overlapping by 5 mm give
 one piece of 150 mm². A 20 mm square and a 10 mm square centred in it,
 as two separate paths, give one piece of 300 mm² with one hole. The same
 two squares in one path wound the same way fill solid under nonzero
 (400 mm², no hole) and leave a hole under even-odd (300 mm², one
 hole), as the SVG rules say. Output piece order and ids follow position,
 not document order.
61e. **Regions mode, line art in a ring.** `ring-cross.svg`: a circle of
 radius 29 mm stroked 2 mm (ink from radius 28 to 30), and two crossing
 straight lines 58 mm long, 2 mm wide, through its centre, on a 60 mm
 tall drawing. Default mode prepares one piece (the ink, 584.3 mm² within
 1) with four holes. With `--regions` it prepares four pieces, congruent
 to within 0.5 mm² of each other, together 2243 mm² (within 1), none of
 them overlapping ink (the pieces and the ink together are the disc of
 radius 30 mm, 2827 mm², within 1), and the report's narrowest gap is
 2.000 mm (within 0.02). The same art with the cross cut short of the
 ring by 1 mm prepares under `--regions` to one piece, a pocket with the
 cross as its hole, and warns `prep_regions_open_ink` (one ink component,
 the cross, encloses no space).
61f. **Clean-up.** On a 100 mm tall design: a 0.1 mm filled dot is dropped
 and a 0.3 mm dot kept at the default `--min-detail` (0.2 % is 0.2 mm), a
 0.1 mm hole in a piece is filled and a 0.3 mm hole kept, and
 `--min-detail 0` keeps all four; a long thin line 0.1 mm wide and 40 mm
 long, nowhere wider, is dropped (a hairline), and one 0.5 mm wide is
 kept. A 40 mm straight edge drawn as a staircase of 0.5 mm steps 0.02 mm
 high prepares, with the default `--smooth` (0.05 mm here), to a straight
 edge with no vertex between its two corners, no output point farther
 than 0.05 mm from the original outline, while a 90° corner of the same piece stays within 0.001 mm of its
 place; `--smooth 0` leaves the staircase. `--min-detail 6` and
 `--smooth -1` are `prep_threshold_range`; `--smooth 0.2` with the
 default `--min-detail` warns `prep_smooth_exceeds_detail`.
61g. **The output file.** A 200 × 100 mm page prepares to a file declaring
 200 mm × 100 mm and a millimetre `viewBox`; every shape is a filled
 `<path>` with no stroke, text, image, clip path, mask, gradient or hidden
 object; a lone rectangle prepares to the same rectangle (bounding box
 within 0.01 mm, area equal); a letter "F" is still an "F" and in the same
 place (no flip, no shift); an existing output file is not overwritten
 (an error naming it); a PNG or JPEG input is `prep_not_svg` and says to
 trace in Inkscape; malformed SVG is an error and no file is written.
 `init` on the output prints none of the 49a warnings and imports one
 shape per piece.
61h. **The report.** On `ring-cross.svg` the report names a design height
 of 60.00 mm and, in `--regions` mode, a narrowest gap of 2.000 mm (3.33 %
 of the height), 3.33 / 6.67 / 10.00 mm at 100 / 200 / 300 mm tall,
 reaching 1.5 mm at 45 mm tall; in default mode a thinnest piece of
 2.000 mm. The same art with 1 mm strokes (about 1.7 % of the height),
 prepared in default mode, warns `prep_regions_suggested`; the 2 mm
 version does not. A filled rectangle over 98 % of the drawing's box warns
 `prep_background_shape`. The report prints counts of what was dropped
 and filled that agree with 61f.
61i. **Determinism.** The same input prepared twice gives the same bytes;
 so do the same paths in reverse document order, and the same drawing
 saved with CRLF line ends.
61j. **No machining inputs.** `image-prep --help` lists no tool, stock,
 depth, feed or size option, and the output of 61g is the same whatever
 the project it is later used in.
61k. **Line art to inlay, end to end.** `ring-cross.svg` prepared with
 `--regions`, `init`, an Inlay (60° 6.35 mm V-bit, a 3.175 mm end mill as
 clearing pass, `depth_mm: 4`, `glue_gap_mm: 1`, `plug_depth_mm: 2`) with
 a circular boundary 8 mm beyond the pockets: `init` prints no 49a
 warning, both boards export with every program passing S1–S19, and the
 pair seats within 0.05 mm as in 57. The male is one board carrying four
 plugs, and the female four pockets.
61l. **Boundary replaces the margin.** The star fixture (60 mm tip to
 tip), the Inlay of 57 (`depth_mm: 4`, `glue_gap_mm: 1`, `plug_depth_mm:
 2`, the 60° 6.35 mm V-bit) and a boundary of one 80 mm square centred on
 the star, 10 mm from each tip, on a 100 × 100 mm male board: with
 `margin_mm: 8` still set `check` warns `inlay_margin_ignored`; with
 `margin_mm: null` and `outline_rounding_mm: null` it exports without a
 warning of that kind. Every male cutting point's cone at the start plane
 lies inside the square and outside the mirrored star, except inside the
 mirrored circle, which is carved as a hole is; no cutting point is below
 Z −5. With `clearing: [t2]` (3.175 mm end mill) the end mill's outermost
 path at Z −5 is the square inset by its radius, 1.5875 mm, within 0.01
 mm, its walls vertical, and the square's corners are cut with that
 radius; no V-bit cut comes within its cone's radius at the male board's
 top (2.887 mm) of the square. Without clearing the square's edge is a V
 wall, as a computed band's is. The female's toolpath is move for move what
 it is with no boundary.
61m. **Mirrored with the design.** The "DC" project of the lettered pair
 (`fixtures/dc.dcv`) with a boundary of its 90 × 60 mm frame moved 3 mm
 right: the room from the letters to the boundary is about 5.3 mm on the
 left and 11.3 mm on the right as drawn, and on the male the mirrored
 letters stand with about 11.3 mm on the left and 5.3 mm on the right, the
 band's edge being the drawn frame mirrored about X = 55 (within the
 flatten tolerance); `check` prints `inlay_male_mirrored` at X = 55 mm.
61n. **Boundary checks.** On the star: a boundary smaller than the star,
 or one passing through a tip, is `inlay_boundary_crosses_design` naming
 the nearest point; an open polyline, `inlay_boundary_open`; an instance
 that is also a target, `inlay_boundary_is_target`; an id the project does
 not have, `inlay_boundary_unknown`; a second boundary shape that holds no
 part of the design, `inlay_boundary_empty` (a warning that does not stop
 export); a boundary shape with a hole round the star's circle, the hole
 ignored with `inlay_boundary_hole_ignored` and the band cut through it as
 band; each located at `operations[i].boundary`. The same star with no
 boundary exports as before, byte for byte.
61o. **Several boundaries.** Two stars 120 mm apart, each with its own
 80 mm square boundary: the male's band is the two squares less the
 stars; a boundary that holds only one star is `inlay_boundary_crosses_design`
 for the other; one 260 × 100 mm oval round both gives one band.
61p. **Boundary, cut-out and the board's edge.** With the boundary of 61l,
 `clearing: [t2]` and a cut-out by t2 with four tabs: every point of the
 cut-out's tool-centre path is 1.5875 mm inside the square, within 0.01
 mm (as 57, #461), and the freed plug's outline is the square inset by
 3.175 mm. With a 68 mm square (4 mm clear of each tip) and no clearing,
 `inlay_cut_out_no_room` names 5.484 mm (2 · P · tan(α/2) + the tool's
 diameter), the figure `margin_mm: 4` gives. A
 boundary that runs 6 mm past the male board's left edge is clipped to it,
 warns `inlay_band_clipped` as 57's does, and nothing is cut past the
 board.
61q. **Format 26.** A format 25 project loads, migrates with
 `boundary: []` in every Inlay, and exports the same bytes as before; a
 project with a boundary saves and loads unchanged; Photo V-Carve's
 files are format 27.
61r. **Safety re-derives from the instance.** A male program exported for
 a boundary and then checked against the same project with the boundary
 instance moved 5 mm, so that some cuts lie outside the new band, fails
 S17 and S9 (the open-edge end mill's cuts, the cut-out's ring); the
 program exported after the move passes. The check never calls a
 generator (S17's rule, unchanged).
61s. **Bench stand-in.** The line-art inlay's synthetic fixture
 (`tests/fixtures/prep/compass.svg`, see Test parts), prepared with
 `--regions`, then an Inlay with a drawn oval boundary, exports both
 boards with every program passing S1–S19, seats within 0.05 mm, and
 every cut lies inside its 130 × 100 mm board (the test part's stock and
 male board; 110 × 80 mm is the lettered pair's) with at least 3 mm to spare.

## Safety requirements

S1–S16 apply to every operation and every post-processor. New in this
phase:

- **S17 A V-bit cuts only its own material.** For every cutting point of a
V-Carve or an Inlay, the bit's cone (its included angle about the point,
up to the cone height) meets the start plane only inside the operation's
carved region: a V-Carve's targets' area, a male Inlay's surround. A
point touching a wall is allowed; a point whose flank would pass through
a wall below the start plane, or whose tip is below the flat depth, is
not. The flank's **engagement**, from the stock top to the tip (or from
the start plane, where an enabled earlier Pocket has cleared it), never
exceeds the cone height, so the shank never cuts; a male Inlay's counts
from the stock top always. The only cutting move allowed above the start
plane is a vertical descent to where a cut begins. A Photo V-Carve's
tip stays inside the picture and above its maximum depth (S19); its
groove's width may run past the picture's edge by up to the full-depth
half-width, since the edge is not a wall, and S7 keeps that on the stock
(changed in the design spec: the draft held the whole cone inside the
picture, which no edge-to-edge carve, RasterCarve's included,
satisfies). Checked exactly, as S9 is, not by sampling: on the toolpath
by the generator before it returns, and again on the parsed program,
where the start plane is the operation's `start_depth_mm` below the
stock top (changed after the security review: the draft sampled every
1 mm, which can miss half a millimetre of wall, and counted the cone
from the start plane). S9's footprint test is this test with a cylinder;
this one is with a cone.
- **S18 A countersink's depth is derived, never set.** A countersink
plunge ends at `diameter_mm / (2 · tan(α/2))` below the hole's top (the
start plane, `start_depth_mm` below the stock top), and nowhere else; it is written as plain moves on every controller; the
mouth is never wider than the bit; S1 and S15 apply with the Drill's
through-cut allowance. Checked on the parsed program against the
circle's centre, the tool's angle and the setting.
- **S19 Grooves stay in the picture, and between-groove moves are fed.**
Every Photo V-Carve cutting point lies inside the picture's footprint
and between the start plane and `start_depth_mm + max_depth_mm`; the
move from one groove to the next is at `stepover_retract_mm` above the
start plane as a feed move, and only a retract to safe Z or a rise to
the clearance height is a rapid, so S2 holds unchanged. Checked on the
parsed program.
- **S17 and S9 with a drawn boundary (3.4, #449).** A male Inlay with a
`boundary` is held to the band that boundary draws, re-derived by the
safety pass from the boundary instances in the project (mirrored with the
design, holes dropped, clipped to the male board) with the same pure
geometry the generator uses, never by calling a generator: the V-bit's
cone at the start plane stays inside the band less the design; with an
open edge the cone at the male board's top stays inside the band's edge;
the end mill clearing the floor, and the cut-out, stay inside the floor
rings that band gives. A boundary changed after export therefore fails
the old program (61r). No new rule number: the rule is S17's and S9's,
applied to a band the user drew.
- **`image-prep` is not a safety layer.** It writes an SVG, never motion.
Every program cut from its output passes S1–S19 exactly as one cut from
any drawing does, and the generators handle an unprepared drawing as they
always did (the stress tests, #464, cover that): preparing art helps the
user and hides no generator defect.
- **S2, S6, S7 unchanged.** A start plane below the stock top changes no
rule: the descent to it is a plunge at the plunge rate. S6 reads a
groove's samples as the straight cuts they are. S7 holds a picture's
grooves, widened by the groove width at their depth, inside the stock.

## Test parts

Four parts, all cut on grblHAL. Record date, outcome and deviations
after each. They follow the sub-phases: the sign (3.2), the lettered
inlay pair (3.3), the line-art inlay (3.4) and the photo (3.5).

### Sign: raised oval, inset lettering

**Sign.** 200 × 80 mm rectangle, outside Profile with tabs; a 190 × 70 mm
rectangle 5 mm inside it V-carved as the background with a 150 × 46 mm
oval nested in it as an island, 2.5 mm flat depth, its floor cleared
by the 6.35 mm end mill as a clearing pass, so the oval stands raised with a 45° edge; the words
"DreamCarve" set on a curve inside the oval (a bold serif, 14 mm cap
height, converted to paths in Inkscape) V-carved inset with no flat depth;
two 3.175 mm hanging holes, countersunk 6 mm. All V-bits are the bench's
1/4 in (6.35 mm) set. Drawn as `tests/fixtures/sign.svg` by
`tools/parity/sign-fixture.ts` (#411): the lettering is Droid Serif Bold
(Apache-2.0, from three.js's typeface file), 14 mm cap height, on an arc
of radius 500 mm; its widest stroke is about 3.3 mm, so it carves at most
1.658 mm deep with the 90° bit, no scaling needed; the hanging holes are
in the background's top corners, at (15, 15) and (185, 15) mm from the
drawing's top-left, since the 5 mm border is too narrow for a 6 mm
countersink and the lettering fills the oval. `tests/fixtures/sign.dcv`
is the quick start's project; op2 sets `step_over_fraction: 0.1` for the
V-bit's floor corners (0.25 mm ridges, where the tool's 0.4 leaves 1 mm).


| Item             | Value                                                                                                                                      |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Material         | Hardwood (poplar or maple) or MDF, nominal 12 mm, measured thickness entered; plywood shows its layers in a V-carve and is not recommended |
| Stock            | 220 × 100 mm, origin bottom-left, Z zero at stock top                                                                                      |
| T1               | 6.35 mm two-flute upcut end mill: 1500 mm/min, plunge 500, 18000 RPM, step-down 4 mm, stepover 40 %                                        |
| T2               | 3.175 mm two-flute upcut end mill: 1000 mm/min, plunge 300, 18000 RPM, step-down 2 mm, stepover 40 %                                       |
| T3               | 90° V-bit, 6.35 mm diameter (cone height 3.175 mm): 1200 mm/min, plunge 400, 18000 RPM, step-down 2.5 mm, stepover 40 %                   |
| op1 Drill (T2)   | two 3.175 mm circles, 12 mm deep, `simple`, through-cut allowance 0.2 mm                                                                   |
| op2 V-Carve (T3) | the 190 × 70 mm rectangle with the oval inside it, `medial`, `flat_depth_mm: 2.5`, `clearing: [t1]` (`offset`, climb, ramp)                                                          |
| op3 V-Carve (T3) | the lettering, `medial`, `flat_depth_mm: null` (inset; strokes 2 to 3 mm wide carve 1 to 1.5 mm deep)                                      |
| op5 Drill (T3)   | the two circles, `countersink: { "diameter_mm": 6, "start_depth_mm": 2.5 }` on op2's floor, which clears its start plane (41, #426): 3 mm below the floor, Z −5.5; listed after op3 and before op4, so the V-bit's three operations share one tool run (changed in #428: the draft had it as op4, before the Profile op5) |
| op4 Profile (T1) | outside, 12 mm deep, 4 tabs 8 × 3 mm, climb, `entry: ramp`, `lead` 3 mm, through-cut allowance 0.2 mm; last in the list                   |
| Safe Z           | 15 mm                                                                                                                                      |
| Controller       | grblHAL, one program per tool run (four: T2, T1 as the clearing pass, T3, T1)                                                                                          |


**Success:** the background floor is flat with no ridge where the end mills
and the V-bit meet; the oval's edge is a clean 45° slope from the floor
to the surface with no step; every serif and corner of the inset
lettering is cut to a sharp point at the surface, and the letters read
clearly; the countersinks are 6 ± 0.3 mm across; the sign releases after
the tabs are cut.

**Cut record:** partial cut, sub-phase 3.1, without the countersinks,
on 2026-10-29
([#412](https://github.com/grayzu/dreamcarve/issues/412#issuecomment-6508098597)).
The full cut, with the countersinks, is #429's.

| Item | Result |
| --- | --- |
| Programs | The four programs of the quick start "A V-carved sign from SVG outlines" before #428 (op1 to op4, no countersinks), exported by the build of the day: T2 holes, T1 floor clearing, T3 slope and lettering, T1 cut-out |
| Controller | Sienci LongMill MK3, grblHAL |
| Outcome | The sign cut as designed, and its dimensions measured as expected |

Deviations: the oval's short axis measured about 47 mm against the 46 mm
drawn, which the maintainer judged a difference in measuring, not in the
cut.

### Lettered inlay pair

**"DC" inlay.** Corbin's Workshop's cutting-board method, 2D, on a
tutorial piece (changed in #446 from a symmetric star, which never showed
the male's mirror): the monogram "DC" in Droid Serif Bold (Apache-2.0),
40 mm cap height, the letters 3 mm apart, 73.4 × 41.1 mm in all, drawn
as `tests/fixtures/dc-inlay.svg` by `tools/parity/inlay-fixture.ts`, with
a 90 × 60 mm `frame` round it that nothing cuts, so that `init` makes the
110 × 80 mm stock with the letters at its centre. The D's counter is a
design hole: an island in the female, a hole in the male's plug. Both
letters face right, so the male carves a reversed "DC" and the flip
direction matters. The female on the stock, the male on the Inlay's
male board, from one project (#498; until format 25 two files, the same
but for `side`). `tests/fixtures/dc.dcv` is the quick start "An inlay
pair"'s project: `t1` is `init`'s unused "Set me", `t2` the end mill,
`t3` the V-bit; the stock and the male board are both 110 × 80 mm
(`male_board` width and height `null`) and 12 mm thick, which the user
replaces with each board's measurement. It exports byte for byte the
programs the two-file pair did, under the board's names; the pair is
kept as migration fixtures in
`crates/dreamcarve/tests/fixtures/inlay-migration/`. Every cut of both
boards lies inside its 110 × 80 mm board with at least 3 mm to spare (the male's clearing pass, which cuts the band's
edge, comes within 10.3 mm of an edge).


| Item         | Value                                                                                                |
| ------------ | ---------------------------------------------------------------------------------------------------- |
| Female board | a light hardwood (maple, oak), nominal 12 mm, 110 × 80 mm stock                                      |
| Male board   | a contrasting dark hardwood (walnut), nominal 12 mm, 110 × 80 mm stock                               |
| T1           | 60° V-bit, 6.35 mm diameter (cone height 5.5 mm): 1000 mm/min, plunge 300, 18000 RPM, step-down 2 mm, stepover 40 % (project tool `t3`) |
| T2           | 3.175 mm two-flute upcut end mill, at least 14.2 mm of flute (12 mm board + 0.2 mm through + 2 mm): 1000 mm/min, plunge 300, 18000 RPM, step-down 2 mm, stepover 40 % (project tool `t2`) |
| Inlay        | `op1` `DC`, targets the two letters; `depth_mm: 4`, `glue_gap_mm: 1`, `plug_depth_mm: 2`, `margin_mm: 8`, `outline_rounding_mm: 10` (#448), `step_over_fraction: 0.1`; clearing pass `offset`, climb, ramp 15° |
| dc.dcv, female board | op1 Inlay (T1) on the stock, `clearing: [t2]` (two runs: `dc-female-1-T2.nc` to Z −4, about 30 s, then `dc-female-2-T1.nc`, about 1 min 6 s); prints `unused_tool` and `entry_fallback` (the C's stroke has no room for the clearing pass's ramp, so it plunges there) |
| dc.dcv, male board | op1 Inlay (T1) on its male board, `male_board.thickness_mm: 12`, `clearing: [t2]`, `cut_out` with `t2`, four rect tabs 6 mm wide and 3 mm tall, 0.2 mm through (three runs: `dc-male-1-T2.nc` to Z −5, which clears the band out to its edge and cuts the edge upright, about 5 min 26 s, `dc-male-2-T1.nc`, the plug's walls and the floor's corners, about 47 s, then the cut-out `dc-male-3-T2.nc` from the floor to Z −12.2, along the band's edge, at least 6.412 mm out from the letters (#461; 4.577 mm before), about 1 min 19 s; changed in #448 from 3 min 11 s, 1 min 4 s and 1 min 32 s with a V-walled edge); prints `unused_tool`, `inlay_male_mirrored` at X = 55 mm and `tool_change_avoidable` (the order is required) |
| Safe Z       | 10 mm                                                                                                |
| Controller   | grblHAL, one program per tool run                                                                    |

The times are `export`'s estimates, cutting and dwells only, without
rapids, acceleration or tool changes.

**Procedure** (Corbin, "Inlay Fit Adjustment"): cut the female, then the
male. Test fit dry: the plug should seat with no gap at the top and stop
before its base touches the female surface. If a gap shows at the top, the
plug bottoms out: raise `glue_gap_mm` and recut the male
(`export --board male`). If the plug will not enter, lower it. Before the test fit, the plug comes out of its
board, since the male board outside the band is at full thickness and
would rest on the female (changed in #437: the draft clamped the male's
whole board face down): the male's third program cuts it out in the
band's floor, leaving four tabs, which are cut by hand (#443; with
`cut_out: null`, saw it free inside the band instead). The plug reads as
a reversed "DC": turn it over left to right, about a vertical line, over
its left or right edge, so its top edge stays at the back; face down it
reads "DC". Glue, clamp, let it dry, saw off the plug's backing, plane
flush. A bench check may substitute its own design and boards, prepared
as the chapter's "Preparing your own design" says.

**Success:** after planing, no visible gap round the letters or the D's
counter (a 0.1 mm feeler gauge does not enter), the serifs are sharp, the
monogram reads "DC" the right way round, and a second pair cut in half
shows about 1 mm of glue gap under the plug and the walls mated along
their height.

**Cut record:** not yet cut.

### Line-art inlay

**Compass.** The sub-phase 3.4 bench check: line art prepared by
`image-prep --regions` and inlaid inside a drawn oval. The drawing is
`tests/fixtures/prep/compass.svg` (drawn by a script, like the others): a
70 mm tall compass in line work, an outer ring, a cross of four straight
lines and four diagonals to the ring, every stroke 2.5 mm wide (3.6 % of
its height), so the board ridges between the pockets are 2.5 mm. It stands
for the maintainer's eagle line art, which is not committed; a bench check
may substitute the user's own art, prepared as the "Preparing artwork"
chapter says, and an oval drawn round it. The oval is a second path in the
SVG, drawn 96 × 84 mm round the ring, which `init` imports as an instance
the Inlay names as its `boundary`.

| Item | Value |
| --- | --- |
| Female board | a light hardwood (maple, oak), nominal 12 mm, 130 × 100 mm stock |
| Male board | a contrasting dark hardwood (walnut), nominal 12 mm, 130 × 100 mm |
| T1 | 60° V-bit, 6.35 mm diameter, as the lettered pair's |
| T2 | 3.175 mm two-flute upcut end mill, as the lettered pair's |
| Preparation | `image-prep compass.svg --regions`, then `init`; the report's narrowest gap, 2.5 mm, is the ridge the cut should show |
| Inlay | `op1`, targets the prepared pockets, `depth_mm: 4`, `glue_gap_mm: 1`, `plug_depth_mm: 2`, `boundary` the oval, `margin_mm` and `outline_rounding_mm` `null`, `step_over_fraction: 0.1`; clearing pass t2 `offset`, climb, ramp 15°; `cut_out` by t2 with four tabs |
| Safe Z | 10 mm |
| Controller | grblHAL, one program per tool run |

**Procedure:** as the lettered pair's, from the preparation on: cut the
female, then the male, free the plug (the cut-out runs 1.5875 mm inside
the oval, so the plug's outline is the oval less 3.175 mm all round), test
fit, glue, clamp, saw off the backing, plane flush.

**Success:** after planing, no visible gap round any pocket (a 0.1 mm
feeler gauge does not enter); the compass's lines read as ridges of the
light board, within 0.2 mm of the 2.5 mm the report gave; each ridge is
unbroken; the oval plug's outline is the drawn oval inset by the end mill's
diameter, within 0.3 mm.

**Cut record:** not yet cut.

### Photo V-carve

**Photo.** A photograph of the user's choice with good contrast, 100 mm
wide, carved into light wood; its program compared with RasterCarve's for
the same picture and settings before cutting.


| Item                   | Value                                                                                                                                      |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Material               | maple or MDF, nominal 12 mm, 150 × 120 mm stock                                                                                            |
| T1                     | 30° V-bit, 6.35 mm diameter: 1000 mm/min, plunge 300, 18000 RPM                                       |
| op1 Photo V-Carve (T1) | `max_depth_mm: 1.5`, `line_angle_deg: 22.5`, `line_spacing_fraction: 1.1`, `strategy: raster`, `sample_mm: 0.25`, `stepover_retract_mm: 1` |
| Safe Z                 | 10 mm                                                                                                                                      |
| Controller             | grblHAL                                                                                                                                    |


**Success:** the picture is recognisable at two metres; the darkest areas
measure 1.5 ± 0.1 mm deep and the lightest are unmarked; the grooves are
evenly spaced with no skipped or doubled line; no groove leaves the
picture's footprint; the program's grooves match RasterCarve's for the same
picture and settings within the tolerance of acceptance 60.

**Cut record:** not yet cut.

## Documentation deliverables

- User Guide (3.4): a new chapter "Preparing artwork" (what goes wrong in
drawings, the `init` checks and each code, `image-prep` with a
shapes-mode and a line-art workflow, tracing a picture in Inkscape first,
the two thresholds, the report and how to read it, why circles come back
as polygons, what to do about a page background); Inlay updated (the
drawn boundary: drawing it, naming it, mirrored, the cut-out's inset,
several, clipping, replacing margin and rounding); Importing and Shapes,
holes and islands updated (the checks); Warnings updated with every code
in capability 49g; The project file updated (format 26, `boundary`).
Quick start: the inlay pair's "Preparing your own design" points to the
chapter, and a new section "From line art" runs `compass.svg` end to end.
- User Guide: V-Carve operation (medial lines, flat depth, clearing
passes, start depth, one pass, the offset strategy, choosing a bit angle);
Inlay (the method, the four values and what each changes, the two
boards of one project and the male board, the fit procedure and
re-cutting the male, gluing and planing); Photo V-Carve (pictures
and `image`, every setting, dark and light wood, picking a bit and depth,
what to do in an image editor first); Drill operation updated
(countersink); Warnings updated
with every code in capability 48; The project file updated for the new
operation kinds, the bitmap and the format version; Tool library updated
(V-bit angles, cone height); Safety updated (S17–S19).
- Quick start: "A V-carved sign from SVG outlines", end to end on the sign
test part.
- Contributing: Porting from PureCutCNC updated with the Y-up rules for
medial lines and offset rings; Golden corpus how-to for the RasterCarve
cases (what is captured, what is not, and why); Crate map updated.
- `CONTEXT.md`: the terms this phase adds, with their PureCutCNC, VCarve
and Corbin names. Image Prep, Ink, Piece, Speck and Boundary were added
with the 3.4 spec.

## Exit criteria

Delivered as sub-phases 3.1 V-Carve, 3.2 Countersink, 3.3 Inlay, 3.4
Image Prep, 3.5 Photo V-Carve, 3.6 Tapered Ball Nose and 3.7 Release,
each a GitHub milestone with its own bench check and, where it changes the
file, its own format version (design spec, "Sub-phases"); the tag comes
at 3.7. (3.4 was inserted on 2026-11-10, at the maintainer's request:
preparing artwork is the hardest part of an inlay. It renumbers Photo
V-Carve from 3.4 to 3.5, Tapered Ball Nose, specified separately in #445,
is 3.6, and Release moves from 3.5 to 3.7.)

- [ ] All acceptance tests and safety tests green on all three OSes.
- [ ] Golden parity cases captured and passing: the PureCutCNC V-carve

  cases and the RasterCarve photo cases.
- [ ] The four test parts cut on grblHAL and recorded above, the inlay

  pairs fitted.
- [ ] Quick start and every chapter above published.
- [ ] `CONTEXT.md` updated.
- [ ] `v0.3.0` tagged with binaries.

## Open questions

Answered by the maintainer on 2026-10-26; the decisions are folded into the
capabilities above, and the record stays here.


| #   | Question                                                                                                                                                                                                                       | Decision                                                      |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------- |
| 1   | Clearing: a Pocket with `clear_for` (one tool per operation, order explicit, rest machining reused), or clearing tools listed inside the V-Carve as VCarve's form does (one operation, two tools, which export has never had)? | Answered 2026-10-26: `clear_for` Pocket. Revised 2026-10-28, once the design showed the cost (fifteen lines of Pocket JSON per tool, five validation rules): clearing tools listed inside the carve, each a run of its own tool (40). |
| 2   | Inlay: its own operation kind with `side`, mirroring the male in place, or an `inlay` block on V-Carve?                                                                                                                        | Answered 2026-10-26: agreed, its own kind (45); V-bit inlays are complex enough to deserve one. |
| 3   | Pictures: embedded in the `.dcv` (one self-contained file, larger) or referenced by path (small file, breaks when moved)?                                                                                                      | Answered 2026-10-26: embedded (46). |
| 4   | Depth passes on V-Carve (`step_down_mm`): in scope, as VCarve cuts, though PureCutCNC cuts in one pass?                                                                                                                        | Answered 2026-10-26: single pass (42); multiple passes may be added later if needed. |
| 5   | Photo parity tolerance: 0.01 mm XY, 0.05 mm Z (acceptance 60)? The design may tighten it once the sampling is matched.                                                                                                         | Answered 2026-10-26: agreed. |
| 6   | `init` on a picture (46): wanted, or `image add` only?                                                                                                                                                                         | Answered 2026-10-26: agreed, both. |
| 7   | Test-part bits: which V-bits does the bench have?                                                                                                                                                                              | Answered 2026-10-26: 1/4 in (6.35 mm) 15°, 30°, 60° and 90°. Every test part and acceptance test uses those; a 90° bit's cone height is 3.175 mm, which caps the sign's flat depth at 2.5 mm and its countersinks at 6 mm. |
| 8   | Countersink depth measured from the stock top (the hole's top), as here, or from the top of an instance with its own `z_top_mm` once that field is read (PureCutCNC measures from each target's own top face)?                 | Answered 2026-10-26: from the hole's top, which must allow for a hole in a pocket floor. So a countersink has a `start_depth_mm` like a carve's (44), checked against an earlier Pocket by `start_depth_mismatch` (41). |

### Sub-phase 3.4: open for the maintainer's review

Added with the 3.4 spec (2026-11-10). Each row has a recommendation and
its reason; the maintainer answers in review, and the decisions are then
folded into capabilities 49a to 49g like the table above. Questions 10 to
15 are #449's, in its order.

| #   | Question | Recommendation |
| --- | -------- | -------------- |
| 9   | Which format number does the boundary take? Photo V-Carve was 26 until 3.4 was inserted; 3.4 lands before it. | The boundary is format 26 (an additive `boundary: []`) and Photo V-Carve moves to 27, since 3.4 ships first and the chain is one version per sub-phase that changes the file. Nothing of Photo's is built, so the renumbering costs a few lines of spec, and `serialize::photo_v_carve_is_format_26` is renamed when its slice lands. A later number for the boundary would leave 26 empty for no reason. |
| 10  | #449: does the boundary replace `margin_mm` (and `outline_rounding_mm`), or does `margin_mm` stay as the minimum band? | Replace (49f). One source for the band's edge: a drawn edge cannot also be "the design grown by M", and a second number that must agree with the picture is a way to fail an export for a reason the user cannot see. What the margin's rules need (an end mill fits, the flank stays inside, the cut-out has room) is read from the narrowest band the drawn edge leaves, which the generator can measure exactly, so the rules lose nothing. With a boundary the margin may be `null`; a set one warns `inlay_margin_ignored` rather than silently winning. `margin_mm` stays the way to ask for a computed band. |
| 11  | #449: is the boundary mirrored with the design, or used as placed? | Mirrored with the design, about the same line (49f). The user draws "round what you see", and the plug is the design mirrored; as placed, the boundary would no longer bear the relation to the design that the user drew: a boundary that follows the design's shape (a contour a few millimetres out, the usual way to draw one) would cross the mirrored design, and an off-centre oval would sit on the wrong side of it (3 mm right of centre in the drawing, with 5.3 mm of room on the left and 11.3 on the right, would give those rooms to the wrong ends of the lettering). Mirroring is an isometry, so the checks run once on what was drawn. |
| 12  | #449: validation. | Closed, a project instance, not a target, and the design strictly inside it, each a named error (49f); a boundary that crosses or touches the design is the one hard geometric error. Not required: inside the stock (the band is clipped to the board, #495, and only the design must stay clear of the board's edge) and a minimum room (the existing `inlay_band_edge_v_wall` and `inlay_cut_out_no_room` already say what a narrow band costs, from the measured narrowest band). The issue's draft asked for the design grown by `P·s` plus the tool's diameter: for an end mill that is the open edge's condition, which decides the edge's kind, not whether the male can be cut, so it is a warning that already exists rather than a new error. |
| 13  | #449: one boundary per disjoint part, or one overall? | A list of instances, whose union is the edge (49f), so one oval round everything and one shape round each group both work and need no extra field. Each design part must be inside the union, not inside one particular shape. A list costs nothing over one id (`targets` is one already), and it spares the user from unioning shapes in the drawing program. |
| 14  | #449: the cut-out on the boundary, inside it, or mid ring? | Inside it, exactly as inside the computed band (49f; #461): tool centre one radius in with an open edge, the slope's foot plus a radius in with a V wall. The kerf then lies inside the band's flat floor, so above the floor it cuts nothing the carve left, and the freed plug has no knife edge. Cutting on the boundary would put half the tool in uncut board, and the mid-ring path was replaced in #461 for the maintainer's reason. The price is that the finished plug is the drawn outline inset by the tool's diameter, which the chapter says plainly. |
| 15  | #449: safety. | The safety pass derives the band from the boundary instances in the project, as it does for #448, with dc-geometry and the shared `male_band` only, never a generator; so a boundary edited after export fails the old program (61r). No new S number. |
| 16  | Default `--min-detail`. | 0.2 (percent of the design's height): 0.4 mm at 200 mm tall, 0.1 mm at 50 mm. It is the level below which traced art's specks live: Inkscape's Trace Bitmap suppresses specks of 2 px by default, which is 0.2 % of a 1000 px picture, and a V-bit cannot meaningfully carve a piece whose widest point is a few tenths of a millimetre. Calibrated by slice 21f (#599): the sweep over 0.05, 0.1, 0.2, 0.3, 0.5 and 1.0 on the synthetic `speckled-dc.svg` keeps the smallest value at which the trace-noise specks (widest point under 0.2 % of the height) go and no intended detail (0.9 % and up, a serif tip, a counter) does. That is 0.2: 0.1 leaves 10 of 16 noise marks, and 1.0 removes two of six intended details. `tests/fixtures/prep/README.md` records the sweep and how to rerun it. The proposal stands. The maintainer's own art, which is not committed, is the check on the bench. |
| 17  | Default `--smooth`. | 0.05 (percent of the height): 0.1 mm at 200 mm tall. Traced edges jitter by about a pixel, 0.05 to 0.1 % of a 1000 px picture; this takes the staircase out and stays within a quarter of the speck threshold, so smoothing cannot remove what `--min-detail` keeps (`prep_smooth_exceeds_detail` warns above half). Chosen by the same sweep: on `jagged-dc.svg` 0.02 removes 54 % of the vertices (under the 80 % required), 0.05 removes 89 % and changes 0.08 % of the design's area, and 0.1 changes 0.12 %, over the 0.1 % allowed. The proposal stands. |
| 18  | When does the report suggest `--regions`? | When the ink's median width is under 2 % of the height (49c). The eagle is 0.7 %, a bold serif letter about 8 %, a plain stroke monogram 3 to 4 %. A suggestion only; the user decides. |
| 19  | "Relative to the design's height": a wide, short design (a banner 400 × 40 mm) makes every threshold a tenth of what its width would give. | Keep the height, as decided on 2026-11-01, and print it in the report so a surprise is visible. If the maintainer prefers, the longer side is a one-line change in the design (`design_scale_mm`), but it makes the report's "at 100, 200 and 300 mm tall" figures ambiguous for a wide design, which is why height is recommended. |
| 20  | Nested paths: a path inside another is a hole (as `init` reads it, 49c step 3), whatever the paint, or should paint order and colour decide (a white shape on a black one is a hole, a black one on black is solid)? | Nesting, as `init` does: the prepared file then means exactly what `init` will read, one rule in both places. Reading paint order and colour is the start of colour separation (deferred, see out of scope), and it needs a rule for every colour pair. `svg_hole_as_separate_path` tells the user when two filled paths nest, so the surprise is reported. |
| 21  | Page backgrounds. A white rectangle behind the art covers the page, so the art nests inside it and becomes holes, or, in `--regions`, makes the ink a solid. | The report flags a filled shape over 98 % of the drawing's box (`prep_background_shape`) and the chapter says to delete it. Do you want a `--skip <svg id>` option (repeatable) as well? Recommended yes, as a small explicit switch that needs no colour logic; left out of 49b until you say so, because the list of options you agreed on is shorter. |
| 22  | Line art with small gaps in its outline encloses nothing, so `--regions` finds no spaces there. | Not in 3.4: the report counts ink components that enclose no space (`prep_regions_open_ink`) and the chapter says to close the line in the drawing program. A `--close-gaps <pct>` that closes the ink by half of the gap before finding spaces is the natural follow-up; it changes the art more than anything here, so it waits for a real need. |
| 23  | Circles and arcs in the input come out as polygons (the pipeline's booleans are polygonal). Is that acceptable? | Yes for inlay work: V-Carve flattens its regions before the medial axis anyway, and the tolerance, 0.005 % of the height, is finer than the carve's own. It matters for a Drill on circles from prepared art, so the chapter says: drill holes come from the original drawing. Passing an untouched path's curves through to the output is possible and listed as out of scope for 3.4. |
| 24  | The synthetic `compass.svg` as the committed stand-in for the maintainer's eagle (test part 3). | Yes, as 49 and the test part say: the eagle is the motivating case and the bench check may use it, but nothing of it is committed. The compass has the same shape of problem: thin ink, many pockets, ridges of a known width. |
