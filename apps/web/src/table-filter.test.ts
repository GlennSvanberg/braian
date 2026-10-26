import { describe, expect, it } from 'vitest'
import { objectTableFilter } from './table-filter'

const cheese = { query: { type: 'article', group: 'food:cheese' } }
const juice = { query: { type: 'article', group: 'food:juice' } }

describe('object table filter', () => {
  it('keeps the view type when no group is selected', () => {
    expect(objectTableFilter('product', null, [cheese])).toBe('type_key:=product')
    expect(objectTableFilter('', null, [])).toBe('')
  })

  it('uses the assortment type for a selected group', () => {
    expect(objectTableFilter('product', 'food:cheese', [cheese, juice])).toBe(
      'type_key:=article && groups:=food:cheese',
    )
  })

  it('lists the whole group when no assortment names it', () => {
    expect(objectTableFilter('product', 'food:dairy', [cheese])).toBe('groups:=food:dairy')
  })

  it('joins every assortment type that shares the group', () => {
    const products = { query: { type: 'product', group: 'food:cheese' } }
    expect(objectTableFilter('product', 'food:cheese', [cheese, products])).toBe(
      '(type_key:=article || type_key:=product) && groups:=food:cheese',
    )
  })
})
