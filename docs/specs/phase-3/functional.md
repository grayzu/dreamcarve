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
with a V-bit. Every program passes the phase 1 and 2 safety rules and the
new ones below.

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
tutorial: a star with a circle in it, walnut into maple. They make
`female.dcv` and `male.dcv` from the same SVG, each with one Inlay
operation and the same 60° V-bit, `depth_mm: 4`, `glue_gap_mm: 1`,
`plug_depth_mm: 2`; one says `side: female`, the other `side: male`.
`export` of the male says the design is mirrored. Alex cuts both, test
fits, glues, clamps, saws off the plug's backing and planes it flush. The
top shows no gap; a test piece cut in half shows the 1 mm glue gap under
the plug.
- **Alex, after a test fit** that bottoms out before the top closes,
raises `glue_gap_mm` to 1.5 and re-exports only the male, as Corbin's fit
procedure says: change one number, cut again.
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
phase 1's nine. Vocabulary is `CONTEXT.md`'s; this phase adds **Flat
Depth**, **Start Depth**, **Clearing**, **Inlay**, **Glue Gap**,
**Plug Depth**, **Photo V-Carve**, **Bitmap** and **Countersink**, defined
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
 one before warns `clearing_not_descending` (it still runs, and clears
 what it can); a pass that can reach nothing warns `clearing_no_room`.
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
 operation that cuts one **side** of a V-bit inlay of its targets: the
 **female**, a pocket in one board, or the **male**, a plug standing on
 another, so the plug's V-sloped walls seat in the pocket's. The same
 four values describe both sides, so the two projects are made from one
 design by changing `side`:
    - `depth_mm` (Corbin "Inlay Depth" `inlay_depth`; VCarve's female
     "Flat Depth"): how deep the female pocket is. Corbin: about 2.5 mm
     for a superficial inlay, 6 mm for a cutting board.
    - `glue_gap_mm` (Corbin "Inlay Fit Adjustment" `inlay_fit_adjustment`,
    "the glue gap under the plug"): the gap left under the seated plug.
    The male's start depth is `depth_mm − glue_gap_mm` (VCarve's male
    "Start Depth"). 0 is a plug that bottoms exactly; must be below
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
    the male's surround is carved. Used by the male only; the female
    ignores it.
     With `side: "female"` the operation is a V-Carve of its targets with
     `flat_depth_mm: depth_mm` and start depth 0, move for move. With
     `side: "male"` the targets are **mirrored** left to right in place,
     about the vertical centre line of their joint bounding box, because
     the plug is turned over onto the pocket; `export` and `check` say so
     once per male operation (`inlay_male_mirrored`). The male then
     V-carves the **surround**: the mirrored outlines offset outward by
     `margin_mm`, minus the mirrored design, so the band round the design
     and every hole in the design (a circle inside a star) is carved and
     the design stands. Its start plane is `depth_mm − glue_gap_mm` below
     the stock top and its flat depth `plug_depth_mm`, so the surround's
     floor is `depth_mm − glue_gap_mm + plug_depth_mm` deep. At the start
     plane the plug's walls pass exactly through the mirrored outline;
     above it the plug narrows by `z · tan(α/2)` at height `z`, which is
     the female wall's inset at depth `z`, so turned over, the plug's walls
     meet the pocket's walls along their whole height and the plug's top
     stops `glue_gap_mm` short of the floor. The tool is a V-bit, with the
     same checks as V-Carve, except that the cone-height check counts the
     flank's whole engagement from the stock top: `depth_mm` on the
     female, and `depth_mm − glue_gap_mm + plug_depth_mm` on the male,
     whose stock above its start plane is the plug and stays (changed
     after the security review: the draft counted the male's
     `plug_depth_mm` alone, and with the bench's 90° bit, cone height
     3.175 mm, Corbin's usual 2.5, 0.25 and 1.5 mm engage 3.75 mm). The
     chapter says both sides must use the same bit angle, which two files
     cannot check. `safe_z_mm` and
     `machining_order` are as V-Carve's, and each side is one pass (42); either side takes
     `clearing` passes (40). The male stock must leave a base under the
     surround's floor: less than 6 mm warns `inlay_plug_base_thin` (Corbin:
     "Plug Base Thickness" at least 6 mm, the backing that holds the plug
     together until the glue dries). Cutting the plug stock out of a larger
     board, and cutting the backing off after gluing, are the user's: a
     Profile on an outline they draw, and a saw.

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
 `inlay_male_mirrored`, `photo_lines_overlap`, `image_colour_converted`,
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
curved surfaces (Corbin's 3D mode). Inlays whose two sides are in one
project on one board; two files, or two instances the user places, cover
it. PhotoVCarve's "Selected Vectors" strategy and RasterCarve's
experimental dots mode. Picture editing: contrast, gamma, borders, cropping
and rotation are done in an image editor first, as Vectric's "Edit Picture"
does before the toolpath. Heightmap carving, where brightness is a surface
rather than a groove depth (phase 7, Surface operations). 16-bit and TIFF
pictures (read as 8-bit; TIFF in phase 7 with heightmaps). Canned cycles
for a countersink. Depth passes on a carve: one pass whatever the depth
(42). Fluting, texture and prism toolpaths (post-1.0).

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
 floor cutting at Z −2 lies only where the end mill could not reach
 (within 1.5875 mm of the floor boundary) and is under a third of the
 length it had in 50; the union of both runs' footprints at Z −2 still
 covers the floor within 0.05 mm. A second pass with a 1.5 mm end mill
 after the first clears only the band and corners the first left; the
 two passes the other way round warn `clearing_not_descending`. A pass
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
 with a 12 mm circle at its centre), `side: "female"`, 60° V-bit,
 `depth_mm: 4`: the toolpath equals a V-Carve of the same targets with
 `flat_depth_mm: 4`, move for move.
