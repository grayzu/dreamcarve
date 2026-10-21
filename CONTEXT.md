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
depth and feeds. The kinds are Profile, Pocket, Drill, V-Carve and Engrave,
with Surface Rough and Surface Finish from phase 7.
_Avoid_: Job, cut, pass (a pass is one depth step inside an operation)

**Profile**:
An operation that cuts along a closed shape's outline, inside, outside or on
the line, optionally leaving holding tabs.
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

**Pocket Allowance**:
Material a Pocket leaves on its walls and round its islands, `allowance_mm`
(0 or more), so a later, smaller tool can finish them. Not the Through-cut
Allowance, which is depth below the stock.
_Avoid_: Stock to leave, offset, finish allowance
_PureCutCNC_: stockToLeaveRadial

**Rest Machining**:
A Pocket that names an earlier Pocket of the same shapes in `rest_of` and
clears only what that Pocket's tool could not reach (corners and slots
narrower than it, and its Pocket Allowance), worked out from that Pocket's
tool and allowance, never from its toolpath.
_Avoid_: Rest pocket, cleanup pass, finishing pass, rest milling
_PureCutCNC_: rest regions (Pocket Finish after Pocket Rough)

**Stepover**:
The sideways distance between adjacent clearance passes, expressed as a
percentage of tool diameter.
_Avoid_: Step-over distance, overlap, pass width

**Drill**:
An operation that plunges a tool at one or more points, optionally pecking.
_Avoid_: Hole, bore
_PureCutCNC_: Drilling

**V-Carve**:
An operation that drives a V-bit along the medial lines of a shape so the cut
width follows the shape width.
_Avoid_: Carve, vcarve, chamfer
_PureCutCNC_: V-Carve

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
_Avoid_: Cut order, depth first, level first, optimisation
_PureCutCNC_: `machiningOrder`; `feature_first` is Full Depth First,
`level_first` is Pass First

**Link**:
The tool motion inside an operation between the end of one cut and the start
of the next: a retract to Safe Z, a rapid and a plunge, or a move that stays
down in material the operation has already cut.
_Avoid_: Transition, traverse, keep-tool-down

**Tab**:
A short bridge of material left uncut on a Profile so the part stays attached
to the stock.
_Avoid_: Bridge, holding tab, clamp (a clamp is a workholding obstacle)

**Tool**:
A cutter with geometry (diameter, type, angle, flutes) and feeds for a given
material. A project embeds copies of the tools it uses.
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
The height above the stock at which rapid moves between cuts travel.
_Avoid_: Clearance height; not the Retract Height, a Drill's lower plane (retract alone is the per-move lift)

**Retract Height**:
In a Drill, the height `retract_mm` above the stock top, capped at Safe Z,
where each hole's plunge starts and a full-retract peck lifts to. A
canned cycle's `R` plane.
_Avoid_: Retract plane, R plane, clearance
_PureCutCNC_: retractHeight

**Peck**:
One plunge of a Drill that stops short of the hole's depth, followed by a
lift to clear chips: to the Retract Height (full) or by a small gap (chip
break).
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
