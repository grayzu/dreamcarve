# Phase 4: GUI, CAM only — Design Spec

**Status:** accepted (merged in #460, 2026-11-03); amended by the
screenshot review of #549 (2026-11-10): "Review redesign" below and the
review slices 23–36
**Functional spec:** `functional.md`
**Visual design:** `experience.md` (the design language: principles, type,
colour, controls, interaction, egui realisation, review checklist)

## Overview

The app is a second shell on the engine the CLI already drives. Nothing
about toolpaths, warnings or G-code changes in this phase (functional S20);
what changes is where the pipeline lives and who calls it. Three crates are
added and one is made thin:

- **`dc-job`** (library, pure): the pipeline that today sits in the CLI
  binary, moved down a level so both shells call the same functions: the
  project built from a drawing or a picture (`init`'s logic), import of a
  drawing into a project, validation, generation of every enabled
  operation, the order and tab checks, emission, the safety pass, program
  naming, extents and estimates, the SVG preview, and tab placement. The
  CLI keeps every byte of its output; the GUI gets the same bytes because
  it has no other way to get them. The GUI crates depend on `dc-cam` and
  `dc-gcode` only through `dc-job`, which Cargo enforces: neither appears
  in their manifests.
- **`dc-app`** (library, pure, no egui): the application core. One
  `History` of `Project` snapshots, a `Command` enum whose `apply` makes
  the next snapshot and whose `label` names the undo step, the selection,
  the view settings, the generation bookkeeping (what needs regenerating,
  what is stale, what came back), the preferences record, the string
  table, units formatting and parsing, viewport geometry (camera, picking,
  transform handles, tab dragging, the marks a toolpath draws). Every
  acceptance test but the smoke and the screenshots runs against this
  crate with no window.
- **`dreamcarve-app`** (binary): the egui + wgpu shell. The window, menus,
  panels and widgets; the viewport's wgpu line renderer; the worker thread
  that runs `dc-job`; files, dialogs, clipboard, the OS appearance; the
  preferences file. It owns every `std::fs`, thread and `eframe` type.
- **`dreamcarve-shell`** (library): the file I/O both binaries share and
  `dc-*` crates may not do: the staged write (`StagedFile`, `commit_all`),
  the config directory, `config.toml`, and `gui.toml`.
- **`dreamcarve`** (the CLI) keeps its commands and its tests and loses its
  pipeline modules to `dc-job` and `dreamcarve-shell`. Slice 1 is that
  move, with every CLI test green and every golden byte unchanged, before
  any GUI code exists.

Data flow for one edit: a widget commits a `Command` → `dc-app` applies it
to the current `Project`, pushes the snapshot, recomputes which operations'
inputs changed (a per-operation digest) → the shell posts those operations
to the worker → the worker calls `dc_job::generate_operation` and, when
every operation is current, `dc_job::finish` (order and tab checks, a dry
emission for warnings, extents and estimates) → results come back over a
channel keyed by snapshot and operation → `dc-app` stores them, the
Warnings panel and the Viewport read them next frame. Validation
(`dc_project::validate`) runs synchronously on every snapshot, so errors
about unset values appear at once. Export calls `dc_job::build` on the
current snapshot and writes through `dreamcarve-shell`, as the CLI does.

The design spec names types and algorithms as the phase 1–3 specs did; the
look of every widget is `experience.md`'s and is not repeated here.

## PureCutCNC sources

PureCutCNC's UI is React and CSS, so what is ported is behaviour, read
from these files; a Rust module that transcribes one of them takes the
ported-file header.

| DreamCarve module | PureCutCNC file(s) | What is taken, what changes |
| --- | --- | --- |
| `dc-app::editor::fields` | `src/components/cam/operationFields.ts`, `operationFields.test.ts` | The idea that field order, grouping and applicability are one table the panel walks, with a compile-time renderer per field. DreamCarve's table is per `Operation` variant and matches functional capability 68 exactly; PureCutCNC's kinds, passes and trochoidal fields do not exist here. |
| `dc-app::export::plan` | `src/components/export/exportOperationSelection.ts`, `exportBlockReason.ts`, `ExportDialog.tsx` | The two-tier message list and "an error blocks Export and the save path alike". No per-operation checklist (functional Q5). |
| `dc-app::viewport::camera`, `::pick` | `src/components/canvas/SketchCanvas.tsx` (navigation only), `hitTest.ts`, `hitTest.test.ts` | Wheel zoom about the cursor, pan, fit; hit testing resolved world-space outlines with a pixel tolerance, topmost wins, click cycles overlaps. Y-up re-derived: the one flip is in `Camera::to_screen`. |
| `dc-app::viewport::arrows` | `src/components/canvas/toolpathArrows.ts`, `toolpathArrows.test.ts` | Direction-arrow placement along cuts at a screen-space interval, cached per (toolpath, scale). Ported with its tests. |
| `dreamcarve-app::viewport::lod` | `src/components/canvas/toolpathDisplay.ts` | Coalescing same-style strokes below a pixel of travel for display only; never touches emitted motion. |
| `dc-app::view` | `src/components/toolpathVisibility.ts`, `toolpathLevels.ts` | The visibility record (cuts, rapids, plunges, arrows, start points, tab marks) as view state, not project state. |
| `dc-app::warnings` | `src/engine/toolpaths/warningCodes.ts` (`warningSeverity`), `src/i18n/warningText.ts` | Tiers at presentation, codes in the engine. DreamCarve's codes are `dc-cam`'s and `dc-project`'s; the text is their `Display`. |
| `dc-app::tree` | `src/components/feature-tree/FeatureTree.tsx`, `PropertiesPanel.tsx`, `mixedValue.ts` | Row model (eye, lock, badges), selection sync, mixed-value display for a multi-selection. No folders, no regions, no construction. |
| `dc-app::library::filter` | `src/components/cam/toolLibraryDialogModel.ts`, its test | Search and type filter over the library, "in project" marking. Ported with its tests. |
| `dc-app::commands` | `src/store/slices/historySlice.ts`, `operationsSlice.ts` (reorder, duplicate), `tabsSlice.ts` | One history entry per user action; a drag is one step on release; typing is one step on commit. |
| `dreamcarve-app::onboarding` | `src/components/onboarding/EmptyStateOverlay.tsx` | The empty-state card's three actions and examples. |
| `dreamcarve-app::shell` | `planning/DESKTOP_DESIGN.md` | Native open/save dialogs, dirty state, recent files, close confirmation, file association; the shell never duplicates engine logic. |

Not ported: `GenerationSettingsMenu` (one backend here), the GPU sketch
renderer (the viewport is wgpu from the start), CAM Plan, the booklet,
nesting, tablet mode.

## Data model

No change to the `.dcv` format in this phase. The one-file Inlay (#498,
format 25) is a phase 3.3 slice and lands first; this spec reads its
`InlayOp::male_board`. The phase introduces types in `dc-job` and `dc-app`
only. Signatures below are the contract; names inside modules may differ.

### dc-job

```rust
/// What `dreamcarve export` and `check` compute, without I/O. Moved from
/// `crates/dreamcarve/src/export.rs`; same fields, same bytes.
pub struct Built { pub programs: Vec<Program>, pub findings: Findings, pub generated: Option<Vec<EmitOp>> }
pub struct Findings { pub errors: Vec<Finding>, pub warnings: Vec<Finding> }
pub struct Finding { pub field: Option<String>, pub code: Option<String>, pub message: String,
                     pub tier: Tier, pub op: Option<OperationId>, pub shape: Option<ShapeRef> }
pub enum Tier { Error, Warning, Hint }   // `tool_change_avoidable` and `unused_tool` are hints
pub fn build(project: &Project, split: ProgramSplit, date: &str) -> Built;

/// The per-operation half of `build`, for the GUI's incremental generation.
pub fn generate_operation(project: &Project, op: &OperationId) -> Result<(Vec<EmitOp>, Vec<Warning>), CamError>;
/// The project-level half once every enabled operation has its runs: the
/// order and tab checks, then a dry emission for export warnings, program
/// list, extents and estimates. Pure: `date` is the caller's.
pub fn finish(project: &Project, generated: &[EmitOp], split: ProgramSplit, date: &str, estimate: &EstimateConfig) -> Finished;
pub struct Finished { pub findings: Findings, pub programs: Vec<ProgramSummary> }
pub struct ProgramSummary { pub name: String, pub board: Board, pub tool: ToolId, pub operations: Vec<OperationId>,
                            pub extents: Extents, pub edge_extents: Extents, pub estimate: Option<Estimate>, pub before: String }

/// Program names as the CLI writes them (`<stem>-<k>-T<n>.<ext>`, per board with an Inlay).
pub fn program_names(project: &Project, stem: &str, programs: &[Program]) -> Vec<String>;

pub mod preview {               // moved from `crates/dreamcarve/src/preview.rs`
    pub fn render(project: &Project, ops: &[Run<'_>]) -> String;
    /// The marks the SVG draws, as geometry in mm, so the Viewport draws the same ones.
    pub fn marks(run: &Run<'_>, tool: &Tool) -> Marks;
    pub struct Marks { pub tab_marks: Vec<Polyline>, pub start_points: Vec<StartMark>, pub holes: Vec<(Point, f64)> }
}

pub mod project_from {          // moved from `crates/dreamcarve/src/init.rs`
    pub const STOCK_MARGIN_MM: f64;
    pub fn drawing(import: Drawing, post: PostProcessorRef, units: Units, with_defaults: bool) -> (Project, Vec<Warning>);
    pub fn picture(bitmap: Bitmap, ...) -> (Project, Vec<Warning>);
}
pub mod import {
    /// A drawing's definitions and instances appended to `project`, placed as drawn (functional 55).
    pub fn drawing_into(project: &Project, import: Drawing) -> (Project, Vec<InstanceId>);
    pub enum Drawing { Svg(SvgImport), Dxf(DxfImport) }
}
pub mod tabs { pub use dc_cam::placed_tabs::{place, place_on_outlines, replace, crossed_by}; pub fn place_request(project: &Project, op: &OperationId) -> Result<Project, CamError>; }
```

`with_defaults: false` is what the GUI passes (functional Q1: no
placeholder tool, no Profile); the CLI passes `true` and writes what it
writes today.

### dc-app

```rust
pub struct App {
    history: History,                 // dc_project::History, MAX_UNDO_STEPS = 100
    labels: Vec<Label>,               // one per `past` snapshot, for the Edit menu
    selection: Selection,             // instances, tabs, or one operation
    view: View,                       // camera, visibility, colour mode, panel state the shell mirrors
    generation: Generation,           // per-operation digests, results, stale flags, pending set
    strings: Strings,
    prefs: Prefs,
    source: Option<SourceRef>,        // path and the bytes last saved, for "unsaved"
}

/// Every change to the project. `apply` is pure; `label` is the undo text.
pub enum Command {
    SetStock(Stock), SetOrigin(MachineOrigin), SetHeights(Heights), SetTolerances { flatten_mm: f64, arc_fit_mm: Option<f64> },
    SetDisplayUnits(Units), SwitchPost { id: String }, SetPostSettings(PostProcessorRef),
    AddTools(Vec<Tool>), UpdateTool(Tool), RemoveTool(ToolId),
    ImportDrawing { drawing: Drawing, name: String }, ImportPicture { bitmap: Bitmap },
    TransformInstances { ids: Vec<InstanceId>, delta: Transform },   // about the selection's centre
    SetInstanceFlags { ids: Vec<InstanceId>, visible: Option<bool>, locked: Option<bool> },
    RenameInstance { id: InstanceId, name: String }, DuplicateInstances(Vec<InstanceId>), DeleteInstances(Vec<InstanceId>),
    AddOperation(Operation), UpdateOperation(Operation), ReorderOperation { id: OperationId, to: usize },
    SetOperationEnabled { id: OperationId, enabled: bool }, DuplicateOperation(OperationId), DeleteOperation(OperationId),
    PlaceTabs { op: OperationId }, AddTab(PlacedTab), MoveTab { id: TabId, x_mm: f64, y_mm: f64 }, UpdateTab(PlacedTab), DeleteTabs(Vec<TabId>),
    CentreOnStock(Vec<InstanceId>), FitStockToShapes { margin_mm: f64 },
}
impl Command {
    pub fn apply(&self, project: &Project) -> Result<Project, CommandError>;
    pub fn label(&self, strings: &Strings, project: &Project) -> String;   // "Move “Oval”"
}
impl App {
    pub fn run(&mut self, command: Command) -> Result<(), CommandError>;    // apply, push, label, re-digest
    pub fn undo(&mut self) -> Option<String>; pub fn redo(&mut self) -> Option<String>;
    pub fn project(&self) -> &Project;
    pub fn pending(&self) -> Vec<Job>;                                      // what the worker should run next
    pub fn accept(&mut self, result: JobResult);                            // ignored unless its snapshot id is current
    pub fn warnings(&self) -> &[Finding];                                   // validation + generation + finish, merged, tiered
    pub fn is_dirty(&self) -> bool;
}

/// Why an operation regenerates: a digest of everything its generator reads.
pub struct Job { pub snapshot: SnapshotId, pub op: OperationId, pub digest: u64 }
pub enum JobResult { Operation { job: Job, result: Result<(Vec<EmitOp>, Vec<Warning>), CamError> },
                     Finished { snapshot: SnapshotId, finished: Finished } }

pub struct View { pub camera: Camera, pub visibility: Visibility, pub colour: ColourMode, pub only_selected: bool,
                  pub extents: bool, pub hidden_ops: BTreeSet<OperationId> }
pub struct Camera { pub centre_mm: Point, pub px_per_mm: f64, pub viewport_px: Vec2 }
impl Camera { pub fn to_screen(&self, p: Point) -> Pos; pub fn to_world(&self, s: Pos) -> Point;  // the one Y flip
              pub fn zoom_about(&mut self, s: Pos, factor: f64); pub fn fit(&mut self, bounds: Rect); }

pub enum Hit { Instance(InstanceId), Tab(TabId), Toolpath(OperationId), MaleBoardShape(OperationId), Nothing }
pub fn pick(project: &Project, scene: &Scene, camera: &Camera, at: Pos, tolerance_px: f64, cycle: usize) -> Hit;

pub struct Handles { pub bounds: Rect, pub corners: [Point; 4], pub rotation: Point }
pub fn handles(project: &Project, selection: &[InstanceId]) -> Option<Handles>;
pub fn drag_delta(handles: &Handles, grab: Grab, from: Point, to: Point, uniform: bool) -> Transform;
pub fn tab_drag(project: &Project, tab: &PlacedTab, to_mm: Point) -> (f64, f64);   // nearest point on its outline, definition space

pub struct Strings;  impl Strings { pub fn get(&self, key: StrKey) -> &str; pub fn fmt(&self, key: StrKey, args: &[(&str, &str)]) -> String; }
pub enum StrKey { /* generated by build.rs from strings/en.toml */ }

pub struct Prefs { pub window: Option<WindowGeometry>, pub panels: Panels, pub sections: BTreeMap<String, bool>,
                   pub visibility: Visibility, pub colour: ColourMode, pub recent: Vec<PathBuf>,
                   pub export_dirs: BTreeMap<PathBuf, PathBuf>, pub theme: Theme /* System | Light | Dark */ }
```

`Scene` is the derived read model the Viewport and `pick` share: every
visible instance's outlines in project space (definition transformed, via
`dc_project::locate`'s resolver), each bitmap's footprint, each tab's
project-space point, each operation's runs and marks, and, with an Inlay,
the male board's frame and mirrored outlines offset to the right of the
stock by one stock-width plus 20 mm (display offset only; nothing in the
file). It is rebuilt when the snapshot or a result changes, never per
frame.

### Digest and staleness

`Generation::digest(project, op)` hashes, in a fixed order: the operation's
record; every tool it names (its own, its clearing passes', its cut-out's);
every target instance and the definition each names; `stock`, `origin`,
`heights`, `flatten_tolerance_mm`, `arc_fit_tolerance_mm`; the placed tabs
on its targets; and, when the operation reads an earlier operation (a
Pocket's `rest_of`; any operation with a start plane or a countersink,
whose `start_depth_mismatch` reads earlier Pockets; an Inlay or V-Carve
whose cone check reads `cleared_start_plane`), the records of every enabled
operation before it. Over-invalidation is accepted; an unlisted dependency
is a bug, and the test `generation::digest_covers_every_generator_input`
mutates each field of a fixture project in turn and asserts that the
generator's output changes only when the digest does. `finish` reruns
whenever any digest changed, since the order check reads everything.

An operation is **stale** when its stored result's digest differs from the
current one; the Viewport draws the stored runs at the faded alpha until
the new result arrives. A result whose `snapshot` is not the current one
is dropped. **Stop** clears `pending` and marks the running job's result
to be dropped; the worker thread finishes the job it is on, since the
generators have no cancellation point, and the UI shows it stopped at
once. Acceptance 87's second is the UI's.

### Units

Fields display through `dc_app::units::format(mm, units, decimals)` and
parse through `dc_project::units_input` (the CLI's parser, capability 32),
so `1/4 in` and `0.5` mean what they mean in a file. A field commits on
focus loss or Enter; Escape restores. The stored value is millimetres.

### Strings

`crates/dc-app/strings/en.toml` holds every user-visible string, keyed
`panel.section.item` with `{name}` placeholders. `build.rs` generates the
`StrKey` enum, so a key that is not in the table fails to compile, and the
test `strings::every_key_is_used` scans the two GUI crates for each
variant (acceptance 91). Warning and error text comes from `dc-cam`'s and
`dc-project`'s `Display`, the CLI's text, with the code beside it; phase 8
keys them.

### Preferences

`gui.toml` beside `config.toml` (functional Q11), read at start and written
on change and on exit by `dreamcarve-shell`; `dc-app` holds the record. A
missing or unreadable file means defaults and a one-line note in the
Warnings panel, never a refusal to start.

## Architecture

### The shell (`dreamcarve-app`)

`eframe` with the `wgpu` backend, one `App` struct implementing
`eframe::App`. `update` does, in order: drain the worker channel into
`dc_app::App::accept`; draw the menu bar, the toolbar, the left panel
(`SidePanel::left`), the right panel, the status bar (`TopBottomPanel`),
the Warnings drawer, then the Viewport in the `CentralPanel`; collect the
commands the widgets produced this frame and run them; post `pending()` to
the worker. Widgets never hold project data between frames: they read the
current snapshot and emit commands. Transient editing state (a field's
text while focused, a drag in progress, a menu's open state) lives in the
shell's per-widget state, and a drag's live preview is a `Transform`
applied in the Viewport's draw, not a snapshot.

Threads: one worker owning a receiver of `Job`s and a sender of
`JobResult`s; it runs `dc_job::generate_operation` per job and
`dc_job::finish` when told every operation is current. The UI thread never
calls `dc-cam` or `dc-gcode`. A second short-lived thread reads or writes
files on Open, Save and Export so a slow disk does not freeze a frame; the
window shows the modal progress sheet meanwhile.

Files: `rfd` for native open and save dialogs; drops via
`egui::DroppedFile`; `--open <dcv>` and the OS file association hand the
path to the same open path. Unsaved-changes prompts are egui modals drawn
by the shell (`experience.md`, sheets). Clipboard via `egui`'s own.

Appearance: `eframe`'s system theme detection drives `Theme::System`;
the two palettes in `experience.md` are applied to `egui::Visuals` once per
theme change in `dreamcarve-app::theme`.

### The Viewport renderer

`dreamcarve-app::viewport` registers an `egui_wgpu::Callback` for the
central panel. Geometry is uploaded in project millimetres and transformed
on the GPU: one uniform holds the camera (centre, scale, viewport size,
the Y flip), one storage buffer per layer holds line segments as
`(a: vec2, b: vec2, style: u32)`, and the vertex shader expands each
segment into a screen-space quad of the style's pixel width (round joins
by overlapping caps), so pan and zoom re-upload nothing. Layers, drawn
back to front: grid, stock, male board frame, bitmaps (textured quads),
shapes, hidden-op toolpaths (none), rapids, cuts per operation (one buffer
each, so an operation's eye toggles a draw call), marks, selection and
handles, hover. Arcs are flattened for display at a pixel tolerance derived
from the current zoom and cached per zoom band. Direction arrows are
`dc-app::viewport::arrows`' placement rendered as two triangles each.
`lod` coalesces segments shorter than a pixel into one for display only
and rebuilds when the zoom band changes; emitted motion never passes
through it.

Colour by depth: the style word carries the move's depth bucket; the
shader reads the ramp from a small texture built from `experience.md`'s
ramp for the active theme.

Picking, handles and tab dragging are `dc-app` functions on the `Scene`
and the `Camera`; the renderer only draws their results. Cursor
coordinates are `camera.to_world(pointer)` in project units.

### The editors

`dc-app::editor::fields` is the table of functional capability 68: for
each `Operation` variant, an ordered list of `(group, field, kind)` with
the field's key path into the record, its unit, its tool-derived default
(if any), its validation message key and its help key. The shell walks
the table with one renderer per `kind` (length, rate, angle, percentage,
count, choice, toggle, tool picker, instance list, tab request, clearing
list, cut-out block, male board block); there is no per-kind JSX
equivalent. Acceptance 73 is a table-driven test in `dc-app` that, for
each row, sets a non-default value through the same path the renderer
uses, saves and reloads, and asserts the editor exposes exactly the
table's fields and the record's fields, no more and no fewer (a
`serde_json::Value` walk of the record against the table).

A field edit commits as `UpdateOperation(record)` with the whole record,
built by `fields::set(record, key, value)`; the label is "Set <field> of
<name>". Located warnings (`Finding::field` naming `operations[op].<path>`)
are matched to rows by key path and drawn under them by the shell.

Since the review (functional 66, 66a): `Group::Identity` is gone (the
name is the header's, `enabled` the list's) and `Group::Setup` is first;
a row's group is still decided only in the table, so moving a field to
Setup is a table edit and acceptance 73's walk is unchanged.
`editor::summary::group(kind, group, record, project, units) -> String`
builds a closed group's one-line summary from the same rows, through
`units::format`, so it can never show a value the fields do not. The
editor's open state per `(kind, group)` lives in `Prefs::sections`; a
group holding a located finding opens on selection.

### The Design Tree and placement

`dc-app::tree` produces rows from the snapshot (instances in file order,
then tabs grouped by outline, then bitmaps), with the per-row flags the
functional spec lists and the operations targeting each instance. The
Properties panel's placement fields read the selection's joint bounding
box in project space (`Scene`), and a typed value becomes
`TransformInstances { delta }` about the box's centre: translation for X
and Y, scale for W and H (uniform unless the padlock is open), rotation
for the angle, mirror as a scale of −1 on one axis. `dc_geometry::Transform`
composes; definitions are never written. Locked instances are filtered out
of `TransformInstances` by `apply`, which returns `CommandError::Locked`
naming them, and the shell shows that in the status bar.

### Targets on the drawing

`dc-app::editor::targets` (review, functional 67) holds the
target-editing state: `TargetEdit { op, pending: BTreeSet<InstanceId> }`
in `App`, entered and left by the shell, never a `Command`; `Done` emits
one `UpdateOperation` with the new targets and the label *Set targets of
"<name>"*. While it is `Some`, `pick` results toggle membership instead
of selecting, and `TransformInstances` is not emitted. The `Scene` gains
a per-shape style (`Normal`, `Target(OpColour)`, `Faded`) derived from
the operation selection, so the highlight costs no extra pass, and
`tree::rows` gains `target_of: Option<OpColour>`.

### Arrange

`dc-app::arrange` turns the toolbar's actions into
`TransformInstances` about the selection's bounding-box centre:
`rotate(deg)` (positive anticlockwise, Y-up; the toolbar's 90° clockwise
is `rotate(-90)`) and `flip(Axis)` (scale −1 on one axis), filtering
locked instances and returning the names it left out for the status
bar, and `enabled(selection) -> Result<(), Reason>` for the disabled
state. Properties loses rotation and mirror; `properties::placement`
keeps X, Y, W, H and the proportion link (`Prefs`, not project data).

### Tabs

`PlaceTabs { op }` is `dc_job::tabs::place_request` (the CLI's `tabs
place` logic). `AddTab` from the Tab tool takes the clicked outline point
(`pick` returns the instance and the nearest profile and point), stores it
in definition space, with the last-used width, height and shape from
`Prefs`. `MoveTab` takes `tab_drag`'s snapped point. The Viewport draws
each tab's span across each toolpath that crosses it from
`dc_job::preview::marks` and draws a tab no run crosses hollow using
`dc_cam::placed_tabs::crossed_by` per enabled Profile.

### Warnings

`App::warnings()` merges, per snapshot: validation problems (`Severity`
mapped to `Tier`, `field` kept), generation warnings per operation,
`finish`'s findings, and import notes, deduplicated as `build` deduplicates
(the codes validation already reports once are not repeated from the
generator). Rows are grouped by operation, project-level first, in the
order `check` prints them, so acceptance 82's comparison is a line-by-line
one against the CLI's text. *Copy* is the same text.

Carve notes (review, functional 76a): `dc_job::summary::carve_facts(
project, generated) -> Vec<CarveFacts>`, one per carving operation, each
a list of boards, each a list of `(FactKey, FactValue)`; the existing
`describe_carves` becomes a formatter over it, so `check`'s text is
byte-identical and the CLI's tests are the guard. `App::warnings()` puts
each operation's facts last in its group as `Row::Note`, which the
counts skip. `warnings::groups(rows)` returns a heading per group only
when there are two or more.

### Export

The dialog is `dc-app::export::plan(project, split, board_filter)`: it
refuses while any operation is pending or stale, then calls `dc_job::build`
(a fresh full build, not the incremental results, so the dialog's bytes are
`build`'s bytes), names the files with `program_names` and the chosen
stem, and lists `ProgramSummary` rows. *Export* writes every program, the
preview SVG when ticked, through `dreamcarve-shell::commit_all`, with the
same marker check on an existing preview the CLI makes. The project file is
not written. The result (functional 77a) is built from `dc_job::report`'s
parts, not its text: `export::result(&Built, names, dir) -> ExportResult
{ status, files: Vec<FileRow>, folder, instructions: Vec<Step>, notes:
Vec<String> }`, with a test that every line of the CLI's report text
appears in exactly one part.

The setup sheet (functional 77b) is `dc_job::setup_sheet::render(
project, built, names, date, strings) -> String`: pure, deterministic
with the date pinned, HTML with inline CSS only, escaped through one
function. It lives in `dc-job` beside the report so a later CLI flag
would print the same bytes (open question 7); phase 4 adds no CLI
flag. The shell writes it through `dreamcarve-shell`'s staged write.

### The Tool Library

`dc-app::library` (review, functional 64, 64a, 64b) gains: `groups(lib,
filter) -> Vec<VendorGroup>` (by `vendor`, sorted, `None` last); an
`LibraryEdit` session holding the library as opened, so *Revert* is a
copy back; `edit(lib, id, key, value) -> Library`, validated with
`dc_library::validate_tool` and written at once by the shell; and
`review(lib, incoming) -> ImportReview`, which runs
`dc_library::import::merge` twice in memory (`replace` false and true)
and reports the counts and per-tool outcome of each, so the review shows
what either choice writes and *Import* writes the chosen one. The
library's display units are `Prefs::library_units`; the editing pane
reuses the project tool editor's rows with those units.

### Test harness

`dc-app`'s `test_support::Harness` builds an `App` from fixture bytes, runs
commands, and executes `pending()` inline on the test thread with the real
`dc_job` functions, so every acceptance test that reads toolpaths,
warnings or programs runs headless in `cargo test` on all three OSes.
`crates/dreamcarve/tests/gui_equality.rs` holds acceptance 83 and 90: the
harness saves a project, the test runs the `dreamcarve` binary
(`CARGO_BIN_EXE_dreamcarve`) on it with `--date` pinned, and compares bytes
with `dc_app::export::plan`'s programs.

`dreamcarve-app` uses `egui_kittest` (`eframe`, `wgpu` and `snapshot`
features; the crate tracks egui's version, 0.36 at writing) for the window
smoke (acceptance 92: open the plaque, render one frame, export) and for
widget-level behaviour through its accessibility tree (click a row, type
in a field, assert the command emitted). Snapshot images are kept to a
handful, generated on Linux only, with `kittest.toml` thresholds; the
other platforms run the behaviour tests without image comparison, since
font rasterisation differs per OS. CI's Linux job installs Mesa's lavapipe
for a software Vulkan adapter; macOS runners render with Metal; Windows
runners use the DX12 WARP adapter, and if WARP proves unreliable the
Windows job keeps the egui-only (CPU) harness and the rendering tests run
on the other two.

### Visual design

`experience.md` is the authority: tokens, type, icon set, control states,
interaction, motion, accessibility and the egui realisation of each. The
shell's `theme` module is the one place the tokens become `egui::Visuals`
and `Style`; widgets take colours from the theme, never literals. The
fonts and the icon set it names are bundled under
`crates/dreamcarve-app/assets/` with their licences added to `NOTICE`. The
review checklist at its end is applied to every GUI slice's screenshots
before merge.

## Module layout

```
crates/dc-job/src/{lib.rs, build.rs (build, generate_operation, finish, Findings, Tier), names.rs (program_names), summary.rs (extents, estimates, carve_facts, describe_carves), report.rs, setup_sheet.rs (review: render), preview.rs (render, marks; moved), project_from.rs (drawing, picture; moved from init.rs), import.rs (drawing_into), tabs.rs (place_request and re-exports)}
crates/dc-app/src/{lib.rs, app.rs (App, SnapshotId, dirty), command.rs (Command, apply, label, CommandError), history.rs (labels beside dc_project::History), selection.rs, view.rs (View, Visibility, ColourMode), generation.rs (digest, Job, JobResult, staleness), scene.rs (Scene, male board offset), viewport/{camera.rs, pick.rs, handles.rs, arrows.rs (port), tabs.rs (tab_drag)}, editor/{fields.rs (the capability 68 table), set.rs (key-path set/get), targets.rs, clearing.rs}, tree.rs, warnings.rs (merge, tiers, grouping), export.rs (plan), library.rs (filter; port), units.rs, strings.rs (+ build.rs, strings/en.toml), prefs.rs, test_support.rs (Harness)}
crates/dreamcarve-app/src/{main.rs (eframe entry, --open), shell/{window.rs, menu.rs, toolbar.rs, status.rs, files.rs (open/save/recent/unsaved), worker.rs (thread, channels), theme.rs (tokens → Visuals), strings.rs (loader)}, panels/{tree.rs, properties.rs, project.rs, operations.rs, editor.rs (renderers per field kind), tools.rs, library.rs (window), warnings.rs, export.rs (sheet), import.rs (sheet), empty.rs}, viewport/{mod.rs, renderer.rs (wgpu callback, buffers, shader.wgsl), lod.rs, handles.rs, cursor.rs}, widgets/{unit_field.rs, segmented.rs, section.rs, row.rs, icon.rs, tabs.rs}}
crates/dreamcarve-app/assets/{fonts/, icons/, strings/ (symlink or include of dc-app's table)}
crates/dreamcarve-shell/src/{lib.rs, staged.rs (StagedFile, commit_all; moved from cli.rs), config_dir.rs (moved from config.rs), config.rs (config.toml), gui_prefs.rs (gui.toml)}
crates/dreamcarve/src/{cli.rs, export.rs, check.rs, init.rs, tabs.rs, tools.rs, config.rs, image.rs}  — thin: argument parsing, printing, calls into dc-job and dreamcarve-shell
crates/dreamcarve/tests/gui_equality.rs
docs/specs/phase-4/{functional.md, design.md, experience.md, mockups/}
docs/book/src/{quick-start/app.md, user-guide/{app.md, importing.md, placing.md, stock-origin.md, preview.md, export.md, keyboard.md}}
```

The review slices (23–36) add, with no new crate: `dc-app/src/{arrange.rs
(rotate, flip, enabled), status.rs (the status bar's cells as data),
editor/summary.rs (closed-group summaries), onboarding.rs (example
thumbnails as outlines)}`, `TargetEdit` in `editor/targets.rs`, `groups`,
`LibraryEdit` and `review` in `library.rs`, notes in `warnings.rs`, the
depth legend's ticks in `view.rs`, `result` in `export.rs`;
`dc-job/src/setup_sheet.rs` and `carve_facts` in `summary.rs`;
`dreamcarve-app/src/{viewport/popover.rs (the toolpath view, replacing
card.rs), viewport/legend.rs, viewport/target_bar.rs, panels/result.rs,
widgets/split_button.rs, widgets/link.rs}`. `Prefs` gains
`library_units` and the left column's panel open states.

Dependencies, all `[workspace.dependencies]`, versions re-checked at slice
time: `eframe`/`egui`/`egui_wgpu`/`egui_extras` (MIT/Apache-2.0; `wgpu`
and `winit` through `eframe`, `default-features = false` plus `wgpu`,
`default_fonts` off since fonts are bundled), `egui_kittest` (dev),
`rfd` (MIT), `serde` and `toml` (already present), `png` (already present,
for bitmaps on the Viewport). Bundled assets, as `experience.md` names
them: **Inter** (OFL 1.1, no Reserved Font Name,
github.com/rsms/inter) as three static instances with tabular figures
frozen in, since egui cannot select OpenType features; **JetBrains Mono**
(OFL 1.1, github.com/JetBrains/JetBrainsMono) for codes, paths and
G-code; **Lucide** (ISC, with the Feather-derived glyphs under MIT,
github.com/lucide-icons/lucide) as its icon font. `NOTICE` gains all
three with their copyright lines; `cargo deny` stays the gate. The eight
operation-kind glyphs are DreamCarve's own, painter-drawn so they take
the operation's colour.
`docs/rust-guidelines.md`'s crate graph gains the new crates:

```
dc-geometry → dc-project → { dc-cam, dc-import, dc-library } → dc-job → dc-app → dreamcarve-app
dc-project → dc-cam → dc-gcode → dc-job → dreamcarve
dreamcarve-shell → { dreamcarve, dreamcarve-app }
```

`dc-app` and `dreamcarve-app` list neither `dc-cam` nor `dc-gcode`; the
types they need (`Toolpath`, `Move`, `Warning`, `EmitOp`, `Program`) come
re-exported from `dc-job`. That is S20's source rule.

## Test plan

Names are `crate::module::test`; "(port)" marks a PureCutCNC test ported
with its upstream name; "(acc N)" the functional acceptance test it
carries. The CLI's existing tests are the regression net for slice 1.

**dc-job (slice 1)**
- `build::cli_programs_are_byte_identical_after_the_move` (every golden case and quick-start project through `dc_job::build` equals the committed programs).
- `build::generate_operation_then_finish_equals_build` (for each fixture: per-operation generation plus `finish` gives the same findings and programs as `build`).
- `build::findings_carry_tier_op_and_shape` (`tool_change_avoidable` and `unused_tool` are hints; an error has no code; a located warning names its field).
- `names::program_names_match_the_cli` (single tool, per tool run, per board with an Inlay).
- `preview::render_is_unchanged_after_the_move`, `preview::marks_equal_what_the_svg_draws` (tab marks, start points, holes extracted from the SVG match `marks`).
- `project_from::drawing_with_defaults_equals_init`, `project_from::drawing_without_defaults_has_no_tool_and_no_operation` (acc 63).
- `import::drawing_into_places_as_drawn_and_returns_the_new_ids` (acc 64).
- `tabs::place_request_equals_tabs_place` (acc 78, first half).

**dreamcarve-shell (slice 1)**
- `staged::existing_cli_tests_pass_unchanged` (the moved tests), `gui_prefs::missing_file_is_defaults`, `gui_prefs::round_trip`, `gui_prefs::unknown_field_is_a_note_not_an_error`.

**dc-app core (slice 2)**
- `command::every_command_round_trips_through_the_file` (each variant applied, saved, loaded, equal).
- `command::labels_name_the_thing` (acc 86 labels), `command::locked_instances_refuse_transforms` (acc 66).
- `history::twenty_mixed_edits_undo_and_redo_in_order`, `history::new_edit_clears_redo`, `history::open_clears_history` (acc 62, 86).
- `app::save_unchanged_is_byte_identical` (acc 62, every fixture), `app::dirty_tracks_the_saved_bytes` (acc 88).
- `strings::every_key_is_used`, `strings::placeholders_are_filled` (acc 91).
- `units::inch_project_displays_inches`, `units::unit_strings_parse_as_the_file_does` (acc 89).
- `prefs::view_state_is_not_project_state` (acc 81).

**Viewport geometry (slice 4, 16, 17)**
- `camera::to_screen_flips_y_once_and_to_world_inverts_it`, `camera::zoom_about_the_cursor_keeps_it_fixed`, `camera::fit_shows_stock_and_shapes`.
- `pick::nearest_outline_within_tolerance_wins` (port: `hitTest.test.ts` cases), `pick::click_cycles_overlapping_instances`, `pick::hidden_is_not_picked_locked_is`, `pick::toolpath_hit_selects_its_operation`, `pick::male_board_shape_selects_the_inlay` (acc 75).
- `handles::corner_scale_is_about_the_centre`, `handles::rotation_handle_rotates_about_the_centre`, `handles::uniform_unless_unlocked` (acc 66).
- `arrows::*` (port: `toolpathArrows.test.ts`, the rules and the oracle on its fixture).
- `scene::marks_equal_the_svg_marks` (acc 79), `scene::male_board_sits_beside_the_stock_in_display_only` (acc 76), `scene::stale_results_are_flagged_until_replaced` (acc 87).
- `tabs::tab_drag_snaps_to_the_outline_in_definition_space`, `tabs::add_tab_at_clicked_point_with_last_size`, `tabs::hollow_when_no_profile_crosses` (acc 78).

**Tree and placement (slice 5)**
- `tree::rows_follow_file_order_with_badges`, `tree::operations_targeting_each_instance`, `tree::duplicate_is_a_linked_copy` (acc 67), `tree::delete_removes_targets_and_tabs_and_warns` (acc 68), `tree::rename_is_one_step`.
- `properties::typed_x_y_w_h_rotation_mirror_produce_the_expected_transforms` (acc 66), `properties::multi_selection_shows_mixed_values` (port: `mixedValue.test.ts`).
- `commands::centre_on_stock`, `commands::fit_stock_to_shapes_equals_init` (acc 69).

**Import (slice 6)**
- `import::dxf_without_units_blocks_until_chosen`, `import::layer_subset`, `import::unread_entities_noted_once` (acc 64), `import::picture_at_width_equals_image_add` (acc 65), `import::one_import_is_one_undo_step`.

**Project panel (slice 7)**
- `project::stock_origin_heights_write_the_fields` (acc 69), `project::tool_change_z_offered_only_with_g53`, `project::post_switch_keeps_settings` (acc 70, with `switch_post`), `project::m6_and_canned_cycles_offered_per_definition`, `project::thickness_unset_disables_export`.

**Tools and library (slice 8)**
- `tools::add_from_library_equals_tools_add`, `tools::missing_rate_is_an_error_and_blocks_export`, `tools::remove_in_use_is_refused_naming_operations`, `tools::save_to_library` (acc 71).
- `library::filter_and_search` (port: `toolLibraryDialogModel.test.ts`), `library::fusion_import_report_equals_tools_import` (acc 71), `library::in_project_marking`.

**Generation and warnings (slice 9)**
- `generation::digest_covers_every_generator_input`, `generation::editing_one_operation_regenerates_only_dependents`, `generation::results_from_an_old_snapshot_are_dropped`, `generation::stop_clears_pending` (acc 87).
- `warnings::panel_equals_check_line_for_line` (acc 82, every fixture with expected `check` output), `warnings::located_rows_name_the_field`, `warnings::copy_is_the_cli_text`, `warnings::tiers_match_the_cli`.
- `kittest::window_answers_input_while_generating` (acc 87). "Answers input within 100 ms" is read over every UI frame from opening the 500-shape project, through *Stop*, to the end of *Regenerate All* (repeated until 60 frames are timed), including the frames that accept results. Each egui pass is timed on its own, so a click counts as three passes. The test needs at least 60 frames, the 95th percentile within 100 ms and no frame over 250 ms. It does not take the single slowest frame because every kittest test runs its own window and worker in parallel on CI's few shared cores, so scheduling can delay a frame that has little work (#573).

**Operations and editors (slices 10–15)**
- `editor::fields_table_matches_the_records` (acc 73, table-driven over every kind), `editor::set_and_get_by_key_path_round_trip`, `editor::tool_derived_defaults_show_the_tools_value_and_write_null`, `editor::located_warning_matches_its_row`.
- `operations::add_targets_the_selection_with_cli_defaults` (acc 72), `operations::add_menu_partitions_by_eligibility`, `operations::reorder_enable_duplicate` (acc 74), `operations::delete_keeps_placed_tabs`.
- `editor::clearing_list_orders_and_warns` (acc 75), `editor::inlay_both_boards` (acc 76, after #498), `editor::photo_v_carve_fields` (acc 77, after 3.5), `editor::drill_cycle_fields_follow_the_cycle`, `editor::profile_tab_request_and_place_tabs` (acc 78).

**Export (slice 18)**
- `export::plan_waits_for_generation`, `export::plan_lists_programs_as_the_cli_prints_them`, `export::board_tick_filters_without_saving` (acc 76, 85), `export::errors_disable_warnings_do_not` (acc 84), `export::override_not_saved` (acc 85), `export::preview_svg_equals_the_cli` (acc 83).
- `gui_equality::app_export_equals_cli_export_on_six_posts` (acc 83, `crates/dreamcarve/tests/`), `gui_equality::quick_start_parts_from_the_app` (acc 90).

**Shell (slices 3, 19, 20, 22)**
- `kittest::opens_plaque_renders_and_exports` (acc 92, three OSes), `kittest::unsaved_changes_prompt` (acc 88), `kittest::keyboard_shortcuts_reach_their_commands` (acc 54), `kittest::field_commits_on_blur_not_per_keystroke` (acc 86), `kittest::panels_collapse_below_the_minimum` (acc 50), `kittest::theme_follows_system_and_both_palettes_apply` (50a), `kittest::snapshot_workspace_light_and_dark` (Linux only, `experience.md` review).

## Slices

Ordered; each is one GitHub issue with the tests above as its Tests
section. Numbers in brackets are dependencies. The visual design
(`experience.md`) is applied from slice 3 on, not in a pass at the end;
slice 20 is the review against its checklist.

1. **`dc-job` and `dreamcarve-shell`.** Move the pipeline, the preview, `init`'s project builder, staged writes and config paths out of the CLI; `Tier`; `generate_operation` and `finish`; `program_names`; `preview::marks`; `with_defaults`. Every CLI test green, goldens byte-identical. Guidelines crate graph updated. [—]
2. **`dc-app` core.** `App`, `Command`, labels, history, dirty, selection, `View`, `Prefs`, strings table with `build.rs`, units. `Harness`. [1]
3. **Shell skeleton.** `dreamcarve-app` with eframe and wgpu; menus, toolbar, status bar, the four regions and their collapse and persistence; theme module from `experience.md`; open, save, save as, recent, drops, `--open`, unsaved prompt; the empty state; `gui.toml`. Smoke test on three OSes with CI's software adapters. [2]
4. **Viewport.** Camera, wgpu line renderer with screen-space widths, grid, stock, origin, park point, shapes, bitmaps; pan, zoom, fit; picking and selection sync; cursor readout. [3]
5. **Design Tree and placement.** Rows, flags, badges, rename, context menu; Properties placement fields; handles and drag; nudge; duplicate, delete; centre on stock, fit stock. [4]
6. **Import.** Drawing and picture sheets; DXF units and layers; warnings into the panel; *New from drawing* and *New from picture*. [3]
7. **Project panel.** Stock, origin, heights, tolerances, post-processor switch and settings; validation under fields. [3]
8. **Tools and the Tool Library.** Project tools tab and editor; the library window with search, filter, import report, export, remove, add to project, save to library. [3]
9. **Generation and the Warnings panel.** Worker thread, digests, staleness, Stop and Regenerate; the panel, tiers, grouping, located rows, copy; status-bar counts and progress. [4]
10. **Operations list and editor framework.** List with drag, enable, eye, status; Add menu partitioned by eligibility; `fields` table and the per-kind renderers; Targets group with Pick; tool-derived placeholders; located warnings under rows. [9]
11. **Profile, Pocket and Engrave editors.** Every field of the three, the tab request, `rest_of`, last pass, start point. Acceptance 73's harness. [10]
12. **Drill and Bore editors.** Cycles including countersink, pecks, retract; pitch and stock to leave. [10]
13. **V-Carve editor and clearing passes.** Strategy, flat depth, start plane; the clearing list with drag and `clearing_not_descending`. [10]
14. **Inlay editor and the male board.** Values once, male board group, derived line, male board on the Viewport, per-board export rows. [10, 4, #498 merged]
15. **Photo V-Carve editor and bitmaps.** Picture target, the fields, grooves on the Viewport. [10, phase 3.5 merged]
16. **Toolpath display.** Per-operation colours, marks, rapids, arrows, visibility panel, by-depth mode and legend, only-selected, extents box, stale fade, `lod`. [9, 4]
17. **Tabs.** Place tabs, drag along the outline, Tab tool, per-tab properties, hollow marks, the Tabs section of the tree. [5, 11]
18. **Export.** The sheet, program rows per board, override, preview tick, errors and warnings, staged write, result sheet, estimates in the status bar. [9, 7]
19. **Keyboard and units polish.** Every shortcut of capability 54, focus order, field commit rules, inch display, unit strings in fields. [10]
20. **Visual review.** `experience.md`'s checklist against every panel in both palettes; the snapshot pair; fixes. [3–19]
21. **Docs and vocabulary.** The quick start "From SVG to G-code in the app", the seven new chapters, the updated chapters with screenshots from the build, `CONTEXT.md`'s three terms, the contributing chapters. [20]
22. **Release.** cargo-dist app bundles for three platforms, file association, the bench check (coaster from the app), exit-criteria audit, changelog, `v0.4.0`. [21]

Slice 1 can start now, on `main`, while phase 3 finishes: it changes no
output. Slices 2–3 can follow immediately. Slice 14 waits for #498 and
slice 15 for 3.5; nothing else waits for phase 3.

Slice 20 (#549) took its first screenshots on 2026-11-10. The review
turned into the redesign below; #549's exit, the maintainer's sign-off
on a second set, is now slice 36, and slices 21 and 22 wait for it.

## Review redesign slices

The screenshot review of slice 20 (#549, 2026-11-10) changed the
functional spec in place ("Review redesign" there) and `experience.md`
(its section 15 and the amended sections). These slices carry it. Each
is one GitHub issue, opened after this spec merges, with the tests
below as its Tests section and #549 as its parent; the process is the
phase's (test first, the full gate, PR into `main`). Every slice's
screenshots are checked against `experience.md`'s checklist, items
1–34. Dependencies in brackets; "main" means it can start on `main` as
it stands. Slices with no dependency between them touch some of the same
files (`shell/status.rs`, `panels/editor.rs`); the second to merge
rebases, and none waits for another to start.

### Shell, layout and status bar

23. **Window title, panels and the CAM panel** (functional 50, 50a, 53,
    60; acceptance 93). The project's name without extension in the OS
    window title with `*` when unsaved, and no file name in the toolbar;
    toolbar labels *Add drawing* and *Add picture*; the left column's
    lower half as two collapsible panels, Project Settings on top and
    Properties on selection, scrolled into view; Project Settings' open
    rule (open on a new project and on a project-level error); Units as
    Project Settings' first row; the CAM panel's title band and the Tools,
    Operations tab order with the opening rule. [main]

    Tests:
    - `kittest::window_title_is_the_project_name_without_extension`: the
      plaque gives the title `plaque — DreamCarve`; after one edit,
      `plaque* — DreamCarve`; after Save, `plaque — DreamCarve` again;
      *New* gives `Untitled — DreamCarve`; no node in the accessibility
      tree has a label containing `plaque.dcv`.
    - `kittest::cam_panel_has_a_title_and_tools_first`: a node labelled
      `CAM` exists above the tab strip; the tabs' labels in order are
      `Tools · 3`, `Operations · 5`; a new project opens on Tools, the
      plaque on Operations.
    - `kittest::left_column_is_project_settings_then_properties`: with
      nothing selected there is a `Project Settings` panel and no
      `Properties`; with the square selected both exist, Project
      Settings' rect above Properties'; clicking each title collapses it
      to its 32-px band; the open state is in `Prefs` after a restart.
    - `kittest::project_settings_opens_on_a_project_error`: on a new
      project (thickness unset) it is open whatever `Prefs` says; on the
      plaque it follows `Prefs`.
    - `kittest::units_is_the_first_row_of_project_settings`: the first
      field under the title is the `mm` / `in` segmented control, and
      choosing `in` is one undo step that leaves the file's lengths
      unchanged.

24. **Status bar, toolpath view and the Warnings drawer** (functional
    50b, 71, 73, 73a, 76; acceptance 80, 94). The status bar's cells as
    data (`dc_app::status`), drawn by the shell: units menu, cursor,
    Toolpaths button, generation (blank, generating with Stop, stopped
    with *Regenerate all*), counts, estimate, Z range, message; no stock
    size. The toolpath view as a popover from the status bar with the
    by-operation key, replacing the Viewport card. The depth legend at the
    Viewport's bottom-left with the positive band and work-coordinate
    labels; Fit leaving room for it. *Regenerate all* removed from View
    and from the drawer; the drawer's group heading only with two or
    more groups. [main]

    Tests:
    - `status::cells_follow_the_state`: table-driven; a project with
      shapes and no operation has no Toolpaths cell and no generation
      text; the plaque mid-generation has `Generating 1 of 5…` and a
      Stop action; after Stop, `Stopped` and a Regenerate-all action;
      idle, the generation cell is `None`; with no cursor position the
      cursor cell is empty, never `X — Y —`.
    - `status::stock_size_is_never_a_cell`: for every fixture, no cell's
      text contains the stock's dimensions.
    - `view::depth_legend_ticks_include_clearance_and_safe_z`: on the
      plaque the ticks in order are `+10 clearance`, `+5 safe Z`,
      `0 stock top`, `−1 Border and squiggle`, `−4 Panel`, `−6 Square
      recess`, `−12 Screw holes, Cut out`; with Z zero at the stock
      bottom the stock top's tick is `+11.8` and the cuts' are positive
      or zero.
    - `view::by_operation_key_lists_each_operation_colour`: one entry
      per enabled operation in list order, colour from the palette by
      index, names from the file.
    - `kittest::toolpaths_button_opens_the_popover_and_escape_closes_it`:
      absent on a project with no toolpath; present on the plaque; a
      click shows a node `Toolpaths` with the eight toggles; Escape
      removes it; no Viewport card exists in either state.
    - `kittest::regenerate_all_only_after_stop`: no menu item and no
      drawer button is labelled `Regenerate all`; after Stop the status
      bar has one, and clicking it makes `pending()` hold every enabled
      operation.
    - `warnings::single_group_has_no_heading`: rows of a project whose
      findings all belong to one group yield no heading; with a project
      row and an operation row, two headings.
    - `kittest::units_menu_switches_display_units`: the status bar's
      units menu sets inches as one undo step labelled as the Project
      Settings switch is.

25. **Design Tree and Arrange** (functional 54, 57, 58, 58a; acceptance
    66, 95, 96). Shape glyphs (`square`, `circle`, `spline`) and no
    `open` badge; the `component` linked-copy badge; Rename on F2 and in
    the context menu; empty sections without a *None* row; the lock's
    tooltip and the locked Properties note with *Unlock*; Properties as
    X, Y, W, H with the proportion link to their right; the toolbar's
    Arrange group with the Rotate split button and its popover; the
    Arrange menu with *Centre on stock*; the shortcuts R, Shift R, Ctrl
    Shift H, Ctrl Shift V. [main]

    Tests:
    - `tree::rows_carry_a_shape_glyph`: on the plaque, `holes-1` is
      `Circle`, `outline` is `Closed`, `squiggle` is `Open`; no row
      model has a text badge.
    - `tree::empty_sections_have_no_placeholder_row`: the plaque's
      Pictures section yields its header and no row.
    - `kittest::f2_and_context_menu_rename_in_place`: F2 on the selected
      square and *Rename* in its context menu each turn the name into a
      field with the text selected; typing and Enter is one undo step
      `Rename "square"`; Escape leaves the file unchanged.
    - `arrange::rotate_90_and_flip_equal_the_typed_transforms`: the
      square's transform after Rotate (90° clockwise), after
      `rotate(30)` and after each flip equals the transform acceptance
      66 expects for the same rotation or mirror about the bounding-box
      centre, each one undo step with its label.
    - `arrange::disabled_with_a_reason`: nothing selected, all locked,
      only pictures: each gives its reason string key.
    - `arrange::partly_locked_turns_the_unlocked_and_names_the_rest`.
    - `properties::placement_is_x_y_w_h_only`: the field rows for a
      selection are exactly X, Y, W, H; `properties::broken_link_scales_one_axis`:
      with the link broken, typing W changes only the X scale.
    - `kittest::shortcuts_reach_the_arrange_commands`: R, Shift R, Ctrl
      Shift H and Ctrl Shift V each emit the command the toolbar emits.

### Operation editors

26. **Editor framework: header, Setup and summaries** (functional 66,
    66a, 66b; acceptance 73, 97). The editor header with the inline
    name; `Group::Identity` removed and `Group::Setup` first, with each
    kind's Setup rows moved in the table; other groups closed by default
    with their one-line summaries; a group with a located finding opens;
    the required-field state in `unit_field` (outline, glyph, *Required*,
    message as tooltip), used by the editors, Project Settings'
    thickness and, from slice 29, the tools. The *check says* box leaves
    the editor (its content arrives in slice 28). [main]

    Tests:
    - `editor::fields::setup_rows_lead_each_kind`: for each of the eight
      kinds the first rows are Targets (Picture for Photo V-Carve), Tool,
      then 66a's rows in 66a's order.
    - `editor::fields::every_field_appears_once`: no key path is in two
      groups; acceptance 73's table-driven test passes unchanged.
    - `editor::fields::no_identity_group`: no row has key `name` or
      `enabled`.
    - `editor::summary::closed_groups_summarise_their_values`: the
      plaque's Panel Pocket gives `Raster 0°, climb, wall last` for
      Strategy and `Safe Z 5 mm` for Heights; in an inch project
      `Safe Z 0.197 in`; a tool-derived `null` reads `tool's 3 mm`.
    - `editor::summary::located_finding_opens_its_group`: a
      `relief_corner_too_tight` on the Pocket opens Corners and leaves
      the others closed.
    - `kittest::editor_header_renames_in_place`: clicking the header
      name, typing and Enter is one undo step; Escape reverts; there is
      no `Enabled` checkbox and no `Name` field in the editor.
    - `kittest::required_field_shows_required_not_a_message`: a tool
      with no feed shows a Feed field whose placeholder is `Required`,
      with no label node under it carrying the message, and a tooltip
      equal to the file's message; Project Settings' thickness on a new
      project the same.
    - `kittest::editor_has_no_check_says_box`: no node labelled
      `check says` on any kind.

27. **Targets on the drawing** (functional 57, 67; acceptance 98). The
    target highlight on the Viewport and in the tree for the selected
    operation, with the hover preview from the list; the Setup Targets
    row (count, names, warning, Edit, `⋯`); target editing with its bar,
    click and marquee toggling, Done and Cancel; *Add to* and *Remove
    from* in the tree's context menu; *Use selection* and *Add
    selection*; Photo V-Carve's Picture dropdown. [26]

    Tests:
    - `scene::selected_operation_targets_are_marked`: selecting the
      plaque's Panel gives `pocket` and `island` the style
      `Target(colour 3)` and every other shape `Faded`; clearing the
      operation selection returns all to `Normal`.
    - `tree::target_rows_marked_for_selected_operation`.
    - `targets::editing_toggles_and_commits_one_step`: entering on
      Panel, clicking the square then the island, and Done emits one
      `UpdateOperation` whose targets are `pocket, square`, labelled
      `Set targets of "Panel"`; Cancel emits nothing and the saved file
      is byte-identical.
    - `targets::editing_blocks_selection_and_transforms`: while editing,
      a click never changes the instance selection and a drag emits no
      `TransformInstances`.
    - `targets::marquee_adds_alt_marquee_removes`.
    - `targets::row_summary`: `2 shapes · pocket, island`; a Pocket
      targeting the squiggle carries the warning flag.
    - `kittest::target_editing_bar_enter_is_done_escape_is_cancel`.
    - `kittest::tree_context_menu_adds_and_removes_targets`.

28. **Carve notes** (functional 50b, 76a; acceptance 82, 99).
    `dc_job::summary::carve_facts` and `describe_carves` as its
    formatter; notes in the drawer, last in their group, as labelled
    facts with board sub-headings; not counted; the selected
    operation's first fact in the status bar's message cell, opening
    the drawer at it. [main]

    Tests:
    - `summary::describe_carves_is_unchanged`: for every fixture with
      expected `check` output, the CLI's text is byte-identical (the
      CLI's own tests are the second guard).
    - `summary::carve_facts_format_to_describe_carves`: formatting each
      fixture's facts gives `describe_carves`' strings.
    - `warnings::notes_follow_their_operation_and_are_not_counted`: on
      the sign, Lettering's note is the last row of its group with a
      `Deepest cut` fact; the counts equal those without notes.
    - `warnings::inlay_notes_have_board_subheadings`: the inlay fixture's
      note has `Female board` and `Male board` sections.
    - `warnings::copy_includes_notes_as_check_prints_them` (acceptance
      82 stays line-for-line).
    - `status::selected_operation_summary_in_message_cell`.

### Tools and library

29. **Tools tab: New tool and Delete** (functional 63, 79; acceptance 71,
    100). *New tool* (`tool<n>`, `init`'s placeholder geometry, no rates,
    name focused); *Remove* renamed *Delete* everywhere it deletes; the
    rates' required-field look; *Save to library* in the editor's
    footer; the empty Tools list with *New tool* and *Add from
    library…*. [26]

    Tests:
    - `tools::new_tool_is_a_placeholder_named_by_number`: on an empty
      project the first is `tool1` (end mill, 6.35 mm, 2 flutes), the
      second `tool2`; on a project whose third tool is already named
      `tool4`, a new fourth tool is `tool5`.
    - `tools::new_tool_never_fills_a_rate`: feed, plunge rate, RPM,
      step-down and stepover are `None` (principle 5, S20).
    - `tools::export_refuses_a_new_tool_until_its_rates_are_set`: the
      findings equal the CLI's refusal for the saved file.
    - `kittest::new_tool_focuses_its_name`: after *New tool* the focused
      node is the Name field with its text selected.
    - `strings::no_deletion_is_labelled_remove`: no value in `en.toml`
      for a delete action reads `Remove`; `Remove from "{name}"` is the
      only `Remove` and is a target action.

30. **Tool Library: vendor groups, editing and units** (functional 64,
    64a; acceptance 101, first half). The two-pane sheet; groups by
    vendor; the editing pane writing the library on leave; *Revert
    changes*; *New tool*, *Duplicate*, *Delete* (asked in place);
    *Export library…* with its tooltip; the library's display units in
    `gui.toml`; the empty library's two buttons. [29]

    Tests:
    - `library::groups_by_vendor_with_no_vendor_last`: the fixture
      library's groups are sorted by vendor with `None` last; a search
      for `amana` leaves one group.
    - `library::edit_writes_the_library_and_not_the_project`: setting a
      library tool's feed yields library bytes equal to
      `dc_library::save` of the edited library; the open project and
      the undo history are unchanged.
    - `library::revert_restores_the_tool_as_opened`.
    - `library::units_are_display_only`: with inches the 6.35 mm end
      mill reads `0.25 in`; the library file and the project's units are
      unchanged; `Prefs::library_units` round-trips through `gui.toml`.
    - `library::new_and_duplicate`: *New tool* gives `tool<n>` numbered
      within the library; a duplicate has a new id, `source: None` and
      ` (2)` on its name.
    - `kittest::library_delete_asks_in_place_and_names_the_tools`.

31. **Library import review** (functional 64b; acceptance 71, 101,
    second half). The review replacing the panes in place; the counts;
    the Keep mine / Replace with imported choice shown only when a tool
    is *Changed*; the banner and its *Details*; no *Replace existing*
    checkbox on the table. [30]

    Tests:
    - `library::import_review_counts_equal_tools_import`: importing
      `fixtures/fusion/sample.tools` into the fixture library reports
      the counts `tools import` prints, and the library file is untouched
      until *Import*.
    - `library::changed_tools_offer_keep_or_replace`: after editing one
      imported tool, a second import shows one *Changed*; *Keep mine*
      writes what `merge(replace: false)` writes and *Replace with
      imported* what `merge(replace: true)` writes.
    - `library::no_choice_when_nothing_changed`.
    - `kittest::import_review_replaces_the_panes_in_place`: no second
      sheet opens; *Back* returns to the table unchanged.

### Export, its result and the setup sheet

32. **Export sheet trimmed** (functional 77; acceptance 83, 84, 102,
    first half). No preview tick, warnings list, total or footnote;
    *Program Details* closed by default; the one blocking line with
    *Show*; Export's disabled tooltip. The Viewport's SVG dump carries
    acceptance 83's preview check. [main]

    Tests:
    - `kittest::export_sheet_has_no_preview_warnings_total_or_note`: no
      node labelled with the preview tick, `Warnings`, `Estimated total`
      or `Every file is written`.
    - `kittest::program_details_start_closed`: the disclosure is closed
      on open and its rows equal `export::plan`'s when opened.
    - `export::blocking_line_names_the_error_count`: one error gives `1
      error blocks export`; none gives no line.
    - `export::preview_svg_equals_the_cli` is replaced by
      `scene::svg_dump_equals_preview` (acceptance 79, 83).

33. **Export result and setup sheet** (functional 77a, 77b; acceptance
    102, second half). The result in Status, Files and Run instructions;
    *Show in folder* and *Copy path*; *Save setup sheet…* in the footer
    and in the File menu after an export; `dc_job::setup_sheet::render`.
    [32]

    Tests:
    - `export::result_parts_are_status_files_and_instructions`: the
      plaque's export gives `2 programs written`, the files
      `plaque-1-T2.nc`, `plaque-2-T1.nc` in run order, the folder, and
      instructions equal to the CLI's step-before lines and notes.
    - `export::result_holds_every_fact_of_the_cli_report`: each line of
      the CLI's report text is in exactly one part.
    - `setup_sheet::render_is_self_contained`: the output has no
      `<script`, no `http`, no `<img` and no `url(`.
    - `setup_sheet::matches_the_export`: its programs, tools,
      step-before lines, extents and estimates equal `Built` and the
      `ProgramSummary` rows.
    - `setup_sheet::deterministic_with_the_date_pinned`: two renders are
      byte-identical and equal the committed
      `crates/dc-job/tests/fixtures/setup_sheet/plaque.html`.
    - `setup_sheet::uses_display_units`: an inch project's sheet shows
      inches.
    - `strings::setup_sheet_strings_are_keyed` (acceptance 91).

### Empty state, import and the File menu

34. **Empty state** (functional 79; acceptance 103, first half). The
    640-px card with the two wells, *Your drawing* and *Examples*; the
    example cards with thumbnails drawn from each example's shapes; the
    shorter copy; examples opening untitled; the empty Tools list's
    buttons are slice 29's. [main]

    Tests:
    - `kittest::empty_state_offers_two_paths`: nodes `Your drawing` and
      `Examples`; three example buttons; no label containing `phase` or
      `unsaved`.
    - `onboarding::example_thumbnails_draw_the_examples_shapes`: each
      thumbnail's outlines are the example's visible instances' outlines
      fitted to 96 × 56 with a 4-px inset (same count, same bounds
      ratio).
    - `kittest::example_opens_untitled`: the title is `Untitled —
      DreamCarve` and Save asks for a path.

35. **Import sheet, menus and the unsaved-changes sheet** (functional 51,
    55, 56, 79a; acceptance 63, 88, 103, second half). The import
    preview without the stock box or placement note; the File, Edit,
    Arrange, View and Project menus as 79a lists them, the labels *New
    Project from Drawing…*, *Add Drawing to Project…*, *Export G-code…*;
    View ▸ Appearance with System, Light and Dark; the unsaved sheet's
    *Save*, *Don't Save*, *Cancel* on close, quit, open and every *New*.
    [main]

    Tests:
    - `import::preview_is_the_drawing_alone`: the preview geometry's
      bounds equal the drawing's bounds and it holds no stock outline.
    - `kittest::file_menu_labels_in_order`: the File menu's items equal
      79a's list.
    - `kittest::appearance_has_three_checked_choices`: System is checked
      by default; choosing Dark checks it, sets `Prefs::theme` and
      applies the dark visuals.
    - `kittest::unsaved_sheet_is_save_dont_save_cancel`: on close, quit,
      open and each *New*, the sheet's buttons are exactly those three;
      *Don't Save* proceeds without writing (acceptance 88).
    - `kittest::save_on_untitled_asks_where_and_cancel_cancels_all`.

### Second visual review

36. **Second visual review** (functional 50a; acceptance 104; #549's
    exit). A new screenshot set of every panel, sheet, menu and popover
    in both appearances at 1× and 2×, each captioned with what it shows
    and the state it was taken in; `experience.md`'s checklist, items
    1–34, ticked; the snapshot pair regenerated; fixes. Maintainer
    sign-off closes #549; #550 (docs) and #551 (release) follow. [23–35]

    Tests:
    - `kittest::snapshot_workspace_light_and_dark` (Linux only),
      regenerated.
    - `theme::no_colour_literal_outside_theme` still green.
    - A checklist comment on #549 with every item ticked and every
      screenshot captioned.

## Open questions

Answered by the maintainer on 2026-11-03: every recommendation accepted.
The decisions stand as the slices assume them; the record stays here.

| # | Question | Recommendation |
| --- | --- | --- |
| 1 | Binary name and bundle: `dreamcarve-app` as the binary with "DreamCarve" as the bundle name, or fold the GUI into the `dreamcarve` binary behind a subcommand? | A separate binary. A GUI binary needs the Windows GUI subsystem (no console), a macOS bundle and a file association; the CLI needs none of those and stays scriptable. cargo-dist builds both. |
| 2 | Snapshot images on Linux only, with the other two OSes running behaviour tests without image comparison? | Yes. Font rasterisation differs per OS and the egui authors recommend behaviour tests over image comparison; one snapshot pair (light, dark) documents the look and catches regressions where it is stable. |
| 3 | `rfd` for native dialogs, or egui's own file browser? | `rfd`: native dialogs are what users expect and `DESKTOP_DESIGN.md` says the same. |
| 4 | Stop cannot interrupt a running generator. Add a cancellation token to the long generators (V-Carve medial, pocket raster) in a later phase, or accept that Stop drops the result and lets the thread finish? | Accept for phase 4; note it in the chapter. A cancellation point in `dc-cam` is a phase 6 concern when simulation adds longer jobs. |
| 5 | Operation palette (`experience.md`, "Operation colours"): four of the CLI's eight `--preview` colours sit at 2.2–2.6:1 on white, and the designer proposes darkened light-mode hexes for green, purple, orange, cyan and pink, plus a dark-mode set. Functional 73 says the app uses the same palette as `--preview`. Change `preview.rs`'s `PALETTE` to the proposed light set, so the CLI's SVG and the app agree, or keep the CLI's hexes in light mode? | Change `preview.rs` to the proposed light set in slice 16, same order, so there is one light table and acceptance 79 holds; the dark set is the app's alone, since the SVG is on white. The goldens do not contain the preview, so no bytes under test change; the quick-start pictures are regenerated once. |
| 6 | macOS menu bar: egui's in-window menu bar on every platform, or a native `NSMenu` on macOS? | egui's on all three for phase 4: one implementation, one test path, every shortcut in one place. A native bar is phase 9 polish, where signing and bundle work already touch the macOS shell. |

Raised by the review redesign (2026-11-10), open until the maintainer
answers on the spec PR:

| # | Question | Recommendation |
| --- | --- | --- |
| 7 | The setup sheet (functional 77b) is rendered in `dc-job`. Should the CLI get `export --setup-sheet` in phase 4, so the app's HTML is also the CLI's, as S20's spirit suggests? | Not in phase 4. The renderer lives in `dc-job` so the flag is a small later change; adding CLI surface now widens the release for a feature nobody has asked for from the command line. |
