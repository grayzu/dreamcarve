<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Bundled fonts

The fonts the Text tool offers without the user adding any (phase 5
functional spec, capability 91; `docs/specs/phase-5/design.md`, "Bundled
fonts"). They are compiled into `dc-text` (`src/bundled.rs`) and so
travel in every binary that links it. Each file is as its publisher
released it: none is renamed, subset or modified. Each folder holds the
licence the files came with, and every font is listed in `NOTICE`.

The single-line font, *Simple Stroke*, is not a file: it is PureCutCNC's
table of strokes, ported in `src/stroke.rs` under Apache-2.0.

| File | Version | Source | SHA-256 |
| --- | --- | --- | --- |
| `inter/Inter-Regular.ttf` | 4.001 (release v4.1) | https://github.com/rsms/inter, release [v4.1](https://github.com/rsms/inter/releases/tag/v4.1), `Inter-4.1.zip` (SHA-256 `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`), `extras/ttf/Inter-Regular.ttf` | `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82` |
| `inter/Inter-Bold.ttf` | 4.001 (release v4.1) | the same archive, `extras/ttf/Inter-Bold.ttf` | `288316099b1e0a47a4716d159098005eef7c0066921f34e3200393dbdb01947f` |
| `source-serif-4/SourceSerif4-Regular.ttf` | 4.005 | https://github.com/adobe-fonts/source-serif, branch `release`, `TTF/SourceSerif4-Regular.ttf` | `e5a4ee6a3d87bb9024796be390c6771e2a0eb1883dae25effaf57ca01668e24b` |
| `source-serif-4/SourceSerif4-Bold.ttf` | 4.005 | the same branch, `TTF/SourceSerif4-Bold.ttf` | `7cf4f4e1ad74f45058d5bc61716b82560442fbdcd9d3654d2dea96bf6c683d86` |
| `roboto-slab/RobotoSlab-Bold.ttf` | 2.002 | https://github.com/googlefonts/robotoslab, branch `main`, `fonts/ttf/RobotoSlab-Bold.ttf` | `09cd9e359296e2f9482e2445ba921041913b55e652645df083079a85a272fce4` |
| `pacifico/Pacifico-Regular.ttf` | 3.001 | https://github.com/google/fonts, branch `main`, `ofl/pacifico/Pacifico-Regular.ttf` (the family's own repository is https://github.com/googlefonts/Pacifico) | `5b6c0d5334a7bf77dea52b975c5a0c408878c0f7115ed5b6fb151f634b7bf701` |
| `allerta-stencil/AllertaStencil-Regular.ttf` | 1.02 | https://github.com/google/fonts, branch `main`, `ofl/allertastencil/AllertaStencil-Regular.ttf` | `8c504d1be20768941ab04251a389ad82d5782d647fa1c4029c811dc5bc2abd59` |

Fetched on 2026-11-23. The versions are the fonts' own (name table, id
5).

## Licences

| Family | Licence | Copyright | Reserved Font Name |
| --- | --- | --- | --- |
| Inter | SIL Open Font License 1.1 (`inter/LICENSE.txt`) | Copyright (c) 2016 The Inter Project Authors (https://github.com/rsms/inter) | none |
| Source Serif 4 | SIL Open Font License 1.1 (`source-serif-4/LICENSE.md`) | Copyright 2014 - 2023 Adobe (http://www.adobe.com/) | Source |
| Roboto Slab | Apache License 2.0 (`roboto-slab/LICENSE.txt`) | Copyright 2018 The Roboto Slab Project Authors (https://github.com/googlefonts/robotoslab) | not applicable |
| Pacifico | SIL Open Font License 1.1 (`pacifico/OFL.txt`) | Copyright 2018 The Pacifico Project Authors (https://github.com/googlefonts/Pacifico) | none |
| Allerta Stencil | SIL Open Font License 1.1 (`allerta-stencil/OFL.txt`) | Copyright (c) 2010, Matt McInerney (matt@pixelspread.com) | Allerta |

The Open Font License lets a font be bundled and embedded with software,
and asks that its copyright notice and the licence travel with it; a
Reserved Font Name may not be used for a modified version. Nothing here
is modified, so the names stand. Roboto Slab is under the Apache License
2.0, as DreamCarve is.

## Replacing or adding a font

Fetch the file from its publisher, put it with its licence in a folder
here, record its source and checksum above, add it to `BUNDLED` in
`src/bundled.rs` and to `NOTICE`, and run the crate's tests:
`every_bundled_font_parses` holds the table to the files' own names.
