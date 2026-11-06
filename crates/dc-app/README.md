# dc-app

The application core of the DreamCarve app (`docs/specs/phase-4/design.md`,
"Data model → dc-app"): one `History` of `Project` snapshots, the `Command`
enum whose `apply` makes the next snapshot and whose `label` names the undo
step, the selection, the view settings, the generation bookkeeping (each
operation's digest, what is pending and stale, Stop and Regenerate all),
the Warnings panel's rows (`check` on the project as it stands), the
preferences record, the string table, units formatting and parsing, the
Viewport's geometry (the `Scene` it draws, the camera with the one Y
flip, picking and the grid), the project tools' editing rules (`tools`)
and the Tool Library window's model (`library`, ported from PureCutCNC).

Pure, as every `dc-*` crate: no file, terminal, clock or network I/O, no
egui, no panics. `dc-cam` and `dc-gcode` are reached through `dc-job`
only (functional spec, S20). The shell (`dreamcarve-app`) owns the window,
the worker thread and every file.
