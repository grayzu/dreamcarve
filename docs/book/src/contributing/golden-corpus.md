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
| `crates/dc-gcode/tests/golden.rs` | One test per case. Each builds the DreamCarve project equivalent to the `.camj`. |
| `crates/dc-gcode/tests/golden/harness.rs` | The comparison. |

## The cases

| Case | Covers |
| --- | --- |
| `profile-outside-rect` | Outside Profile of a 100 × 50 mm rectangle, off-centre, so a lost Y flip moves every point. |
| `profile-inside-circle` | Inside Profile of a 60 mm circle, 10 mm deep in 4 mm passes: the last pass is shallower. |
| `profile-ring` | A circle in a circle: the inner one cut as a hole. PureCutCNC needs two operations for what DreamCarve does in one. |
| `profile-tabs-circle` | The coaster: a 90 mm circle with four rect tabs. |
| `smoke` | The phase 0 check that capture runs and is repeatable. No test compares it. |

Each case's `README.md` lists its known differences from PureCutCNC. Read
it before you change the case or the code it covers.

## Run the comparison

```sh
cargo test -p dc-gcode --test golden
```

It also runs as part of `cargo test --all`.

## What the harness checks

`harness::check` emits DreamCarve's program for the case, reads both
programs back as a controller would, and compares them in the machine frame
they share:

1. **Safety** on both programs: no Z below the stock bottom minus the
   allowance, rapids only at or above safe Z, and every cut at one of the
   tool's two rates.
2. **Levels.** The Z values of the cutting passes are the same.
3. **Moves per level.** At each level, every point of every move, sampled
   every 0.05 mm, lies within 0.01 mm (plus rounding) of the other
   program's moves, travels the same way, and is cut at the same feed. The
   total cut length per level agrees. Comparing sets of moves lets the two
   programs start a loop at different points, or cut contours in another
   order.
4. **Vertical moves.** Plunges and lifts have the same feeds, except where
   the case lists a `Difference`.

Header, tool-change and spindle lines are not compared: they differ by
design.

On failure, the message names the check and prints a unified diff of the
two programs, with the dated header line normalised.

## Intentional differences

Where DreamCarve deliberately differs from PureCutCNC, the harness checks
each program against its own rule rather than comparing them. Each
difference is:

- a variant of `harness::Difference`, such as `RectTabStepsAtPlungeRate`
  (DreamCarve steps onto and off a rect tab at the plunge rate; PureCutCNC
  at the cutting feed);
- listed in the `differences` of each case that exercises it;
- explained in the case's notes, and so in its `README.md`.

A difference needs a reason in a spec or a safety rule. "Our output is
different" is not one: fix the port instead.

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
4. Add a `#[test]` to `crates/dc-gcode/tests/golden.rs` that builds the
   equivalent project with the file's `project`, `rect` and `circle`
   helpers and calls `harness::check`. Its doc comment says what the case
   covers and how its Y-down coordinates map to Y-up.
5. Run `cargo test -p dc-gcode --test golden`.
6. Commit the case, its notes, the golden files, and the test together,
   with a message that says what the case covers.

The DreamCarve stock and origin must give the same machine frame as the
`.camj`. A point at PureCutCNC `y` is at DreamCarve `height − y`.

## Change a golden file

Never edit a golden file by hand. Re-capture it, and commit it with a
message that says why it changed: a new PureCutCNC commit, a changed case,
or a fixed capture script.