57. **Inlay male.** The same with `side: "male"`, `glue_gap_mm: 1`,
 `plug_depth_mm: 2`, `margin_mm: 8`: `check` prints
 `inlay_male_mirrored`; every cutting point lies in the band from the
 mirrored star out to 8 mm beyond it or inside the mirrored circle; no
 cutting point is below Z −5; the bit's flank passes through every
 mirrored outline point at Z −3 within 0.02 mm (the uncut cross-section
 at Z −3 is the mirrored design; at Z 0 it is the design inset by
 1.732 mm); for each sample point on the star's outline, the female
 wall's inset at depth `z` and the plug's inset at height `z` above
 its start plane agree within 0.02 mm for `z` from 0 to 3. A
 `glue_gap_mm` of 4 is `inlay_glue_gap_not_below_depth`; on 10 mm stock
 the male warns `inlay_plug_base_thin` (5 mm left) and on 12 mm it does
 not. With `clearing: [t2]` (a 3.175 mm end mill) the male's first run
 clears the surround's floor at Z −5 and its V-bit floor cutting shrinks
 as in 51.
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
- **S2, S6, S7 unchanged.** A start plane below the stock top changes no
rule: the descent to it is a plunge at the plunge rate. S6 reads a
groove's samples as the straight cuts they are. S7 holds a picture's
grooves, widened by the groove width at their depth, inside the stock.

## Test parts

Three parts, all cut on grblHAL. Record date, outcome and deviations
after each.

### Sign: raised oval, inset lettering

