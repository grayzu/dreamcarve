# DreamCarve

The domain language for DreamCarve, a CAD/CAM application for CNC hobbyists.
Where a term differs from the PureCutCNC reference implementation, the upstream
name is listed so porting work can map between them.

## Language

### Design

**Project**:
The single document a user works on: stock, features, tools, operations, and
machine setup. Saved in DreamCarve's own file format.
_Avoid_: Document, file, job, .camj

**Stock**:
The block of material the project is machined from. Defines the material
bottom below which no cut may go.
_Avoid_: Workpiece, material block, blank

**Feature Definition**:
The canonical, untransformed shape shared by one or more feature instances.
_Avoid_: Shape, definition, template
_PureCutCNC_: FeatureDefinition

**Feature Instance**:
A placed copy of a feature definition with its own position, rotation, depth,
visibility and lock state. Every row in the design tree is one.
_Avoid_: Copy, object, element
_PureCutCNC_: FeatureInstance

**Slot**:
A closed shape made of two parallel lines joined by semicircular ends,
defined by its two centre points and a width. A drawing primitive in its own
right, not a rounded rectangle.
_Avoid_: Obround, stadium, rounded rectangle, keyhole
_PureCutCNC_: `slotProfile`, creation shape `slot`

**Linked Copy**:
A second feature instance sharing the same feature definition, so editing the
shape updates both.
_Avoid_: Reference copy, clone, component

**Model**:
Imported 3D surface geometry that surface operations machine: a mesh (STL,
OBJ) or a heightmap. Positioned by a feature instance like any other shape.
_Avoid_: STL, mesh (when the source may be a heightmap), relief, 3D object
_PureCutCNC_: `stl` feature kind, "imported model"

**Heightmap**:
A model defined by a grayscale image: each pixel's brightness is a height
between a user-set top and bottom over a user-set footprint. Not a mesh.
_Avoid_: Depth map, grayscale relief, bump map, height field (that is the
simulation's internal grid)

### Machining

**Operation**:
A machining strategy applied to one or more feature instances, with a tool,
depth and feeds. The kinds are Profile, Pocket, Drill, Bore, V-Carve and Engrave,
with Surface Rough and Surface Finish from phase 7.
_Avoid_: Job, cut, pass (a pass is one depth step inside an operation)

**Profile**:
An operation that cuts along a closed shape's outline, inside, outside or on
the line, optionally leaving holding tabs. On the line it also cuts open
paths, and Left or Right it cuts beside them: one tool radius to the left
or right looking from the path's first vertex to its last.
_Avoid_: Contour, edge route, outline cut
_PureCutCNC_: Edge Route

**Pocket**:
An operation that clears all material inside a closed shape to a depth.
_Avoid_: Clearing, area clear
_PureCutCNC_: Pocket Rough, Pocket Finish

**Clearance Strategy**:
How a Pocket removes the material inside its boundary. **Offset** clears with
concentric passes stepping inward from the wall; **Raster** clears with
parallel straight passes at a user-set angle. Both finish with a wall pass
along the boundary.
_Avoid_: Pattern, fill, clearing mode, parallel (use Raster)
_PureCutCNC_: pocketPattern `offset`, `parallel` (with `pocketAngle`)

**Stock to Leave**:
Material a Pocket leaves on its walls and round its islands, or a Bore on
its hole's wall, `stock_to_leave_mm` (0 or more), so a later operation,
often with a smaller tool, can finish them. Fusion's "Stock to Leave"
(radial); VCarve's "Pocket Allowance". Not the Through-cut Allowance,
which is depth below the stock, nor the Last Pass Stock to Leave, which the
same Profile takes off. (changed: maintainer decision 2026-10-25, #394:
was Pocket Allowance, `allowance_mm`)
_Avoid_: Allowance (the Through-cut Allowance is the only allowance),
Pocket Allowance, offset
_PureCutCNC_: stockToLeaveRadial

**Last Pass Stock to Leave**:
Material every pass of a Profile but the last leaves on the wall,
`last_pass.stock_to_leave_mm` (more than 0, at most a third of the bit), so
the last pass, on the part's path at full depth, takes an even skin off the
whole wall and leaves no step marks. The passes before it run on the
**stock path**, the part's path that much farther into the waste.
VCarve's "Last Pass Allowance"; Fusion's radial stock to leave with a
finishing pass. (changed: maintainer decision 2026-10-25, #394: was Last
Pass Allowance, `last_pass.allowance_mm`, and the allowance path)
_Avoid_: Last Pass Allowance, allowance path, finishing pass (it is the
same operation's last pass), Stock to Leave (a Pocket's or a Bore's, left
for another operation)
_PureCutCNC_: none (a separate operation with stockToLeaveRadial)

**Corner Relief**:
A short out-and-back notch a Pocket or Profile cuts into each inside corner
its round tool cannot reach, so a square part fits: a **dogbone** along the
corner's bisector, or a **T-bone** along its longer edge (`corner_relief`).
The pass that cuts the final wall cuts the notches as it goes round, at
every level; a separate notch pass cuts only what that pass cannot reach,
such as a Pocket's corners when it leaves Stock to Leave. VCarve's
"Create Fillets".
_Avoid_: Fillet (a fillet is the rounded corner relief removes), overcut
_PureCutCNC_: cornerRelief

**Rest Machining**:
A Pocket that names an earlier Pocket of the same shapes in `rest_of` and
clears only what that Pocket's tool could not reach (corners and slots
narrower than it, and its Stock to Leave), worked out from that Pocket's
tool and stock to leave, never from its toolpath.
_Avoid_: Rest pocket, cleanup pass, finishing pass, rest milling
_PureCutCNC_: rest regions (Pocket Finish after Pocket Rough)

