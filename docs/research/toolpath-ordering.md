# Toolpath ordering: research and proposal

Status: research note. The decisions taken from it are in section 7 and in
`docs/specs/phase-2/NOTES.md`; the specs are the authority. Terms follow
`CONTEXT.md`.

## The question

> Many CAM packages do some optimisation when turning toolpaths into G-code.
> For example, do we cut each part to full depth, or cut the same depth on
> each part before moving down to the next layer? Should DreamCarve offer an
> option letting users choose the optimisation type if there are several?

Short answer: yes, offer the depth-first / level-first choice per operation in
phase 2, as PureCutCNC does. Everything else that changes cut order should be
either a fixed safety rule (inside before outside) or a deterministic
optimisation with no setting (nearest-neighbour ordering). The order of
operations stays the user's. The post-processor never reorders motion.
Section 1 has one safety defect in phase 1 that should be fixed before
`v0.1.0`.

## Contents

1. What DreamCarve does today
2. What PureCutCNC does
3. What other CAM packages expose
4. Trade-offs on a hobby router
5. Proposal
6. Safety invariants and tests
7. Decisions

---

## 1. What DreamCarve does today

### One operation (`dc-cam`)

`crates/dc-cam/src/profile.rs`, `generate_profile`:

- **Region order is drawing order.** `dc_geometry::nest`
  (`crates/dc-geometry/src/region.rs`) returns regions ordered by the input
  position of their outer contour, and holes by their own input position. The
  user guide states this as rule 6, "Order follows the drawing"
  (`docs/book/src/user-guide/shapes-and-holes.md`). There is no
  nearest-neighbour step.
