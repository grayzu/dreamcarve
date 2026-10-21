# Post-processors

Controllers read slightly different G-code. A **post-processor** is the
definition of one controller's dialect: how comments are written, what goes
at the start and end of a program, and how the spindle and tool changes are
handled. DreamCarve bundles six:

```text
grblhal   grblHAL
grbl      GRBL 1.1
linuxcnc  LinuxCNC
mach3     Mach3
uccnc     UCCNC
generic   Generic RS274/NGC
```

(`dreamcarve posts` prints this list.) Every line each one writes was
checked against that controller's own documentation or source code; see
[Controllers compared](#controllers-compared). The motion is the same on
all six: only the dialect lines differ.

## In the project

The project does not hold the definition itself. Its `post_processor`
block names a bundled definition and holds your settings for it:

```json
"post_processor": {
  "id": "grblhal",
  "definition_version": 3,
  "file_extension": "nc",
  "spindle_control": "automatic",
  "spindle_on_dwell_s": 3.0,
  "tool_change_mode": "separate_programs",
  "decimal_places": {
    "mm": 3,
    "inch": 4
  },
  "use_canned_cycles": false
}
```

`id` and `definition_version` say which definition, and which version of
it. Every key is required. The other six are the [settings](#settings)
you may change. The rest of the dialect (comment style, header and footer,
the spindle and tool-change commands) is not in the file, so it cannot be
changed by mistake: each bundled definition was checked against the
controller's own documentation or source code, and a project always uses
it as checked.

### Versions

When a bundled definition has to change in a way that changes the G-code,
DreamCarve adds a new version of it and keeps the old ones unchanged. A
project keeps the version it names, so a DreamCarve upgrade never changes
what an existing project exports. New projects get the latest version.

To move a project to the latest version of its post-processor, run
`export --post` with its own id:

```text
dreamcarve export coaster.dcv --post grblhal
```

It saves the project with the latest version and exports, and says which
version it came from:

```text
Updated coaster.dcv: post-processor grblhal v2 → v3, keeping your settings.
```

Your [settings](#settings) are kept, so an upgrade changes only
`definition_version`. To take every setting from the new version's defaults
instead, add `--reset-settings`
([Switching keeps your settings](#switching-keeps-your-settings)).

| Version | What changed |
| --- | --- |
| grblHAL v2, GRBL v2 | The first line is `( Project: coaster )` (GRBL: `; Project: coaster`) instead of `( coaster )` (`; coaster`). Some controllers act on a comment that starts with a keyword, such as `(MSG, …)` or `(ABORT, …)` on grblHAL, so the project name never starts one. Nothing else in the program changes. |
| grblHAL v3, GRBL v3 | The header has `G80` and then `G40` after `G94`, and so does the setup written again after every pause and tool change. They cancel a drilling cycle and cutter compensation left on by an earlier job or by commands you typed, so neither changes the program's moves. Nothing else in the program changes. See [Modes the header cancels](#modes-the-header-cancels). |
| LinuxCNC v2, Mach3 v2, UCCNC v2 | The header no longer has `G49` (`G90 G17 G40 G80 G94`; Mach3 `G90 G94 G17 G40 G80`). If a tool length offset was active when you set Z zero, version 1's `G49` cancelled it and moved every Z by its length, which could cut deeper than the drawing. Nothing else in the program changes. Upgrade a version 1 project with `export --post <id>`. |

A file that names a post-processor or a version this DreamCarve does not
have is refused when it is loaded, with the line and column of the value
and the ones that are available.

### Files from before version 7

Older project files held a full copy of the definition. DreamCarve upgrades
such a file when it opens it: your five settings keep their values, and the
copy is replaced by a reference to version 1 of the bundled definition with
the same `id`, but only if the rest of the copy is exactly that definition.

If it is not, because the copy was edited or was made by an older
DreamCarve whose definition has since changed, the file is not loaded.
Using the bundled definition would change the program without telling
you. The message names each field that differs, with both values, and
ends with the block to paste in its place, your settings filled in:

```text
error: cannot load coaster.dcv: migration from version 6 to 7 failed: the post-processor in this file differs from the bundled `grblhal` definition, version 1. […] It differs in:
  post_processor.motion.rapid_command: this file has "G1", grblhal version 1 has "G0"
To use the bundled definition, change "version": 6 to "version": 7 and replace the whole post_processor block with this one, which keeps your settings:
"post_processor": {
  "id": "grblhal",
  …
}
```

Check that the differences are ones you can do without, then make both
changes in the file. A post-processor that was never bundled is refused
the same way, and the message offers the bundled one closest to it.

## Choosing one

- `dreamcarve init part.svg --post grbl` creates a project with GRBL.
  grblHAL is the default.
- `dreamcarve export part.dcv --post grbl` switches an existing project to
  the latest version of GRBL, saves the project with it, and exports. Your
  settings, such as a longer spindle dwell, are kept where GRBL can use
  them ([Switching keeps your settings](#switching-keeps-your-settings)).
  The project is checked against the new post-processor, so the result
  can differ from what `check` reported. For example, a tool with no
  `rpm` passes with manual spindle control but fails after
  `--post grblhal --reset-settings`, which turns automatic spindle control
  back on, so the program sets the speed itself. If export fails, nothing
  is written and the project keeps its old post-processor.

An unknown id is a usage error (exit code 2) that lists the known ones.

### Switching keeps your settings

`export --post` decides each of the six [settings](#settings) by one rule:

- A setting still at the old post-processor's default takes the new one's
  default. grblHAL's `file_extension` `nc` becomes `ngc` when you switch to
  LinuxCNC.
- A setting you changed is kept: a `spindle_on_dwell_s` of 6, an extension
  of `gcode`, manual spindle control.
- Unless the new post-processor cannot use it. Then it takes the new
  default, and `export` says so, with both values and why. Two settings can
  be reset this way: `tool_change_mode: "m6"` on a post-processor without
  `M6` (every one but grblHAL), and `use_canned_cycles: true` on one without
  [canned cycles](#canned-cycles) (all but LinuxCNC and Mach3):

```text
Updated coaster.dcv: post-processor grblhal v2 → grbl v2, keeping your settings.
  Reset tool_change_mode "m6" → "separate_programs": post-processor grbl has no M6 tool change.
```

Your [configured](configuration.md) `tool_change_mode` applies only when
the mode is reset; a mode the switch keeps stays as it is.

`--reset-settings` (only with `--post`) takes every setting from the new
post-processor's defaults, with your configured `tool_change_mode` on
top, as `init` writes them:

```text
dreamcarve export coaster.dcv --post grblhal --reset-settings
```

## What a program contains

For the coaster, with grblHAL:

| Part | Lines | Why |
| --- | --- | --- |
| Header | `( Project: coaster )`, `( Generated by DreamCarve on … )`, `G90`, `G17`, `G94`, `G80`, `G40`, `G21`, `G54` | Absolute coordinates, XY plane, feed per minute, no drilling cycle, no cutter compensation, units, work coordinate system. A controller left in another state is corrected before any move. |
| Operation header | `( Operation 1: Profile )`, `( Tool 1: Set me )` | Comments for you. |
| First move | `G0 Z5.000` | Rise to safe Z, measured from the work zero you set with the bit before starting. |
| Spindle start | `M3 S18000`, `G4 P3.000` | Start the spindle clockwise at the tool's RPM, then wait 3 seconds for it to reach speed. |
| Motion | `G0`, `G1`, `G2`, `G3` | The toolpath. Rapids (`G0`) only at or above safe Z. |
| Footer | `M5`, `M30` | Stop the spindle; end the program. |

The program does not stop for its first bit. Fit it, then set work zero
(X, Y and Z) with it, before you start the program, as with any other CAM
program. `export` names the bit:

```text
Before you start: fit T1 Set me and set work zero with it.
This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero.
```

### Modes the header cancels

A controller keeps some modes from one job to the next, or from commands
you type between jobs. The grblHAL and GRBL headers (version 3) cancel
the two that would change what the program does:

- **`G80`** cancels a drilling cycle (`G81`, `G83`, …). grblHAL has them,
  and one left on would drill at every point of the program.
- **`G40`** cancels cutter radius compensation, which grblHAL can be built
  with and which would move every cut sideways.

GRBL has neither mode and accepts both codes without doing anything, so
a GRBL program also runs safely on grblHAL. Both come again after every
pause and tool change, and the first move after them writes its `G0` or
`G1`, which both controllers require after `G80`.

**The tool length offset is left alone.** Some senders and grblHAL's own
tool-change modes measure each bit with a tool setter and store the
difference as a tool length offset (`G43.1`). grblHAL and GRBL subtract
that offset when you set Z zero, so it becomes part of your zero. A
program that cancelled it with `G49` would then move every Z by the
offset, down into the stock if it was positive. So grblHAL and GRBL
programs never write `G49`, and `export` prints the note above: if your
sender sets an offset, let it do so before you set Z zero, not after. If
you set Z zero by touching each bit to the stock, as the quick starts do,
there is no offset and nothing to do. LinuxCNC, Mach3 and UCCNC programs
leave the offset alone too, from version 2 of each (see
[Controllers compared](#controllers-compared)).

## Projects with more than one tool

When the enabled operations use more than one tool, `export` writes one
complete program for each **tool run**: a run of consecutive operations
with the same tool, in operation order. Operations are never regrouped by
tool, because that would change the order of the cuts, for example a through-cut
before the detail work. T1, T2, T1 is three programs.

The programs are named `<stem>-<k>-T<n>.<ext>`: `<k>` is the run's place
(1, 2, …) and `<n>` the tool's number: its `number` in the project file
when set, otherwise its place in the project's tool list (the first tool
is 1; see [Tools](project-file.md#tools)). `<stem>` is the project file's name,
or the name given with `-o`: `-o out/job.nc` writes `out/job-1-T1.nc`,
`out/job-2-T2.nc`, … A project with one tool writes one program, named as
before (`coaster.nc`).

Each program is complete on its own, exactly as a one-tool project exports:
header, rise to safe Z, spindle start (or the pause to switch the router
on), cuts, end. It has no `M6` and no `M61`, passes the safety check on its
own, and `export` prints its extents. If any program fails, no file is
written. `export` lists the programs in the order to run them, with what to
do before each:

```text
Wrote 2 programs; run them in this order:
  1. coaster-1-T1.nc  Before you start: fit T1 Set me and set work zero (X, Y and Z) with it.
  2. coaster-2-T2.nc  Before you start: fit T2 3mm finisher and set Z zero with it. Keep X and Y zero.
```

Between programs, fit the next bit and set Z zero with it. A new bit has a
new length, so set Z zero even if it is a bit you zeroed with before. Leave
X and Y zero as they are. Each program starts by rising to safe Z,
measured from the Z zero you just set.

**Why one program per tool.** In one program, the tool change is an `M6`,
and whether `M6` stops the machine depends on your sender, not on the
controller. Several senders by default do not stop there (see
[Senders](#senders) below). gSender, for example, comments the `M6` line
out and keeps streaming, so the program switches the spindle off and on
and cuts the second tool's paths with the first bit still fitted. Separate
programs stop at every bit change with every sender and every controller,
including GRBL.

The choice is a machine setting, `tool_change_mode` in `post_processor`:

| Value | Export of more than one tool |
| --- | --- |
| `"separate_programs"` | One program per tool run. The default, on every controller. |
| `"m6"` | One program that stops with `T<n> M6` at each tool change. grblHAL only; see the next section. On every other post-processor it is an error. |

`--post <id>` resets the settings to the definition's defaults, with your
own default on top (next paragraph). With no config file that is
`"separate_programs"`.

**Your default.** If every project you make should use `"m6"`, set it once
in the [user config file](configuration.md):

```toml
[export]
tool_change_mode = "m6"
```

`init` and `export --post` then write `"m6"` into every new
`post_processor` block. On a post-processor without `M6` (every one but
grblHAL) that cannot apply, and both commands stop with an error naming the config file rather
than write a project that `check` would reject. Nothing else reads the
config: a project exports the same wherever it is opened.

**For one export.** Two `export` flags override the project's mode without
changing the project file:

| Flag | Export of more than one tool |
| --- | --- |
| `--programs-per-tool` | One program per tool run, whatever the project says. |
| `--one-program` | One program with `T<n> M6` tool changes, whatever the project says. On a post-processor without `M6` (every one but grblHAL) it is an error, and nothing is written. |

They contradict each other, so giving both is a usage error. The flags are
not saved: to change the project, edit `tool_change_mode`.

## One program with M6

On grblHAL you can export a multi-tool project as one program with `M6`
tool changes, if your sender stops there (see [Senders](#senders)). Set,
in `post_processor`:

```json
"tool_change_mode": "m6",
```

GRBL has no `M6`, so `"m6"` with the GRBL definition is an error: `check`
reports it at the field, and export writes nothing. The same holds for
LinuxCNC, Mach3, UCCNC and generic, for the reasons in [Controllers
compared](#controllers-compared).

`export` then writes one program and adds:

```text
This program changes tools with M6, so your sender must stop at M6; see "One program with M6" in the post-processors chapter.
```

Every tool change in the program works the same way, in both spindle
modes:

1. The program rises to safe Z, so the bit is clear of the work, and stops
   the spindle (or, with manual control, pauses for you to switch the
   router off).
2. It stops with `T<n> M6` and the message `Fit T<n> <name>. Set Z zero,
   then resume.` grblHAL waits in its tool-change state, where you can jog.
   Fit the bit, jog it down and set Z zero with it, then resume with cycle
   start. A new bit has a new length, so set Z zero even if it is a bit you
   zeroed with before. Leave X and Y zero as they are. `<n>` is the tool's
   number, chosen as in the file names above. The tool's number and
   name are DreamCarve's; the line replaces the definition's own `M6` line.
   A long tool name is shortened so the message stays within 96
   characters; the instruction is never cut.
3. After you resume, it sets the controller's state again: the header's
   `G90`, `G17`, `G94`, `G80` and `G40`, the units, and the work coordinate system. You may
   have changed any of them while setting Z zero, for example `G91` to jog.
   This happens after every pause and tool change that the program moves
   again after.
4. It rises to safe Z again before the spindle starts, so the height is
   measured from the Z zero you just set. The next move writes its `G0` or
   `G1` and feed again, in case you probed or moved the machine by hand to
   set Z zero.

grblHAL waits at the operation's safe Z, often too low to swap a bit. Jog Z
up first.

A program that changes tools starts with `M61 Q<n>`, which tells grblHAL
the number of the bit you fitted before starting, without stopping or
moving. grblHAL skips an `M6` for the tool it believes is already in the
spindle, and it remembers that number from the previous job, so without
this line a later tool change could be skipped and the program would cut
with the wrong bit.

**GRBL cannot change tools within a program.** It has no `M6`, and at an
`M0` pause it is in feed hold, where it refuses to jog, so you could not
set Z zero for the new bit. That is why `"m6"` is refused with GRBL; the
default, one program per tool run, works on it.

### grblHAL settings for tool changes

The tool change depends on these controller settings. Check them once
before running a program with `M6`:

- **`$341` (tool change mode) must not be 4 ("Ignore M6").** With 4,
  grblHAL skips the stop and cuts with the old bit and the old Z zero.
  Mode 0 ("Normal", the default) is the one DreamCarve expects: grblHAL
  stops without moving and lets you jog to set Z zero. Modes 1 to 3 also
  stop, but grblHAL then moves the machine itself (Z up, and with 2 and 3 to
  a tool setter at `G59.3`) and requires homing first. Use them only if you
  have set them up on purpose.
- **Your controller's input stream must support tool changes.** `M6` needs
  a driver that can suspend reading the program while it waits. Without
  that, grblHAL stops the program at the `M6` line with `error:20`
  (unsupported command). This happens on the machine, not at export.
- **The tool number.** grblHAL skips `T<n> M6` when `<n>` is already the
  current tool. The `M61` line at the start prevents that.

### Senders

Most senders read `M6` themselves before the controller sees it, and their
defaults differ. Before you use `"m6"`, set your sender so that it stops at
`M6`:

| Sender | What it does with `M6` by default | For one program with `M6` |
| --- | --- | --- |
| gSender | Ignores it: comments the line out as `(M6)` and keeps streaming, with no pause. Pause and the re-zero and tool-sensor wizards are opt-in. | Set Tool Changing to Pause, or to one of its wizards. Never Ignore. |
| CNCjs | Replaces `M6` with `(M6)`, but holds the stream with a message. `M6` never reaches the controller. | Any tool-change policy except "send M6" holds. |
| Universal Gcode Sender | Removes `M6`, sends `G0 Z<max Z of the file>` and `M5`, then shows a change-tool dialog. | The default works. |
| ioSender | Passes `M6` to grblHAL, which handles it as `$341` says. | Keep `$341` other than 4 (see above). |
| Candle | Sends `M6` to the controller. Whether the sender also pauses is a setting whose default is unclear. | Enable "Pause sender on M6". |
| OpenBuilds CONTROL | Splits the file at `M6` lines into per-tool sections. | Use its per-tool run menu. |

This behaviour was read from each sender's source code on 2026-10-19, for
interoperability only:

- gSender (`Sienci-Labs/gsender`):
  `src/server/controllers/Grblhal/GrblHalController.js`, "ignore is default
  and comments line out with no other action".
- CNCjs (`cncjs/cncjs`): `src/server/controllers/Grbl/GrblController.js`,
  about lines 301 to 335; the `tool.toolChangePolicy` default is
  `TOOL_CHANGE_POLICY_IGNORE_M6_COMMANDS`.
- Universal Gcode Sender (`winder/Universal-G-Code-Sender`):
  `ugs-core/.../utils/Settings.java` line 85 (`handleToolChanges = true`);
  `services/interceptor/ToolChangeInterceptor.java`.
- ioSender (`terjeio/ioSender`): `CNC Controls/AppConfig.cs` line 245;
  `JobControl.xaml.cs`, about line 665.
- Candle (`Denvi/Candle`): `src/candle/frmmain.cpp`, about line 2546;
  `help/en/settings/index.md` line 33 (the UI and the help disagree on the
  default).
- OpenBuilds CONTROL (`OpenBuilds/OpenBuilds-CONTROL`): `app/js/toolchange.js`.

Senders change; check yours on a test file before you rely on it. With the
default, one program per tool run, none of this matters.

## grblHAL and GRBL compared

The motion is identical. Only these differ:

| | grblHAL | GRBL 1.1 |
| --- | --- | --- |
| Comments | `( text )` | `; text` |
| Messages | Shows `(MSG, text)` comments to the sender as `[MSG:text]` | Has no messages: comments are removed before the line is run |
| More than one tool | One program per tool run; with `"m6"`, one program with `T<n> M6`, after `M61 Q<n>` at the start | One program per tool run; `"m6"` is an error |

## Controllers compared

Each definition was checked line by line against the controller's own
documentation or source, and where a document says nothing or leaves it
to a setting, DreamCarve takes the safe choice: plain moves rather than a
canned cycle, and no `M6`. The sources, line by line, are in
`crates/dc-project/src/definitions/README.md`.

| | LinuxCNC | Mach3 | UCCNC | Generic |
| --- | --- | --- | --- | --- |
| Checked against | LinuxCNC 2.9 G-code and M-code references, and its source | *Using Mach3Mill* 1.84 and the Mach3 configuration guide | UCCNC user's manual 1.0047 | NIST RS274/NGC version 3 (NISTIR 6556) |
| Extension | `.ngc` | `.tap` | `.txt` | `.nc` |
| Header | `G90 G17 G40 G80 G94`, `G91.1`, units, `G54` | `%`, then `G90 G94 G17 G40 G80`, `G91.1`, units, `G54` | `G90 G17 G40 G80 G94`, `G54`; no units word | `G90`, `G17`, `G94`, units, `G54` |
| Line numbers | No | Every line, `N10` up in tens | No | No |
| Spin-up wait | `G4 P3.000` (seconds) | `G4 P3.000` (seconds) | `G4 P3000` (milliseconds) | `G4 P3` (seconds) |
| Numbers | `10.500` | `10.500` | `10.500` | `10.5` |
| End | `M5`, `M2` | `M5`, `M30` | `M5`, `M30` | `M5`, `M30` |
| Canned cycles | `G81`, `G82` | `G81`, `G82` | None | None |
| More than one tool | One program per tool run; `"m6"` is an error | One program per tool run; `"m6"` is an error | One program per tool run; `"m6"` is an error | One program per tool run; `"m6"` is an error |

`G40` and `G80` cancel cutter compensation and canned cycles, in case an
earlier program left one on. The header does not touch the tool length
offset (no `G43` or `G49`). If one is active when you set Z zero, for
example after a tool-setter routine, the zero includes it, and cancelling
it would move every Z by its length, possibly into the spoilboard. So the
program leaves it as you set it. If your setup applies an offset, set it
before you set Z zero, and do not change it until the program ends. Version
1 of these three post-processors did write `G49`; see
[Versions](#versions). `G91.1` makes arc
centres (`I`, `J`) relative to the arc's start, which is how DreamCarve
writes them. On Mach3 this is a setting you can change, so the header sets
it.

**Why no peck cycles.** A `G83` goes back into the hole to just above the
last depth, and a `G73` lifts a little between pecks. How far is not a
word in the program. LinuxCNC 2.9 fixes it at 0.254 mm, but LinuxCNC 2.10
makes it a setting with a different default. Mach3 and UCCNC make the
`G73` lift a setting, and Mach3's `G83` says only "backed off a bit".
DreamCarve checks the moves a cycle makes before it writes one, so where
the distance is unknown it writes the pecks as plain moves, with the
warning `canned_cycle_expanded`. UCCNC's manual describes its `G81` with
no rapid down to the retract height, unlike the others, so UCCNC gets no
canned cycles at all.

**Why no `M6`.** DreamCarve's one-program tool change stops at `M6` and
asks you to fit the bit and set Z zero. None of these controllers
documents a stop where you can do that:

- **LinuxCNC.** With a manual tool change set up, `M6` shows a dialog that
  waits for Continue, but LinuxCNC handles a new bit's length with a tool
  table and `G43`, which DreamCarve does not write. Without one set up,
  `M6` does not stop at all.
- **Mach3 and UCCNC.** What `M6` does is a setting: ignore it, stop and
  wait for cycle start, or run a macro. With "ignore", the program would
  cut with the old bit.
- **Generic.** A basic controller's `M6` is unknown.

Export each tool run as its own program, the default.

**Before your first cut**, check these on the controller:

- **Mach3.** In Config > Logic, "G04 Dwell param in ms" is unchecked, or
  the 3 s spin-up wait is 3 ms. The IJ mode may be either; the header
  sets it. Run the program in Mach3's simulation first: there is no
  offline Mach3 interpreter to test against.
- **UCCNC.** UCCNC has no `G20`/`G21`: it reads every number in the
  units of its profile. The program has no units word, and `export` warns
  `units_set_by_controller` with the program's units. Use a profile in
  those units. Leave "Measure dwell in seconds instead of msec" unticked,
  or the 3 s wait becomes 3000 s. UCCNC does not show `(MSG, ...)` text at
  a pause; `export` lists every pause and what to do there.
- **LinuxCNC.** If your configuration has a spindle-at-speed signal,
  LinuxCNC waits for it, and the 3 s wait comes on top. Programs end with
  `M2`.
- **Generic.** Check that your controller takes `G94`, `G54` and `G4 P`
  in seconds.

## Units

`display_units` in the project chooses the program's units: `"mm"` writes
`G21` and millimetres, `"inch"` writes `G20` and inches (feeds in inches per
minute). Choose it with `init --units inch`, or change it in the file. The
project's own values stay in millimetres.

UCCNC has no units words. Its programs carry none, and run in the units
of the UCCNC profile, so `export` warns `units_set_by_controller` with the
program's units.

## Settings

The whole `post_processor` block, in the order DreamCarve writes it:

| Field | Meaning |
| --- | --- |
| `id` | Which bundled post-processor: `"grblhal"`, `"grbl"`, `"linuxcnc"`, `"mach3"`, `"uccnc"` or `"generic"`. Change it with `export --post`, which also sets the version and resets the settings. |
| `definition_version` | Which version of it. See [Versions](#versions). |
| `file_extension` | The program's extension, without the dot. 1 to 8 letters or digits, and not `dcv`. Default `nc`; `ngc` on LinuxCNC, `tap` on Mach3, `txt` on UCCNC. |
| `spindle_control` | `"automatic"` (default) or `"manual"`. See below. |
| `spindle_on_dwell_s` | Seconds to wait after the spindle starts. Default 3. Raise it if your spindle or VFD takes longer to reach speed; 0 means no wait. |
| `tool_change_mode` | `"separate_programs"` (default) or `"m6"` (grblHAL only). See [Projects with more than one tool](#projects-with-more-than-one-tool). The [user config file](configuration.md) can change the default new projects get. |
| `decimal_places` | Digits after the point: `mm` at least 2, `inch` at least 3. Default 3 and 4. |
| `use_canned_cycles` | `false` (default) or `true`: whether a Drill operation is written as the controller's drilling cycles (`G81`, `G82`, `G83`, `G73`) where the post-processor has them. See [Canned cycles](#canned-cycles). LinuxCNC and Mach3 have `G81` and `G82`; on grblHAL, GRBL, UCCNC and generic `true` is an error. The holes are drilled the same either way. |

Names that DreamCarve puts in comments (the project, tool and operation
names) are reduced to plain ASCII without `( ) ; ! ? ~`. GRBL and grblHAL
act on `!`, `?` and `~` immediately, even inside a comment, and `)` would
end a comment early. On LinuxCNC, Mach3, UCCNC and generic a name never
starts a comment (`( Project: coaster )`), because LinuxCNC acts on a
comment that starts with a word such as `MSG,` or `LOGOPEN,`.

## Canned cycles

A canned cycle drills a whole hole in one line: `G81` (drill), `G82`
(drill and dwell), `G83` (peck, retracting fully), `G73` (peck, breaking
the chip). DreamCarve writes them only when you ask, with
`"use_canned_cycles": true`, and only where the post-processor's
definition lists them in its `canned_cycles` block.

| Post-processor | Canned cycles |
| --- | --- |
| LinuxCNC, Mach3 | `G81` (simple) and `G82` (simple with a dwell). Peck Drills are plain moves, with `canned_cycle_expanded` ([why](#controllers-compared)). |
| grblHAL, GRBL, UCCNC, generic | None: a Drill is always plain `G0`/`G1` moves, and `true` is an error at `check`. |

What is written, and when DreamCarve writes plain moves instead, is in
[Drill: canned cycles](drill.md#canned-cycles). In short:

- `G98` once, so the bit returns to safe Z after every hole, one cycle
  line per hole with `X Y Z R` and `Q` or `P`, `F` on the first, and `G80`
  at the end.
- `export` reads every cycle line back as the moves the controller makes,
  from the controller's documentation, and checks those moves with the
  same safety rules as plain moves, and against the Drill's own moves
  ([Safety](safety.md)). A `G83` re-enters the hole, and a `G73` lifts, by
  a distance each controller fixes itself, not by a word in the program;
  the definition states it (`canned_cycles.reentry_gap_mm`). A definition
  that does not state it never writes `G83` or `G73`.
- If the cycles would not drill exactly what the plain moves drill, the
  whole operation is written as plain moves with the warning
  `canned_cycle_expanded`. A program never mixes the two for one
  operation.

## Manual spindle control

Many hobby machines use a trim router that you switch on and off by hand
rather than a spindle the controller starts. For those, set:

```json
"spindle_control": "manual",
```

in `post_processor`. The program then sends no `M3`, `M5` or `S`
word and no spin-up wait. Instead it pauses before each bit's first cut,
and after its last:

- Before the first cut, it rises to safe Z and pauses to turn the router
  on:

  ```text
  G0 Z5.000
  M0 (MSG, Turn the router ON. Speed 18000 RPM. Resume to start cutting.)
  G90 … G54
  G0 Z5.000
  ```

  (`G90 … G54` stands for the five setup lines.) You may jog or set Z
  zero while it is paused, so the program rises to safe Z again before it
  moves across the work.

  Switch the router on, set its dial to the speed shown, wait until it is
  at speed, then resume with cycle start. Without an `rpm` on the tool
  the message leaves out the speed. With manual control `rpm` is optional;
  the feed and plunge rates are still required.
- After the tool's last cut, it rises to safe Z and pauses:

  ```text
  M0 (MSG, Turn the router OFF.)
  ```

  Switch the router off and resume. After the last tool the program then
  ends. With one program per tool run, the default, every program has its
  own pair of pauses. With `"m6"` (grblHAL only), before a second tool the
  program stops with `T2 M6` and `Fit T2 …`, as with automatic control,
  then rises to safe Z again and pauses with `Turn the router ON. …` for
  the next tool. So the router starts only after the bit has been lifted
  from the Z zero you set.

For the coaster, the program pauses twice: to turn the router on, and to
turn it off. `export` lists each pause with its line number:

```text
Before you start: fit T1 Set me and set work zero with it.
Pauses and tool changes; resume each with cycle start:
  line 11: Turn the router ON. Speed 18000 RPM. Resume to start cutting.
  line 57: Turn the router OFF.
```

### What the controller does with the pause

From the GRBL and grblHAL source code:

- **The pause is reliable on both.** `M0` waits until every earlier move has
  finished, then holds. The machine resumes only on cycle start (`~`, or
  your sender's Resume or Start button). While it holds, neither
  controller lets you jog, which is why a tool change is an `M6`, not an
  `M0`.
- **The message is not.** GRBL does not support messages: it removes
  comments before running a line, so it never shows the text. grblHAL sends
  a `(MSG, text)` comment to the sender as `[MSG:text]`.
- Whether your sender shows that message is not yet checked on real
  senders. Do not rely on it. Use the pause list `export` prints, or read
  the line your sender stopped on.

The router is switched by hand, so DreamCarve cannot check that it is
running. Never resume a "Turn the router ON" pause until it is running at
speed.

## Not yet checked on a real machine

The bundled definitions were checked against each controller's source or
documentation, not yet on a running machine. For LinuxCNC, Mach3, UCCNC
and generic, see the checks under [Controllers
compared](#controllers-compared). On GRBL and grblHAL, before your first
cut, check on your controller:

- Laser mode is off (`$32=0`). Laser mode is for lasers and changes how the
  controller treats spindle commands.
- The maximum spindle speed (`$30`) is at or above the RPM you set, or the
  spindle runs slower than asked.
- The spin-up wait is long enough for your spindle.
- Your sender shows comments and pauses the way you expect.

The sources are listed in the repository, in
`crates/dc-project/src/definitions/README.md`.