**Stepover**:
The sideways distance between adjacent clearance passes, expressed as a
percentage of tool diameter.
_Avoid_: Step-over distance, overlap, pass width

**Drill**:
An operation that plunges a tool at one or more points, optionally pecking,
or countersinks them.
It skips a circle smaller than the tool, unless its
`allow_tool_wider_than_hole` is set, since the hole is always the tool's size.
A hole bigger than the bit is a Bore's.
_Avoid_: Hole, bore (a Bore is its own operation)
_PureCutCNC_: Drilling

**Bore**:
An operation that cuts each circular shape bigger than its end mill to the
drawn size: the bit spirals down a helix inside the hole, `pitch_mm` per
turn, to the depth, then cuts one full circle there so the wall is round
and to size. It skips a hole the bit has no room to spiral in, and one
wider than twice the bit, which would leave a core standing (a Pocket
clears that). Fusion's "2D Bore". (changed: maintainer decision 2026-10-24)
_Avoid_: Helical drill, helix (a helix is also an Entry), counterbore, ream
_PureCutCNC_: Drilling, `helical` drill type

**Countersink**:
A Drill cycle (format 21) that opens a conical seat with a V-bit at each
circle: one plunge at the centre to the depth that makes the cone
`diameter_mm` wide at the hole's top, `start_depth_mm` below the stock
top. The depth is derived from the bit's angle, never set, and it is
always written as plain moves. A circle at least as wide as the mouth is
skipped.
_Avoid_: Chamfer, counterbore (a flat-bottomed step, a Bore or Pocket)
_PureCutCNC_: Drilling, `countersink` drill type

**V-Carve**:
An operation that drives a V-bit along the medial lines of a shape so the cut
width follows the shape width.
_Avoid_: Carve, vcarve, chamfer
_PureCutCNC_: V-Carve

**Inlay**:
An operation (format 22) that cuts one **side** of a V-bit inlay of a design:
the **female**, a flat-bottomed V-carved pocket `depth_mm` deep, or the
**male**, a plug of the design mirrored in place, left standing by carving
the band round it. The plug is turned over and glued into the pocket, its
V-sloped walls seating in the pocket's, and the excess planed off. One design
and the same values make both sides; only `side` changes.
_Avoid_: Insert, female/male V-Carve
_PureCutCNC_: none (Corbin's Workshop's method)

