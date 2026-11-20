# Keyboard shortcuts

Every command in the table works the same way on Windows, Linux and
macOS. On macOS, ⌘ takes the place of Ctrl and ⌥ the place of Alt. Each
menu row shows its shortcut at the right of the row.

The shortcuts that act on the selection (Select All, Duplicate, Delete,
Hide, Show, Lock, Rename, Rotate, Flip, the arrows, F, T and Escape) do
nothing while you are
typing in a field. In a field, Ctrl A selects the field's text, and the
arrows move the text cursor. The file and undo shortcuts work everywhere
except in an open sheet.

<!-- shortcuts:begin (generated from crates/dreamcarve-app/src/keyboard.rs) -->
| Windows and Linux | macOS | Does | Menu |
| --- | --- | --- | --- |
| Ctrl+N | ⌘N | Start a new project | File ▸ New |
| Ctrl+O | ⌘O | Open a project | File ▸ Open… |
| Ctrl+S | ⌘S | Save the project | File ▸ Save |
| Ctrl+Shift+S | ⇧⌘S | Save the project under a new name | File ▸ Save As… |
| Ctrl+I | ⌘I | Import an SVG or DXF drawing | File ▸ Add Drawing to Project… |
| Ctrl+E | ⌘E | Open the Export step, to export the G-code programs | File ▸ Export G-code… |
| Ctrl+Q | ⌘Q | Quit DreamCarve | File ▸ Quit |
| Ctrl+Z | ⌘Z | Undo the last change | Edit ▸ Undo |
| Ctrl+Shift+Z | ⇧⌘Z | Redo the change undone | Edit ▸ Redo |
| Ctrl+A | ⌘A | Select every visible shape | Edit ▸ Select All |
| Ctrl+D | ⌘D | Make linked copies of the selection | Edit ▸ Duplicate |
| Delete or Backspace | ⌦ or ⌫ | Delete the selection | Edit ▸ Delete |
| H | H | Hide the selection | Edit ▸ Hide |
| Shift+H | ⇧H | Show the selection | Edit ▸ Show |
| L | L | Lock the selection, or unlock it when every shape in it is locked | Edit ▸ Lock |
| F2 | F2 | Rename the selected shape in place | Edit ▸ Rename |
| R | R | Rotate the selection 90° clockwise | Arrange ▸ Rotate 90° Clockwise |
| Shift+R | ⇧R | Rotate the selection 90° anticlockwise | Arrange ▸ Rotate 90° Anticlockwise |
| Ctrl+Shift+H | ⇧⌘H | Flip the selection left to right | Arrange ▸ Flip Horizontal |
| Ctrl+Shift+V | ⇧⌘V | Flip the selection top to bottom | Arrange ▸ Flip Vertical |
| F | F | Fit the Viewport to the stock and shapes | View ▸ Fit View |
| Shift+F | ⇧F | Fit the Viewport to the selection | View ▸ Zoom to Selection |
| Ctrl+1 | ⌘1 | Open the Design step | View ▸ Go to Design |
| Ctrl+2 | ⌘2 | Open the Stock step | View ▸ Go to Stock |
| Ctrl+3 | ⌘3 | Open the Tools step | View ▸ Go to Tools |
| Ctrl+4 | ⌘4 | Open the Operations step | View ▸ Go to Operations |
| Ctrl+5 | ⌘5 | Open the Export step | View ▸ Go to Export |
| T | T | Turn the Tab tool on or off: a click on a shape's outline adds a tab | — |
| Arrow keys | Arrow keys | Nudge the selection 1 mm | — |
| Shift+arrow keys | ⇧ arrow keys | Nudge the selection 10 mm | — |
| Alt+arrow keys | ⌥ arrow keys | Nudge the selection 0.1 mm | — |
| Esc | Esc | Clear the selection, turn off the Tab tool, stop picking on the Viewport, or close a menu | — |
<!-- shortcuts:end -->

The table is generated from the app's own shortcut table, so it always
matches the build it documents.

## What the selection shortcuts act on

"The selection" in the table is the selected shapes. Some of the keys
reach further, and some less far:

- **Delete**, **H**, **Shift H**, **L**, **F2**, **Shift F** and the
  **arrow keys** act on a selected picture as they do on a shape.
  **Delete** also deletes the selected tabs.
- **Ctrl D** and the rotate and flip keys act on shapes only. A picture
  is not duplicated, turned or flipped.
- **F2** with an operation selected, and no shape, renames the operation
  in the Inspector's header.
- None of them acts on the Tools or Operations lists. Those have their
  own **Duplicate** and **Delete** buttons.
- **Ctrl A** selects every shape that is not hidden. It does not select
  pictures or tabs.
- **Ctrl 1** to **Ctrl 5**, like the file and undo shortcuts, work while
  you are typing in a field.

## With the mouse

These are held while you click or drag on the Viewport. They are not in
the table because they have no command of their own.

| Hold | While you | To |
| --- | --- | --- |
| Shift | Click a shape, a tab or a row | Add it to the selection |
| Ctrl (⌘ on macOS) | Click a shape or a row | Add it, or take it out |
| Shift | Drag the rotate handle | Turn in steps of 15° |
| Alt (⌥) | Drag a corner handle | Stretch the shape without keeping its proportions |
| Alt (⌥) | Drag a box while editing an operation's targets | Take the shapes out of the targets |
| Space, or the middle button | Drag | Pan the Viewport |

While you edit an operation's targets, **Enter** is **Done** and
**Escape** is **Cancel**. While a name is being edited, in the list or in
the Inspector's header, **Enter** keeps the new name and **Escape** the
old one.

## Moving through the window

Tab moves the keyboard focus through the window from top left to bottom
right: the header (the menus, Undo, Redo and the primary button), the
rail (the five steps, then Warnings, Appearance and Help), the step
panel, the Viewport, the Inspector when something is selected, and the
status bar, then back to the header. Shift Tab goes the other way.
Ctrl 1 to Ctrl 5 (⌘1 to ⌘5 on macOS) open the Design, Stock, Tools,
Operations and Export steps from anywhere.
Inside an open sheet, such as the Tool Library, Tab stays in the sheet.

The control with the focus has a ring in the accent colour around it. The ring shows only
when you moved there with the keyboard, not when you clicked.

- **Enter** presses the focused button.
- **Space** turns the focused switch or checkbox on or off.
- **The arrows** move between the rows of a list. On the Viewport they
  nudge the selection.
- **Escape** leaves the focused control. In a field, it also puts back
  the value the field had before you started typing.

## Typing values

A field keeps what you type until you leave it, with Tab, Enter or a
click elsewhere. Then the value is applied as one step, which Undo takes
back in one go. Escape leaves the field and keeps the old value.

Lengths and feed rates show in the project's units, set on the Stock
step. Any length or rate field also takes a value with its own unit, as
the project file does: `1/4 in`, `1 1/4 in` or `6.35 mm` for a length,
`60 in/min` or `1500 mm/min` for a feed rate. A number with no unit is in
the project's units. The project file always stores millimetres, so
switching the units changes only what you see.
