import { describe, expect, it } from 'vitest'
import {
  EMPTY_CELL,
  formatCellValue,
  formatColumnTitle,
  isColumnComplete,
  nextQualifierSteps,
  summarizeList,
  cardinalityStepForLink,
  convertVolume,
  convertMeasure,
  cardinalityCrumbLabel,
  qualifierShortLabel,
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
  it('lists ml and l as unit options', () => {
    const step = nextQualifierSteps(volume, [])[0]!
    expect(step.options.map((o) => o.id)).toEqual(['ml', 'l'])
  })

  it('lists g and kg as Net weight unit options', () => {
    const weight: FieldDef = {
      id: 'net_weight',
      name: 'Net weight',
      typeLabel: 'Number',
      shape: 'unit',
      units: ['g', 'kg'],
    }
    const step = nextQualifierSteps(weight, [])[0]!
    expect(step.options.map((o) => o.id)).toEqual(['g', 'kg'])
  })

  it('formats specific titles from label (size), not raw GTIN value', () => {
    expect(
      qualifierShortLabel({
        kind: 'cardinality',
        optionId: 'specific',
        label: '1.5L',
        value: '7300002000022',
      }),
    ).toBe('1.5L')
    expect(
      qualifierShortLabel({
        kind: 'cardinality',
        optionId: 'specific',
        label: '500ml · 7300007000071',
        value: '7300007000071',
      }),
    ).toBe('500ml · 7300007000071')
    expect(
      formatColumnTitle(
        ['Articles', 'Net volume'],
        [
          { kind: 'cardinality', optionId: 'specific', label: '1.5L', value: '7300002000022' },
          { kind: 'unit', optionId: 'ml', label: 'ml' },
        ],
      ),
    ).toBe('Articles › Net volume (1.5L, ml)')
  })

  it('requires language / unit / value_list / range steps', () => {
    expect(nextQualifierSteps(description, []).map((s) => s.kind)).toEqual(['language'])
    expect(nextQualifierSteps(volume, []).map((s) => s.kind)).toEqual(['unit'])
    expect(nextQualifierSteps(allergens, []).map((s) => s.kind)).toEqual(['value_list'])
    expect(nextQualifierSteps(storage, []).map((s) => s.kind)).toEqual(['range'])
  })

  it('requires cardinality for many-links only', () => {
    expect(cardinalityStepForLink(articlesLink.link!)?.kind).toBe('cardinality')
    expect(cardinalityStepForLink(articlesLink.link!)?.options.find((o) => o.id === 'specific')?.drill).toBeFalsy()
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

  it('summarizes lists as A, B +N with full title for tooltips', () => {
    expect(summarizeList(['A', 'B']).text).toBe('A, B')
    const long = summarizeList(['A', 'B', 'C', 'D'])
    expect(long.text).toBe('A, B +2')
    expect(long.title).toBe('A, B, C, D')
  })

  it('converts litres to millilitres and kilograms to grams', () => {
    expect(convertVolume(1, 'l', 'ml')).toBe(1000)
    expect(convertVolume(1.5, 'l', 'ml')).toBe(1500)
    expect(convertVolume(1000, 'ml', 'l')).toBe(1)
    expect(convertMeasure(1, 'kg', 'g')).toBe(1000)
    expect(convertMeasure(450, 'g', 'kg')).toBe(0.45)
    expect(convertMeasure(500, 'g', 'kg')).toBe(0.5)
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
      formatCellValue({ value: { min: 2, max: 8 } }, storage, [
        { kind: 'range', optionId: 'max', label: 'Max' },
      ]),
    ).toBe('8 °C')
  })

  it('exports empty cell marker', () => {
    expect(EMPTY_CELL).toBe('—')
  })

  it('isColumnComplete tracks cardinality and leaf qualifiers', () => {
    expect(isColumnComplete(volume, [], true)).toBe(false)
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
