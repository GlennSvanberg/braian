import { describe, expect, it } from 'vitest'
import { shouldHoldTablePaint } from './table-live-paint'

describe('shouldHoldTablePaint', () => {
  it('does not hold before the first paint', () => {
    expect(
      shouldHoldTablePaint({ hasPaintedData: false, objectsFetching: true, pathBusy: true }),
    ).toBe(false)
  })

  it('holds while either side of a live refresh is in flight', () => {
    expect(
      shouldHoldTablePaint({ hasPaintedData: true, objectsFetching: true, pathBusy: false }),
    ).toBe(true)
    expect(
      shouldHoldTablePaint({ hasPaintedData: true, objectsFetching: false, pathBusy: true }),
    ).toBe(true)
  })

  it('releases when both sides have settled', () => {
    expect(
      shouldHoldTablePaint({ hasPaintedData: true, objectsFetching: false, pathBusy: false }),
    ).toBe(false)
  })
})
