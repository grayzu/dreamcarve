# Golden case: smoke

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case smoke --post grblhal`.

- Source: `tools/parity/cases/smoke.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (edge_route_outside)
- Header date frozen to 2026-01-01 by the capture script.
- Capture check only: the phase 0 test that the capture runs and is
  repeatable (two captures are byte-identical). No test compares DreamCarve
  against this case.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
