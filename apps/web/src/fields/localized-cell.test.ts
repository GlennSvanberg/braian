import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { attributeByKey } from '../index-fields'
import { parseColumnId } from './columns'
import { readPathFromDoc, summarizePathValues } from '../path-cell'
import { cellEditShouldPatch } from '../cell-edit'
import type { Json } from '../api'

const defs = JSON.parse(
  readFileSync(new URL('../../../../crates/server/demo/definitions.json', import.meta.url), 'utf8'),
) as Json

/** Mirrors panes columnNeedsPathValues for searchable-only / language fields. */
function columnNeedsPathValues(id: string, defsJson: Json) {
  const { path, qualifiers } = parseColumnId(id)
  if (path.includes('>')) return true
  if (qualifiers.some((q) => q.kind === 'unit' || q.kind === 'cardinality')) return true
  if (path.includes('@')) return true
  const key = (path.split('.')[0] ?? path).split('@')[0] ?? path
  const attr = attributeByKey(defsJson, key)
  if (!attr) return false
  if (attr.language_dependent) return true
  const index = attr.index as Json | undefined
  if (index && !index.filterable && !index.sortable) return true
  return false
}

describe('localized Description (en) cells', () => {
  it('flags description@en for pathValues because it is searchable-only', () => {
    const id = 'description@en#language:en'
    expect(columnNeedsPathValues(id, defs)).toBe(true)
    const attr = attributeByKey(defs, 'description')
    expect(attr?.language_dependent).toBe(true)
    const index = attr?.index as { filterable?: boolean; sortable?: boolean } | undefined
    expect(index?.filterable).toBe(false)
    expect(index?.sortable).toBe(false)
  })

  it('cannot read description from a Typesense hit that only has search_en', () => {
    const doc = {
      id: 'prod-cheddar',
      key: 'PROD-CHEDDAR',
      search_en: "A firm cow's-milk cheddar aged for several months. Synthetic demo recipe.",
    }
    expect(readPathFromDoc(doc, defs, 'description@en')).toBe('')
  })

  it('renders pathValues text for a localized description cell', () => {
    const text =
      "A firm cow's-milk cheddar aged for several months. Synthetic demo recipe."
    const summary = summarizePathValues([text])
    expect(summary.text).toContain("firm cow's-milk cheddar")
  })
})

describe('cell edit commit policy', () => {
  it('skips PATCH when the draft equals the original', () => {
    const original: string = "A firm cow's-milk cheddar"
    const draft: string = "A firm cow's-milk cheddar"
    expect(cellEditShouldPatch(original, draft)).toBe(false)
  })

  it('allows PATCH only when the draft changed (including clear-to-empty)', () => {
    const original: string = "A firm cow's-milk cheddar"
    const draft: string = ''
    expect(cellEditShouldPatch(original, draft)).toBe(true)
  })

  it('treats Esc-restored original as no-op even if draft was briefly emptied', () => {
    const original: string = "A firm cow's-milk cheddar"
    // cancel restores original; commit gate must see equality
    expect(cellEditShouldPatch(original, original)).toBe(false)
  })
})
