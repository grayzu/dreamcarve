# Testing post-processors end to end

Status: plan for issue #299. It states, for each controller DreamCarve
writes for, what is checked automatically and how, what cannot be, and
the manual procedure that covers the rest. Sources were read on
2026-10-21; versions are given where they matter.

The maintainer's machine runs grblHAL and GRBL, so those two can be
tested on a real machine. LinuxCNC, Mach3, UCCNC and generic RS274/NGC
cannot. This plan gets as close to a real run as the tools allow, and is
honest about where that stops.

## The layers

Each layer checks something the one below it cannot.

| Layer | What it shows | How | Runs |
| --- | --- | --- | --- |
| 0. Unit, golden and safety tests | DreamCarve writes what its definitions say. Its own reading of the program (`dc-gcode/src/parse.rs`) passes S1-S16. The motion is the same on all six controllers (`post::linuxcnc_mach3_uccnc_generic_differ_only_in_dialect`). Golden programs are byte for byte. | `cargo test --all` | Every PR |
| 1. Reference interpreter accepts it | LinuxCNC's own interpreter runs the LinuxCNC and generic programs to `M2`/`M30` with no error or warning. | `rs274 -g`: PR #264, `tools/gcode-conformance` | CI once #264 merges |
| 2. Same motion as the reference interpreter | The moves `rs274` makes are the moves DreamCarve's reading says: kind, end point, arc centre, feed, dwell, canned-cycle expansion and `G53` lifts. The safety pass checks DreamCarve's reading, so this checks that the pass checks the real motion. Covers LinuxCNC, generic, Mach3 and UCCNC, with the bridge below. | `tools/rs274-motion` (this issue) | Locally now; CI proposed on #264 |
| 3. Full controller simulation | LinuxCNC's task and motion layers, beyond the interpreter: the motion task sends to motion, `M0` pauses, homing, soft limits on a `G53` lift, INI units and startup codes, the end position. | `tools/linuxcnc-sim` (#328): `linuxcnc -r` with a sim INI, headless, with `motion-logger` and with the motion module | Locally; CI proposed on #264 |
| 4. The controller's own software | Mach3 and UCCNC read the program as their manuals say. | Manual, in each product's documented demo or simulation mode, on Windows | Before a release, by a person |
| 5. A real machine | It cuts what the preview shows. | Air cut, then foam or scrap, on grblHAL and GRBL. Other controllers need a user with one. | Before a release, by a person |

Layers 0 to 3 are automatic; layer 3 runs locally for now. Layers 4 and 5
are manual, and the post-processors chapter keeps saying "Not yet
checked on a real machine" for a controller until layer 5 has been done
on it.

## Controller by controller

### LinuxCNC

**Automatic.**

- Layer 1: PR #264 runs `rs274` on the LinuxCNC golden, the canned-cycle
  coaster and the plaque. It also runs probes: an arc 1 mm off its
  circle must be rejected, and so must `G90.1` followed by an incremental
  arc.
- Layer 2: `rs274-motion` runs the golden, the version 1 expected program,
  and the plaque in five variants. The variants are plain moves, `G81`
  canned cycles, `G82` with a dwell, a `G53 G0 Z-5` lift, and inches. For
  each it compares `rs274`'s canonical calls with DreamCarve's reading,
  source line by source line. The parameter file sets a G54 offset of
  (25.4, 50.8, -76.2) mm. Under it a `G53` move lands at work Z 71.2, so
  the check sees `rs274` go to machine Z -5, not work Z -5. A sixth
  variant adds manual spindle control, so the program has `M0` pauses,
  one right after the lift.
