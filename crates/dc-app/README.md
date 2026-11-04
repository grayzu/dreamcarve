# dc-app

The application core of the DreamCarve app (`docs/specs/phase-4/design.md`,
"Data model → dc-app"): one `History` of `Project` snapshots, the `Command`
enum whose `apply` makes the next snapshot and whose `label` names the undo
step, the selection, the view settings, the generation bookkeeping, the
preferences record, the string table, and units formatting and parsing.

Pure, as every `dc-*` crate: no file, terminal, clock or network I/O, no
egui, no panics. `dc-cam` and `dc-gcode` are reached through `dc-job`
only (functional spec, S20). The shell (`dreamcarve-app`) owns the window,
the worker thread and every file.
