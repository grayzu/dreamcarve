// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Capture the outline-text polygons of PureCutCNC's medial-axis tests.
 *
 *   npx tsx vcarve-fixtures.ts --purecutcnc ../../../purecutcnc
 *
 * DreamCarve has no fonts until phase 5, so the ported text tests of
 * `dc-geometry::medial` (#402) run on the regions PureCutCNC's own resolver
 * makes from its outline fonts. For each fixture this builds the project the
 * upstream test builds, runs `resolvePocketRegions`, and writes the regions to
 * `tests/fixtures/vcarve/<name>.json` in DreamCarve's frame: millimetres,
 * Y-up (`x_mm = x · scale`, `y_mm = −y · scale`, `scale` 25.4 for an inch
 * project, 1 for mm). Beside each region it records what PureCutCNC's
 * medial axis gives on it (resolution, node, edge, component, corner, leaf
 * and chain counts) twice: `purecutcnc` in PureCutCNC's own frame, as its
 * tests see it, and `purecutcnc_y_up` on the written coordinates, the input
 * the Rust tests give DreamCarve. The two differ where a region's topology
 * sits on the noise floor (ga-small's "A"). It also writes the fixtures'
 * README with the font notice.
 *
 * Fixtures (docs/specs/phase-3/design.md, "Golden corpus"):
 *   rag.json       "Rag", 10 mm  (testScaledOutlineTextTopologyStaysStable)
 *   g.json         "g", 10 and 30 mm (testOutlineGRejectsFlatteningSpokesAcrossScales)
 *   ga-large.json  the saved v-carve-noise-test project, both operations
 *                  (testSavedGlyphProjectRejectsNoiseAcrossLinkedScales)
 *   ga-small.json  the same glyphs regenerated at 0.25 in
 *                  (testFreshQuarterInchGlyphRejectsGridCornerNoise)
 *
 * Dev-only tool, not part of the Cargo workspace. Run it twice: the output
 * must be byte-identical.
 */

import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..')

function arg(name: string, fallback: string): string {
  const i = process.argv.indexOf(`--${name}`)
  if (i !== -1 && process.argv[i + 1]) return process.argv[i + 1]
  return fallback
}

const checkout = resolve(arg('purecutcnc', join(repoRoot, '..', 'purecutcnc')))

async function load<T>(relativePath: string): Promise<T> {
  return (await import(pathToFileURL(join(checkout, relativePath)).href)) as T
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Any = any
interface Pt { x: number; y: number }
interface Region { outer: Pt[]; islands: Pt[][] }
interface Graph { nodes: Array<Pt & { clearance: number }>; adjacency: number[][] }

interface Engine {
  resolvePocketRegions: (project: Any, operation: Any) => { bands: Array<{ regions: Region[] }> }
  computeMedialAxis: (region: Region, options: { resolution: number }) => Graph
  extractChains: (graph: Graph) => number[][]
  regionConvexCorners: (region: Region) => unknown[]
  resolveMedialResolution: (region: Region) => { resolution: number } | null
  normalizeProject: (input: Any) => Any
  projectWithFeatures: (base: Any, features: Any[]) => Any
  defaultTool: (units: string, index: number) => Any
  newProject: (name: string, units: string) => Any
  rectProfile: (x: number, y: number, w: number, h: number) => Any
  getTextFontOptions: (style: string) => Array<{ id: string; label: string }>
}

async function engine(): Promise<Engine> {
  const parts = await Promise.all([
    load<Partial<Engine>>('src/engine/toolpaths/resolver.ts'),
    load<Partial<Engine>>('src/engine/toolpaths/vcarveMedial/index.ts'),
    load<Partial<Engine>>('src/store/helpers/projectFormat.ts'),
    load<Partial<Engine>>('src/test/projectFixtures.ts'),
    load<Partial<Engine>>('src/types/project.ts'),
    load<Partial<Engine>>('src/text/index.ts'),
  ])
  return Object.assign({}, ...parts) as Engine
}

function upstreamCommit(): string {
  return execFileSync('git', ['-C', checkout, 'rev-parse', 'HEAD'], { encoding: 'utf8' }).trim()
}

// --- Projects, as the upstream tests build them ----------------------------

function vBit(e: Engine): Any {
  return { ...e.defaultTool('mm', 1), id: 't1', type: 'v_bit', vBitAngle: 60, diameter: 6 }
}

function medialOp(featureIds: string[]): Any {
  return {
    id: 'op1', name: 'op', kind: 'v_carve_medial', pass: 'rough', enabled: true,
    showToolpath: true, debugToolpath: false,
    target: { source: 'features', featureIds }, toolRef: 't1',
    stepdown: 2, stepover: 0.4, feed: 800, plungeFeed: 300, rpm: 18000,
    pocketPattern: 'offset', pocketAngle: 0, stockToLeaveRadial: 0, stockToLeaveAxial: 0,
    finishWalls: true, finishFloor: true, carveDepth: 1, maxCarveDepth: 2,
    cutDirection: 'conventional', machiningOrder: 'level_first',
  }
}

/** `makeOutlineTextFeature` of `vcarveMedial.test.ts`. */
function textProject(e: Engine, text: string, size: number, bounds: { width: number; height: number }): { project: Any; fontId: string } {
  const font = e.getTextFontOptions('outline')[0]
  const feature = {
    id: 'f-text', name: text, kind: 'text',
    text: { text, style: 'outline', fontId: font.id, size },
    folderId: null,
    sketch: {
      profile: e.rectProfile(0, 0, bounds.width, bounds.height),
      origin: { x: 0, y: 0 }, orientationAngle: 0, dimensions: [], constraints: [],
    },
    operation: 'subtract', z_top: 0, z_bottom: -5, visible: true, locked: false,
  }
  return { project: e.projectWithFeatures({ ...e.newProject('test', 'mm'), tools: [vBit(e)] }, [feature]), fontId: font.id }
}

function noiseProject(e: Engine): Any {
  const path = join(checkout, 'src/engine/test-fixtures/v-carve-noise-test.camj')
  return e.normalizeProject(JSON.parse(readFileSync(path, 'utf8')))
}

/** `quarterInchNoiseRegressionProject` of `noiseRegression.test.ts`. */
function quarterInchNoiseProject(e: Engine): Any {
  const project = structuredClone(noiseProject(e))
  const definition = project.featureDefinitions['f-0004']
  const scale = 0.25 / definition.text.size
  const origin = definition.profile.start
  const scalePoint = (p: Pt): Pt => ({ x: origin.x + (p.x - origin.x) * scale, y: origin.y + (p.y - origin.y) * scale })
  definition.text = { ...definition.text, size: 0.25 }
  definition.profile = {
    ...definition.profile,
    start: scalePoint(definition.profile.start),
    segments: definition.profile.segments.map((s: Any) => ({ ...s, to: scalePoint(s.to) })),
  }
  return project
}

// --- Statistics, as the upstream tests compute them ------------------------

function componentCount(graph: Graph): number {
  const seen = new Array<boolean>(graph.nodes.length).fill(false)
  let components = 0
  for (let start = 0; start < graph.nodes.length; start += 1) {
    if (seen[start]) continue
    components += 1
    const queue = [start]
    seen[start] = true
    while (queue.length > 0) {
      const node = queue.pop() as number
      for (const n of graph.adjacency[node]) {
        if (!seen[n]) { seen[n] = true; queue.push(n) }
      }
    }
  }
  return components
}

function stats(e: Engine, region: Region): Record<string, number> {
  const resolved = e.resolveMedialResolution(region)
  if (!resolved) throw new Error('fixture region has no medial resolution')
  const graph = e.computeMedialAxis(region, { resolution: resolved.resolution })
  const leaves = graph.adjacency.filter((n) => n.length === 1).length
  const zeroLeaves = graph.nodes.filter((n, i) => n.clearance < 1e-9 && graph.adjacency[i].length === 1).length
  return {
    resolution: resolved.resolution,
    nodes: graph.nodes.length,
    edges: graph.adjacency.reduce((sum, n) => sum + n.length, 0) / 2,
    components: componentCount(graph),
    corners: e.regionConvexCorners(region).length,
    non_corner_leaves: leaves - zeroLeaves,
    chains: e.extractChains(graph).length,
  }
}

// --- Output ----------------------------------------------------------------

function toDreamCarve(region: Region, scale: number): Region {
  const map = (p: Pt): Pt => ({ x: p.x * scale, y: -p.y * scale })
  return { outer: region.outer.map(map), islands: region.islands.map((loop) => loop.map(map)) }
}

function loopJson(loop: Pt[]): string {
  return `[${loop.map((p) => `[${p.x}, ${p.y}]`).join(', ')}]`
}

interface Case { name: string; units: string; regions: Region[] }

/** One README row per captured region. */
interface Row { fixture: string; name: string; region: number; upstream: Record<string, number>; yUp: Record<string, number> }

function fixtureJson(e: Engine, fixture: string, rows: Row[], source: string, font: string, cases: Case[]): string {
  const lines: string[] = ['{']
  lines.push(`  "source": ${JSON.stringify(source)},`)
  lines.push(`  "font": ${JSON.stringify(font)},`)
  lines.push(`  "purecutcnc_commit": ${JSON.stringify(upstreamCommit())},`)
  lines.push('  "frame": "millimetres, Y-up: x_mm = x * scale, y_mm = -y * scale of PureCutCNC\'s Y-down project units",')
  lines.push('  "cases": [')
  cases.forEach((c, ci) => {
    const scale = c.units === 'inch' ? 25.4 : 1
    lines.push('    {')
    lines.push(`      "name": ${JSON.stringify(c.name)},`)
    lines.push(`      "purecutcnc_units": ${JSON.stringify(c.units)},`)
    lines.push(`      "scale": ${scale},`)
    lines.push('      "regions": [')
    c.regions.forEach((region, ri) => {
      const mapped = toDreamCarve(region, scale)
      const upstream = stats(e, region)
      const yUp = stats(e, mapped)
      const drift = Object.keys(upstream).filter((k) => k !== 'resolution' && upstream[k] !== yUp[k])
      if (drift.length > 0) {
        console.warn(`vcarve-fixtures: ${c.name} region ${ri}: the frame change moves ${drift.map((k) => `${k} ${upstream[k]} -> ${yUp[k]}`).join(', ')}`)
      }
      rows.push({ fixture, name: c.name, region: ri, upstream, yUp })
      lines.push('        {')
      lines.push(`          "purecutcnc": ${JSON.stringify(upstream)},`)
      lines.push(`          "purecutcnc_y_up": ${JSON.stringify(yUp)},`)
      lines.push(`          "outer": ${loopJson(mapped.outer)},`)
      lines.push(`          "holes": [${mapped.islands.map(loopJson).join(', ')}]`)
      lines.push(`        }${ri + 1 < c.regions.length ? ',' : ''}`)
    })
    lines.push('      ]')
    lines.push(`    }${ci + 1 < cases.length ? ',' : ''}`)
  })
  lines.push('  ]')
  lines.push('}')
  return `${lines.join('\n')}\n`
}

function regionsOf(e: Engine, project: Any, operation: Any): Region[] {
  return e.resolvePocketRegions(project, operation).bands
    .flatMap((band) => band.regions)
    .map((r) => ({ outer: r.outer, islands: r.islands }))
}

function threeVersion(): string {
  const pkg = JSON.parse(readFileSync(join(checkout, 'node_modules/three/package.json'), 'utf8')) as { version: string }
  return pkg.version
}

function readme(rows: Row[], textFont: string, glyphFont: string): string {
  const stat = (s: Record<string, number>): string =>
    `${s.nodes} / ${s.edges} / ${s.components} / ${s.corners} / ${s.non_corner_leaves} / ${s.chains}`
  const table = rows.map((r) =>
    `| \`${r.fixture}\` | ${r.name} | ${r.region} | ${r.upstream.resolution} | ${stat(r.upstream)} | ${stat(r.yUp)} |`)
  const droidNotice = readFileSync(join(checkout, 'node_modules/three/examples/fonts/droid/NOTICE'), 'utf8')
    .split('\n').map((line) => line.trim()).filter((line) => line.length > 0)
  return [
    '<!--',
    'Copyright 2026 The DreamCarve Contributors',
    'SPDX-License-Identifier: Apache-2.0',
    '-->',
    '# V-Carve text fixtures',
    '',
    'Outline-text regions for the ported medial-axis tests of',
    '`dc-geometry::medial` (#402). DreamCarve has no fonts until phase 5, so',
    'these are the polygons PureCutCNC\'s resolver (`resolvePocketRegions`)',
    'makes from its outline fonts, written by `tools/parity/vcarve-fixtures.ts`',
    `from PureCutCNC commit \`${upstreamCommit()}\`. Do not edit them by hand;`,
    're-run the tool.',
    '',
    'Frame: millimetres, Y-up (ADR-0001): `x_mm = x · scale`,',
    '`y_mm = −y · scale` of PureCutCNC\'s Y-down project units, `scale` 25.4',
    'for the inch project and 1 for the millimetre ones.',
    '',
    '| File | Source |',
    '| --- | --- |',
    '| `rag.json` | "Rag" at 10 mm, as `testScaledOutlineTextTopologyStaysStable` builds it |',
    '| `g.json` | "g" at 10 and 30 mm, as `testOutlineGRejectsFlatteningSpokesAcrossScales` builds it |',
    '| `ga-large.json` | PureCutCNC\'s `src/engine/test-fixtures/v-carve-noise-test.camj` ("gA", 0.4 in, an inch project), both `v_carve_medial` operations, as `testSavedGlyphProjectRejectsNoiseAcrossLinkedScales` |',
    '| `ga-small.json` | the same glyphs regenerated at 0.25 in, operation `op0005`, as `testFreshQuarterInchGlyphRejectsGridCornerNoise` |',
    '',
    '## Captured values',
    '',
    'PureCutCNC\'s medial axis on each region at its automatic resolution:',
    'nodes / edges / components / convex corners / non-corner leaves / chains.',
    '"Upstream frame" is PureCutCNC\'s own input, which its tests assert on;',
    '"Y-up" is the same code on the coordinates written here, which are what',
    'DreamCarve\'s tests read. Resolution is in PureCutCNC\'s units.',
    '',
    '| File | Case | Region | Resolution | Upstream frame | Y-up |',
    '| --- | --- | --- | --- | --- | --- |',
    ...table,
    '',
    'Where the two columns differ the region\'s topology sits on the noise',
    'floor: the frame change alone keeps or drops a spur in PureCutCNC\'s own',
    'code. The ported tests say which column they assert.',
    '',
    '## Font notice',
    '',
    `The glyphs are converted outline data of the Droid fonts. "Rag" and "g"`,
    `use \`${textFont}\` (Droid Sans Bold, PureCutCNC's first outline font by`,
    `label); "gA" uses \`${glyphFont}\` (Droid Serif Bold with PureCutCNC's`,
    `italic slant). PureCutCNC reads them from three.js ${threeVersion()},`,
    '`examples/fonts/droid/*.typeface.json`, whose NOTICE reads:',
    '',
    '```text',
    ...droidNotice,
    '```',
    '',
    'The Droid fonts are licensed under the Apache License, Version 2.0, the',
    'licence in this repository\'s `LICENSE` file. These fixtures are modified',
    'from them: outlines flattened to polygons and resolved into regions by',
    'PureCutCNC, then scaled and mirrored into DreamCarve\'s frame by the',
    'capture tool. They are test data and are not part of any DreamCarve',
    'binary. (The phase 3 design spec expected Noto Sans SC under the OFL;',
    'PureCutCNC uses Noto Sans SC only for its PDF booklets, not for outline',
    'text.)',
    '',
  ].join('\n')
}

async function main(): Promise<void> {
  const e = await engine()
  const outDir = join(repoRoot, 'tests', 'fixtures', 'vcarve')
  mkdirSync(outDir, { recursive: true })
  const rows: Row[] = []
  const write = (name: string, body: string): void => {
    writeFileSync(join(outDir, name), body)
    console.log(`vcarve-fixtures: wrote tests/fixtures/vcarve/${name}`)
  }

  const rag = textProject(e, 'Rag', 10, { width: 60, height: 14 })
  write('rag.json', fixtureJson(e, 'rag.json', rows, 'vcarveMedial.test.ts makeOutlineTextFeature("Rag", size 10)', rag.fontId, [
    { name: 'Rag 10 mm', units: 'mm', regions: regionsOf(e, rag.project, medialOp(['f-text'])) },
  ]))

  const gCases = [10, 30].map((size) => {
    const g = textProject(e, 'g', size, { width: size * 2, height: size * 2 })
    return { fontId: g.fontId, c: { name: `g ${size} mm`, units: 'mm', regions: regionsOf(e, g.project, medialOp(['f-text'])) } }
  })
  write('g.json', fixtureJson(e, 'g.json', rows, 'vcarveMedial.test.ts makeOutlineTextFeature("g", sizes 10 and 30)', gCases[0].fontId, gCases.map((g) => g.c)))

  const saved = noiseProject(e)
  const savedFont = saved.featureDefinitions['f-0004'].text.fontId as string
  const medialOps = (saved.operations as Any[]).filter((op) => op.kind === 'v_carve_medial')
  write('ga-large.json', fixtureJson(e, 'ga-large.json', rows, 'src/engine/test-fixtures/v-carve-noise-test.camj, both v_carve_medial operations', savedFont,
    medialOps.map((op) => ({ name: `${String(op.id)} ${String(op.name)}`, units: saved.meta.units as string, regions: regionsOf(e, saved, op) }))))

  const quarter = quarterInchNoiseProject(e)
  const op5 = (quarter.operations as Any[]).find((op) => op.id === 'op0005')
  write('ga-small.json', fixtureJson(e, 'ga-small.json', rows, 'noiseRegression.test.ts quarterInchNoiseRegressionProject, operation op0005 (0.25 in text)', savedFont, [
    { name: 'op0005 at 0.25 in', units: quarter.meta.units as string, regions: regionsOf(e, quarter, op5) },
  ]))

  write('README.md', readme(rows, rag.fontId, savedFont))
}

main().catch((err: unknown) => {
  console.error(err)
  process.exit(1)
})
