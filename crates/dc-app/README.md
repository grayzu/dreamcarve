# dc-app

The application core of the DreamCarve app (`docs/specs/phase-4/design.md`,
"Data model → dc-app"): one `History` of `Project` snapshots, the `Command`
enum whose `apply` makes the next snapshot and whose `label` names the undo
step, the selection, the view settings, the generation bookkeeping (each
operation's digest, what is pending and stale, Stop and Regenerate all),
the Warnings panel's rows (`check` on the project as it stands), the
preferences record, the string table, units formatting and parsing, the
Viewport's geometry (the `Scene` it draws, with each operation's runs and
marks, the camera with the one Y flip, picking, the grid, the transform
handles and the direction arrows' placement, ported from PureCutCNC),
the toolpath display's view state (`view`: visibility, *By depth*'s ramp
and legend, the by-operation key, the program extents), the status bar's
cells (`status`), the Design Tree's
rows (`tree`), the Properties panel's placement fields (`properties`,
with the mixed values of `mixed`, ported from PureCutCNC), the project
tools' editing rules (`tools`), the Tool Library window's model
(`library`, ported from PureCutCNC), the Import drawing sheet's model
(`import`: the bytes the shell read, parsed by `dc-import` with the
options the sheet shows), the Operations list with its Add menu and each
kind's new operation (`operations`), and the operation editor's table of
rows per kind with its key-path edits (`editor`, the idea of
PureCutCNC's `operationFields.ts`).

Pure, as every `dc-*` crate: no file, terminal, clock or network I/O, no
egui, no panics. `dc-cam` and `dc-gcode` are reached through `dc-job`
only (functional spec, S20). The shell (`dreamcarve-app`) owns the window,
the worker thread and every file.
