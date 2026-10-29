import { describe, expect, it } from 'vitest'
import { columnIdOf, columnTitle, parseColumnId } from './columns'
import { readPathFromDoc } from '../path-cell'

describe('qualified column ids in the grid', () => {
  it('strips #qualifiers before reading Typesense fields', () => {
    const defs = {
      attributes: [{ id: '018f-desc', key: 'description', language_dependent: true }],
    }
    const doc = { a_018f_desc__en: "A firm cow's-milk cheddar aged for several months. Synthetic demo recipe." }
    const id = 'description@en#language:en'
    const { path } = parseColumnId(id)
    expect(path).toBe('description@en')
    expect(readPathFromDoc(doc, defs, path)).toContain("firm cow's-milk cheddar")
  })

  it('keeps hop path text without cardinality/unit encoding', () => {
    const { path, qualifiers } = parseColumnId('articles>net_weight#cardinality:all|unit:kg')
    expect(path).toBe('articles>net_weight')
    expect(qualifiers.map((q) => `${q.kind}:${q.optionId}`)).toEqual(['cardinality:all', 'unit:kg'])
  })

  it('round-trips ART-CHED-200 Specific label through encode and parse', () => {
    const id = columnIdOf('articles>net_weight', [
      {
        kind: 'cardinality',
        optionId: 'specific',
        label: 'ART-CHED-200',
        value: '018f1000-0000-7000-8000-000000000022',
      },
      { kind: 'unit', optionId: 'g', label: 'g' },
    ])
    expect(id).toContain('ART-CHED-200')
    const parsed = parseColumnId(id)
    expect(parsed.qualifiers.find((q) => q.kind === 'cardinality')?.label).toBe('ART-CHED-200')
    expect(parsed.qualifiers.find((q) => q.kind === 'cardinality')?.value).toBe(
      '018f1000-0000-7000-8000-000000000022',
    )
    expect(
      columnTitle({
        id,
        path: 'articles>net_weight',
        fieldPath: ['articles', 'net_weight'],
        pathNames: ['Articles', 'Net weight'],
        qualifiers: parsed.qualifiers,
        leaf: { id: 'net_weight', name: 'Net weight', typeLabel: 'number', shape: 'unit', units: ['g', 'kg'] },
        viaManyLink: true,
      }),
    ).toMatch(/ART-CHED-200/)
  })
})