- Layer 3: `rs274` is the interpreter only. It does not run LinuxCNC's
  task layer or trajectory planner, so it does not apply soft limits,
  does not stop at `M0` (it prints `PROGRAM_STOP()` and carries on), and
  needs no homing. `linuxcnc-sim` runs the same LinuxCNC and generic
  programs under `linuxcnc -r`, headless, with a small Python UI in place
  of a GUI, as LinuxCNC's own `tests/motion-logger` does. The INI is a
  three-axis mill in mm with the same G54 offset. Three sessions run:
  - With `motion-logger` in place of the motion module, every move task
    sends to motion is compared with DreamCarve's reading, line by line,
    with `rs274-motion`'s comparison. Inch programs go through the mm
    INI.
  - The same, for the LinuxCNC programs, under the startup code
    `G20 G91 G90.1 G18 G93 G55 G99`, which the header must override.
  - With the motion module on a homed machine, each program must run to
    completion. Each `M0` must pause, with the machine where DreamCarve
    reads it, until the UI resumes it. The run must end where DreamCarve
    reads it.

  Tests add a machine whose Z travel ends at machine -20 mm, where the
  lift to -5 must end the run with LinuxCNC's soft-limit error, and an
  unhomed machine, which must refuse to run.

**Not automatic, and why.**

- The machine's own INI is the user's. Layer 3 covers units and the
  modes the header resets, under a startup code that sets each of them.
  Other state the user's machine may carry, such as a `G92` offset in
  the parameter file or a tool length offset, is not modelled.
- Real-time behaviour (following error, servo timing) is not simulated:
  each joint's command is looped back as its feedback.

**Manual procedure (layer 4/5 stand-in).** Use LinuxCNC's own simulator,
which is free:

1. On Debian 12 or 13, install `linuxcnc-uspace`, or use the LinuxCNC
   live image.
2. Start `linuxcnc` and choose `sim/axis/axis_mm.ini`.
3. Home all axes. A `G53` lift needs a homed machine.
4. Touch off a work Z zero above the table, as you would on stock.
5. Open the `.ngc` file. In the AXIS preview, check that the toolpath
   matches `dreamcarve export --preview`.
6. Run it. Check that each `M0` stops with its message, that the lift
   goes to the machine Z the export names, and that the program ends with
   the spindle off.

### Generic RS274/NGC

The generic definition follows NIST's RS274/NGC report (NISTIR 6556),
which LinuxCNC's interpreter descends from.

**Automatic.** Layers 1 and 2 run as for LinuxCNC, on the golden and the
plaque. Generic has no canned cycles and no `G53`, so the plain variant
is the one that runs.

**Not automatic, and why.** "Generic" names no real controller, so no
end-to-end test can exist. What can be shown is that the reference
interpreter reads it as DreamCarve does.

**Manual procedure.** None of its own. Users run it on a controller
whose dialect matches, and that controller's procedure applies.

### Mach3

**Automatic (layer 2, bridged).**

- Mach3's manual (*Using Mach3Mill* 1.84, chapter 10) follows the NIST
  report for the words DreamCarve writes: `G0` to `G3` with incremental
  `I`/`J` after `G91.1`, `G81` and `G82` with `G98`, `G4 P` in seconds,
  and `N` words. `rs274-motion` runs the Mach3 golden, the version 1 and
  2 programs, and the plaque (plain, `G81`, `G82`) through `rs274`.
- The bridge drops `G50` and `G69`, which LinuxCNC does not have. LinuxCNC
  has no scaling and no `G68` rotation, so the identity those words
  restore is already in force.
- The bridge also renumbers every line, so a canonical call names its
  source line. `N` is a label; renumbering moves nothing.
- Every bridged line is printed.

What this shows: if Mach3 behaves as its manual says, it makes this
motion.

**Not automatic, and why.**