**Sign.** 200 × 80 mm rectangle, outside Profile with tabs; a 190 × 70 mm
rectangle 5 mm inside it V-carved as the background with a 150 × 46 mm
oval nested in it as an island, 2.5 mm flat depth, its floor cleared
by the 6.35 mm end mill as a clearing pass, so the oval stands raised with a 45° edge; the words
"DreamCarve" set on a curve inside the oval (a bold serif, 14 mm cap
height, converted to paths in Inkscape) V-carved inset with no flat depth;
two 3.175 mm hanging holes, countersunk 6 mm. All V-bits are the bench's
1/4 in (6.35 mm) set. Drawn as `fixtures/sign.svg`.


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
| op4 Drill (T3)   | the two circles, `countersink: { "diameter_mm": 6 }`                                                                                       |
| op5 Profile (T1) | outside, 12 mm deep, 4 tabs 8 × 3 mm, climb, `entry: ramp`, `lead` 3 mm, through-cut allowance 0.2 mm                                      |
| Safe Z           | 15 mm                                                                                                                                      |
| Controller       | grblHAL, one program per tool run (four: T2, T1 as the clearing pass, T3, T1)                                                                                          |


**Success:** the background floor is flat with no ridge where the end mills
and the V-bit meet; the oval's edge is a clean 45° slope from the floor
to the surface with no step; every serif and corner of the inset
lettering is cut to a sharp point at the surface, and the letters read
clearly; the countersinks are 6 ± 0.3 mm across; the sign releases after
the tabs are cut.

**Cut record:** not yet cut.

### Inlay pair

**Star inlay.** Corbin's Workshop's cutting-board method, 2D: a four-point
star 60 mm tip to tip with a 12 mm circle at its centre, drawn as
`fixtures/star.svg`; the female in one board, the male in another, from
the same file with `side` changed.


| Item         | Value                                                                                                |
| ------------ | ---------------------------------------------------------------------------------------------------- |
| Female board | walnut (or any hardwood), nominal 12 mm, 100 × 100 mm stock                                          |
| Male board   | maple (contrasting), nominal 12 mm, 100 × 100 mm stock                                               |
| T1           | 60° V-bit, 6.35 mm diameter (cone height 5.5 mm): 1000 mm/min, plunge 300, 18000 RPM, step-down 2 mm |
| T2           | 3.175 mm two-flute upcut end mill: 1000 mm/min, plunge 300, 18000 RPM, step-down 2 mm, stepover 40 % |
| Inlay        | `depth_mm: 4`, `glue_gap_mm: 1`, `plug_depth_mm: 2`, `margin_mm: 8`                                  |
| female.dcv   | op1 Inlay (T1) `side: female`, `clearing: [t2]` (two runs: T2, then T1)                                    |
| male.dcv     | op1 Inlay (T1) `side: male`, `clearing: [t2]` (two runs: T2, then T1)                                      |
| Safe Z       | 10 mm                                                                                                |
| Controller   | grblHAL, one program per tool run                                                                    |


**Procedure** (Corbin, "Inlay Fit Adjustment"): cut the female, then the
male. Test fit dry: the plug should seat with no gap at the top and stop
before its base touches the female surface. If a gap shows at the top, the
plug bottoms out: raise `glue_gap_mm` and recut the male. If the plug will
not enter, lower it. Glue, clamp the male's board face down on the female,
let it dry, saw off the plug's backing, plane flush.

**Success:** after planing, no visible gap round the star or the circle
(a 0.1 mm feeler gauge does not enter), the star's points are sharp, and
a second pair cut in half shows about 1 mm of glue gap under the plug and
the walls mated along their height.

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

- User Guide: V-Carve operation (medial lines, flat depth, clearing
passes, start depth, one pass, the offset strategy, choosing a bit angle);
Inlay (the method, the four values and what each changes, the two-file
workflow, the fit procedure, gluing and planing); Photo V-Carve (pictures
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
and Corbin names.

## Exit criteria

Delivered as sub-phases 3.1 V-Carve, 3.2 Countersink, 3.3 Inlay, 3.4
Photo V-Carve and 3.5 Release, each a GitHub milestone with its own
format version and bench check (design spec, "Sub-phases"); the tag
comes at 3.5.

- [ ] All acceptance tests and safety tests green on all three OSes.
- [ ] Golden parity cases captured and passing: the PureCutCNC V-carve

  cases and the RasterCarve photo cases.
- [ ] The three test parts cut on grblHAL and recorded above, the inlay

  pair fitted.
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
