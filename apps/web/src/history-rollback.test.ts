import { describe, expect, it } from 'vitest'
import { rollbackTargetSeq } from './history-rollback'

describe('rollbackTargetSeq', () => {
  it('undoes the selected change by targeting the previous seq', () => {
    expect(rollbackTargetSeq([1, 2, 5], 5)).toBe(2)
    expect(rollbackTargetSeq([1, 2, 5], 2)).toBe(1)
  })

  it('keeps the selected seq when it is the first change (create)', () => {
    expect(rollbackTargetSeq([1, 2, 5], 1)).toBe(1)
    expect(rollbackTargetSeq([3], 3)).toBe(3)
  })

  it('ignores duplicates and unsorted input', () => {
    expect(rollbackTargetSeq([5, 1, 2, 2], 5)).toBe(2)
  })
})
