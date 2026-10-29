import { describe, expect, it } from 'vitest'
import { gtinSearchVariants } from './peer-search'

describe('gtinSearchVariants', () => {
  it('matches with and without a leading zero', () => {
    expect(gtinSearchVariants('07310000000024')).toEqual(
      expect.arrayContaining(['07310000000024', '7310000000024']),
    )
    expect(gtinSearchVariants('7310000000024')).toEqual(
      expect.arrayContaining(['7310000000024', '07310000000024']),
    )
  })

  it('ignores non-GTIN text', () => {
    expect(gtinSearchVariants('cheddar')).toEqual([])
    expect(gtinSearchVariants('ART-CHED')).toEqual([])
  })
})
