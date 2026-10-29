import { describe, expect, it } from 'vitest'
import { columnTitle, makeColumn, readCell } from './columns'
import { MOCK_PRODUCTS } from './mock-catalog'

describe('prototype columns', () => {
  it('builds Articles › Net volume (all, ml) and formats cells', () => {
    const col = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )
    expect(col).not.toBeNull()
    expect(columnTitle(col!)).toBe('Articles › Net volume (all, ml)')
    const oat = MOCK_PRODUCTS[0]!
    expect(readCell(oat, col!)).toBe('1 ml, 250 ml')
  })

  it('formats first and specific article volumes', () => {
    const first = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'first', label: 'First' },
        { kind: 'unit', optionId: 'l', label: 'l' },
      ],
    )!
    expect(readCell(MOCK_PRODUCTS[0]!, first)).toBe('1 l')
    const specific = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'specific', label: 'Specific', value: '250ml' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )!
    expect(readCell(MOCK_PRODUCTS[0]!, specific)).toBe('250 ml')
  })

  it('formats language, allergens, range, and supplier hops', () => {
    const desc = makeColumn(['description'], [{ kind: 'language', optionId: 'sv', label: 'SV' }])!
    expect(readCell(MOCK_PRODUCTS[0]!, desc)).toBe('Krämig havredryck till kaffe') // english-check: allow-line
    const allergen = makeColumn(['allergens'], [{ kind: 'value_list', optionId: 'label', label: 'Label' }])!
    expect(readCell(MOCK_PRODUCTS[0]!, allergen)).toBe('Gluten')
    const temp = makeColumn(['storage_temperature'], [{ kind: 'range', optionId: 'min', label: 'Min' }])!
    expect(readCell(MOCK_PRODUCTS[0]!, temp)).toBe('2')
    const supplier = makeColumn(['supplier', 'country'], [])!
    expect(columnTitle(supplier)).toBe('Supplier › Country')
    expect(readCell(MOCK_PRODUCTS[0]!, supplier)).toBe('Sweden')
  })
})