- **Holes before outer, within a region.** The loop chains
  `region.holes()` then `region.outer()` ("Holes first, so a through-cut part
  is freed only by its last contour"). Documented in
  `docs/book/src/user-guide/profile.md`, "What the program does".
- **Depth-first per contour.** `emit_contour` rapids to the contour start at
  Safe Z, then cuts every pass of that contour (plunge straight down at the
  plunge rate, one lap at the feed rate, ending at the start), then retracts
  to Safe Z. Between passes of one contour the tool stays down: the lap ends
  where the next plunge begins. This is PureCutCNC's `feature_first` at the
  contour level, which is also its default for new operations (section 2).
- **Start point** is the first vertex of the tool-centre contour as the offset
  returns it. No seam choice.
- **Direction** is the same on every pass (`traversal_is_ccw`). No
  alternation.
- **Linking** is always retract to Safe Z, rapid, plunge. There is no
  stay-down link between contours and no lower retract height.

The golden tests (`crates/dc-gcode/tests/golden.rs`, e.g.
`tests/golden/profile-ring/README.md`) compare each Z level as a set of moves,
"so the order of the contours is not checked". Reordering contours does not
break parity, and parity cannot catch an ordering bug either.

Warnings name contours by index "in cut order"
(`WarningCode::TabsOverlap`, `TabsCoverMostOfContour` in
`crates/dc-cam/src/warning.rs`). Any reordering changes those params.

### Defect: an island is cut after the hole around it

"Holes before outer" is applied only inside one region. A part inside a
hole of another part (an island, nest depth 2) is its own region, ordered by
where its outline appears in the drawing. Take a frame (outline A, hole B)
with a coaster C drawn inside the hole, in that order. The cut order is B, A,
C. Cutting B through frees the slug of waste inside the hole, and C is then
cut out of a loose slug (or one held only by B's tabs). If C was drawn first
the order is C, B, A, which is right. So the output's safety depends on the
drawing order.

The rule that fixes it: a contour is cut before every contour that encloses
it (cut by nest depth, deepest first, or as a post-order walk of the
containment tree). This belongs in phase 1, since the coaster-in-frame case is
reachable with one Profile today. It needs a red test first
(`profile::island_is_cut_before_the_hole_around_it`) and a sentence in
`profile.md`. See the invariants in section 6.

### Several operations (`dc-gcode`)

`crates/dc-gcode/src/emit.rs`, `emit`: enabled operations are written in the
order of `project.operations`. A tool change is written wherever two
consecutive operations use different tools; there is no grouping by tool. The
CLI passes operations in project order (`crates/dreamcarve/src/export.rs`).

Safety rule S6 (`docs/specs/phase-1/functional.md`) already makes the
post-processor order-preserving by construction. The safety pass reads each
operation's G-code back and checks that it is the operation's toolpath "move
for move". A post-processor that reordered motion would fail its own export.

Issue #73 ("Workflow gaps from the CNC review") lists "**Operation order.**
Not checked: an outer ring cut before the hole inside it reports no warning",
and asks for separate heights (a clearance height above Safe Z, a tool-change
height, a park position).

---

## 2. What PureCutCNC does

Local checkout `/Users/markg/git/purecutcnc` (`../purecutcnc`).

### Machining order: `feature_first` (new operations) or `level_first`

- Type: `MachiningOrder = 'level_first' | 'feature_first'`, the
  per-operation field `machiningOrder?` (`src/types/project.ts`). New
  operations are created with `feature_first`
  (`src/store/helpers/operationDefaults.ts`). A missing value means
  `level_first` (`normalize.ts`, `isFeatureFirst` in
  `src/engine/toolpaths/multiFeature.ts`), only so projects saved before the
  field existed keep their old behaviour: commit `faa7314f` says "Feature
  first (default for new ops) ... Projects saved before this field existed
  load as level first to preserve their prior behaviour."
- Why level first stays available (`planning/archive/MACHINING_ORDER_Implementation_Plan.md`):
  close features, where it limits deflection on a stiff setup, and
  overlapping or close targets, which it merges into one region per level.
  `planning/TROCHOIDAL_EDGE_DESIGN.md` notes that closely spaced parts "can
  skip spans (or fail) under `feature_first` where `level_first` succeeds".
- Which kinds offer it: `CLEARING_CONTROL_SUPPORT` in
  `src/engine/toolpaths/clearingControls.ts` (pocket, surface clean, rough
  surface; edge routes share it, per the file's header comment). V-carve also
  honours it, except with closed-line targets (`isFeatureFirst`).
- **`feature_first`**: `perFeatureOperations` splits the operation into one
  sub-operation per target feature, generates each alone, then
  `mergeToolpathResults(..., { orderBlocks: 'nearest' })` orders the blocks
  by greedy nearest neighbour (`orderPartsByNearestBlock`,
  `greedyNearestNeighbor` in `src/engine/toolpaths/geometry.ts`, ties broken by
  original index). Used by `generateEdgeRouteToolpath`
  (`src/engine/toolpaths/edge.ts`), `pocket.ts`, `vcarve.ts`, `surface.ts`,
  `roughSurface.ts`.
- **`level_first`**: one depth level across every target before the next.
  For an outside edge route with several targets, `edgeLevelRuns`
  (`src/engine/toolpaths/edgeLevelRuns.ts`) groups the levels into runs that
  cut the same targets, and each run **unions** the targets into one outline
  (`unionPaths` in `edge.ts`), so close parts whose tool paths overlap are
  routed around together rather than gouging each other.
- Tests: `src/engine/toolpaths/toolpaths.test.ts`
  (`testPocketLevelFirstOrder`: Z transitions `[-2, -4]`;
  `testPocketFeatureFirstOrder`: `[-2, -4, -2, -4]`;
  `testPocketFeatureFirstNearestBlockOrder`: targets listed a, c, b are cut
  a, b, c).
- Design note: `planning/TROCHOIDAL_EDGE_DESIGN.md`, "Machining order":
  `feature_first` treats the other targets as obstacles, so "closely spaced
  parts can therefore skip spans (or fail) under `feature_first` where
  `level_first` succeeds ... That is the honest consequence of the ordering
  the user picked."

### Order inside one level, and start points

- Inside routes and pocket walls: `cutClosedContours` →
  `orderClosedContoursGreedy` (`src/engine/toolpaths/pocket.ts`) orders the
  closed contours by greedy nearest neighbour from the current position and
  rotates each loop to start at its vertex nearest the tool
  (`rotateContourToNearestEntry`). That is start-point selection.
- Outside routes: `appendFragmentedContoursAtLevels` (`edge.ts`) cuts the
  contours of a level in the order the geometry returns them.
- Drilling: `sortTargetsByNearestNeighbor`
  (`src/engine/toolpaths/drilling.ts`), from the current position or the
  origin.
- Follow line (DreamCarve's Engrave): `generateFollowLineToolpath`
  (`src/engine/toolpaths/carving.ts`) cuts each fragment at every level
  before the next, retracting to Safe Z between levels. No direction
  alternation.

### Linking

- `transitionToCutEntry` (`pocket.ts`): same XY → plunge straight down (stay
  down between levels of one contour). From Safe Z → rapid then plunge. A
  move of at most `maxLinkDistance` → a cut move at depth (stay down),
  optionally gated by a `safeLinkCheck`. Anything else → `retractToSafe`,
  rapid, plunge.
- Edge routes set `maxLinkDistance = tool.diameter` (`edge.ts`) and pass no
  safe-link check. There is no user-facing "keep tool down" or lower retract
  height. Phase 2's pocket links (raster zigzag inside the region,
  `docs/specs/phase-2/NOTES.md`) are this mechanism.

### Inside before outside, and operations

- PureCutCNC has no automatic inside-before-outside within an operation in
  the DreamCarve sense: an outside route only follows `add` features and an
  inside route only `subtract` ones, so a part with a hole takes two
  operations (see `tests/golden/profile-ring/README.md`).
- Operations are cut in project order. Export generates in parallel but
  "the assembled rows below are put back into project order regardless,
  because the order operations are posted in is the order they will be cut"
  (`prepareExport`, `src/app/toolpathGeneration/exportPreparation.ts`).
  `runPostProcessor` (`src/engine/gcode/postprocessor.ts`) emits a tool
  change whenever the tool changes. No grouping by tool.
- CAM Plan (`src/engine/operations/camPlan/createCamPlan.ts`) proposes outside
  edge routes that depend on every internal operation (`draft.dependencies =
  [...internalKeys]`), and groups outside routes by tool
  (`nextOutsideEdgeToolGroup`). So PureCutCNC encodes "outside last" as a
  planning recommendation, not a generator rule. CAM Plan is DreamCarve
  phase 8.

---

## 3. What other CAM packages expose

All of this comes from published user documentation and official forums.
FreeCAD (LGPL) and Kiri:Moto (MIT) source was read only to describe
behaviour; none of it is copied. No proprietary program or its output was
inspected. "Unverified" means the docs I could reach did not say.

### Per package

- **Vectric VCarve / Aspire** (Profile toolpath help,
  <https://docs.vectric.com/docs/V12.5/Aspire/ENU/Help/form/uiProfileMachineForm/index.html>).
  Inside before outside is always on, with no setting: "the program will
  always cut the inner vectors before the outer vectors to ensure the part
  remains attached to the original material as long as possible ...
  regardless of the selection order". On the Order tab the user ticks one or
  more of Vector Selection Order, Left to Right, Bottom to Top and Grid; with
  several ticked "the program will calculate the result of using each option
  and select the one which results in the shortest machining time". The
  start point is chosen automatically to shorten link moves unless "Use
  Start Point" is ticked. Other settings: separate last pass (with an
  optional reversed direction), "Maintain exact tool pass depth", ramps "at
  the start of each level", Climb or Conventional. Merged toolpaths with
  "Merge By Part":
  <https://docs.vectric.com/docs/V12.0/VCarvePro/ENU/Help/form/Create%20Merged%20Toolpath>.
  Depth-first vs level-first within one profile: unverified.
- **Fusion, 2D Contour**
  (<https://help.autodesk.com/view/fusion360/ENU/?contextId=MFG-REF-2D-CONTOUR-CMD>).
  "Order by Depth" (level first) and "Order by Islands" (by profile) are both
  user settings. With "Preserve Order" off (the default), "Fusion optimizes
  the cut order". "Keep Tool Down" with "Maximum Stay-Down Distance"
  (<https://help.autodesk.com/cloudhelp/ENU/Fusion-CAM/files/GUID2E5ABA80-7240-4F1A-989C-2410C4A0CCE5.htm>).
  "Both Ways" alternates direction, for open profiles only. Lead-in, ramp
  and tabs. "Reorder to Minimize Tool Changes" at post time (forum only).
  Defaults for the order settings: unverified.
- **Carbide Create** (staff answer on the official forum,
  <https://community.carbide3d.com/t/order-of-operations/33604>). Within a
  toolpath the order is optimised automatically "for min. travel/time", with
  no control. Toolpaths are cut in list order.
- **Estlcam.** Each toolpath has a machining-order number. Positive numbers
  are cut first in ascending order, then "Auto" ones in an order Estlcam
  chooses, then negative ones. Holes default to 10 and parts to 20, so holes
  usually come first
  (<https://forum.v1e.com/t/carve-machining-order-frustrations/13578>, and a
  university guide:
  <https://kunst2.tuwien.ac.at/files/modellbau/PDFs/240925_AnleitungESTLCAMenglisch.pdf>).
- **CamBam** (<http://www.cambam.info/doc/1.0/cam/Profile.html>). "Cut
  Ordering" per operation: "Controls whether to cut to depth first or all
  cuts on this level first" (Depth First or Level First). "Optimisation
  Mode" is New, Legacy, or None ("written in the order they were
  generated"). Start Point. "Max Crossover Distance": past it, "a retract,
  rapid and plunge ... via the clearance plane, is inserted". Milling
  direction Conventional, Climb or Mixed. Final depth increment.
- **FreeCAD CAM** (source: `src/Mod/CAM/Path/Main/Job.py`,
  `Path/Op/Profile.py`, `Path/Op/Area.py`; docs:
  <https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/CAM_Profile.md>).
  The Job's "Order Output By" is Fixture (default), Tool or Operation;
  ordering by Tool groups operations to cut tool changes. Profile
  "SortingMode" is Automatic (nearest neighbour) or Manual (selection
  order). "RetractThreshold" skips retracts for short moves. Start point is
  set with "UseStartPoint" or "UseLongestEdge". Roughing is level first
  ("all roughing offsets at one height before stepping down"), with
  finishing passes last. No inside-first rule.
- **Kiri:Moto** (`src/kiri/app/conf/defaults.js`, `web/kiri/lang/en.js`,
  <https://docs.v1e.com/software/kiri/>). "Depth first" ("optimize pocket
  cuts with depth priority") defaults on. "Inner first" ("always cut inner
  to outer") defaults off. Moves shorter than about one tool diameter become
  cutting moves; "force z max" (off by default) sends every move between
  pockets to the top.
- **Easel** (Inventables on the official forum,
  <https://forum.easel.com/t/depth-first-carving-dev-inventables/20328>).
  Depth first per object, automatic: "All shapes that intersect each other
  are grouped as an object and carved at all layers before processing the
  next object." The user controls neither the order nor the start point.
- **LightBurn** (laser, but the same problem,
  <https://docs.lightburnsoftware.com/2.0/Reference/OptimizationSettings/>).
  Order by Layer, Group or Priority (default Layer, then Priority). On by
  default: "Cut inner shapes first", "Reduce travel moves", "Choose best
  starting point", "Choose best direction". Each one can be turned off.
- **SheetCAM** (forums only). With an outside offset the cutouts are cut
  before the outline. Job options offer "Minimise tool changes" or "Manual
  optimisation (keep parts together)", and "Quick cut sequence" lets the user
  pick the order and start points by clicking
  (<https://plasmaspider.com/viewtopic.php?p=82327>).
- **MeshCAM** (forum): a "Depth First" option on waterline finishing
  (<https://community.carbide3d.com/t/are-meshcam-tool-paths-always-quite-insane/1366>).
- **Inkscape Gcodetools** (from its extension UI definition): "Cutting order"
  is Subpath by subpath (default), Path by path, or Pass by Pass. "Sort paths
  to reduce rapid distance" defaults on.

### By dimension

| Dimension | Who exposes it, and the default |
| --- | --- |
| Depth first vs level first | A per-operation choice in Fusion, CamBam, Kiri:Moto (default depth first), Gcodetools (default per subpath, i.e. depth first) and PureCutCNC (default feature first for new operations; level first for files that predate the setting). Fixed in Easel (depth first) and FreeCAD Profile (level first). Unverified for Vectric and Carbide Create. **No consensus on the default.** |
| Inside before outside | Fixed rule in Vectric and Easel. On by default with a toggle in LightBurn. Off by default in Kiri:Moto. Follows the offset side in SheetCAM. Via default priorities in Estlcam. Not documented in Fusion, CamBam or FreeCAD. |
| Travel optimisation (nearest neighbour, etc.) | Automatic in Carbide Create. Default on in Fusion, FreeCAD, CamBam, LightBurn and Gcodetools, each with a way back to selection order. Vectric picks the fastest of the orders the user ticks. |
| User / selection order | Vectric, Fusion ("Preserve Order"), FreeCAD (Manual), CamBam (None), Estlcam (numbers), LightBurn (Priority). |
| Operation order | The user's list everywhere. Optional grouping by tool in FreeCAD (default off) and SheetCAM, and in Fusion at post time. |
| Stay-down links | Fusion "Keep Tool Down" with a distance. CamBam "Max Crossover Distance". FreeCAD "RetractThreshold". Automatic under about one tool diameter in Kiri:Moto and PureCutCNC. |
| Start point | Automatic shortest-link with a manual override in Vectric, CamBam, FreeCAD, LightBurn and SheetCAM. None in Easel or Carbide Create. |
| Direction alternation | Only for open profiles (Fusion "Both Ways"). CamBam "Mixed". Vectric can reverse the last pass. |

Three patterns stand out. **Inside before outside** is the usual fixed or
default-on rule, and the packages that make it fixed (Vectric, Easel) are
the ones aimed at hobbyists. **Depth first vs level first** is a
per-operation choice wherever it is exposed, but packages disagree on the
default. **Travel optimisation** is on by default, with an escape hatch to
selection order in most packages.

---

## 4. Trade-offs on a hobby router

| Concern | Depth-first (full depth first) | Level-first (pass first) |
| --- | --- | --- |
| Rapid and Z time | Fewest retracts: one per shape. Hobby Z axes are slow, so this is usually the faster program. | One retract per shape per pass. With 3 passes and 20 parts that is 60 retract-rapid-plunge cycles against 20. |
| When parts are freed | One part at a time, at the end of its outline. Kerfs open through early. | Every part stays attached until the final pass. All kerfs open through in the last level. |
| Hold-down | Each through-kerf weakens a vacuum table or onion-skin hold for the parts still to cut. With screws and clamps the difference is small. | Better for vacuum and for thin sheet: the sheet stays whole until the end. |
| Tabs | Same tab moves either way: tabs only change passes below the tab top. | Same. Tabs matter more under level-first only in that every part is "on its tabs" for the whole final level. |
| Heat and chips | The tool stays in one kerf for several passes. Plastics (acrylic, HDPE) can melt and weld chips; a deep narrow kerf packs chips. | Each kerf cools while the others are cut. Chips from the previous level are still in the kerf either way. |
| Thin parts and flex | A small part cut to full depth early can flex or chatter on the later passes of a neighbour if its tabs are thin. | The sheet stays stiff longer. |
| Tool deflection | Same per pass. | Same per pass. |
| Close parts | Each shape alone: tool paths of close parts can overlap and gouge the neighbour unless the generator checks (DreamCarve has no such check today). | PureCutCNC unions same-level outlines, which is what protects close parts. |
| Failure mid-job | If the job stops (broken bit, lost steps), finished parts are complete. | Nothing is finished until the end. |

Other points:

- **Rapid moves at reduced heights.** A retract lower than Safe Z saves Z
  time but crosses uncut stock, clamps and tabs at low clearance. DreamCarve
  has one height, and S2 requires every rapid at or above Safe Z. Lowering
  any rapid needs a proof that the path is clear (only above floor this
  operation has already cut), which needs the material model of phase 6.
- **Determinism.** Any optimiser must be a pure function of its inputs
  (`docs/rust-guidelines.md`, "Floating point and determinism"): greedy
  nearest neighbour with distances compared by `f64::total_cmp` and ties
  broken by input index, no `HashMap` order, no randomness, no time-boxed
  search. Two runs on two machines produce identical G-code. Stochastic TSP
  solvers (simulated annealing, 2-opt with random restarts) are out unless
  seeded and iteration-bounded, and buy little for hobby part counts.
- **Through-cut Allowance** decides which pass frees material: only the final
  pass, and only when depth reaches the stock bottom. Ordering rules about
  "freeing" apply to the final pass of a contour whose depth reaches the
  stock bottom, and with tabs to the tab-level passes too.
- **Direction alternation** between passes flips climb and conventional on a
  closed profile. It saves nothing on a closed loop (the lap ends at its
  start) and spoils the wall finish. It only helps open paths (Engrave),
  where it removes a rapid back to the start each pass.

---

## 5. Proposal

### 5.1 Dimensions, ranked by user value

| Rank | Dimension | Kind | Phase |
| --- | --- | --- | --- |
| 1 | Containment order: a shape is cut before anything enclosing it | Safety rule, no setting | **1** (fix the island defect) |
| 2 | Operation order is the user's; warn when an operation frees a part before another cuts inside it | User list + `check` warning | 2 |
| 3 | Machining order: full depth first or pass first | Per-operation setting | 2 |
| 4 | Nearest-neighbour order of shapes, drill points and pocket regions | Automatic, deterministic, no setting | 2 |
| 5 | Start point (seam) on closed contours | Automatic with lead-in/out; manual later | 2 (automatic), 4+ (manual) |
| 6 | Stay-down links: between passes of one contour (done) and short at-depth links inside a cleared area (pocket rings, raster zigzag) | Automatic, generator-internal | 1 (done), 2 |
| 7 | Tool grouping | Not automatic; a `check` hint when the same tool is used non-contiguously | 2 |
| 8 | Separate heights (clearance above Safe Z, tool-change height, park) | Per-project | 2 or later (issue #73) |
| 9 | Lower retract height for rapids | Not offered | Post-phase 6, if ever |
| 10 | Direction alternation | Only for open paths (Engrave), if at all | 2 (Engrave spec decides) |

### 5.2 Recommended defaults

1. **Containment order: always on.** Within an operation, a contour's final
   pass comes after the final pass of every contour it encloses. Holes before
   their outline (done), islands before the hole around them (missing). This
   is not optional: AGENTS.md's CNC safety rules exist so that following the
   program cannot hurt the user, and cutting inside a freed slug can throw it.
2. **Machining order default: full depth first** for Profile, Pocket and
   Engrave. Reasons: it matches PureCutCNC's default for new operations
   (`feature_first`); it is what phase 1 already does and documents; it gives
   the fewest retracts on slow hobby Z axes; a stopped job leaves finished
   parts; and it is the default in Kiri:Moto, Easel and Gcodetools. A missing
   field means full depth first, so phase 1 files keep their behaviour, the
   same reasoning PureCutCNC applied to its own older files.
3. **Nearest neighbour: on, with no setting in phase 2**, as in PureCutCNC's drilling,
   pocket contours and `feature_first` blocks. It is constrained by the
   containment order (a shape only becomes available once everything inside
   it is done) and starts from the operation's first shape in drawing order,
   so a one-shape operation is unchanged. Ties break by drawing order.
4. **Operation order: exactly the project's list.** DreamCarve never reorders
   operations, never groups by tool, and `check` warns where the order looks
   unsafe (5.5). Op order carries intent the software can't infer: rough
   before finish, drill before profile, inside before outside, a test cut
   first.
5. **Linking: retract to Safe Z between shapes.** Stay down only where the
   path provably stays in material this operation has already cleared at
   this depth: between passes of one contour (done) and the phase 2 pocket
   links that NOTES.md already describes. No user-facing "keep tool down".

### 5.3 What to expose, and where

Per operation (in the `.dcv`, beside `direction` and `step_down_mm`):

```json
"machining_order": "full_depth_first"
```

with values `full_depth_first` and `pass_first`. Absent means the default, so
phase 1 files load unchanged (a serde default, not a migration; see
`docs/rust-guidelines.md`, serde and migration rules). Offered on Profile,
Pocket and Engrave from phase 2; Drill has one pass per point and ignores it;
V-Carve decides in phase 3.

Not exposed: nearest neighbour, containment order, seam choice (until a GUI
can show it), link policy, tool grouping, lower retracts.

No project-level or user-config default for machining order in phase 2.
PureCutCNC has none either, and an export-time override would make the same
`.dcv` cut differently on two machines. If users ask, a user-config default
for new operations can follow the pattern of the multi-tool export default.

### 5.4 Glossary additions

**Machining Order** and **Link** are now in `CONTEXT.md`. The values are named
by the order, not the thing ordered: "shape" is an avoided term there, and
"feature" means a Feature Instance, which is not the unit being ordered. A
"Freeing Cut" term was considered and left out until a warning needs it.

### 5.5 Where it belongs

- **Not `dc-gcode`.** S6 already requires each operation's G-code to be its
  toolpath move for move, and `emit` writes operations in list order. Keep
  both. The post-processor formats; it doesn't plan.
- **`dc-cam`, in a sequencing step shared by the generators.** Today
  `generate_profile` builds the moves directly (`emit_contour`). Phase 2 adds
  three more generators that all need the same ordering, linking and
  invariants. Proposed shape:
  - Generators produce **cut blocks**: one contour (or open path, or drill
    point) at one pass Z, with its entry and exit points, its pass index, its
    containment parent, and whether it frees material.
  - `dc_cam::sequence` (a pure function) orders the blocks by machining
    order, containment precedence and nearest neighbour, then writes the
    links, producing the `Toolpath`. Full depth first groups a shape's blocks;
    pass first groups by pass index. Tabs stay in the generator (they shape
    a block's moves, not the order).
  - One module means one place for the invariants in section 6 and one set
    of tests, instead of four generators each getting them right.
- **Operation order: `dc-project` validation and `dreamcarve check`.** The
  operation list is data. A new warning, e.g. `CutInsideFreedPart`, fires
  when operation N frees a shape (Profile reaching the stock bottom) and a
  later operation M cuts inside that shape's outline. This answers the
  issue #73 gap. It needs the toolpaths (or at least the resolved shapes and
  depths), so it is computed in the export pipeline next to the other
  warnings, not in `dc-project::validate`.

### 5.6 Phase fit

- **Phase 1 (before `v0.1.0`)**: fix the island order (containment order),
  test first, and warn when the tool-centre paths of two shapes in one
  operation overlap (D6). No new setting. Optionally update `shapes-and-holes.md` rule 6
  to say that cut order puts nested shapes first.
- **Phase 2**: `machining_order` on Profile, Pocket and Engrave; the
  `sequence` module with nearest neighbour and containment precedence; drill
  point nearest neighbour; pocket stay-down links per NOTES.md; the
  `CutInsideFreedPart` warning and a non-contiguous-tool hint; lead-in/out
  chooses the seam. Fold this into `docs/specs/phase-2/NOTES.md` so the
  phase 2 specs pick it up. The phase 2 golden cases should include one
  multi-part case per machining order, and the parity comparison for those
  cases should check order, not only per-level sets.
- **Phase 4**: the GUI shows the order (numbered shapes, link lines in the
  preview) and the machining order setting; manual start points become
  possible.
- **Phase 6**: the simulation could support lower retracts later. Not
  planned.

---

## 6. Safety invariants and tests

The ordering must never violate these. Each is checked on the toolpath (and,
where marked, on the parsed G-code like S1–S6), not by trusting the
sequencer.

| # | Invariant | Test |
| --- | --- | --- |
| O1 | Within an operation, a contour's freeing pass comes after the last pass of every contour it encloses. | `profile::hole_is_cut_before_its_outline` (exists in spirit: `profile.rs` "hole first"); `profile::island_is_cut_before_the_hole_around_it` (frame + coaster, both drawing orders give island, hole, outline); `profile::four_levels_cut_innermost_first`; property test over random nestings (issue #72). |
| O2 | Sequencing is a permutation: the multiset of cutting moves at each Z is the same for every machining order and with or without nearest neighbour. | `sequence::orders_cut_the_same_moves` (full depth first vs pass first on 3 parts with holes and tabs); the per-level golden comparison already checks this against PureCutCNC. |
| O3 | Every link that moves in XY is at or above Safe Z, unless it is a stay-down link whose whole path lies in the area this operation has already cleared at that Z. (S2 on G-code.) | `test_support::assert_safe` already checks rapids; add `sequence::links_between_shapes_retract_to_safe_z` and `sequence::stay_down_link_only_inside_cleared_area`. |
| O4 | Within one contour the passes run top to bottom; under pass first, no pass at depth k+1 starts before every shape's pass k that it depends on is done. | `sequence::full_depth_first_z_transitions` (`[-2,-4,-2,-4]` style, ported from `testPocketFeatureFirstOrder`); `sequence::pass_first_z_transitions` (`[-2,-4]`, from `testPocketLevelFirstOrder`). |
| O5 | Determinism: same input → byte-identical G-code; ties break by drawing order. | `sequence::equidistant_shapes_keep_drawing_order`; `sequence::nearest_neighbour_orders_a_c_b_as_a_b_c` (from `testPocketFeatureFirstNearestBlockOrder`); run twice and compare. |
| O6 | A one-shape operation is unchanged by any ordering setting. | `sequence::single_shape_is_identical_in_both_orders` (PureCutCNC has the same "per-kind regression check", `toolpaths.test.ts` header). |
| O7 | The post-processor preserves order. | Already enforced by S6. Keep `safety::extra_move_between_points_fails` and add a test that a reordered operation fails S6. |
| O8 | Operation order is never changed; `check` warns when an operation cuts inside a shape an earlier operation freed. | `export::operations_keep_project_order`; `check::cut_inside_freed_part_warns` (outside Profile through, then a Pocket inside it); `check::inside_before_outside_is_quiet`. |
| O9 | Tabs survive reordering: on every pass below the tab top each tab span is raised, whatever the order. | Extend `tabs::final_pass_has_exactly_four_raised_segments_at_tab_z` to run under both machining orders. |
| O10 | No emitted Z below stock bottom minus allowance, for every order. (S1.) | Existing S1 safety pass, run on the multi-part fixtures under both orders. |

---

## 7. Decisions

Taken with the maintainer on 2026-10-18. `docs/specs/phase-2/NOTES.md`
carries the phase 2 items; phase 1 items are tracked as issues.

| # | Decision | Phase |
| --- | --- | --- |
| D1 | Fix the island order (containment order, invariant O1) before `v0.1.0`, test first. | 1 |
| D2 | Default machining order is full depth first. This matches PureCutCNC's default for new operations, so no ADR. | 1 (as built) |
| D3 | Machining order is a per-operation setting on Profile, Pocket and Engrave. No project, export or user-config default; a missing field means full depth first. | 2 |
| D4 | Values are `full_depth_first` and `pass_first`; glossary terms Machining Order and Link. | 2 |
| D5 | Nearest-neighbour order, constrained by containment, ties by drawing order, with no setting. A drawing-order escape hatch only if users ask. | 2 |
| D6 | No union of close outlines yet. Warn when the tool-centre paths of two shapes in one operation overlap, before `v0.1.0`. | 1 |
| D7 | Operations are never reordered or grouped by tool. `check` hints at avoidable tool changes and warns `CutInsideFreedPart`. | 2 |
| D8 | No lower retract and no keep-tool-down option. Every link that moves in XY between shapes is at or above Safe Z. | — |
| D9 | Direction alternation on open paths is left to the Engrave spec, off by default. | 2 |
| D10 | Sequencing is one shared `dc_cam::sequence` step over cut blocks. `dc-gcode` never reorders. `generate_profile` moves onto it early in phase 2. | 2 |
