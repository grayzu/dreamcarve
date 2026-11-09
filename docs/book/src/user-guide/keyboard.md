# Keyboard shortcuts

Every command in the table works the same way on Windows, Linux and
macOS. On macOS, ⌘ takes the place of Ctrl and ⌥ the place of Alt. Each
menu row shows its shortcut at the right of the row.

The shortcuts that act on the selection (Select All, Duplicate, Delete,
Hide, Show, Lock, the arrows, F, T and Escape) do nothing while you are
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
| Ctrl+I | ⌘I | Import an SVG or DXF drawing | File ▸ Import Drawing… |
| Ctrl+E | ⌘E | Export the G-code programs (arrives with the export sheet) | File ▸ Export… |
| Ctrl+Q | ⌘Q | Quit DreamCarve | File ▸ Quit |
| Ctrl+Z | ⌘Z | Undo the last change | Edit ▸ Undo |
| Ctrl+Shift+Z | ⇧⌘Z | Redo the change undone | Edit ▸ Redo |
| Ctrl+A | ⌘A | Select every visible shape | Edit ▸ Select All |
| Ctrl+D | ⌘D | Make linked copies of the selection | Edit ▸ Duplicate |
| Delete or Backspace | ⌦ or ⌫ | Delete the selection | Edit ▸ Delete |
| H | H | Hide the selection | Edit ▸ Hide |
| Shift+H | ⇧H | Show the selection | Edit ▸ Show |
| L | L | Lock the selection, or unlock it when every shape in it is locked | Edit ▸ Lock |
| F | F | Fit the Viewport to the stock and shapes | View ▸ Fit View |
| Shift+F | ⇧F | Fit the Viewport to the selection | View ▸ Zoom to Selection |
| T | T | Turn the Tab tool on or off: a click on a shape's outline adds a tab | — |
| Arrow keys | Arrow keys | Nudge the selection 1 mm | — |
| Shift+arrow keys | ⇧ arrow keys | Nudge the selection 10 mm | — |
| Alt+arrow keys | ⌥ arrow keys | Nudge the selection 0.1 mm | — |
| Esc | Esc | Clear the selection, turn off the Tab tool, stop picking on the Viewport, or close a menu | — |
<!-- shortcuts:end -->

The table is generated from the app's own shortcut table, so it always
matches the build it documents.

## Moving through the window

Tab moves the keyboard focus through the window from top left to bottom
right: the menu bar, the toolbar, the Design Tree, the Properties panel,
the Viewport, the CAM panel (its list, then the operation editor), and
the status bar, then back to the menu bar. Shift Tab goes the other way.
Inside an open sheet, such as the Tool Library, Tab stays in the sheet.

The control with the focus has a blue ring around it. The ring shows only
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

Lengths and feed rates show in the project's units, set in the Project
panel. Any length or rate field also takes a value with its own unit, as
the project file does: `1/4 in`, `1 1/4 in` or `6.35 mm` for a length,
`60 in/min` or `1500 mm/min` for a feed rate. A number with no unit is in
the project's units. The project file always stores millimetres, so
switching the units changes only what you see.
