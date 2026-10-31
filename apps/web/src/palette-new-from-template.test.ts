import { describe, expect, it } from 'vitest'
import { copy } from './i18n'
import { filterPaletteRows, visibleRows } from './palette-model'
import {
  isNewFromTemplateRoot,
  newFromTemplateRootItem,
  newFromTemplateSubmenuRows,
} from './palette-new-from-template'
import type { TypeName } from './type-labels'

const types: TypeName[] = [
  { id: 't-product', key: 'product', kind: 'object', singular: 'Product', plural: 'Products' },
  { id: 't-article', key: 'article', kind: 'object', singular: 'Article', plural: 'Articles' },
]

describe('new from template palette helpers', () => {
  it('shows New from template › when typing template', () => {
    const root = newFromTemplateRootItem()
    const rows = visibleRows('template', [root], [], {})
    expect(rows.some((row) => (row.sourceId ?? row.id) === 'workspace:new')).toBe(true)
    expect(root.title).toBe(`${copy.newWorkspace} ›`)
  })

  it('lists only templates under the submenu', () => {
    const rows = newFromTemplateSubmenuRows(types)
    expect(rows.every((row) => row.id.startsWith('template:'))).toBe(true)
    expect(rows.map((row) => row.title)).toEqual(
      expect.arrayContaining(['Products', 'Articles', 'Data model', 'Review']),
    )
    expect(rows.some((row) => /Recent|On selected/i.test(row.title))).toBe(false)
  })

  it('recognises the root id', () => {
    expect(isNewFromTemplateRoot('workspace:new')).toBe(true)
    expect(isNewFromTemplateRoot('template:model')).toBe(false)
  })

  it('filters submenu rows so Enter can pick Review by query', () => {
    const rows = newFromTemplateSubmenuRows(types)
    expect(filterPaletteRows(rows, 'Review').map((row) => row.title)).toEqual(['Review'])
  })
})
