import { describe, expect, it } from 'vitest'
import { attributeCards, classRows } from './class-view'
import type { Json } from './api'

const defs: Json = {
  attributes: [
    { id: 'a-weight', key: 'weight', labels: { en: 'Weight' }, kind: { type: 'number', unit_family: 'mass' }, required: false },
    { id: 'a-voltage', key: 'voltage', labels: { en: 'Voltage' }, kind: { type: 'number' }, required: true, multiple: false },
  ],
  unit_families: [{ id: 'mass', key: 'mass' }],
  structures: [
    {
      key: 'power_tools',
      classes: [
        { id: 'c-tool', key: 'Power tool', attributes: ['a-weight'] },
        { id: 'c-drill', key: 'Drill', parent: 'c-tool', attributes: ['a-voltage'] },
      ],
    },
  ],
}

describe('class view', () => {
  it('marks an ancestor attribute as inherited once', () => {
    const cards = attributeCards(defs, 'Drill')
    const weight = cards.find((card) => card.key === 'weight')
    const voltage = cards.find((card) => card.key === 'voltage')
    expect(weight).toMatchObject({ inherited: true, from: 'Power tool', unit: 'mass', kind: 'number' })
    expect(voltage).toMatchObject({ inherited: false, required: true })
    expect(cards.filter((card) => card.key === 'weight')).toHaveLength(1)
  })

  it('nests a child under its parent', () => {
    const rows = classRows(defs)
    expect(rows.map((row) => row.key)).toEqual(['Power tool', 'Drill'])
    expect(rows[1]?.parentKey).toBe('Power tool')
    expect(rows[1]?.depth).toBe(1)
  })
})
