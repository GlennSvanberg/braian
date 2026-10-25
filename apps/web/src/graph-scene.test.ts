import { forceCenter, forceCollide, forceLink, forceManyBody, forceSimulation } from 'd3-force'
import { describe, expect, it } from 'vitest'
import type { Json } from './api'
import { buildScene, capScene, mergeScene, nodeMark, nodeRadius, relationLabel, structureCaption, visibleLabelIds } from './graph-scene'

const defs: Json = {
  types: [{ key: 'artikel', labels: { en: 'Article', sv: 'Artikel' } }, { key: 'produkt', labels: { en: 'Product' } }],
  value_lists: [
    {
      key: 'status',
      options: [{ code: 'ok', labels: { en: 'OK', sv: 'OK' } }],
    },
    {
      key: 'farg',
      options: [{ code: 'gron', labels: { en: 'Green', sv: 'Grön', de: 'Grün' } }],
    },
  ],
  attributes: [
    { key: 'nummer', labels: { en: 'Number', sv: 'Nummer' }, kind: { type: 'text' } },
    { key: 'namn', labels: { en: 'Name', sv: 'Namn' }, kind: { type: 'text' } },
    { key: 'beskrivning', labels: { en: 'Description', de: 'Beschreibung' }, kind: { type: 'text' } },
    { key: 'farg', labels: { en: 'Colour', de: 'Farbe', sv: 'Färg' }, kind: { type: 'value_list', list: 'farg' } },
    { key: 'vikt', labels: { en: 'Weight' }, kind: { type: 'number' } },
  ],
  structures: [
    {
      key: 'sortiment',
      labels: { en: 'eCom', sv: 'eCom' },
      groups: [
        { id: 'outdoor', key: 'friluft', labels: { en: 'Outdoor' } },
        { id: 'tents', key: 'talt', parent: 'outdoor', labels: { en: 'Tents' } },
      ],
    },
  ],
}

const graph: Json = {
  id: 'tent',
  key: 'ART-TALT-2',
  type: 'artikel',
  groups: { sortiment: 'talt' },
  relations: [{ kind: 'article_of', object_id: 'prod', key: 'PROD-TALT', type_id: 'ignored' }],
  attributes: [
    { key: 'namn', label: 'Name', sources: [{ from: 'type' }] },
    { key: 'nummer', label: 'Number', sources: [{ from: 'type' }] },
    { key: 'beskrivning', label: 'Description', sources: [{ from: 'type' }] },
    { key: 'farg', label: 'Colour', sources: [{ from: 'type' }] },
    {
      key: 'vikt',
      label: 'Weight',
      sources: [{ from: 'group', structure: 'sortiment', class: 'friluft', group: 'friluft', inherited: true }],
    },
  ],
}

const values: Json = {
  namn: { sv: 'Tält 2 personer', en: '2-person tent', de: 'Zelt 2 Personen' },
  nummer: 'ART-TALT-2',
  beskrivning: { en: 'Light 2-layer hiking tent.', sv: 'Lätt tält.' },
  farg: ['gron'],
  vikt: { value: '2.4', unit: 'kg' },
}

describe('graph marks', () => {
  it('puts a short mark inside and keeps the full name for the caption', () => {
    expect(nodeMark('2-person tent')).toBe('2T')
    expect(nodeMark('Fjäll tents')).toBe('FT')
    expect(nodeMark('Colour')).toBe('C')
    expect(nodeMark('OK')).toBe('OK')
    expect(nodeMark('Description').includes('…')).toBe(false)
    expect(nodeRadius('object', 8)).toBeGreaterThan(nodeRadius('value', 1))
    expect(nodeRadius('object', 1)).toBeLessThan(nodeRadius('object', 6))
  })

  it('hides the lower-priority label when two captions occupy the same box', () => {
    const shown = visibleLabelIds(
      [
        { id: 'value', priority: 0, left: 0, top: 0, right: 40, bottom: 16 },
        { id: 'object', priority: 3, left: 10, top: 0, right: 50, bottom: 16 },
        { id: 'free', priority: 0, left: 80, top: 0, right: 120, bottom: 16 },
      ],
      1,
    )
    expect([...shown].sort()).toEqual(['free', 'object'])
    const zoomedOut = visibleLabelIds(
      [
        { id: 'value', priority: 0, left: 0, top: 0, right: 20, bottom: 12 },
        { id: 'object', priority: 3, left: 40, top: 0, right: 60, bottom: 12 },
      ],
      0.4,
    )
    expect([...zoomedOut]).toEqual(['object'])
  })
})

