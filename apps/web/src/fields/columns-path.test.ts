import { describe, expect, it } from 'vitest'
import { parseColumnId } from './columns'
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
})
