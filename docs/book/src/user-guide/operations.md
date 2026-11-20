# Operations in the app

An **operation** is one kind of cut on a set of shapes with one tool: a
Profile round an outline, a Pocket inside one, a Drill at some circles.
The **Operations** step is where you add them, put them in order and set
each one. The chapters that follow describe what each kind does to the
wood; this one describes the parts of the app they all share.

The examples use the plaque of the
[app quick start](../quick-start/app.md#the-plaque-a-second-tool).

![The Operations step of the plaque: five operations in the list, each with a grip, a ticked box, a coloured glyph, its name, its kind and tool, and an eye; "Cut out" is selected and its editor is in the Inspector, with Setup's rows (Targets outline, Tool 1/4 end mill (T1), Side Outside, Depth 12 mm, Tab request on with a count of 4) over six closed groups that each say what they hold](figures/app/operations-step.png)

## The list

The step panel lists the project's operations in **cut order**: the
machine cuts the top one first. From the left, a row has:

| Part | What it does |
| --- | --- |
| A **grip** | Drag the row up or down by it to change the cut order |
| A **tick box** | Unticked, the operation is left out of the programs and its time out of the estimate, with the warning `operation_disabled` to remind you. It keeps all its settings |
| A **glyph** for the kind | Coloured by the row's place in the list: blue, red, green, purple, orange, and so on for eight colours |
| The **name**, and under it the kind and the tool | Click the row to open the operation in the Inspector |
| An **eye** | Hides the operation's toolpath on the Viewport. This is how you look at the job, not part of it: a hidden toolpath is still exported |
| A **status mark** at the right | A spinner while the toolpath is worked out, a warning mark with a count, or a red mark for an error. Nothing when the operation is clean |

Holding the pointer on a row marks the shapes it cuts, on the Viewport
and in the Design step's list.

> **Toolpath colours.** On the plaque each toolpath has its row's
> colour. That is not always so. The Viewport gives a colour to each
> **tool run**, as the command line's preview does: a V-Carve or an Inlay
> with clearing passes takes one colour for each pass and one for the
> V-bit, an Inlay takes more for its male board, and an operation that
> is unticked or cannot be worked out takes none. The operations after
> it then have a different colour on the Viewport from their row's. The
> **Layers** key shows the colours as drawn.

Over the list are three buttons:

- **Add** adds an operation ([Adding an operation](#adding-an-operation)).
- **Duplicate** copies the selected operation below itself, with its
  shapes and settings, as `<name> (2)`.
- **Delete** deletes the selected operation. Tabs you placed stay: they
  belong to the part, not to the operation ([Tabs](tabs.md#in-the-app)).

Reordering, ticking, duplicating and deleting are each one step for
**Undo**: *Undo Reorder operations*, *Undo Disable “Panel”*, *Undo
Duplicate “Cut out”*, *Undo Delete “Cut out (2)”*.

The order matters. Cut the inside of a part before you cut the part
free, and cut holes and pockets while the stock is still held all round.
[Machining order and cut order](machining-order.md) says more.

## Adding an operation

1. Select the shapes the operation is to cut: on the Viewport, or from
   the list on the **Design** step. The selection stays when you change
   step.
2. On the **Operations** step, click **Add** and choose the kind.

![The Add menu with the plaque's squiggle, an open path, selected: Profile and Engrave at the top, and under a line, greyed, Pocket "An open path cannot take a Pocket", Drill "A Drill needs circles", Bore "A Bore needs circles", V-Carve, Inlay, and Photo V-Carve "A Photo V-Carve needs a picture"](figures/app/operations-add-menu.png)

The menu lists the kinds the selection can take first. The others are
under a line, greyed, each with its reason. With nothing selected every
kind is greyed, and the menu says *Select shapes to add an operation*.

| Kind | Needs | Chapter |
| --- | --- | --- |
| **Profile** | Any shape. An open path is cut on the line, or to its left or right | [Profile](profile.md#in-the-app) |
| **Pocket** | Closed shapes | [Pocket](pocket.md#in-the-app) |
| **Drill** | Circles | [Drill](drill.md#in-the-app) |
| **Bore** | Circles | [Bore](bore.md#in-the-app) |
| **Engrave** | Any shape | [Engrave](engrave.md#in-the-app) |
| **V-Carve** | Closed shapes | [V-Carve](v-carve.md#in-the-app) |
| **Inlay** | Closed shapes | [Inlay](inlay.md#in-the-app) |
| **Photo V-Carve** | A picture, not shapes | [Photo V-Carve](photo-v-carve.md#in-the-app) |

The new operation:

- cuts the selected shapes, in the order you selected them;
- is named for its kind and a number, `Engrave 1`;
- takes the project's first tool that suits the kind (a V-bit for a
  carve, an end mill for a Pocket), or **No tool** when the project has
  none;
- has the same starting values the command line gives that kind;
- opens in the Inspector, with its **Depth** saying **Required**.

A shape's row on the Design step has the same menu: right-click the row
and choose **Add operation**.

## The editor

The Inspector edits the selected operation. There is no OK button: each
field applies when you leave it, and is one step for Undo, *Undo Set
depth of “Panel”*.

- The **header** has the kind's glyph, the operation's name, and its
  kind. Click the name to change it, and press Enter. The name is written
  as a comment in the program.
- **Setup** is always open. It holds what the operation cannot cut
  without: its **Targets**, its **Tool**, and the one or two values that
  decide where the bit goes and how deep.
- Under it are **groups**, in the same order for every kind: **Depth**,
  **Strategy**, **Entry and leads**, **Corners and tabs**, **Order** and
  **Heights**. A kind has only the groups it has settings for. An Inlay
  adds **Male board** and **Cut-out**.

A group starts closed, and its header says what it holds: `Entry and
leads  Ramp 15°, lead`, `Heights  Safe Z 15 mm`. So the whole operation
reads at a glance, and nothing is set out of sight. Click a header to
open the group. The app remembers which groups you keep open, for each
kind of operation.

Hold the pointer on a row's label for a sentence on what it does, and
the name of the field it sets in the project file. Each kind's chapter
has a table of its rows beside those field names.

## Targets

The **Targets** row says what the operation cuts: the shape's name when
it is one, `outline`, or a count, `2 shapes`. While an operation is
selected its targets are outlined in its colour on the Viewport and the
other shapes fade, and on the Design step each target's row has a mark.

There are three ways to change them.

**Edit targets.** Click the pencil in the Targets row. A bar across the
top of the Viewport says what you are doing, and the step panel shows
the Design step's list with a tick box on every row:

![Editing the targets of the Panel pocket: the bar "Editing targets of “Panel”: click shapes to add or remove them" with Cancel and Done; the Design step's list with ticks on pocket, island and square; and the square outlined in the operation's colour on the Viewport](figures/app/operations-targets.png)

Click a shape on the Viewport, or its row, to add it or take it out. A
box dragged on the stock adds every shape it touches; with Alt (⌥ on
macOS) held it takes them out. Nothing else can be selected or moved
meanwhile. **Done**, or Enter, writes the new targets as one step for
Undo. **Cancel**, or Escape, leaves the operation as it was.

**The row's menu.** Select shapes first, then click **⋯** in the Targets
row: **Use selection** makes the selection the targets, and **Add
selection** adds it to them.

**A shape's menu.** While the operation is selected, right-click a
shape's row on the Design step: **Add to “Panel”**, or **Remove from
“Panel”**. Removing a shape from an operation deletes nothing.

An operation takes any shape you give it. A shape its kind cannot cut,
such as an open path in a Pocket or a square in a Drill, is skipped when
the toolpath is worked out, with a warning that names it.

A [Photo V-Carve](photo-v-carve.md#in-the-app) has a **Picture** row in
place of Targets.

## The tool

The **Tool** row lists the project's tools that suit the kind, each with
its number and diameter. A V-Carve, an Inlay and a Photo V-Carve offer
carving bits only. **Add from library…**, at the foot of the list, opens
the [Tool Library](tool-library.md#the-tool-library-sheet) to bring
another tool into the project.

To change a tool's feed or speed, go to the
[Tools step](tool-library.md#the-tools-step): they belong to the tool,
and every operation that uses it gets them.

## Required values, and values from the tool

A value DreamCarve will not guess is outlined in red and says
**Required** until you type it: a cut's **Depth**, a Photo V-Carve's
**Max depth**, an Inlay's four values. An operation with one missing
draws no toolpath, and nothing can be exported.

A value the **tool** can give is empty to begin with and shows the tool's
in grey: **Step-down** reads `tool's: 4 mm`, a Pocket's **Stepover**
`tool's: 40 %`. Leave it empty to follow the tool. Type a value to give
this operation its own; a small arrow then appears in the field, **Use
the tool's value**, which empties it again.

![The Panel pocket with a step-down of 8 typed: the Step-down field outlined in amber with the arrow that goes back to the tool's value, and under it "operation `Panel` (`op3`) cuts 8 mm per pass, deeper than the 6.35 mm diameter of tool `1/4 end mill`; passes that deep can break the bit, so use a step-down of at most 6.35 mm"; a warning mark in the editor's header](figures/app/operations-field-warning.png)

## Problems at a field

A warning or an error that is about one value is shown under that value,
as above, in amber for a warning and red for an error. The group that
holds it opens by itself, the editor's header and the operation's row
carry the mark, and the same line is in the
[Warnings drawer](warnings.md).

The app keeps a value that is out of range, so that you can see what you
typed, and says what is allowed:

```text
error: operations[op3].entry.helix.angle_deg is 60, expected an angle greater than 0 and at most 45 degrees
```

An error stops export until it is fixed. Text that is not a number is
not kept: the field goes back to the value it had, outlined in red, with
the reason under it, and the project is unchanged.

Some problems belong to the whole operation, not to one field: a cut
deeper than the stock allows, or a shape too small for the bit. Those
are in the Warnings drawer, and mark the row and the header.

## When the toolpath is worked out

There is no Generate button. A moment after any change, the app works
out the toolpath of each operation the change affects, and only those.
The row shows a spinner, the old toolpath stays on the Viewport, faded,
and the status bar says *Generating 2 of 5…* with a **Stop** button.

The status bar's **Est.** is the estimated time of every enabled
operation. Hold the pointer on a row for the estimated time of the
program that operation is cut in.

[Toolpath preview](toolpath-preview.md) says how to read what the
Viewport draws, and [Checking and exporting](exporting.md) what happens
next.
