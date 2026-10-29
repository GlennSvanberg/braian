import { describe, expect, it } from 'vitest'
import {
  articleOptionsForField,
  columnTitle,
  columnWidthPx,
  makeColumn,
  readCell,
  readCellDetail,
  withReplacedQualifier,
} from './columns'
import { EMPTY_CELL } from './qualifiers'
import { MOCK_PRODUCTS } from './mock-catalog'

describe('prototype columns', () => {
  it('builds Articles › Net volume (all, ml) and converts litres', () => {
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
    expect(readCell(oat, col!)).toBe('1000 ml, 250 ml')
  })

  it('exposes full list title for +N tooltips', () => {
    const col = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )!
    const milk = MOCK_PRODUCTS[1]! // 1L, 1.5L, 250ml → three values
    const detail = readCellDetail(milk, col)
    expect(detail.text).toBe('1000 ml, 1500 ml +1')
    expect(detail.title).toBe('1000 ml, 1500 ml, 250 ml')
  })

  it('shows an em dash for empty cells', () => {
    const specific = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'specific', label: 'Missing', value: 'no-such' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )!
    expect(readCell(MOCK_PRODUCTS[0]!, specific)).toBe(EMPTY_CELL)
  })

  it('formats specific titles as (1.5L, ml)', () => {
    const col = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'specific', label: '1.5L', value: '1.5L' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )!
    expect(columnTitle(col)).toBe('Articles › Net volume (1.5L, ml)')
  })

  it('lists Specific articles as GTIN · key, one product subtitle, sorted by product then GTIN', () => {
    const options = articleOptionsForField('net_volume')
    const oneFive = options.find((item) => item.pickValue === '1.5L')
    expect(oneFive).toBeTruthy()
    expect(oneFive!.id).toMatch(/^7300/)
    expect(oneFive!.label).toMatch(/^7300.* · 1\.5L$/)
    expect(oneFive!.subtitle).toBe('Whole milk')
    expect(oneFive!.subtitle).not.toContain(',')
    // Sorted by product name, then GTIN — no merged multi-product subtitles
    for (let i = 1; i < options.length; i += 1) {
      const prev = options[i - 1]!
      const cur = options[i]!
      const byProduct = prev.subtitle.localeCompare(cur.subtitle, 'en')
      expect(byProduct <= 0).toBe(true)
      if (byProduct === 0) expect(prev.id.localeCompare(cur.id) <= 0).toBe(true)
    }
    const weightOpts = articleOptionsForField('net_weight')
    expect(weightOpts.every((item) => !item.subtitle.includes(','))).toBe(true)
    expect(weightOpts.find((item) => item.subtitle === 'Greek yogurt')).toBeTruthy()
  })

  it('uses Net weight (g/kg) for solids and — when volume is absent', () => {
    const volume = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )!
    const weight = makeColumn(
      ['articles', 'net_weight'],
      [
        { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
        { kind: 'unit', optionId: 'g', label: 'g' },
      ],
    )!
    const yogurt = MOCK_PRODUCTS.find((row) => row.sku === 'YGT-003')!
    const cheddar = MOCK_PRODUCTS.find((row) => row.sku === 'CHSE-008')!
    const pasta = MOCK_PRODUCTS.find((row) => row.sku === 'PSTA-006')!
    const bread = MOCK_PRODUCTS.find((row) => row.sku === 'BRD-005')!
    const oat = MOCK_PRODUCTS[0]!
    expect(readCell(yogurt, volume)).toBe(EMPTY_CELL)
    expect(readCell(cheddar, volume)).toBe(EMPTY_CELL)
    expect(readCell(pasta, volume)).toBe(EMPTY_CELL)
    expect(readCell(bread, volume)).toBe(EMPTY_CELL)
    expect(readCell(yogurt, weight)).toBe('450 g')
    expect(readCell(cheddar, weight)).toBe('200 g, 400 g')
    expect(readCell(pasta, weight)).toBe('500 g, 1000 g')
    expect(readCell(bread, weight)).toBe('800 g')
    expect(readCell(oat, weight)).toBe(EMPTY_CELL)
    const weightKg = makeColumn(
      ['articles', 'net_weight'],
      [
        { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
        { kind: 'unit', optionId: 'kg', label: 'kg' },
      ],
    )!
    expect(readCell(pasta, weightKg)).toBe('0.5 kg, 1 kg')
  })

  it('keeps Allergens None as a real label and uses — only when missing', () => {
    const allergen = makeColumn(['allergens'], [{ kind: 'value_list', optionId: 'label', label: 'Label' }])!
    const juice = MOCK_PRODUCTS.find((row) => row.sku === 'JCE-004')!
    expect(readCell(juice, allergen)).toBe('None')
    const specific = makeColumn(
      ['articles', 'net_weight'],
      [
        { kind: 'cardinality', optionId: 'specific', label: 'nope', value: 'no-such' },
        { kind: 'unit', optionId: 'g', label: 'g' },
      ],
    )!
    expect(readCell(juice, specific)).toBe(EMPTY_CELL)
  })

  it('gives long headers a roomy width', () => {
    const col = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )!
    expect(columnWidthPx(col)).toBeGreaterThanOrEqual(200)
  })

  it('formats language, allergens, range °C, and supplier hops', () => {
    const desc = makeColumn(['description'], [{ kind: 'language', optionId: 'sv', label: 'SV' }])!
    expect(readCell(MOCK_PRODUCTS[0]!, desc)).toBe('Krämig havredryck till kaffe') // english-check: allow-line
    const allergen = makeColumn(['allergens'], [{ kind: 'value_list', optionId: 'label', label: 'Label' }])!
    expect(readCell(MOCK_PRODUCTS[0]!, allergen)).toBe('Gluten')
    const temp = makeColumn(['storage_temperature'], [{ kind: 'range', optionId: 'min', label: 'Min' }])!
    expect(readCell(MOCK_PRODUCTS[0]!, temp)).toBe('2 °C')
    const supplier = makeColumn(['supplier', 'country'], [])!
    expect(columnTitle(supplier)).toBe('Supplier › Country')
    expect(readCell(MOCK_PRODUCTS[0]!, supplier)).toBe('Sweden')
  })

  it('replaces cardinality in place and keeps unit', () => {
    const all = makeColumn(
      ['articles', 'net_volume'],
      [
        { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ],
    )!
    const first = withReplacedQualifier(all, { kind: 'cardinality', optionId: 'first', label: 'First' })
    expect(columnTitle(first)).toBe('Articles › Net volume (first, ml)')
    expect(readCell(MOCK_PRODUCTS[0]!, first)).toBe('1000 ml')
  })
})
