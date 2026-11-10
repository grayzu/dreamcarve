<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Inter

The app's UI face (`docs/specs/phase-4/experience.md`, section 4).

- Source: https://github.com/rsms/inter, release
  [v4.1](https://github.com/rsms/inter/releases/tag/v4.1), archive
  `Inter-4.1.zip` (SHA-256
  `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`).
- Files: the static instances `extras/ttf/Inter-Regular.ttf`,
  `Inter-Medium.ttf` and `Inter-SemiBold.ttf` (egui renders only a
  variable font's default instance, so the static weights are bundled),
  and `LICENSE.txt`.
- Licence: SIL Open Font License 1.1, copyright 2016 The Inter Project
  Authors, with no Reserved Font Name (`LICENSE.txt`), so a modified copy
  keeps the family name. Listed in `NOTICE`.

## Modification: tabular figures frozen in

egui selects no OpenType features, and Inter's default figures are
proportional, so each file has its `tnum` feature frozen into the
character map with OpenType Feature Freezer 1.32.2 (MIT;
https://github.com/twardoch/fonttools-opentype-feature-freezer) on
fontTools 4.68.0:

```sh
pyftfeatfreeze -f tnum extras/ttf/Inter-Regular.ttf Inter-Regular.ttf
pyftfeatfreeze -f tnum extras/ttf/Inter-Medium.ttf Inter-Medium.ttf
pyftfeatfreeze -f tnum extras/ttf/Inter-SemiBold.ttf Inter-SemiBold.ttf
```

Afterwards the ten digits of each weight share one advance width (1328,
1327 and 1325 units for Regular, Medium and SemiBold), and the names are
unchanged.

Inter's `tnum` also gives punctuation a tabular form. For most of it the
change is a few units, but the hyphen grows from 942 to 1328 units, which
spaced out every hyphenated word (`holes-1`, `Bottom-left`). The visual
review (#549) mapped U+002D back to the proportional `hyphen` glyph in
each file, with fontTools 4.60.1; the true minus U+2212, which numbers
use, stays tabular:

```python
from fontTools.ttLib import TTFont
for path in ["Inter-Regular.ttf", "Inter-Medium.ttf", "Inter-SemiBold.ttf"]:
    font = TTFont(path)
    for table in font["cmap"].tables:
        if table.isUnicode() and table.cmap.get(0x2D) == "hyphen.tf":
            table.cmap[0x2D] = "hyphen"
    font.save(path)
```

`theme::figures_are_tabular_and_hyphens_are_not` checks both. SHA-256 of
the files as committed:

| File | SHA-256 |
| --- | --- |
| `Inter-Regular.ttf` | `1fb20b6265de25599122da86b1f52933245da15a48a89759f927c28f09a194a0` |
| `Inter-Medium.ttf` | `fc227267da6b66eb5ccd1cb5e2035c1c64db672c62590191b6f40ba7bed02a12` |
| `Inter-SemiBold.ttf` | `088c12b393f84f65df270ac1cc32fea12a2b00d475b099639e878a89bf925c18` |