describe('graph scene', () => {
  it('names nodes and provenance edges in English', () => {
    const scene = buildScene({ graph, values, defs })
    const captions = scene.nodes.map((node) => node.caption)
    expect(captions).toContain('2-person tent')
    expect(captions).toContain('Colour')
    expect(captions).toContain('Number')
    expect(captions).toContain('Description')
    expect(captions).toContain('Outdoor')
    expect(captions).toContain('Tents')
    expect(captions).toContain('eCom')
    expect(captions.some((caption) => ['farg', 'namn', 'friluft', 'nummer', 'Nummer', 'Farbe', 'Beschreibung'].includes(caption))).toBe(false)

    const colour = scene.nodes.find((node) => node.caption === 'Colour')
    expect(colour?.subtitle).toBe('Green')
    expect(scene.edges.some((edge) => edge.source === colour?.id && edge.label === 'From type' && edge.target === 'object:tent')).toBe(true)

    const weight = scene.nodes.find((node) => node.caption === 'Weight')
    expect(weight?.subtitle).toContain('2.4')
    expect(scene.edges.some((edge) => edge.source === weight?.id && edge.label === 'Inherited' && edge.target === 'class:sortiment:friluft')).toBe(true)
    expect(scene.edges.some((edge) => edge.label === 'Applies in' && edge.source === 'class:sortiment:friluft' && edge.target === 'group:sortiment:friluft')).toBe(true)
    expect(weight?.properties.some((property) => property.label === 'Origin' && property.value === 'Inherited: Outdoor')).toBe(true)
    expect(scene.edges.some((edge) => edge.label === 'Article of' && edge.target === 'object:prod')).toBe(true)
    const unresolved = scene.nodes.find((node) => node.id === 'object:prod')
    expect(unresolved?.caption).toBe('Object')
    expect(unresolved?.rawCaption).toBe(true)
    expect(unresolved?.properties.some((property) => property.value === 'PROD-TALT')).toBe(true)
    expect(unresolved?.caption.includes('PROD-TALT')).toBe(false)
    const named = buildScene({
      graph,
      values,
      defs,
      known: { prod: { name: 'Fjäll tents', typeKey: 'produkt' } },
    })
    const resolved = named.nodes.find((node) => node.id === 'object:prod')
    expect(resolved?.caption).toBe('Fjäll tents')
    expect(resolved?.rawCaption).toBe(false)
    expect(resolved?.properties.some((property) => property.value === 'PROD-TALT')).toBe(true)
    expect(relationLabel('variant_of')).toBe('Variant of')
    expect(structureCaption('sovsack')).toBe('Sleeping bags')
  })

  it('draws an ancestor group attribute as inherited when it has no class', () => {
    const scene = buildScene({
      graph: {
        id: 'tent',
        key: 'ART-TALT-2',
        type: 'artikel',
        groups: { sortiment: 'talt' },
        relations: [],
        attributes: [
          {
            key: 'vikt',
            label: 'Weight',
            sources: [{ from: 'group', structure: 'sortiment', group: 'friluft', inherited: true }],
          },
        ],
      },
      values: { vikt: { value: '2.4', unit: 'kg' } },
      defs,
    })
    const weight = scene.nodes.find((node) => node.caption === 'Weight')
    expect(scene.edges.some((edge) => edge.source === weight?.id && edge.label === 'Inherited' && edge.target === 'group:sortiment:friluft')).toBe(true)
    expect(scene.edges.some((edge) => edge.source === weight?.id && edge.label === 'From type')).toBe(false)
    expect(weight?.properties.some((property) => property.value === 'Inherited: Outdoor')).toBe(true)
  })

  it('upgrades a key to a human name without duplicating the node', () => {
    const first = buildScene({ graph, values, defs })
    const second = buildScene({
      graph: { id: 'prod', key: 'PROD-TALT', type: 'produkt', groups: {}, relations: [], attributes: [] },
      values: { namn: { en: 'Fjäll tents', sv: 'Fjäll tält' }, nummer: 'PROD-TALT' },
      defs,
    })
    const merged = mergeScene(first, second)
    const matches = merged.nodes.filter((node) => node.id === 'object:prod')
    expect(matches).toHaveLength(1)
    expect(matches[0]?.caption).toBe('Fjäll tents')
    expect(matches[0]?.subtitle).toBe('Product')
    const again = mergeScene(merged, second)
    expect(again.nodes.filter((node) => node.id === 'object:prod')).toHaveLength(1)
    expect(again.edges.filter((edge) => edge.label === 'Article of')).toHaveLength(1)
  })

  it('drops nodes past the cap and the edges that pointed at them', () => {
    const nodes = Array.from({ length: 5 }, (_, index) => ({
      id: `n${index}`,
      kind: 'object' as const,
      caption: `Node ${index}`,
      subtitle: 'Object',
      rawCaption: false,
      properties: [],
    }))
    const capped = capScene(
      {
        nodes,
        edges: [
          { id: 'keep', source: 'n0', target: 'n1', label: 'Has' },
          { id: 'drop', source: 'n0', target: 'n4', label: 'Has' },
        ],
      },
      4,
    )
    expect(capped.capped).toBe(true)
    expect(capped.scene.nodes.map((node) => node.id)).toEqual(['n0', 'n1', 'n2', 'n3'])
    expect(capped.scene.edges.map((edge) => edge.id)).toEqual(['keep'])
  })

  it('ticks a few hundred nodes inside a second', () => {
    const count = 240
    const nodes = Array.from({ length: count }, (_, index) => ({
      id: String(index),
      x: Math.cos(index) * 120,
      y: Math.sin(index) * 120,
    }))
    const links = Array.from({ length: count }, (_, index) => ({
      source: String(index),
      target: String((index + 7) % count),
    }))
    const started = performance.now()
    const sim = forceSimulation(nodes)
      .force('link', forceLink(links).id((node) => String((node as { id?: string }).id ?? '')).distance(80))
      .force('charge', forceManyBody().strength(-30).distanceMax(240))
      .force('center', forceCenter())
      .force('collide', forceCollide(12))
      .stop()
    for (let step = 0; step < 40; step += 1) sim.tick()
    expect(performance.now() - started).toBeLessThan(1000)
    expect(Number.isFinite(nodes[0]?.x)).toBe(true)
  })
})
