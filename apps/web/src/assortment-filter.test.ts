import { describe, expect, it } from 'vitest'
import { assortmentSearchFilter, labelSearchFilter, selectionMatchesType } from './assortment-filter'

describe('organic assortment filter', () => {
  const organic = { labels: 'organic_eu' }

  it('filters on the organic_eu label and does not text-search', () => {
    expect(assortmentSearchFilter(organic)).toBe('a_labels:=organic_eu')
    expect(assortmentSearchFilter({ ...organic, text: 'Organic' })).toBe('a_labels:=organic_eu')
    expect(labelSearchFilter(organic)).toBe('a_labels:=organic_eu')
  })

  it('uses the selection type and ignores a type stored on the query', () => {
    expect(
      assortmentSearchFilter({ type: 'article', group: 'food:cheese', labels: ['organic_eu', 'vegan'] }, 'product'),
    ).toBe('type_key:=product && groups:=food:cheese && a_labels:=organic_eu && a_labels:=vegan')
  })

  it('matches nothing extra when the query has no label', () => {
    expect(labelSearchFilter({ type: 'article', text: 'Organic' })).toBe('')
    expect(assortmentSearchFilter({ type: 'article', group: 'food:cheese' }, 'article')).toBe(
      'type_key:=article && groups:=food:cheese',
    )
  })

  it('matches a selection only to its own type', () => {
    const cheese = { type: 'product', type_id: '018f0000-0000-7000-8000-000000000200' }
    expect(selectionMatchesType(cheese, 'product', cheese.type_id)).toBe(true)
    expect(selectionMatchesType(cheese, 'article', '018f0000-0000-7000-8000-000000000201')).toBe(false)
    expect(selectionMatchesType({ type_id: cheese.type_id }, '', cheese.type_id)).toBe(true)
    expect(selectionMatchesType(cheese, '', '')).toBe(false)
  })
})
