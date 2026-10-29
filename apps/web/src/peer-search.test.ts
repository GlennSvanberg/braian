import { describe, expect, it } from 'vitest'
import { gtinExactFilter, gtinSearchVariants } from './peer-search'

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

describe('gtinExactFilter', () => {
  it('uses same-field array OR with backticks (no parentheses)', () => {
    const filter = gtinExactFilter(['07310000000024', '7310000000024'])
    expect(filter).toBe('type_key:=article && gtin:=[`07310000000024`,`7310000000024`]')
    expect(filter).not.toContain('||')
    expect(filter).not.toContain('(')
  })
})
