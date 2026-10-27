import { describe, expect, it } from 'vitest'
import { newObjectBody } from './new-object'

describe('new object', () => {
  it('does not place a new object in Bread', () => {
    const body = newObjectBody({
      typeKey: 'product',
      number: 'P-1',
      name: 'Rye loaf',
      group: null,
      structures: ['food'],
    })
    expect(body.groups).toEqual({})
    expect(body.values).toEqual({ number: 'P-1', name: { en: 'Rye loaf' } })
    expect(JSON.stringify(body)).not.toContain('bread')

    const selected = newObjectBody({
      typeKey: 'product',
      number: 'P-2',
      name: 'Cheese',
      group: 'food:bread',
      structures: ['food'],
    })
    expect(selected.groups).toEqual({ food: 'bread' })

    const unplaced = newObjectBody({
      typeKey: 'product',
      number: 'P-3',
      name: 'Loose',
      group: 'food:unplaced',
      structures: ['food'],
    })
    expect(unplaced.groups).toEqual({})

    const supplier = newObjectBody({
      typeKey: 'supplier',
      number: 'S-1',
      name: 'Northmill',
      group: 'food:dairy',
      structures: [],
    })
    expect(supplier.groups).toEqual({})
  })
})
