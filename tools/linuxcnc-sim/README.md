<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# linuxcnc-sim

Dev-only tool for issue #328 (layer 3 of
[`docs/testing/post-processor-e2e.md`](../../docs/testing/post-processor-e2e.md)).

It runs DreamCarve's LinuxCNC and generic programs through LinuxCNC's
full simulation, headless: `linuxcnc -r` with task, the interpreter, and
either the motion module or `motion-logger` in its place. In place of
a GUI, a small driver script talks to LinuxCNC's own command server,
`linuxcncrsh`, over a socket. It then checks what
[`rs274-motion`](../rs274-motion/README.md) cannot, since `rs274` is the
interpreter alone:

- the program runs to completion on a homed machine;
- every move task sends to motion is the move DreamCarve reads (kind, end
  point, arc centre, feed), with the INI in millimetres and the program
  in either unit;
- the same holds when the INI's `RS274NGC_STARTUP_CODE` sets every mode
  the header must reset;
- each `M0` pauses until the UI resumes it, with the machine where
  DreamCarve reads it, including at the `G53` lift's machine Z;
- the run ends where DreamCarve reads it;
- a `G53` lift beyond Z's travel stops the program with LinuxCNC's
  soft-limit error, before the lift;
- a machine that has not homed does not run the program.

LinuxCNC is GPL-2.0. It runs as a separate program, in a container or
natively, and nothing of it is copied into this repository. The files in
`sim/` are DreamCarve's own, written from LinuxCNC's documented INI and
HAL files.

## How DreamCarve talks to LinuxCNC

No DreamCarve code imports a LinuxCNC module or links LinuxCNC. The
driver, `sim/dreamcarve-ui.py`, uses only Python's standard library.
LinuxCNC starts it as the INI's `[DISPLAY]` program, and it talks to two
separate LinuxCNC programs, each through its documented interface:

