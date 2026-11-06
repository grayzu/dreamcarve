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
unchanged. SHA-256 of the files as committed:

| File | SHA-256 |
| --- | --- |
| `Inter-Regular.ttf` | `f473801df83b94aca759501dd148ae60309ed82e897121b1e6c3c03b8dc344f2` |
| `Inter-Medium.ttf` | `64fb3479e46903c636e199ef8b28970f125ed512801a5f200ca209ca5f1769fa` |
| `Inter-SemiBold.ttf` | `636654b1db022e7332d6f2e48c17e670ba4a099f8f6bac97f61abde949c9c1c9` |
