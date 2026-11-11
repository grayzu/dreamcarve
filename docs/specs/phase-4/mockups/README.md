# Phase 4 mock-ups

Mock-ups for the phase 4 functional spec (`../functional.md`). Each is
referenced from the capability it illustrates. Since the screenshot review
of #549 (2026-11-10) they are drawn with `../experience.md`'s light tokens,
type scale, icons and control sizes, so a frame shows the look the review
asked for as well as the layout, labels and the file's field names. The
egui build is the authority on pixels; `experience.md` is the authority on
the design. Yellow boxes are the designer's notes, not app chrome.

| File | Shows | Capabilities |
| --- | --- | --- |
| `01-workspace.svg` | The window: title, Design Tree with target marks, Project Settings and Properties, Viewport, CAM panel, status bar | 50, 50a, 50b, 57, 58, 58a, 66, 67 |
| `02-import-dialog.svg` | Add a drawing (DXF options, layers, the drawing alone in the preview) | 55 |
| `03-stock-origin.svg` | Project Settings: units, stock, origin, heights, post-processor; XY zero moving the marker | 53, 60–62 |
| `04-tool-library.svg` | Tool Library: vendor groups, editing pane, library units; the import review | 64, 64a, 64b |
| `05-operation-profile.svg` | Operation editor, Profile: header, Setup, closed groups with summaries | 66, 66a, 69 |
| `06-operation-vcarve.svg` | V-Carve with clearing passes; Inlay with the male thickness in Setup | 66a, 68, 68a, 70 |
| `07-operation-fields.svg` | Drill (countersink), Bore, Engrave, Photo V-Carve editors | 66a, 68 |
| `08-tabs-on-canvas.svg` | Placed tabs on the Viewport, the Tab tool | 69 |
| `09-preview-legend.svg` | The toolpath view popover, the depth legend with the positive band, extents, stale toolpaths | 71–74 |
| `10-warnings-panel.svg` | Warnings drawer with carve notes; one group without a heading | 76, 76a |
| `11-export-dialog.svg` | Export sheet, Program Details closed and open, blocked by an error | 77 |
| `12-empty-state.svg` | First run: two paths, pictured examples | 79 |
| `13-export-result.svg` | Export result and the printed setup sheet | 77a, 77b |
| `14-menus-and-sheets.svg` | File, Arrange and View menus, Appearance, the unsaved-changes sheet | 51, 58a, 79a |
| `15-targets.svg` | Target highlighting and target editing | 57, 67 |
| `16-tools-tab.svg` | Tools tab: New tool, required rates | 63, 66b |

The plaque from the phase 2 quick start is the project in most frames, so
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
  --window-size=1280,820 --screenshot=/tmp/01.png "file://$PWD/01-workspace.svg"
```

GitHub renders the SVGs inline in the spec, so a PR review needs neither.
