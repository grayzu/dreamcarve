# Checking and exporting

Before a program is written, DreamCarve checks the whole project: every
value, every toolpath, and the program itself, move by move, against its
[safety rules](safety.md#what-dreamcarve-checks). In the app the check
runs by itself after every change. This chapter is about reading what it
finds, in the **Warnings drawer**, and about the **Export** step, which
writes the G-code.

From the command line the same two things are `dreamcarve check` and
`dreamcarve export`. They report the same findings in the same words and
write the same files.

## Errors, warnings and hints

A finding is one of three kinds.

| Kind | Means | Export |
| --- | --- | --- |
| **Error** | Something is missing or wrong: a thickness not set, a tool with no feed, a cut below the stock | Refused until it is fixed |
| **Warning** | Something you should look at: a step-down deeper than the bit is wide, an open path nothing cuts | Allowed |
| **Hint** | Something worth knowing that needs no action: a tool no operation uses | Allowed |

The status bar counts each kind, or says *No problems*. The **Warnings**
button in the rail carries the count of the worst kind found, in its
colour. A step whose content has an error or a warning has a dot in the
rail ([The app](the-app.md#the-six-steps)).

## The Warnings drawer

Click the counts in the status bar, the **Warnings** button in the rail,
or **View ▸ Warnings**. The drawer opens over the bottom of the Viewport:

![The Warnings drawer over the Viewport: "Warnings · 1 error" with Copy and a close button, and one row: error, "tools[t2].feed_mm_min is not set (mm/min) for tool 1/8 end mill", and the field's name at the right](figures/app/app-warnings-drawer.png)

Each row has:

- its **kind**;
- its **code**, such as `unused_tool`, when it has one. The
  [Warnings](warnings.md) chapter explains each code;
- the **message**, which names the operation, the shape or the tool;
- at the right, the **value** it is about, as the project file names it.

Rows are grouped by operation, with the project's own first, under a
heading for each group when there is more than one.

**Click a row** to go to what it is about. The app opens the step that
owns it: an operation's field in its editor, a tool in the Tools step, a
shape on the Design step, a stock or output setting in its step. The
field itself carries the problem's mark, and resting the pointer on the
mark shows the same message.

**Copy** puts the whole list on the clipboard, as `dreamcarve check`
prints it, which is the form to paste into a bug report.

### Notes

An operation that carves (a V-Carve, an Inlay, a Photo V-Carve) also has
a **note** in the drawer: what the carve comes to, such as its deepest
cut, its flat floor and the ridges left on it. A note is information, not
a problem, and is not counted. The status bar's message shows the
selected carve's note in one line, and a click on it opens the drawer
there.

## The Export step

**Export** is the last step in the rail. **File ▸ Export G-code…** and
Ctrl E (⌘E on macOS) open it too. The step's green button, **Export
G-code**, is at the foot of its panel. It stays there however far the
settings above it scroll, and the line beside it says what it will
write, or why it cannot yet.

![The plaque's Export step, ready: "No problems", an estimated time of 3 min 39 s and a Z range of −12..15 mm; the Post-processor, Heights and Tolerances groups; Programs, "One program per tool"; Program Details with the two programs; the destination and the file name; and at the foot "Ready to write 2 programs." beside Export G-code](../quick-start/figures/app/18-plaque-export.png)

### Is it ready

The card at the top shows the counts, the **estimated time** of the
whole job, and the **Z range** the bit will move through.

The estimated time is the cutting and the spindle's spin-up waits at the
feed rates you set. It leaves out the rapids unless you give your
machine's rapid rate in the
[configuration](configuration.md#estimated-run-time), and it does not
know how your machine speeds up and slows down, so the real job takes a
little longer.

While an error blocks export, the step lists it, and **Export G-code** is
off, with the reason beside it:

![The Export step blocked: "1 error blocks export" in a red card with a Show button, and under it the finding, that the 1/8 end mill's feed is not set; at the foot, "1 error blocks export" beside a greyed Export G-code](figures/app/export-blocked.png)

Click the finding to go and fix it. **Show** opens the Warnings drawer.
Warnings are not listed here; the counts at the top include them. Read
them before you export.

### Output settings

Three groups hold the settings that shape the program. Each is closed
until you open it, with its values on its header.

![The Post-processor group open: Controller grblHAL v4 (latest), File extension nc, Spindle control Automatic, Spin-up dwell 3 s, Decimals for mm and inch, and Canned drill cycles, not offered on grblHAL](figures/app/export-post-processor.png)

- **Post-processor**: the **Controller** your machine runs, and how the
  program is written for it: the file extension, whether the program
  switches the spindle or pauses for you to switch a router, and how long
  to wait for the spindle to reach speed. A setting a controller does not have is not offered, with the
  reason. Switching controller keeps the settings the new one can use and
  says which it reset. [Post-processors](post-processors.md) explains
  every setting and each controller.
- **Heights**: the clearance height, the tool-change height and the park
  point ([Stock, origin and heights](stock-and-origin.md#the-heights)).
- **Tolerances**: **Flatten**, how far a flattened curve may stray from
  the true one, and **Arc fitting**, how far a fitted arc may stray from
  the lines it replaces ([Arc fitting](project-file.md#arc-fitting)).

These are project settings: they are saved with the project and Undo
takes a change back.

### The programs

A job that uses one tool is one program. With more than one tool,
**Programs** says how they are written:

- **One program per tool**: a file for each tool, to run one after
  another. You fit each bit and set its Z zero before you start its
  program.
- **One program for all tools**: one file, which stops at each tool
  change for you to fit the next bit and set Z zero. It is off, with the
  reason in its tooltip, on a controller whose post-processor cannot
  stop for a tool change
  ([Projects with more than one tool](post-processors.md#projects-with-more-than-one-tool)).

This is a setting of the project, the one place it is set: it is saved
with the project, and Undo takes a change back.

A project with an Inlay has a tick for each board, **Female board** and
**Male board**. Untick one to export the other alone, which is how you
cut the plug again after a test fit. The ticks are for this export and
are not saved.

**Program Details** lists each program the export will write, in the
order to run them: its file name, its tool, its operations, its time and
Z range, and what to do before you start it: fit this bit and set work
zero with it, or fit the next bit and set Z zero only. Operations run in
the order of the Operations list, and a tool that is needed again later
gets a second program.

With **One program for all tools** there is one file, and Program
Details lists every tool it uses:

![The plaque's Export step with Programs set to "One program for all tools" and Program Details open: one file, plaque.nc, 3 min 39 s, Z −12..15; under it "2 tools, 1 tool change", then T2 1/8 end mill, 45 s, Screw holes, Border and squiggle, and T1 1/4 end mill, 2 min 54 s, Panel, Square recess, Cut out; then "Before: fit T2 1/8 end mill and set work zero with it."; at the foot, "Ready to write 1 program."](figures/app/export-one-program.png)

The file's line has the time and the Z range of the whole program. The
line under it, *2 tools, 1 tool change*, says how many bits the program
uses and how many times it stops for you to fit the next one. Then comes
each tool in the order the program uses it, with its time and the
operations it cuts. The tools' times add up to the file's. The time you
take to change a bit is not counted. A tool that is needed again later
is listed again, and is one more tool change. The line at the end says
what to do before you start: fit the first bit and set work zero with
it.

### Where they go

**Destination** is the folder, beside the project file unless you
**Choose…** another; the app remembers it for the project. **File name**
is the start of each program's name, and the line under it lists the
files that will be written.

A project that has never been saved has no folder, so **Export G-code**
asks for one first.

### Exporting

Click **Export G-code**. The app writes every program, or none: if one
cannot be written, nothing is. It never writes over the project file, and
exporting never changes the project.

A sheet then shows what was written:

![The result of exporting the coaster, a sheet over the window: "1 program written", the file coaster.nc with its tool and time, the folder with Show in folder and Save setup sheet, the Run instructions, and Done](../quick-start/figures/app/14-export-result.png)

- **The files**, in the order to run them, with each one's tool and
  time, and the **folder**, with a button to copy its path and **Show in
  folder**. One program for all tools says *2 tools, 1 tool change* as
  its tool, and has a line under it for each tool, with what the tool
  cuts and its time.
- **Run instructions**: for each program, what to do before you start
  it, its extents and its time; then notes about your controller, such as
  what the program does and does not do with tool length offsets. One
  program for all tools also lists each tool change, with the line of
  the program it stops at and the bit to fit there.
- **Done** closes the sheet. So do Escape and a click outside it.

The Export step then says what was written, *2 programs written.*,
and **Show result** opens the sheet again. Both stay until you change
the project. Once the project changes, the files on disk are no longer
what it would write, and the rail's check on **Export** goes: export
again.

If nothing could be written, no sheet opens: the step says why, under
the card at its top.

### The setup sheet

**Save setup sheet…** writes one page to take to the machine, by default
`<name>-setup.html` beside the programs. Open it in a browser and print
it. It has the project's name and the date, the post-processor, the
stock and the origin, then each program in the order to run it, with its
tool, its operations, what to do before it, its time, its extents and its
Z range, and the export's warnings and notes. One program for all tools
has each of its tools in the order the program uses them, with the
operations the tool cuts and its time.

**File ▸ Save Setup Sheet…** saves it again for the last export.

## Before you cut

A program that exported cleanly has passed DreamCarve's checks. It has
not been checked against your machine. Before every first run of a
program:

- check that the bit in the spindle is the one the program names, and
  that work zero is set with that bit where the project's origin says;
- place clamps outside the program's extents
  ([Toolpath preview](toolpath-preview.md#program-extents));
- do an air cut: set Z zero above the stock and run the whole program
  once.

[Safety](safety.md) says what DreamCarve checks, what it cannot, and what
to do before every cut.
