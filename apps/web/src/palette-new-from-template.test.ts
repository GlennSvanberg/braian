import { describe, expect, it } from 'vitest'
import { copy } from './i18n'
import { filterPaletteRows, visibleRows } from './palette-model'
import {
  isNewFromTemplateRoot,
  newFromTemplateRootItem,
  newFromTemplateSubmenuRows,
  templatePaletteItem,
} from './palette-new-from-template'
import { workspaceTemplates } from './templates'
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

  it('marks Create vs Open existing from share_token presence', () => {
    const review = workspaceTemplates(types).find((item) => item.id === 'review')
    expect(review).toBeTruthy()
    expect(templatePaletteItem(review!).subtitle).toMatch(/^Create · /)
    expect(
      templatePaletteItem(review!, [{ id: 'w1', name: 'Review', share_token: 'braian-review' }]).subtitle,
    ).toMatch(/^Open existing · /)
    const rows = newFromTemplateSubmenuRows(types, [
      { id: 'w1', name: 'Review', share_token: 'braian-review' },
    ])
    const reviewRow = rows.find((row) => row.id === 'template:review')
    expect(reviewRow?.subtitle).toMatch(/^Open existing · /)
    const modelRow = rows.find((row) => row.id === 'template:model')
    expect(modelRow?.subtitle).toMatch(/^Create · /)
  })
})
