import { describe, expect, it } from 'vitest'
import { appendPathFilter, objectTableFilter } from './table-filter'

describe('object table filter', () => {
  it('keeps the view type when no group is selected', () => {
    expect(objectTableFilter('product', null)).toBe('type_key:=product')
    expect(objectTableFilter('', null)).toBe('')
  })

  it('keeps the view type when a group is selected', () => {
    expect(objectTableFilter('product', 'food:cheese')).toBe('type_key:=product && groups:=food:cheese')
    expect(objectTableFilter('article', 'food:cheese')).toBe('type_key:=article && groups:=food:cheese')
  })

  it('lists nothing extra when the view has no type', () => {
    expect(objectTableFilter('', 'food:dairy')).toBe('groups:=food:dairy')
  })

  it('appends an attribute path without dropping the type', () => {
    expect(appendPathFilter('type_key:=article', 'net_weight.unit', 'g')).toBe(
      'type_key:=article && net_weight.unit:=g',
    )
    expect(appendPathFilter('', 'product>brand', 'Meadow')).toBe('product>brand:=Meadow')
  })
})