- Mach3 is proprietary, closed and Windows-only. It is still sold by
  ArtSoft ($175; <https://machsupport.com/licensing/>).
- Its documented demo mode runs "Without a license", limited to 500
  lines for Mill. The vendor recommends Simulation mode, which "can be
  accessed by uninstalling (or not installing) the Parallel Port Driver",
  for testing code on a computer with no machine
  (<https://www.machsupport.com/?p=321>).
- It has no documented way to export or log the toolpath it runs. Reading
  its DROs from a VB macro is in a 2010 draft macro reference that is
  hosted only by third parties. AGENTS.md forbids inspecting a
  proprietary product to learn how it computes, so we do not script
  around that.
- Its EULA is not published on the site. Whether it may run unattended
  in a VM or on a CI runner is not known. Nothing goes into CI until a
  maintainer has read the installer's licence and found that it allows
  this.
- What the bridge removes (`G50`, `G69`), and Mach3's own settings ("G04
  Dwell param in ms", the IJ mode, "Ignore Tool Change"), can only be
  checked in Mach3 itself.
- Mach4 is a different product with its own dialect (Fanuc-style
  macro-B). This plan does not cover it, and neither does the Mach3
  definition.

**Manual procedure (layer 4).** On a Windows PC or VM, by a maintainer or
a volunteer who has read the licence:

1. Install Mach3 without the parallel port driver, which gives
   Simulation mode with no line limit.
2. In Config > General Config, check that "G04 Dwell param in ms" is
   unchecked, as the Mach3 chapter tells users. Set the IJ mode to
   absolute on purpose: the header's `G91.1` must override it, and the
   circles must still come out right.
3. Load the `.tap` from `dreamcarve export --post mach3`. Mach3 must
   load it with no error, including the `G50` and `G69` lines.
4. Compare Mach3's toolpath display with `dreamcarve export --preview`.
5. Run it. Check that the dwell after `M3` lasts the configured seconds,
   that `M0` stops, and that `M30` ends the program.
6. Repeat with canned cycles on (`use_canned_cycles`) and a simple Drill
   with a dwell, which writes `G81` and `G82`. The holes must be where the
   preview shows them.
7. Record the Mach3 version and the result on the release's checklist
   issue.

### UCCNC

**Automatic (layer 2, bridged).** `rs274-motion` runs the UCCNC golden,
the version 1 and 2 programs, and the plaque in mm and in inches. The
bridge makes three changes, each from the definition, and prints each
line it changes:

- It drops `G50` and `G69`, as for Mach3.
- It divides the `P` of `G4` by 1000. UCCNC's `P` is milliseconds,
  LinuxCNC's is seconds.
- It puts `G21` or `G20` on the first line with code. UCCNC writes no
  units word; the controller reads lengths in the units it is set up in.

**Not automatic, and why.**

- UCCNC is proprietary and Windows-only, from CNC Drive. Its page says it
  "Runs in demo (simulation mode) without a software license key", and
  that licence keys are tied to the motion controller's serial number,
  not to a PC (<https://www.cncdrive.com/UCCNC.html>, version 1.2115). No
  line limit is documented.
- It documents no toolpath export. Its C# macros can read machine
  positions, but those are forum examples, not a documented export.
- Its licence text was not found online, so CI use is out for the same
  reason as Mach3.
- The millisecond dwell and the missing units word are what makes UCCNC
  differ from LinuxCNC, and the bridge rewrites exactly those. `rs274`
  cannot check them; only UCCNC can.

**Manual procedure (layer 4).** On Windows:

1. Install UCCNC and start it with no controller, which gives demo mode.
2. Set the units to mm.
3. Load the `.txt` from `dreamcarve export --post uccnc`. Compare the
   toolpath view with the preview.
4. Run it. The dwell after `M3` should last 3 s, not 3000 s or 3 ms. `M0`
   should stop, and the program should end with `M30`.
5. Repeat with an inch project and the units set to inches.

### grblHAL and GRBL

These are out of scope for #299, which asks about the other four, but
they are the controllers with a real machine. They are the trustworthy
layer 5 for the motion that all six share. `post::linuxcnc_mach3_uccnc_generic_differ_only_in_dialect`
already shows that the XYZ motion is the grblHAL program's.

**Automatic.** The grblHAL Simulator (<https://github.com/grblHAL/Simulator>,
GPL-3.0, active in October 2026) builds the real grblHAL core for a
desktop and has a `grblHAL_validator`. It is the grblHAL counterpart of
`rs274`. A follow-up issue proposes running the grblHAL corpus through
it. grbl-sim (<https://github.com/grbl/grbl-sim>, GPL-3.0) does the same
for GRBL 1.1, but it is a proof of concept.

**Manual procedure (layer 5): air cut, then a real cut.** On the
maintainer's machine:

1. **Check mode.** Send `$C`, then stream the program. GRBL's check mode
   "does not move any of the axes, ignores dwells, and powers off the
   spindle and coolant". It catches parser errors and soft-limit
   violations ([Grbl v1.1 Commands](https://github.com/gnea/grbl/wiki/Grbl-v1.1-Commands);
   grblHAL keeps `$C`). Send `$C` again to leave it, which resets.
2. **Air cut.** Home. Set work Z zero at least the deepest cut plus 5 mm
   above the stock top, so every move happens in air. Leave the spindle
   off, or on at low speed if the program's `M3` and dwell are part of the
   test. Start with feed override at 50 % (real-time `0x90` to `0x94`).
   Keep a hand on feed hold. Watch for:
   - each rapid at safe Z;
   - each plunge at the plunge rate;
   - arcs turning the way the preview shows;
   - every `M0` stopping with its message, in senders that show one;
   - the `G53` lift going to the machine Z the export names;
   - the end at the park position.
3. **Foam or scrap.** Run it again with the real Z zero in foam or scrap
   wood, at full feed. Measure a pocket depth and a hole position against
   the project.
4. **Record it.** Note the controller version, the sender, the program
   and the result on the release checklist issue.

## What converting to grblHAL would test

A converter would take a LinuxCNC, Mach3 or UCCNC program and rewrite it
for grblHAL so the maintainer's machine could run it.

- To run on grblHAL it must remove or translate exactly the lines that
  make each dialect differ: `%`, `N` words, `G50`, `G69`, `G91.1`, the
  millisecond dwell and the missing units word.
- What is left is the motion body, which is already the grblHAL
  program's (layer 0). Running it tests grblHAL's reading of grblHAL
  motion, which running DreamCarve's own grblHAL program tests better.
- It tests nothing about how LinuxCNC, Mach3 or UCCNC read the original.

So no converter is planned. The one partial exception is canned cycles.
grblHAL implements `G81`, `G82`, `G83`, `G73`, `G98` and `G99` itself
([grblHAL wiki](https://github.com/grblHAL/core/wiki/Additional-G--and-M-codes)).
An air cut of the `G81`/`G98` lines from a LinuxCNC or Mach3 program
therefore runs a third independent implementation of the RS274/NGC cycle
rules on a real machine. That is optional, and tells us about grblHAL's
cycles, not Mach3's.

Existing tools that expand cycles are not authorities for this either:

- bCNC (GPL-2.0) has `macroGroupG8X`. It drops `G98`/`G99` and does not
  handle `G73`.
- pygcode's `--canned_expand` (GPL-3.0) hard-codes its retract and
  dwell.

## Options considered

| Option | Licence | Fidelity to these dialects | Decision |
| --- | --- | --- | --- |
| LinuxCNC `rs274`, the standalone interpreter in `linuxcnc-uspace` | GPL-2.0 (run as a separate process) | It is LinuxCNC's interpreter. It expands canned cycles (G73 and G83 back off 0.254 mm), handles `G53`, `G98`/`G99`, comments and `N` words. Mach3 and UCCNC need the bridge. | **Used**: layers 1 and 2 |
| LinuxCNC full sim, `linuxcnc -r <ini>` with `motion-logger` and the Python `linuxcnc` module, no display | GPL-2.0 | Adds the task layer, planner and soft limits. LinuxCNC's own tests (`tests/motion-logger/basic`) run it headless with `DISPLAY` unset, and its CI runs them in `debian:bookworm` containers. | **Used**: layer 3, `tools/linuxcnc-sim` (#328), in the `rs274-motion` image |
| LinuxCNC Docker images | `jeffersonjhunt/linuxcnc` and `meri/linuxcnc` state no licence and were last updated 6 and 10 years ago. There is no official image. | | **Not used.** `tools/rs274-motion/Containerfile` installs Debian 13's own `linuxcnc-uspace` 2.9.4, signed by Debian. #264 pins 2.9.10 from linuxcnc.org. |
| CAMotics 1.2 (`planner`, `gcodetool`, `camsim` CLIs) | GPL-2.0+ | "A subset of LinuxCNC GCode" (<https://camotics.org/gcode.html>). Its own list of missing codes includes G73 and G83. | Not used. `rs274` is LinuxCNC itself. CAMotics could later check the cut stock (`camsim` writes an STL), which is a different question. |
| grblHAL Simulator, grbl-sim | GPL-3.0 | The real firmware core | Follow-up issue, for grblHAL and GRBL |
| pygcode | GPL-3.0, last commit 2018 | Low: fixed retract and dwell in its cycle expander | Not used |
| gcodeparser (Python), cncjs gcode-interpreter, gcode-preview | MIT | Parsers or viewers with no machine state or canned cycles | Not used |
| Mach3 in a Windows VM or CI runner | Proprietary; EULA not published | The real product | Manual only (layer 4), until a maintainer has read the licence |
| UCCNC demo mode in a Windows VM | Proprietary; licence text not found | The real product | Manual only (layer 4) |

## Running layer 2

```sh
podman build -t dreamcarve-rs274 tools/rs274-motion   # or docker build
DREAMCARVE_RS274=podman:dreamcarve-rs274 cargo test -p rs274-motion -- --nocapture
cargo run -p rs274-motion -- --rs274 podman:dreamcarve-rs274            # the corpus, with a report
cargo run -p rs274-motion -- --rs274 podman:dreamcarve-rs274 --post mach3 part.tap
```

With `rs274` on `PATH` (Debian or Ubuntu with `linuxcnc-uspace`), no
variable is needed. Without any `rs274`, the two tests that need it pass
and print why they skipped. The test that checks the corpus covers each
controller and feature always runs. More in
[`tools/rs274-motion/README.md`](../../tools/rs274-motion/README.md).

On 2026-10-21 all 31 corpus programs agreed with `rs274` 2.9.4. A
deliberately deepened cut was reported on its line. With the
manual-spindle variant added for layer 3, all 33 agreed.

## Running layer 3

```sh
podman build -t dreamcarve-rs274 tools/rs274-motion   # the same image
DREAMCARVE_LINUXCNC=podman:dreamcarve-rs274 cargo test -p linuxcnc-sim -- --nocapture
cargo run -p linuxcnc-sim -- --linuxcnc podman:dreamcarve-rs274       # the corpus, with a report
```

Containers run with `--security-opt no-new-privileges`, so the setuid
`rtapi_app` runs as the user, in LinuxCNC's non-realtime mode. Without
`DREAMCARVE_LINUXCNC` the tests that need LinuxCNC skip with a message;
nothing is looked up on `PATH`, since a session starts a whole LinuxCNC.
The test that guards the program set always runs. More in
[`tools/linuxcnc-sim/README.md`](../../tools/linuxcnc-sim/README.md).

On 2026-10-21, with LinuxCNC 2.9.4, all 17 LinuxCNC and generic programs
agreed in every session: 4879 logged moves, 4 `M0` pauses and every end
position. The run takes about three minutes, most of it `motion-logger`,
which answers about one command per 10 ms. Under the short Z travel the
lift was refused with "Linear move on line 530 would exceed Z's positive
limit". Motion refuses a move when task queues it, and task reads ahead,
so the program stopped while motion was on line 119, mid-cut, with the
bit in the stock. It did stop before the lift. The post-processors
chapter says such a lift stops the program "before the machine moves",
which reads as if nothing had run; #336 is the follow-up.

## CI

Layers 1 and 2 need `rs274` in CI. PR #264 adds a `gcode-conformance` job
in a `debian:bookworm` container with `rs274` installed. Its last step can
also run `cargo test -p rs274-motion` with `DREAMCARVE_RS274` unset, since
`rs274` is on `PATH` there. This is proposed on #264 rather than as a
second workflow PR, because it is one step in that job. Agents cannot
merge either: the token has no `workflow` scope.

Layer 3 needs a non-root user, which a job container does not have by
default (LinuxCNC refuses to run as root), and about three minutes. So
it is proposed on #264 as its own job on `ubuntu-latest`: build the
`rs274-motion` image with podman, then run `cargo test -p linuxcnc-sim`
with `DREAMCARVE_LINUXCNC=podman:dreamcarve-rs274`.

## Follow-up issues

- LinuxCNC full simulation, headless (layer 3): #328, `tools/linuxcnc-sim`.
- grblHAL Simulator for the grblHAL and GRBL corpus.
- A release checklist issue template that records the manual layer 4 and
  5 runs, and that "Not yet checked on a real machine" is lifted only
  from one.
