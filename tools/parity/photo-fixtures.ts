// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Write the Photo V-Carve test pictures, `tests/fixtures/photo/`
 * (sub-phase 3.5, #645).
 *
 *   node photo-fixtures.ts
 *   npx tsx photo-fixtures.ts
 *
 * - `gradient.png`: acceptance 58 and 59's picture. 64 × 32 pixels, 8-bit
 *   grayscale, column `i` holding `round(255 · i / 63)` (black at the left
 *   edge, white at the right), with one pure white 8 × 8 block in the dark
 *   half and one pure black 8 × 8 block in the light half, so neither
 *   block's depth is the gradient's. `tests/fixtures/README.md` gives their
 *   pixel positions.
 * - `ramp.png` (64 × 32, black left to white right) and `radial.png`
 *   (64 × 64, white centre to black edge): the RasterCarve golden cases'
 *   pictures (#647), smooth so the two programs' resampling agrees.
 * - `colour.jpg`: a 16 × 16 colour JPEG, baseline, 4:4:4, of four flat
 *   8 × 8 blocks (a red, a green, a blue and a gray), for `image add`'s
 *   `image_colour_converted` and the decoder's tests.
 * - `colour-progressive.jpg`: the same picture as a progressive JPEG whose
 *   one scan is the DC scan.
 *
 * Every 8 × 8 block of the JPEGs is flat, so the encoder writes the DC
 * coefficient alone (the AC coefficients are 0: one end-of-block code),
 * with a quantisation table of ones. That is a valid JPEG (ITU T.81) that
 * needs no DCT here, and it decodes to within a level or two of the
 * colours below whatever decoder reads it.
 *
 * Synthetic only: no photograph is committed. Dev-only tool, not part of
 * the Cargo workspace. Run it twice: the output must be byte-identical.
 */

import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { crc32, deflateSync } from 'node:zlib'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..')
const outDir = join(repoRoot, 'tests/fixtures/photo')
mkdirSync(outDir, { recursive: true })

// ---------------------------------------------------------------- PNG

function u32(value: number): Buffer {
  const b = Buffer.alloc(4)
  b.writeUInt32BE(value)
  return b
}

function chunk(kind: string, data: Buffer): Buffer {
  const head = Buffer.concat([Buffer.from(kind, 'latin1'), data])
  return Buffer.concat([u32(data.length), head, u32(crc32(head))])
}

/** An 8-bit grayscale PNG of `rows`, no interlace, every row filter 0. */
function grayPng(width: number, height: number, pixel: (i: number, j: number) => number): Buffer {
  const raw = Buffer.alloc((width + 1) * height)
  for (let j = 0; j < height; j++) {
    raw[j * (width + 1)] = 0
    for (let i = 0; i < width; i++) raw[j * (width + 1) + 1 + i] = pixel(i, j)
  }
  const ihdr = Buffer.concat([u32(width), u32(height), Buffer.from([8, 0, 0, 0, 0])])
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

const GRADIENT_WIDTH = 64
const GRADIENT_HEIGHT = 32
/** The white block: columns 8 to 15, rows 12 to 19 (row 0 at the top). */
const WHITE_BLOCK = { column: 8, row: 12 }
/** The black block: columns 48 to 55, rows 12 to 19. */
const BLACK_BLOCK = { column: 48, row: 12 }
const BLOCK = 8

function inBlock(i: number, j: number, at: { column: number; row: number }): boolean {
  return i >= at.column && i < at.column + BLOCK && j >= at.row && j < at.row + BLOCK
}

const gradient = grayPng(GRADIENT_WIDTH, GRADIENT_HEIGHT, (i, j) => {
  if (inBlock(i, j, WHITE_BLOCK)) return 255
  if (inBlock(i, j, BLACK_BLOCK)) return 0
  return Math.round((255 * i) / (GRADIENT_WIDTH - 1))
})
writeFileSync(join(outDir, 'gradient.png'), gradient)

// The RasterCarve golden pictures (#647). Smooth, so RasterCarve's
// resampling and DreamCarve's bilinear sampling agree closely.

/** `ramp.png`: 64 × 32, column `i` holds `round(255 · i / 63)`. */
const ramp = grayPng(64, 32, (i) => Math.round((255 * i) / 63))
writeFileSync(join(outDir, 'ramp.png'), ramp)

/**
 * `radial.png`: 64 × 64, white at the centre to black at the edge. A pixel
 * whose centre is `r` pixels from the picture's centre holds
 * `round(255 · (1 + cos(π · min(r / 32, 1))) / 2)`: flat at the centre and
 * at the edge's midpoints, black past them (the corners), so the profile
 * has no kink for the two programs' resampling to round differently.
 */
const RADIAL_SIDE = 64
const RADIAL_RADIUS = 32
const radial = grayPng(RADIAL_SIDE, RADIAL_SIDE, (i, j) => {
  const r = Math.hypot(i + 0.5 - RADIAL_SIDE / 2, j + 0.5 - RADIAL_SIDE / 2)
  const t = Math.min(r / RADIAL_RADIUS, 1)
  return Math.round((255 * (1 + Math.cos(Math.PI * t))) / 2)
})
writeFileSync(join(outDir, 'radial.png'), radial)

// --------------------------------------------------------------- JPEG

/** The colours of the JPEGs' four blocks, left to right, top to bottom. */
const COLOURS: [number, number, number][] = [
  [200, 60, 40], // red
  [40, 160, 80], // green
  [50, 70, 200], // blue
  [128, 128, 128], // gray
]

/** JFIF's RGB to YCbCr (ITU T.871), rounded. */
function ycbcr([r, g, b]: [number, number, number]): number[] {
  const clamp = (v: number) => Math.min(255, Math.max(0, Math.round(v)))
  return [
    clamp(0.299 * r + 0.587 * g + 0.114 * b),
    clamp(128 - 0.168736 * r - 0.331264 * g + 0.5 * b),
    clamp(128 + 0.5 * r - 0.418688 * g - 0.081312 * b),
  ]
}

/** Writes bits most significant first, stuffing a 0 after each 0xFF. */
class BitWriter {
  bytes: number[] = []
  acc = 0
  count = 0
  put(value: number, length: number) {
    for (let k = length - 1; k >= 0; k--) {
      this.acc = (this.acc << 1) | ((value >> k) & 1)
      this.count++
      if (this.count === 8) {
        this.bytes.push(this.acc)
        if (this.acc === 0xff) this.bytes.push(0)
        this.acc = 0
        this.count = 0
      }
    }
  }
  finish(): Buffer {
    // Pad the last byte with ones (T.81, F.1.2.3).
    while (this.count !== 0) this.put(1, 1)
    return Buffer.from(this.bytes)
  }
}

/** ITU T.81 table K.3, the luminance DC table: code lengths per size. */
const DC_BITS = [0, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0]
const DC_VALUES = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]

/** The canonical codes of a Huffman table, by symbol (T.81, C.2). */
function huffmanCodes(bits: number[], values: number[]): Map<number, [number, number]> {
  const codes = new Map<number, [number, number]>()
  let code = 0
  let k = 0
  for (let length = 1; length <= 16; length++) {
    for (let n = 0; n < bits[length - 1]; n++) codes.set(values[k++], [code++, length])
    code <<= 1
  }
  return codes
}

const DC_CODES = huffmanCodes(DC_BITS, DC_VALUES)

/** A DC difference: its size category's code, then its extra bits. */
function putDc(writer: BitWriter, diff: number) {
  const size = diff === 0 ? 0 : Math.floor(Math.log2(Math.abs(diff))) + 1
  const [code, length] = DC_CODES.get(size)!
  writer.put(code, length)
  if (size > 0) writer.put(diff > 0 ? diff : diff + (1 << size) - 1, size)
}

function segment(marker: number, data: Buffer): Buffer {
  const head = Buffer.alloc(4)
  head.writeUInt16BE(0xff00 | marker, 0)
  head.writeUInt16BE(data.length + 2, 2)
  return Buffer.concat([head, data])
}

/** A Huffman table segment's body: class and id, 16 counts, the values. */
function dhtBody(classAndId: number, bits: number[], values: number[]): Buffer {
  return Buffer.from([classAndId, ...bits, ...values])
}

/**
 * A 16 × 16 JPEG of the four flat `COLOURS` blocks, 4:4:4, quantisation
 * table of ones. Baseline: one interleaved scan, each block its DC and an
 * end-of-block. Progressive: one interleaved DC scan, no AC scan.
 */
function colourJpeg(progressive: boolean): Buffer {
  const side = 16
  const jfif = Buffer.from([0x4a, 0x46, 0x49, 0x46, 0, 1, 1, 0, 0, 1, 0, 1, 0, 0])
  const dqt = Buffer.from([0, ...new Array(64).fill(1)])
  const sof = Buffer.from([
    8,
    0, side, 0, side,
    3,
    1, 0x11, 0,
    2, 0x11, 0,
    3, 0x11, 0,
  ])
  // The AC table holds one code, `0`, for the end-of-block symbol 0x00.
  const acBits = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
  const dht = progressive
    ? dhtBody(0x00, DC_BITS, DC_VALUES)
    : Buffer.concat([dhtBody(0x00, DC_BITS, DC_VALUES), dhtBody(0x10, acBits, [0])])
  const sos = Buffer.from([3, 1, 0x00, 2, 0x00, 3, 0x00, 0, progressive ? 0 : 63, 0])

  const writer = new BitWriter()
  const previous = [0, 0, 0]
  for (const colour of COLOURS) {
    const levels = ycbcr(colour)
    for (let c = 0; c < 3; c++) {
      // A flat block of level-shifted value s has DC coefficient 8 s.
      const dc = 8 * (levels[c] - 128)
      putDc(writer, dc - previous[c])
      previous[c] = dc
      if (!progressive) writer.put(0, 1) // end of block
    }
  }
  return Buffer.concat([
    Buffer.from([0xff, 0xd8]),
    segment(0xe0, jfif),
    segment(0xdb, dqt),
    segment(progressive ? 0xc2 : 0xc0, sof),
    segment(0xc4, dht),
    segment(0xda, sos),
    writer.finish(),
    Buffer.from([0xff, 0xd9]),
  ])
}

writeFileSync(join(outDir, 'colour.jpg'), colourJpeg(false))
writeFileSync(join(outDir, 'colour-progressive.jpg'), colourJpeg(true))

for (const colour of COLOURS) {
  const [y] = ycbcr(colour)
  console.log(`rgb(${colour.join(', ')}): Y ${y}`)
}
console.log(`Wrote ${outDir}`)
