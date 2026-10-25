import { describe, expect, it } from 'vitest'
import { batchGetBody } from './batch'

describe('batch get body', () => {
  it('defaults to groups and no values', () => {
    expect(batchGetBody(['a', 'b'])).toEqual({ ids: ['a', 'b'], groups: true })
  })

  it('asks only for the named fields and languages', () => {
    expect(batchGetBody(['a'], { fields: ['namn'], languages: ['sv'], groups: true })).toEqual({
      ids: ['a'],
      groups: true,
      fields: ['namn'],
      languages: ['sv'],
    })
  })

  it('sends values only when the caller wants every part', () => {
    expect(batchGetBody(['a'], { values: true, groups: false })).toEqual({
      ids: ['a'],
      groups: false,
      values: true,
    })
  })
})
