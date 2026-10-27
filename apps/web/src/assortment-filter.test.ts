import { describe, expect, it } from 'vitest'
import { assortmentSearchFilter, labelSearchFilter } from './assortment-filter'

describe('organic assortment filter', () => {
  const organic = { labels: 'organic_eu' }

  it('filters on the organic_eu label and does not text-search', () => {
    expect(assortmentSearchFilter(organic)).toBe('a_labels:=organic_eu')
    expect(assortmentSearchFilter({ ...organic, text: 'Organic' })).toBe('a_labels:=organic_eu')
    expect(labelSearchFilter(organic)).toBe('a_labels:=organic_eu')
  })

  it('keeps type and group, then requires every listed label', () => {
    expect(
      assortmentSearchFilter({ type: 'article', group: 'food:cheese', labels: ['organic_eu', 'vegan'] }),
    ).toBe('type_key:=article && groups:=food:cheese && a_labels:=organic_eu && a_labels:=vegan')
  })

  it('matches nothing extra when the query has no label', () => {
    expect(labelSearchFilter({ type: 'article', text: 'Organic' })).toBe('')
    expect(assortmentSearchFilter({ type: 'article', group: 'food:cheese' })).toBe(
      'type_key:=article && groups:=food:cheese',
    )
  })
})
