import { describe, expect, it } from 'vitest'
import type { Json } from './api'
import { visibleRows } from './palette-model'
import {
  affectedLabel,
  commandPlaceholder,
  defaultPlural,
  displayViewTitle,
  emptyDetail,
  migrateSavedWorkspace,
  pluralOf,
  singularOf,
  typeNames,
  typeViewPaletteItems,
} from './type-labels'

const defs: Json = {
  types: [
    {
      id: '200',
      key: 'produkt',
      labels: { en: 'Product', sv: 'Produkt' },
      plural: { sv: 'Produkter' },
    },
    {
      id: '201',
      key: 'artikel',
      labels: { en: 'Article' },
      singular: { en: 'Article' },
      plural: { en: 'Articles' },
    },
  ],
}

describe('type labels', () => {
  it('derives a plural and falls back to the singular label', () => {
    expect(defaultPlural('Product')).toBe('Products')
    expect(defaultPlural('Article')).toBe('Articles')
    expect(defaultPlural('Category')).toBe('Categories')
    expect(defaultPlural('Bus')).toBe('Buses')
    expect(defaultPlural('')).toBe('Objects')
    const names = typeNames(defs, 'en')
    expect(names[0]).toMatchObject({ key: 'produkt', singular: 'Product', plural: 'Products' })
    expect(singularOf(defs, 'produkt', 'sv')).toBe('Produkt')
    expect(pluralOf(defs, 'produkt', 'sv')).toBe('Produkter')
    expect(pluralOf(defs, 'artikel', 'en')).toBe('Articles')
  })

  it('offers one Open view entry per type and finds product', () => {
    const items = typeViewPaletteItems(typeNames(defs, 'en'))
    expect(items.map((item) => item.title)).toEqual(['Products view', 'Articles view'])
    expect(items.every((item) => item.subtitle === 'Open view')).toBe(true)
    const product = visibleRows('product', items, [], {})
    expect(product.some((row) => row.title === 'Products view' && row.subtitle === 'Open view')).toBe(true)
    const key = visibleRows('produkt', items, [], {})
    expect(key.some((row) => row.title === 'Products view')).toBe(true)
    expect(visibleRows('article', items, [], {}).some((row) => row.title === 'Articles view')).toBe(true)
  })

  it('migrates a generic Objects workspace onto the primary type', () => {
    const types = typeNames(defs, 'en')
    const first = migrateSavedWorkspace(
      { name: 'Enrich objects' },
      [
        { id: 'table', type: 'object_table', title: 'Objects', state: { columns: ['nummer'] } },
        { id: 'suppliers', type: 'object_table', title: 'Suppliers', state: {} },
        { id: 'detail', type: 'object_detail', title: 'Detail', state: {} },
      ],
      types,
    )
    expect(first.changed).toBe(true)
    expect(first.name).toBeUndefined()
    expect(first.views).toEqual([
      {
        id: 'table',
        title: 'Products',
        state: { columns: ['nummer'], object_type: '200', type_key: 'produkt' },
      },
    ])
    const again = migrateSavedWorkspace(
      { name: 'Enrich' },
      [{ id: 'table', type: 'object_table', title: 'Products', state: first.views[0]?.state }],
      types,
    )
    expect(again.changed).toBe(false)
    expect(again.views).toEqual([])

    const legacy = migrateSavedWorkspace(
      { name: 'Berika objekt' },
      [{ id: 'table', type: 'object_table', title: 'Objekt', state: {} }],
      types,
    )
    expect(legacy.name).toBeUndefined()
    expect(legacy.views[0]?.title).toBe('Products')

    const renamed = migrateSavedWorkspace(
      { name: 'Catalog' },
      [{ id: 'table', type: 'object_table', title: 'Objects', state: {} }],
      types,
    )
    expect(renamed.name).toBeUndefined()
    expect(renamed.views[0]?.state).toMatchObject({ type_key: 'produkt' })
    expect(migrateSavedWorkspace({ name: 'Enrich objects' }, [], types).name).toBeUndefined()
  })

  it('names list and detail views from the open type', () => {
    const types = typeNames(defs, 'en')
    expect(displayViewTitle({ type: 'object_table', title: 'Objects', state: { type_key: 'artikel' } }, types)).toBe('Articles')
    expect(displayViewTitle({ type: 'object_table', title: 'Objects', state: {} }, types)).toBe('Products')
    expect(displayViewTitle({ type: 'object_detail', title: 'Detail', state: {} }, types, 'artikel')).toBe('Article')
    expect(displayViewTitle({ type: 'object_detail', title: 'Detail', state: {} }, types)).toBe('Detail')
    expect(displayViewTitle({ type: 'object_detail', title: 'Detail', state: {} }, [types[0]!])).toBe('Product')
    expect(emptyDetail(types, 'produkt')).toBe('Select a product to edit its values.')
    expect(emptyDetail(types)).toBe('Select a row to edit its values.')
    expect(commandPlaceholder([types[0]!])).toBe('Search products, open views, or ask AI')
    expect(commandPlaceholder(types)).toBe('Search, open views, or ask AI')
    expect(affectedLabel(['artikel', 'artikel'], types)).toBe('Affected Articles')
    expect(affectedLabel(['artikel', 'produkt'], types)).toBe('Affected objects')
  })
})
