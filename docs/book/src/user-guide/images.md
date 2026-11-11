# Images

A project can hold pictures as well as drawings. A picture placed on the
stock is a **bitmap**. DreamCarve carves a bitmap with a [Photo
V-Carve](photo-v-carve.md); nothing else uses one. This chapter is about
getting a picture into a project: choosing and preparing it, `init` and
`dreamcarve image`, and how a picture is stored in the project file.

To cut the outline of a picture instead, a logo or a silhouette as a
shape to profile, pocket or inlay, trace it into a drawing first:
[Preparing artwork](preparing-artwork.md#tracing-a-picture-in-inkscape).

The commands below are run by the test suite as written, on the
synthetic pictures in `tests/fixtures/photo/`: `gradient.png`, a 64 × 32
pixel black-to-white ramp, and `colour.jpg`, a 16 × 16 pixel picture of
four flat colours.

## Choosing a picture

A Photo V-Carve shows a picture as grooves of different depths, in one
colour of wood. What carves well:

- **Strong contrast.** The darkest parts are carved `max_depth_mm` deep
  and the lightest not at all, so a picture that uses the whole range,
  from near black to near white, gives the most. A flat, gray picture
  carves as a flat, gray board.
- **A simple background.** A plain sky or wall carves as an even field of
  grooves and lets the subject stand out; a busy background competes with
  it. Crop close round the subject.
- **Large shapes over fine detail.** The grooves are a millimetre or two
  apart, so anything smaller than a groove spacing is lost: a face 100 mm
  wide carves well; a crowd does not.
- **Enough pixels, not more.** The carve reads the picture every
  `sample_mm` along each groove (0.25 mm by default), so 4 to 8 pixels
  per millimetre of footprint is plenty: 400 to 800 pixels across for a
  picture carved 100 mm wide. More only makes the project file bigger.

Before you add it, in any image editor:

1. **Crop** it to what you want carved, at the proportions you want on
   the board.
2. **Make it grayscale**, and look at it that way: that is what is
   carved. (DreamCarve converts a colour picture itself, but a colour that
   looks strong can be a dull gray.)
3. **Raise the contrast**, with Levels or Curves, until the darkest part
   is near black and the lightest near white.
4. **Scale it down** to 4 to 8 pixels per millimetre of the size you will
   carve it, and save it as PNG or JPEG.

## Starting a project from a picture

`init` on a PNG or JPEG writes a project to carve it, as `init` on a
drawing writes one to profile it:

<!-- doc-test: run -->
```sh
dreamcarve init gradient.png --width 100
```

```text
Imported a 64 × 32 px picture from gradient.png.
Wrote gradient.dcv: stock 120.00 × 70.00 mm, post-processor grblhal, picture b1 (64 × 32 px) on 100.000 × 50.000 mm at (10.000, 10.000), carved by one Photo V-Carve.
Values to set; export refuses until every one is set:
gradient.dcv:10:21: stock.thickness_mm is not set (mm)
gradient.dcv:35:22: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`
gradient.dcv:36:24: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`
gradient.dcv:37:14: tools[t1].rpm is not set (RPM) for tool `Set me`
gradient.dcv:50:23: operations[op1].max_depth_mm is not set (mm) for operation `Photo V-Carve`
Next: edit gradient.dcv and replace each `null` listed above; the tool is a 60° V-bit, so set the one you have. Then run `dreamcarve check gradient.dcv`.
```

Give the picture's size on the stock with exactly one of `--width` or
`--height`, in millimetres; the other side follows the picture's
proportions, so `--height 50` gives the same 100 × 50 mm here. Both, or
neither, is a usage error, and nothing is written:

<!-- doc-test: run, exit 2 -->
```sh
dreamcarve init gradient.png --width 100 --height 50
```

```text
error: a picture needs exactly one of --width and --height, in mm; the other side follows the picture's proportions
```

The project holds:

- the picture, as bitmap `b1`, named after the file, with its
  bottom-left corner 10 mm in from the stock's, at (10, 10);
- the stock, the picture plus 10 mm all round. A groove along the
  picture's edge is half its width past the picture, and the margin keeps
  that on the stock;
- a 60° V-bit 6.35 mm across, `t1`, with no feeds;
- one Photo V-Carve, `op1`, that carves `b1` with `t1`, with every value
  but `max_depth_mm` set to a starting point ([Photo
  V-Carve](photo-v-carve.md)).

`-o` names the project, and `--post` and `--units` work as for a drawing
([The commands](overview.md#dreamcarve-init-svgdxf)).

## Listing a project's pictures

<!-- doc-test: run -->
```sh
dreamcarve image list gradient.dcv
```

```text
ID  NAME      PX     FOOTPRINT_MM    AT_MM          USED_BY
b1  gradient  64x32  100.000x50.000  10.000,10.000  op1
```

| Column | Meaning |
| --- | --- |
| `ID` | The bitmap's id, which a Photo V-Carve's `image` names. |
| `NAME` | Its name. |
| `PX` | Its size in pixels, width x height. |
| `FOOTPRINT_MM` | Its size on the stock, mm, width x height. |
| `AT_MM` | Its bottom-left corner, X,Y in mm from the stock's bottom-left corner. |
| `USED_BY` | The Photo V-Carves that carve it, or `-` for none. |

A project with no pictures prints `<project> has no pictures.`

## Adding a picture to a project

`dreamcarve image add` puts a picture into a project you already have,
such as one made from a drawing. Here it adds two pictures to the coaster
of [Your first cut](../quick-start/first-cut.md):

<!-- doc-test: run -->
```sh
dreamcarve init coaster.svg
dreamcarve image add coaster.dcv gradient.png --width 60 --at 25,40
dreamcarve image add coaster.dcv colour.jpg --width 16 --at 47,20 --name swatch
dreamcarve image list coaster.dcv
```

```text
Added picture b1 `gradient` to coaster.dcv: 64 × 32 px on 60.000 × 30.000 mm at (25.000, 40.000).
warning: image_colour_converted: the picture is in colour; it was converted to grayscale by luminance (0.299 R + 0.587 G + 0.114 B)
Added picture b2 `swatch` to coaster.dcv: 16 × 16 px on 16.000 × 16.000 mm at (47.000, 20.000).
ID  NAME      PX     FOOTPRINT_MM   AT_MM          USED_BY
b1  gradient  64x32  60.000x30.000  25.000,40.000  -
b2  swatch    16x16  16.000x16.000  47.000,20.000  -
```

(`init`'s own output for the coaster is in that chapter.)

| Option | Meaning |
| --- | --- |
| `--width <MM>` or `--height <MM>` | The picture's size on the stock; exactly one. The other side follows the picture's proportions. |
| `--at <X,Y>` | Where the picture's bottom-left corner goes, in mm from the stock's bottom-left corner. Default `0,0`. |
| `--name <NAME>` | The bitmap's name. Default: the file's name without its extension. |

The new bitmap gets the next free id, `b1`, `b2`, and so on. `image add`
changes nothing else: the stock is not resized, and no operation is
added. To carve it, add a Photo V-Carve to `operations` by hand, as
[Photo V-Carve](photo-v-carve.md#adding-one-by-hand) shows, and make sure
the picture and the groove half-width round it are on the stock, or
`check` refuses it. Like the `tabs` commands, `image add` places any tab
request the project's Profiles have before it saves.

## How a picture is read

PNG and JPEG are read, told apart by the file's extension (`.png`,
`.jpg`, `.jpeg`); a file whose contents are the other format is refused.
Every PNG colour type and bit depth is read, and baseline and progressive
JPEG.

A picture is stored as 8-bit grayscale, 0 black to 255 white:

| What the file has | What is stored | Warning |
| --- | --- | --- |
| Gray pixels | Each pixel as it is. | none |
| Colour | Its luminance, `0.299 R + 0.587 G + 0.114 B`, as a black-and-white photo shows it. A file in a colour format whose pixels are all gray says nothing. | `image_colour_converted` |
| Transparency | Each pixel laid on white first, so a transparent background carves as white: not carved. | `image_alpha_flattened` |
| 16 bits per sample | Each sample's high byte. Nothing a carve can show is lost. | `image_16_bit` |

A warning never stops the import; it is printed once, by `init` or
`image add`, and not again by `check`.

## Where a picture sits on the stock

The picture appears on the stock as it does on screen: its top row is at
the top, the highest Y, and its left column at the lowest X. Its
**footprint** is the rectangle from its corner (`x_mm`, `y_mm`) to
(`x_mm + width_mm`, `y_mm + height_mm`), and each pixel covers an equal
part of it. A picture cannot be rotated in this release: turn it in an
image editor before you add it.

## Pictures in the project file

The picture's pixels are stored in the project file, as the tools are,
so a `.dcv` with a picture is still one file: copy it to another machine
and it exports the same program, without the picture file. The project
grows by about the size of the picture as a grayscale PNG, plus a third
for the text encoding: a few hundred kilobytes for a photo of 600 × 300
pixels.

`init` wrote this bitmap:

<!-- doc-test: contains gradient.dcv -->
```json
  "bitmaps": [
    {
      "id": "b1",
      "name": "gradient",
      "width_px": 64,
      "height_px": 32,
      "x_mm": 10.0,
      "y_mm": 10.0,
      "width_mm": 100.0,
      "height_mm": 50.0,
      "visible": true,
      "locked": false,
```

| Field | Meaning |
| --- | --- |
| `id` | `"b1"`, `"b2"`, …: unique among the bitmaps. A Photo V-Carve's `image` names it. |
| `name` | Your name for it. |
| `width_px`, `height_px` | Its size in pixels, as the stored PNG has it. |
| `x_mm`, `y_mm` | Its bottom-left corner on the stock. Edit these to move it. |
| `width_mm`, `height_mm` | Its size on the stock, above 0. They must keep the picture's proportions: to resize it, change both, or add it again with `image add`. |
| `visible`, `locked` | For the GUI: whether it is shown, and whether it can be moved. They change nothing that is cut. |
| `png_base64` | The picture: an 8-bit grayscale PNG, base64-encoded, on one line. Do not edit it; add the picture again instead. |

[The project file](project-file.md#bitmaps) lists the checks `check`
makes on each field.

## Limits

- A picture is at most 8192 pixels on a side. A larger one is refused
  before it is read; scale it down first.
- The preview `check --preview` writes includes every picture, so a
  preview of a project with a large picture is a large file.
- One picture per Photo V-Carve; several Photo V-Carves may carve one
  picture, or several pictures.
- No rotation, no cropping and no levels in DreamCarve: prepare the
  picture first.

When a file cannot be read, `init` and `image add` stop with an error and
write nothing ([Warnings: Photo V-Carve](warnings.md#photo-v-carve)):

| Message | What to do |
| --- | --- |
| "cannot import …: the file is named as a PNG picture but holds a JPEG; rename it to match" | Rename the file to its real format's extension. |
| "cannot import …: the picture could not be read: …" | The file is damaged, cut short, or not a picture. Save it again from the image editor. |
| "cannot import …: the picture is … × … px, and a side is at most 8192 px; scale it down first" | Scale it down. |
