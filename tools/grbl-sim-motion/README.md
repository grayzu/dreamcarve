<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# grbl-sim-motion

Dev-only tool for issue #329: layers 1 and 2 of
[`docs/testing/post-processor-e2e.md`](../../docs/testing/post-processor-e2e.md)
for grblHAL and GRBL.

It runs DreamCarve's grblHAL and GRBL programs through the controllers'
own firmware, built for a desktop: the
[grblHAL Simulator](https://github.com/grblHAL/Simulator) and
[grbl-sim](https://github.com/grbl/grbl-sim). It is the grblHAL and GRBL
counterpart of [`rs274-motion`](../rs274-motion/README.md). For each
program it checks:

- **The firmware's parser accepts it.** On GRBL this is grbl-sim's
  validator, for programs with no `M0`: the validator has no cycle start
  and waits at one forever, so it is given 60 s. On both it is `$C`
  check mode in the simulator, streamed as
  the GRBL wiki tells a user to try a program: every line must be `ok`
  and nothing may move.
- **The motion is DreamCarve's.** The program runs from the firmware's
  default settings under a G54 offset of (25.4, 50.8, -76.2) mm, the
  offset `rs274-motion` uses. It is compared with DreamCarve's own
  reading (`dc-gcode/src/parse.rs`), the one the safety pass checks.
  After each line has finished moving, the firmware's own reports must
  match where DreamCarve reads the line ending:
  - the machine position (status report `MPos`), within 5e-4 mm, so a
    `G53` lift must land at machine Z -5 mm, not work Z -5;
  - the motion mode (`$G`): `G0` for a rapid or a lift, `G1`, `G2` or
    `G3` for a cut;
  - the feed (`$G`), for a cut, within one unit of the report's last digit (grblHAL truncates it).

  Each `M0`, and on grblHAL each `M6`, must pause once, and the run must
  end where DreamCarve reads it ending.
- **The header sets the modal state.** After MDI leaves incremental
  mode, inches, the XZ plane, `G55` and inverse time (and, on grblHAL, a
  `G81` motion mode and a `G51` scale), each program's header leaves
  `$G` showing every mode it writes and none of those.
- **Scaling on grblHAL.** The version 4 header's `G50` clears a scale that
  includes X. It does not clear `G51 Z-1` or `G51 Y-1`. That is the
  grblHAL bug the definitions README documents ("Scaling on grblHAL"). It
  reproduces at the pinned commit. The test reports it and does not fail,
  and it says so when a newer commit fixes it.
- **No canned cycles.** No grblHAL or GRBL program writes one, and GRBL
  refuses `G81` with error 20.

Both simulators are GPL-3.0. They are built in a container from their
official repositories at pinned commits and run as separate programs.
Nothing of them is copied into this repository. The `Containerfile` is
DreamCarve's own recipe.

## Getting the simulators

```sh
podman build -t dreamcarve-grbl-sim tools/grbl-sim-motion   # or docker build
export DREAMCARVE_GRBL_SIM=podman:dreamcarve-grbl-sim
```

`DREAMCARVE_GRBL_SIM`, or `--sims`, can also be a directory with
`grblHAL_sim`, `grblHAL_validator`, `grbl_sim` and `grbl_validator`.
`stdbuf` (coreutils) must be on `PATH` there. Containers run with
`--network none`. The build uses at most 4 jobs.

## Usage

```sh
cargo run -p grbl-sim-motion                              # the corpus, with a report
cargo run -p grbl-sim-motion -- --match plaque/grblhal    # the corpus cases whose name holds it
cargo run -p grbl-sim-motion -- --post grbl a.nc          # your own files
cargo test -p grbl-sim-motion -- --nocapture              # the tests
DREAMCARVE_GRBL_SIM_TRACE=1 cargo run -p grbl-sim-motion -- b.nc   # print the serial exchange, timed
DREAMCARVE_GRBL_SIM_KEEP=1 cargo run -p grbl-sim-motion -- b.nc    # keep each run's directory
```

The exit codes:

- 0: every program passes.
- 1: one differs, or the firmware rejects one.
- 2: nothing could be checked: no simulators, a probe that fails, a bad
  option, a corpus that does not build.

When `cargo test` finds no simulators, the tests that need them pass and
print why they skipped. `corpus_covers_each_controller_and_feature`
always runs.

Every simulator test first runs a probe, `G0 X1 Y2 Z3` under the offset,
whose result is known. If it does not come out at machine (26.4, 52.8,
-73.2), the test fails as "misconfigured", not as a program the firmware
refused.

## What runs

Pins, checked 2026-10-21:

| | Repository | Commit |
| --- | --- | --- |
| grblHAL Simulator | `grblHAL/Simulator` | `150e3f935e4b7cab7b90263b6b90b78912d9158d` (master, 2026-10-15) |
| grblHAL core | its `src/grbl` submodule, `grblHAL/core` | `4ec1ae44b857a70d2f9298344feac4b43ed0eb6b`, the commit the definitions README cites |
| GRBL | `gnea/grbl` | `bfb67f0c7963fe3ce4aaf8a97f9009ea5a8db36e` (v1.1h), the commit the definitions README cites |
| grbl-sim | `grbl/grbl-sim`, inside GRBL's source as its README says | `9bdd1654c7eb6cacf03e35340b38c962bcebe013` (master, 2018) |

The harness talks to a simulator as a sender talks to the controller. It
sends one line, waits for `ok` or `error:N`, and only then sends the next.
Input past a full receive buffer is dropped, as on the controller. While
it waits it polls with `?`. It answers a `Hold:0` or `Tool` state with
cycle start (`~`), as an operator resumes an `M0` or a tool change.
After each program line it sends `G4 P0`, which is answered only once the
planner is empty, then `?` and `$G`. The controller therefore stops
between lines, which a sender never makes it do. The moves are the same,
and the end of each line's motion is exactly what DreamCarve reads. Each
run gets a new directory, so the firmware starts from its default
settings (`EEPROM.DAT`). Then the harness sets 1000 steps/mm, so every
position is a whole number of the thousandths a status report prints,
and high rates and accelerations, so runs are short. A feed is compared
as programmed, not as reached.

What the build changes, and what running the simulators found:

- **Neither block log is a faithful record, so neither is used.** Both
  simulators can print each planned block's end in steps (`-b`). The
  grblHAL Simulator prints only the newest block, once per pass of the
  firmware's realtime loop, and adds up step counts. Run as fast as
  possible, it finishes short blocks between two passes. Those blocks are
  never printed, and every later position is off by their moves. The
  same unfitted Bezier gave 764 blocks on one run and 1168 on the next,
  where the firmware's status reports were right both times. grbl-sim
  samples its planner once per simulated serial byte from another thread,
  with the same effect on bursts of arc segments. Its step log is no
  better, since it marks a block's end when the stepper's prep stage is
  done with it, ahead of the steps taken. So the arc segments themselves
  are not compared, only where each arc ends.
- **`grblHAL_validator` crashes on every input** at the pinned commit, with
  a segmentation fault. `validator.c` clears the HAL and never sets
  `hal.get_elapsed_ticks`, which the core's `task_execute` calls
  (`grbllib.c` 566). So layer 1 on grblHAL is `$C` check mode in the
  simulator, which runs the same parser on the full firmware.
- **grbl-sim's clock does not run on arm64.** `platform_ns`
  (`platform_LINUX.c`) stores `tv_sec*1e9+tv_nsec`, a double far past
  2^32, in a `uint32_t`. That is undefined behaviour. x86 happens to wrap
  it, arm64 saturates it, and then grbl-sim never advances and prints
  nothing. The `Containerfile` computes it in 64-bit integers. grbl-sim's
  Makefile also needs `-fcommon` on GCC 10 and later.
- **grbl-sim's serial emulation hands bytes between two threads with no
  lock.** A lost update of the "data register empty" flag left GRBL's
  `ok` queued and never sent, so a run stalled. A reordered write garbled
  a byte now and then (`4Pos:` for `MPos:`). The `Containerfile` makes it
  send while any byte is queued. The harness pins grbl-sim to one CPU
  (`taskset -c 0`) and asks again for a status report it cannot read.
- grbl-sim reads standard input with `select` and then `getchar`, so
  stdio buffering would hide queued input. Both simulators buffer
  standard output. The harness runs them under `stdbuf` (`-i0` and
  `-oL`).
- grbl-sim simulates every 16 MHz tick, so it runs at about real time
  whatever `-t` asks for. A GRBL coaster takes about a minute. The
  grblHAL Simulator skips idle ticks and runs the whole grblHAL corpus in
  seconds.
- A simulator that stops answering is killed after 60 s without a
  reply. Its container is killed by name, since killing `podman run`
  leaves the container running. The run is then made once more from the
  start, which the report notes on standard error. The grblHAL Simulator
  stalls this way on a `G4 P0` now and then: twice in the seven plaque
  runs one day, never in the twenty coaster runs. A retry has always
  passed so far.

## What it does not check

- Homing, soft limits and hard limits. Neither simulator models limit
  switches (their `TODO`s say so), so a lift's safety on a machine whose
  Z homes downward is still for the manual procedure in the plan.
- The feed actually reached, acceleration and junction speeds. The feed
  is compared as programmed.
- The path between line ends: an arc's segments are not compared, only
  where it ends (see "What runs").
- Spindle speed, `M3`/`M5`, dwell length and `(MSG, ...)` text.
- Senders. How gSender, UGS, CNCjs or ioSender show an `M0` message is in
  the plan's manual procedure.

## The corpus

`src/corpus.rs` builds the programs:

- The committed grblHAL and GRBL programs in
  `crates/dreamcarve/tests/expected/`: the coasters of every definition
  version, manual spindle control (`M0`), two tools (`M6` on grblHAL),
  and an unfitted Bezier.
- The quick-start plaque, exported in-process for grblHAL and GRBL:
  plain, with a `G53` lift to machine Z -5 mm, and in inches.
