import { describe, expect, it } from 'vitest'
import {
  formatCellValue,
  formatColumnTitle,
  isColumnComplete,
  nextQualifierSteps,
  summarizeList,
  cardinalityStepForLink,
  convertVolume,
  cardinalityCrumbLabel,
  type AppliedQualifier,
  type FieldDef,
} from './qualifiers'

const volume: FieldDef = {
  id: 'net_volume',
  name: 'Net volume',
  typeLabel: 'Number',
  shape: 'unit',
  units: ['ml', 'l'],
}

const description: FieldDef = {
  id: 'description',
  name: 'Description',
  typeLabel: 'Text',
  shape: 'language',
  languages: ['en', 'sv'],
}

const allergens: FieldDef = {
  id: 'allergens',
  name: 'Allergens',
  typeLabel: 'Value list',
  shape: 'value_list',
  valueListModes: ['label', 'code'],
}

const storage: FieldDef = {
  id: 'storage_temperature',
  name: 'Storage temperature',
  typeLabel: 'Range',
  shape: 'range',
  rangeParts: ['min', 'max'],
  rangeUnit: '°C',
}

const articlesLink: FieldDef = {
  id: 'articles',
  name: 'Articles',
  typeLabel: 'Link',
  shape: 'link',
  link: { many: true, peerFields: [volume], peerLabel: 'Articles' },
}

describe('qualifier module', () => {
  it('requires language / unit / value_list / range steps', () => {
    expect(nextQualifierSteps(description, []).map((s) => s.kind)).toEqual(['language'])
    expect(nextQualifierSteps(volume, []).map((s) => s.kind)).toEqual(['unit'])
    expect(nextQualifierSteps(allergens, []).map((s) => s.kind)).toEqual(['value_list'])
    expect(nextQualifierSteps(storage, []).map((s) => s.kind)).toEqual(['range'])
  })

  it('requires cardinality for many-links only', () => {
    expect(cardinalityStepForLink(articlesLink.link!)?.kind).toBe('cardinality')
    expect(
      cardinalityStepForLink({ many: false, peerFields: [], peerLabel: 'Supplier' }),
    ).toBeNull()
  })

  it('formats column titles with path and qualifiers', () => {
    const qs: AppliedQualifier[] = [
      { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
      { kind: 'unit', optionId: 'ml', label: 'ml' },
    ]
    expect(formatColumnTitle(['Articles', 'Net volume'], qs)).toBe('Articles › Net volume (all, ml)')
  })

  it('summarizes lists as A, B +N', () => {
    expect(summarizeList(['A', 'B']).text).toBe('A, B')
    expect(summarizeList(['A', 'B', 'C', 'D']).text).toBe('A, B +2')
  })

  it('converts litres to millilitres', () => {
    expect(convertVolume(1, 'l', 'ml')).toBe(1000)
    expect(convertVolume(1.5, 'l', 'ml')).toBe(1500)
    expect(convertVolume(1000, 'ml', 'l')).toBe(1)
  })

  it('formats cells for language, unit conversion, value list, range °C, and cardinality', () => {
    expect(
      formatCellValue({ value: { en: 'Hello', sv: 'Hej' } }, description, [
        { kind: 'language', optionId: 'sv', label: 'SV' },
      ]),
    ).toBe('Hej')
    expect(
      formatCellValue({ value: { value: 1, unit: 'l' } }, volume, [
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ]),
    ).toBe('1000 ml')
    expect(
      formatCellValue({ value: { value: 250, unit: 'ml' } }, volume, [
        { kind: 'unit', optionId: 'ml', label: 'ml' },
      ]),
    ).toBe('250 ml')
    expect(
      formatCellValue({ value: { value: 1, unit: 'l' } }, volume, [
        { kind: 'unit', optionId: 'l', label: 'l' },
      ]),
    ).toBe('1 l')
    expect(
      formatCellValue({ value: { code: 'MILK', label: 'Milk' } }, allergens, [
        { kind: 'value_list', optionId: 'code', label: 'Code' },
      ]),
    ).toBe('MILK')
    expect(
      formatCellValue({ value: { min: 2, max: 8 } }, storage, [
        { kind: 'range', optionId: 'max', label: 'Max' },
      ]),
    ).toBe('8 °C')
    expect(
      formatCellValue({ value: { min: 2, max: 8 } }, storage, [
        { kind: 'range', optionId: 'min', label: 'Min' },
      ]),
    ).toBe('2 °C')

    const peers = [
      { key: '1L', value: 1, unit: 'l' },
      { key: '250ml', value: 250, unit: 'ml' },
      { key: '330ml', value: 330, unit: 'ml' },
    ]
    expect(
      formatCellValue(
        { peers: peers.map((p) => ({ key: p.key, value: p.value, unit: p.unit })) },
        volume,
        [
          { kind: 'cardinality', optionId: 'all', label: 'All, as list' },
          { kind: 'unit', optionId: 'ml', label: 'ml' },
        ],
      ),
    ).toBe('1000 ml, 250 ml +1')
    expect(
      formatCellValue(
        { peers: peers.map((p) => ({ key: p.key, value: p.value, unit: p.unit })) },
        volume,
        [
          { kind: 'cardinality', optionId: 'first', label: 'First' },
          { kind: 'unit', optionId: 'l', label: 'l' },
        ],
      ),
    ).toBe('1 l')
  })

  it('isColumnComplete tracks cardinality and leaf qualifiers', () => {
    expect(isColumnComplete(volume, [], true)).toBe(false)
    expect(
      isColumnComplete(volume, [{ kind: 'cardinality', optionId: 'all', label: 'All' }], true),
    ).toBe(false)
    expect(
      isColumnComplete(
        volume,
        [
          { kind: 'cardinality', optionId: 'all', label: 'All' },
          { kind: 'unit', optionId: 'ml', label: 'ml' },
        ],
        true,
      ),
    ).toBe(true)
  })

  it('cardinality crumb labels', () => {
    expect(cardinalityCrumbLabel('all')).toBe('All')
    expect(cardinalityCrumbLabel('first')).toBe('First')
  })
})
