import { describe, expect, it } from 'vitest'
import { attributeConflict, attributeRows, effectiveFields } from './model-view'
import type { Json } from './api'

const defs: Json = {
  attributes: [
    { id: 'name', key: 'name', labels: { en: 'Name' }, kind: { type: 'text' }, index: {} },
    { id: 'weight', key: 'weight', labels: { en: 'Weight' }, kind: { type: 'number', unit_family: 'mass' }, index: { searchable: false, filterable: true, sortable: false } },
    { id: 'length', key: 'length', labels: { en: 'Length' }, kind: { type: 'number', unit_family: 'length' }, index: {} },
    { id: 'badge', key: 'badge', labels: { en: 'Badge' }, kind: { type: 'text' }, index: {} },
  ],
  unit_families: [
    { id: 'mass', key: 'mass' },
    { id: 'length', key: 'length' },
  ],
  types: [{ key: 'product', labels: { en: 'Product' }, key_attribute: 'name', attributes: ['name', 'weight'] }],
  structures: [
    {
      key: 'assortment',
      groups: [
        { id: 'outdoor', key: 'outdoor', attributes: ['weight'] },
        { id: 'tents', key: 'tents', parent: 'outdoor', attributes: ['length'] },
      ],
      classes: [],
    },
    {
      key: 'campaigns',
      groups: [{ id: 'summer', key: 'summer', attributes: ['weight', 'badge'] }],
      classes: [],
    },
  ],
}

describe('effective attributes', () => {
  it('keeps one row when a type and two structures contribute the same attribute', () => {
    const fields = effectiveFields(defs, 'product', [
      { structure: 'assortment', group: 'tents' },
      { structure: 'campaigns', group: 'summer' },
    ])
    const weight = fields.find((field) => field.key === 'weight')
    expect(fields.map((field) => field.key)).toEqual(['name', 'weight', 'length', 'badge'])
    expect(weight?.sources).toEqual([
      { kind: 'type', key: 'product' },
      { kind: 'group', structure: 'assortment', group: 'outdoor' },
      { kind: 'group', structure: 'campaigns', group: 'summer' },
    ])
    expect(fields.find((field) => field.key === 'length')?.sources).toEqual([
      { kind: 'group', structure: 'assortment', group: 'tents' },
    ])
    expect(effectiveFields(defs, 'product', [])).toHaveLength(2)
  })

  it('shows an ancestor group attribute as inherited, once', () => {
    const rows = attributeRows(defs, { kind: 'group', structure: 'assortment', group: 'tents' })
    expect(rows.find((row) => row.key === 'length')).toMatchObject({ inherited: false })
    expect(rows.find((row) => row.key === 'weight')).toMatchObject({ inherited: true, fromKey: 'outdoor', unit: 'mass' })
    expect(rows.filter((row) => row.key === 'weight')).toHaveLength(1)
  })

  it('projects a class attribute onto its group and inherits it downward', () => {
    const withClasses: Json = {
      attributes: defs.attributes,
      unit_families: defs.unit_families,
      types: defs.types,
      structures: [
        {
          key: 'assortment',
          groups: [
            { id: 'outdoor', key: 'outdoor' },
            { id: 'tents', key: 'tents', parent: 'outdoor' },
          ],
          classes: [{ group: 'outdoor', attributes: ['weight'] }],
        },
      ],
    }
    const rows = attributeRows(withClasses, { kind: 'group', structure: 'assortment', group: 'tents' })
    expect(rows.find((row) => row.key === 'weight')).toMatchObject({ inherited: true, fromKey: 'outdoor' })
  })

  it('blocks a second definition with a different type or unit', () => {
    expect(attributeConflict(defs, 'weight', 'text', '')).toBe(true)
    expect(attributeConflict(defs, 'weight', 'number', 'length')).toBe(true)
    expect(attributeConflict(defs, 'weight', 'number', 'mass')).toBe(false)
    expect(attributeConflict(defs, 'span', 'number', 'length')).toBe(false)
  })
})