- **`linuxcncrsh`**, LinuxCNC's text-mode command server
  ([linuxcncrsh(1)](https://linuxcnc.org/docs/2.9/html/man/man1/linuxcncrsh.1.html)).
  The driver starts it and talks to it over TCP on 127.0.0.1. It uses
  `hello` and `set enable`, then `set estop`, `machine`, `mode`,
  `teleop_enable`, `home -1`, `feed_override`, `linear_unit_conversion
  mm`, `open`, `run`, `resume`, `abort` and `set_wait`. It reads state
  with `get program_status`, `program_line`, `abs_act_pos`,
  `joint_homed`, `error`, `operator_display` and `operator_text`.
- **`halcmd`**, for HAL pins
  ([halcmd(1)](https://linuxcnc.org/docs/2.9/html/man/man1/halcmd.1.html)).
  It reads `motion.current-vel` and `motion.in-position` (the motion
  module's pins, [motion(9)](https://linuxcnc.org/docs/2.9/html/man/man9/motion.9.html)),
  and sets `motion-logger.reopen-log` to rotate the log between programs
  ([motion-logger(1)](https://linuxcnc.org/docs/2.9/html/man/man1/motion-logger.1.html)).

`motion-logger` itself is LinuxCNC's, loaded by `sim/logger.hal` as LinuxCNC's own
`tests/motion-logger` does.

The first version of the driver imported LinuxCNC's Python `linuxcnc` and
`hal` modules. Every check it made still runs through `linuxcncrsh` and
`halcmd`. Three readings had no equivalent there, and none was needed:

- `stat.motion_line` beside `stat.current_line`. `program_line` gives one
  line. At a pause it is the line of the last move, so the check that the
  program has not reached the `M0`'s line stays.
- `stat.state` (`RCS_DONE`). A program counts as completed when it ends
  with no error, no refused command and no timeout.
- The interpreter's paused state, apart from task's. `program_status`
  says `PAUSED`, and the pause is recorded then.

## Getting LinuxCNC

The image `rs274-motion` builds has all of LinuxCNC (Debian 13's
`linuxcnc-uspace` 2.9.4, with `linuxcncrsh`, `halcmd` and
`motion-logger`):

```sh
podman build -t dreamcarve-rs274 tools/rs274-motion
export DREAMCARVE_LINUXCNC=podman:dreamcarve-rs274
```

Containers run with `--network none` and `--security-opt
no-new-privileges`. The second is needed: `rtapi_app` is setuid root, and
with the bit in force HAL's shared memory is created as root and refused
to the user. Without it, LinuxCNC runs as the user in its non-realtime
POSIX mode, which is all a simulation needs.

`DREAMCARVE_LINUXCNC`, or `--linuxcnc`, can also be the path to the
`linuxcnc` script of a native install. Unlike `rs274-motion`, nothing is
looked up on `PATH`: a session starts a whole LinuxCNC and the corpus
takes minutes, LinuxCNC refuses to run as root (as in a CI container),
and a native session shares HAL memory with any LinuxCNC already running.

## Usage

```sh
cargo run -p linuxcnc-sim                            # the corpus, with a report
cargo run -p linuxcnc-sim -- --post linuxcnc a.ngc   # your own files
cargo test -p linuxcnc-sim -- --nocapture            # the tests
```

The exit codes:

- 0: every program agrees.
- 1: one does not.
- 2: nothing could be checked.

When `DREAMCARVE_LINUXCNC` is not set, the tests that need LinuxCNC pass
and print why they skipped. `corpus_covers_what_the_full_simulation_checks`
always runs.

## The sessions

The corpus is `rs274-motion`'s, for `linuxcnc` and `generic`. Each check
runs three LinuxCNCs at once (`src/lib.rs`, `check_cases`):

1. **motion-logger.** Every program. `motion-logger` stands in for the
   motion module and logs each command task sends it, with the end point
   in machine coordinates and the source line (`src/motion_log.rs`).
   The moves go through `rs274-motion`'s comparison, line by line.
   Dwells, `M0` and the program end make no move, so they are not
   compared here; the count of `PAUSE` commands must match the `M0`s.
2. **motion-logger, hostile startup code.** The committed LinuxCNC
   programs and the inch and `G81` plaque variants, under
   `G20 G91 G90.1 G18 G93 G55 G99`. The header must override each of
   these. Generic programs are left out: generic writes no `G91.1`, by
   design (the definitions README, "Generic"), so its arcs would follow
   the INI's `G90.1`.
3. **The motion module.** Every program on a homed machine with soft
   limits, at feed override 10000 %. The driver records each `M0` pause
   once the machine has stopped, resumes it, and records the end
   (`src/report.rs`).

The INI (`sim/machine.ini.in`) is a three-axis mill in millimetres. Z
travel is [-150, 0] mm and homes at 0. The parameter file sets a G54
offset of (25.4, 50.8, -76.2) mm, so a `G53 G0 Z-5` lift is told apart
from a work move. The tests add three more machines:

- `ShortZ`, Z travel [-150, -20] mm: the lift to machine Z -5 is
  refused (`full_sim_reports_a_lift_beyond_soft_limits`).
- `Unhomed`: never homed; LinuxCNC refuses to run
  (`full_sim_refuses_to_run_unhomed`).
- The hostile startup code on a program with no header, which shows the
  startup code is in force (`startup_code_applies_without_a_header`).

## What it found

On 2026-10-21, with LinuxCNC 2.9.4, all 17 LinuxCNC and generic corpus
programs agreed in every session: 4879 logged moves, 4 `M0` pauses, every
end. The run takes three to seven minutes, depending on the host's
load; the tests take about as long again. The `G53` lift reached machine
Z -5 under a work offset of Z -76.2, and the router-off pause after it
was there too. A deepened cut was reported on its line.

Under the short travel the lift was refused with "Linear move on line 530
would exceed Z's positive limit", as the definitions README says. But
motion refuses a move when task queues it, and task reads ahead, so the
program stopped mid-cut, with the bit in the stock, long before the lift:
on line 119 in one run and line 51 in another. It did stop before the
lift. #336 corrected the post-processors chapter, which had said
"before the machine moves".

Two things about LinuxCNC are worth knowing when reading a report:

- At an `M0` pause, `get program_line` names the line of the last move,
  not the `M0`'s. The check is that the program has not reached the
  `M0`'s line and the machine is where the move before it ended.
- `get error` returns the latest error only. The driver polls it with
  every status poll and records each new one. Under the short travel it
  caught all three of LinuxCNC's messages.
- `motion-logger` prints positions and speeds with `%.6g`. Below 1000 mm
  that keeps three decimals, as many as DreamCarve writes. Speeds match
  to 1e-5 relative, not `rs274`'s 1e-6.
- When the program goes idle at its end, motion can be in position with
  nothing queued while the last segment still blends into its end point.
  The driver records a pause or an end only once `motion.current-vel` is
  0, `motion.in-position` is true, and `abs_act_pos` has not changed for
  a whole second. The servo thread is
  not realtime in a container, and on a loaded host it stalls for long
  enough that a 100 ms window recorded positions short of the end.

## What it does not check

- The real-time behaviour of a machine: following error, acceleration,
  servo timing. The motion module loops each joint's command back as its
  feedback.
- Dwell length. Task waits a dwell out, but no motion command records it;
  `rs274-motion` checks dwells.
- The user's own INI. The startup-code session covers the modes the
  header resets; others (a `G92` offset in the parameter file, a tool
  length offset) are not modelled.
- Mach3 and UCCNC. LinuxCNC refuses their `G50` and `G69`; `rs274-motion`
  bridges those, and the manual procedures in the plan cover the rest.
