# Golden corpus

The golden corpus is PureCutCNC's own output for a set of small jobs,
committed to this repository. `cargo test` compares DreamCarve's program for
the same job against it. It is how a port proves it matches PureCutCNC, and
how a Y-up mistake is caught (see [Porting](porting.md)).

Node is needed only to capture a case. Running the comparison needs Rust
alone.

## Where things are

| Path | What |
| --- | --- |
| `tools/parity/cases/<case>.camj` | The PureCutCNC project for each case. |
| `tools/parity/cases/<case>.notes.md` | Notes copied into the case README: the equivalent DreamCarve project, the tolerance, and every known difference. |
| `tools/parity/capture.ts` | Runs PureCutCNC's engine on a case and writes its output. |
| `tests/golden/<case>/` | The captured output: `program.nc` (G-code), `toolpath.json` (PureCutCNC's toolpath, Y-down), and `README.md` (the PureCutCNC commit and the notes). |
| `tests/golden/<case>/dreamcarve.dcv` | The equivalent DreamCarve project, for the phase 2 cases. Written by hand, not captured. |
| `crates/dc-gcode/tests/golden.rs` | One test per case. The phase 1 tests build their DreamCarve project in Rust; the phase 2 tests load `dreamcarve.dcv`. |
| `crates/dc-gcode/tests/golden/harness.rs` | The comparison. |

## The cases

| Case | Covers |
| --- | --- |
| `profile-outside-rect` | Outside Profile of a 100 × 50 mm rectangle, off-centre, so a lost Y flip moves every point. |
| `profile-inside-circle` | Inside Profile of a 60 mm circle, 10 mm deep in 4 mm passes: the last pass is shallower. |
| `profile-ring` | A circle in a circle: the inner one cut as a hole. PureCutCNC needs two operations for what DreamCarve does in one. |
| `profile-tabs-circle` | The coaster: a 90 mm circle with four rect tabs. |
| `pocket-offset-rect` | Offset Pocket of a 60 × 40 mm rectangle. |
| `pocket-offset-island` | Offset Pocket round a circular island: each ring starts at its nearest point, on an arc too (#196). |
| `pocket-raster-0deg`, `pocket-raster-45deg` | Raster Pocket at 0° and 45°. The `.camj` says −45°: PureCutCNC's angles are mirrored by its Y-down frame. The scan starts at the machine-frame top, PureCutCNC's Y-down `minY` (#195). |
| `pocket-rough-then-rest` | A rough leaving 0.5 mm, then a rest Pocket with a smaller tool, in one program with `M6`. |
| `drill-simple`, `drill-peck` | Four holes drilled in order, simple and peck. |
| `engrave-open-closed` | Engrave of an open polyline and a closed rectangle. |
| `profile-ramp`, `profile-helix-inside` | Profile entries: a ramp on an outside cut, a helix on an inside one. |
| `profile-lead` | Lead-in and lead-out arcs on an outside Profile. |
| `profile-dogbone` | Dogbone relief on an inside Profile. |
| `order-full-depth-first`, `order-pass-first` | Three parts with holes, in two operations, compared in order. |
| `smoke` | The phase 0 check that capture runs and is repeatable. No test compares it. |

The `post-*-coaster` cases are different: DreamCarve captured them itself,
after their post-processor definitions were checked against the
controllers' documentation. `crates/dreamcarve/tests/cli.rs` compares them
byte for byte. Each one's `README.md` says how.

Each case's `README.md` lists its known differences from PureCutCNC. Read
it before you change the case or the code it covers.

## Run the comparison

```sh
cargo test -p dc-gcode --test golden
```

It also runs as part of `cargo test --all`.

## What the harness checks

`harness::check` generates every enabled operation of the case's project
and emits them as one program, with `T<n> M6` between tools, as
`dreamcarve export --one-program` would. It reads both programs back as a
controller would and compares them in the machine frame they share:

1. **Safety** on both programs: no Z below the stock bottom minus the
   allowance, and every cut at one of the project's tool rates. A rapid
   that moves in XY must be at or above safe Z. A Drill may also rapid
   straight down to its retract plane, and inside its hole between pecks.
2. **Levels.** The Z values of the cutting passes are the same.
3. **Moves per level, after start alignment.** Each run of horizontal
   cuts is split into three kinds of move. *Loops* end where they began: a
   lap, a pocket ring. *Links* are single moves from the end of one loop
   to the start of the next. *Open paths* are the rest: an engraved line,
   raster lines. At each level, every point of every loop and open path,
   sampled every 0.05 mm, lies within 0.01 mm (plus rounding) of the other
   program's moves. It travels the same way as a move there and is cut at
   the same feed. The total length of the loops and open paths agrees.
   Links move with their loops' start points, so only their lengths are
   compared. Comparing sets of moves lets the two programs start a loop at
   different points, or cut contours in another order.
4. **Sloped moves.** Ramps and helices are compared the same way, in three
   dimensions.
5. **Vertical moves.** Plunges and lifts have the same feeds, except where
   the case lists a `Difference`.
6. **Holes.** For a project with a Drill, the holes are compared in the
   order they are drilled: the same places, depths and plunges.

Header, tool-change and spindle lines are not compared: they differ by
design.

On failure, the message names the check and prints a unified diff of the
two programs, with the dated header line normalised.

## Comparing order

Sets do not see the order of the cuts. A case built with `Order::InOrder`
compares that too (functional spec, acceptance test 38). Each program's
runs of horizontal cuts at one Z are taken in program order, and the n-th
run of each must match as a set: one pass of one shape, in the same
sequence in both. `order-full-depth-first` and `order-pass-first` use it.

## Several operations

A case may have several operations, as `pocket-rough-then-rest` and the
`order-*` cases do. The harness numbers each move by the
`( Operation k: … )` comment above it, which both programs write, so a
`Difference` can single out one operation. The `.camj` and the `.dcv`
list their operations in the same order. Where PureCutCNC needs a
different set of operations for the same cuts, the notes say so: it
routes an outside edge only round `add` features and an inside edge only
round `subtract` ones.

## Intentional differences

Where DreamCarve deliberately differs from PureCutCNC, the harness checks
each program against its own rule rather than comparing them. Each
difference is:

- a variant of `harness::Difference`, such as `RectTabStepsAtPlungeRate`
  (DreamCarve steps onto and off a rect tab at the plunge rate; PureCutCNC
  at the cutting feed);
- listed in the `differences` of each case that exercises it;
- explained in the case's notes, and so in its `README.md`.

| Difference | Reason | What is checked instead |
| --- | --- | --- |
| `RectTabStepsAtPlungeRate` | Phase 1 design, "Profile contour" | Tab steps at the plunge rate in DreamCarve, the cutting feed in PureCutCNC. |
| `EntryPlacement` | #147, #165, #174 | The laps are compared, not the ramp or helix. Both programs descend at the entry angle; a helix has the entry's diameter and turns the same way in both; DreamCarve's ramp starts on the path it enters. |
| `PecksFromRetractPlane` | #153 | Hole positions, depths and order. Each program's pecks follow its own rule. |
| `RestMachiningAgainstFinishPass` | #163 | The rough is compared. Both programs cover the pocket at full depth; the rest cuts every level, the finish only the last. |
| `RasterLineOrder` | #146, decision 5; #150 | Raster lines are compared without direction; links between lines are not. PureCutCNC orders the pieces nearest-first outright; DreamCarve prefers pieces a link inside the region reaches, and keeps scan order where that lifts less (#150). |
| `LeadArcsAtCuttingFeed` | #185 | Lead arcs at the tool's feed in DreamCarve, below it in PureCutCNC. |
| `PassOrderWithinLayer` | Design, "Sequence", D5 | The passes are compared in order, each as a set. DreamCarve's walk within a pass is checked against D5. |

A difference needs a reason in a spec, a recorded decision or a safety
rule. "Our output is different" is not one: fix the port instead. When a
case shows a real bug, file an issue with the diff and a proposed fix, and
mark the test `#[ignore = "#<issue>: <what differs>"]` until it is fixed.
Never widen the tolerance to make a case pass.

## Add a case

1. In PureCutCNC, build the job and save it as
   `tools/parity/cases/<case>.camj`. Keep it small. Freeze `meta.created`
   and `meta.modified` so a re-capture is byte-identical.
2. Write `tools/parity/cases/<case>.notes.md`: the equivalent DreamCarve
   project (stock, origin, shapes in Y-up coordinates), the tolerance, and
   every intentional difference.
3. Capture, from `tools/parity/`, with a PureCutCNC checkout that has run
   `npm install`:

   ```sh
   npx tsx capture.ts --purecutcnc ../../../purecutcnc --case <case>
   ```

   Run it twice; the output must not change.
4. Write the equivalent DreamCarve project as
   `tests/golden/<case>/dreamcarve.dcv`. Then add a `#[test]` to
   `crates/dc-gcode/tests/golden.rs` that loads it with `case(…)` and calls
   `harness::check`, with the case's differences and order mode. Its doc
   comment says what the case covers and how its Y-down coordinates map
   to Y-up.

   PureCutCNC turns on some things by default that DreamCarve does not
   do. Set them in the `.camj`: `roundOutsideCorners: false` on a Pocket,
   `roundLinkCorners: false`, `cleanWallCorners: false`, and
   `pocketSlotFeedPercent: 100`, or the slot cuts run at 60 % feed. A
   `circle` segment is sampled coarsely. Where its offsets must match
   within 0.01 mm, draw a fine polygon on the circle in the `.camj`
   instead.
5. Run `cargo test -p dc-gcode --test golden`.
6. Commit the case, its notes, the golden files, and the test together,
   with a message that says what the case covers.

The DreamCarve stock and origin must give the same machine frame as the
`.camj`. A point at PureCutCNC `y` is at DreamCarve `height − y`.

## Change a golden file

Never edit a golden file by hand. Re-capture it, and commit it with a
message that says why it changed: a new PureCutCNC commit, a changed case,
or a fixed capture script.
