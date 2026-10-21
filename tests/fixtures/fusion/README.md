<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->

# Fusion tool library fixtures

Test input for `dc_library::fusion` and `dreamcarve tools import`
(`docs/specs/phase-2/design.md`, "Algorithms → Fusion import").

These files are hand-written by the DreamCarve contributors in the
structure of Autodesk's published Fusion tool library schema
(`FusionToolLibrarySchemas.html` in
`https://cam.autodesk.com/tools/tools/Tooling%20Schema/JSON%20Schema.zip`).
They contain no Autodesk data: every tool, value, name and GUID was made
up for the tests. Autodesk's own sample libraries carry no license
statement and are reference only; they are not committed
(`docs/specs/phase-2/functional.md`, open question 10).

- `sample.json` is a Fusion `.json` library: one tool of each type
  DreamCarve imports (flat, ball, bull nose and slot mills, a chamfer mill
  with `TA` 45, drill, spot drill, counter sink, counter bore, reamer), an
  inch tool, a tool with two presets, a drill preset without `v_f`, a tool
  with `post-process.number` 5 and tools with number 0, and one of each
  family DreamCarve skips (thread mill, tap, face, form, lollipop,
  dovetail and tapered mills, a turning tool, a probe, a holder).
- `sample.tools` is the same document zipped as Fusion writes a `.tools`
  file: one deflated entry, `tools.json`, byte-identical to `sample.json`.
  `fusion::zip_and_plain_json_both_read` checks that the two agree.

To regenerate `sample.tools` after editing `sample.json`:

```sh
mkdir -p /tmp/fusion-zip && cp sample.json /tmp/fusion-zip/tools.json
(cd /tmp/fusion-zip && rm -f sample.tools && zip -X -D -9 sample.tools tools.json)
cp /tmp/fusion-zip/sample.tools .
```
