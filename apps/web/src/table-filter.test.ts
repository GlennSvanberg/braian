import { describe, expect, it } from 'vitest'
import { objectTableFilter } from './table-filter'

const tents = { query: { type: 'artikel', group: 'sortiment:talt' } }
const timber = { query: { type: 'artikel', group: 'sortiment:virke' } }

describe('object table filter', () => {
  it('keeps the view type when no group is selected', () => {
    expect(objectTableFilter('produkt', null, [tents])).toBe('type_key:=produkt')
    expect(objectTableFilter('', null, [])).toBe('')
  })

  it('uses the assortment type for a selected group', () => {
    expect(objectTableFilter('produkt', 'sortiment:talt', [tents, timber])).toBe(
      'type_key:=artikel && groups:=sortiment:talt',
    )
  })

  it('lists the whole group when no assortment names it', () => {
    expect(objectTableFilter('produkt', 'sortiment:friluft', [tents])).toBe('groups:=sortiment:friluft')
  })

  it('joins every assortment type that shares the group', () => {
    const variants = { query: { type: 'variant', group: 'sortiment:talt' } }
    expect(objectTableFilter('produkt', 'sortiment:talt', [tents, variants])).toBe(
      '(type_key:=artikel || type_key:=variant) && groups:=sortiment:talt',
    )
  })
})