**Glue Gap**:
The gap an Inlay leaves under its seated plug, `glue_gap_mm`, 0 or more and
below `depth_mm` (Corbin's "Inlay Fit Adjustment"). The male's start plane is
`depth_mm − glue_gap_mm` below the stock top.
_Avoid_: Fit adjustment, clearance

**Plug Depth**:
How far below the male's start plane its surround is cut flat,
`plug_depth_mm`, so the plug's base clears the pocket's surface when the walls
mate. The male's flank engages `depth_mm − glue_gap_mm + plug_depth_mm` from
the stock top, which must fit the V-bit's cone.
_Avoid_: Plug height

**Engrave**:
An operation that follows an open or closed path with the tool centre on the
line at a fixed depth. Engraving whose width follows the shape with a V-bit is
V-Carve, not Engrave.
_Avoid_: Follow line, trace, scribe
_PureCutCNC_: Follow Line, Carving

**Toolpath**:
The ordered tool motion an operation produces, in project coordinates, before
any post-processor sees it.
_Avoid_: Path, moves, program

**Machining Order**:
Within one operation, whether each closed outline (with its holes) or open
path is cut to full depth before the next (**Full Depth First**), or each pass
is cut across all of them before the next, deeper pass (**Pass First**). It
changes the order of the cuts, never which cuts are made.
_Avoid_: Cut order (for this setting; the cut order is the whole order a
program cuts in: the operations in list order, and within each the
Machining Order, nearest neighbour and containment rules), depth first,
level first, optimisation
_PureCutCNC_: `machiningOrder`; `feature_first` is Full Depth First,
`level_first` is Pass First

**Link**:
The tool motion inside an operation between the end of one cut and the start
of the next: a retract to Safe Z (to the Clearance Height between parts),
a rapid and a plunge, or a move that stays down in material the operation
has already cut.
_Avoid_: Transition, traverse, keep-tool-down

**Entry**:
How a Profile or Pocket takes the tool down to each pass depth (`entry`): a
**plunge** straight down at the plunge rate, a **ramp** zigzagging down at
`angle_deg`, or a **helix** spiralling down at `angle_deg`, always in
material the operation removes. A helix that does not fit falls back to a
ramp, and a ramp to a plunge, with a warning. VCarve's "Ramps", Fusion's
"Ramp Type".
_Avoid_: Lead-in (that is the Lead), approach, plunge strategy, ramp-in
_PureCutCNC_: entryStrategy

**Lead**:
An arc tangent to a Profile's path at the start of each pass (lead-in) and,
with `lead_out`, at its end (lead-out), so the tool takes up and leaves the
cut while moving instead of stopping against the wall (`lead`). The Entry
descends before the lead-in starts. VCarve's "Leads", Fusion's "Lead-In"
and "Lead-Out".
_Avoid_: Entry (that is the descent), approach, lead line (straight leads
are out of scope)
_PureCutCNC_: xyLead

**Start Point**:
Where a Profile's cut starts on each closed outline (`start`): `auto`, or
the point of the bit's path nearest the outline's first vertex as drawn
(`first_vertex`) or nearest a project point (`near`). An explicit start
point wins: a Tab Request's distances count from it, a Lead arrives there
or is dropped, and an Entry with no room there falls back rather than
moving it. VCarve's "Use vector start point" and "Start At"; Fusion's
"Entry Positions".
_Avoid_: Seam (the internal point a path is re-seamed at), entry point
(the Entry ends there, it is not the Entry)
_PureCutCNC_: none

**Tab**:
A short bridge of material left uncut on a Profile so the part stays attached
to the stock.
_Avoid_: Bridge, holding tab, clamp (a clamp is a workholding obstacle)

**Placed Tab**:
A tab as the project holds it: on one outline of one feature instance, at a
point in its definition's space, with its own width, height and shape. Every
enabled Profile that cuts that outline rises over it (ADR-0005). A placed
tab on a hole that the user asked for (`--on-holes`, a request's
`on_holes`) **holds its slug**, the waste the hole would free, and records
it in `holds_slug`, so `tab_on_hole` is not repeated for it (#302).
_Avoid_: Tab footprint, tab rectangle (PureCutCNC's world-space form)
_PureCutCNC_: Tab

**Tab Request**:
A Profile's tab setting (count or distances, size, shape, holes or not):
tabs the operation asks for, which are placed on its targets' outlines
with the layout rules before anything is cut, or written as placed tabs by
`dreamcarve tabs`. It is not cut data. With `avoid_corners` each tab it
lays on a **corner**, where the bit's path turns 30° or more within one
bit diameter of the tab's stretch, slides off it.
_Avoid_: Tab spec, tab setting (in prose), auto tabs

**Tool**:
A cutter with geometry (diameter, type, angle, flutes) and feeds for a given
material, optionally with its vendor and the vendor's product id. A project
embeds copies of the tools it uses.
_Avoid_: Bit, cutter, end mill (that is one tool type)

**Tool Library**:
The user's app-level collection of tools, kept outside any project, from which
tools are picked and copied into a project. Can be populated by importing a
manufacturer or third-party tool database.
_Avoid_: Tool database, tool catalogue, tool list

### Output

**Post-processor**:
The description of how a controller wants its G-code written: header, footer,
comment style, arc support, tool-change and coolant commands. DreamCarve
bundles versioned definitions of them; a project names one by id and
definition version and holds only the user's post-processor settings
(ADR-0004).
_Avoid_: Machine definition, post, driver
_PureCutCNC_: MachineDefinition

**Machine Origin**:
The explicit transform from project coordinates to the controller's
coordinates, including where XY zero and Z zero sit on the stock.
_Avoid_: Datum, zero, offset

**Through-cut Allowance**:
The explicit, per-operation distance a cut may go below the stock bottom, so
a part can be freed into a spoilboard. Zero unless the user sets it.
_Avoid_: Overcut, extra depth, cut-through

**Safe Z**:
An operation's height above the stock (`safe_z_mm`) at which rapid moves
between the cuts of one part travel. Without a Clearance Height, moves
between parts and operations travel at it too. Fusion calls it the
retract height.
_Avoid_: not the Clearance Height; not the Retract Height, a Drill's lower plane (retract alone is the per-move lift)

**Clearance Height**:
The project's height above the stock (`heights.clearance_mm`) for moves
between parts, between operations, at a tool change and at the end; at
least every enabled operation's Safe Z (#283). VCarve's Clearance gap (Z1).
_Avoid_: Safe height, rapid height

**Tool-change Machine Z**:
A height in machine coordinates (`heights.tool_change_machine_z_mm`), below
0, that the program lifts to with `G53 G0 Z` before each tool-change stop
and at the end, on grblHAL, GRBL (#283) and LinuxCNC (#306). Needs a homed machine.
_Avoid_: Home Z, tool-change height (that is a sender's setting)

**Park Point**:
Where the program ends in XY (`heights.park`), reached at the Clearance
Height before the Tool-change Machine Z lift (#283). VCarve's Home
position without its Z.
_Avoid_: Home position (VCarve's has a Z), end point

**Retract Height**:
In a Drill, the height `retract_mm` above the stock top, capped at Safe Z,
where each hole's plunge starts and a full-retract peck lifts to. A
canned cycle's `R` plane.
_Avoid_: Retract plane, R plane, clearance
_PureCutCNC_: retractHeight

**Peck**:
One plunge of a Drill that stops short of the hole's depth, followed by a
lift to clear chips: to the Retract Height (full) or by a small gap (chip
break). Peck depths count from the Retract Height, as a canned cycle's
from `R`: peck k ends `k × peck_depth_mm` below it, or at the hole's depth.
_Avoid_: Step, pass (a pass is a depth level of a cutting operation)

**Dwell**:
A timed pause with no motion: at the bottom of a Drill plunge (`dwell_s`),
or while the spindle reaches speed. Written as `G4`.
_Avoid_: Delay, wait

**Canned Cycle**:
A controller's one-line drilling cycle (`G81`, `G82`, `G83`, `G73`) that
stands for a whole hole's moves. Written for a Drill only when the project
sets `use_canned_cycles` and the controller's motion for it is the Drill's
toolpath; otherwise the hole is written as plain moves (Expanded).
_Avoid_: Drill cycle (that is the Drill's `cycle` setting), fixed cycle
_PureCutCNC_: cannedCycles
