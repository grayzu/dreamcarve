# Phase 4 mock-ups

Low-fidelity wireframes for the phase 4 functional spec (`../functional.md`).
Each is referenced from the capability it illustrates. They show layout,
labels and field names, not visual design: grey boxes, the real labels, the
file's field names, and colour only where the spec gives it a meaning (the
`--preview` operation palette, error red, selection blue).

| File | Shows | Capabilities |
| --- | --- | --- |
| `01-workspace.svg` | The window: Design Tree, Properties, Viewport, CAM panel, status bar | 50, 57, 58, 63, 65, 66, 72, 73 |
| `02-import-dialog.svg` | Import a drawing (DXF options, layers) | 55 |
| `03-stock-origin.svg` | Project panel: stock, origin, heights, post-processor | 60, 61, 62 |
| `04-tool-library.svg` | Tool Library window and the Fusion import report | 64 |
| `05-operation-profile.svg` | Operation editor, Profile, every group open | 66, 67, 68, 69 |
| `06-operation-vcarve.svg` | V-Carve with clearing passes; Inlay (male) | 68, 70 |
| `07-operation-fields.svg` | Drill (countersink), Bore, Engrave, Photo V-Carve editors | 68 |
| `08-tabs-on-canvas.svg` | Placed tabs on the Viewport, the Tab tool | 69 |
| `09-preview-legend.svg` | Visibility panel, depth colouring, marks, extents, stale toolpaths | 71, 72, 73, 74 |
| `10-warnings-panel.svg` | Warnings panel | 76 |
| `11-export-dialog.svg` | Export dialog | 77, 78 |
| `12-empty-state.svg` | First run and the empty project | 79 |

The plaque from the phase 2 quick start is the project in every frame, so
the frames can be checked against a real `check --preview`.

## Rendering

The SVGs are generated; edit `render.py` and re-run it rather than editing
an SVG by hand:

```sh
python3 render.py
```

Standard library only. To look at a frame as a PNG on macOS without extra
tools:

```sh
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
  --window-size=1280,800 --screenshot=/tmp/01.png "file://$PWD/01-workspace.svg"
```

GitHub renders the SVGs inline in the spec, so a PR review needs neither.
