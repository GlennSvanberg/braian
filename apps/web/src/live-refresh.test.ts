import { afterEach, describe, expect, it, vi } from 'vitest'
import { BULK_MAX_MS, BULK_QUIET_MS, INTERACTIVE_LIST_MS, SearchRefresh, classifyLive } from './live-refresh'

afterEach(() => {
  vi.useRealTimers()
})

describe('live search refresh', () => {
  it('classifies interactive edits and bulk summaries', () => {
    expect(classifyLive(JSON.stringify({ op: 'update', object_id: '1' }))).toBe('interactive')
    expect(classifyLive(JSON.stringify({ op: 'bulk_summary', count: 200 }))).toBe('bulk')
    expect(classifyLive(JSON.stringify({ op: 'job', status: 'running' }))).toBe('ignore')
  })

  it('refreshes one object immediately and the list once after a burst of edits', () => {
    vi.useFakeTimers()
    const lists: number[] = []
    const objects: string[] = []
    const refresh = new SearchRefresh(
      () => lists.push(1),
      (id) => objects.push(id),
    )
    refresh.onEvent(JSON.stringify({ op: 'update', object_id: 'a' }))
    refresh.onEvent(JSON.stringify({ op: 'update', object_id: 'b' }))
    expect(objects).toEqual(['a', 'b'])
    expect(lists).toEqual([])
    vi.advanceTimersByTime(INTERACTIVE_LIST_MS - 1)
    expect(lists).toEqual([])
    vi.advanceTimersByTime(1)
    expect(lists).toEqual([1])
    refresh.dispose()
  })

  it('does not search on every bulk summary', () => {
    vi.useFakeTimers()
    const lists: number[] = []
    const objects: string[] = []
    const refresh = new SearchRefresh(
      () => lists.push(1),
      (id) => objects.push(id),
    )
    const start = Date.now()
    for (let step = 0; step < 10; step += 1) {
      vi.setSystemTime(start + step * 200)
      refresh.onEvent(JSON.stringify({ op: 'bulk_summary', count: 200 }), start + step * 200)
    }
    expect(objects).toEqual([])
    expect(lists).toEqual([])
    vi.advanceTimersByTime(BULK_QUIET_MS)
    expect(lists).toEqual([1])
    refresh.dispose()
  })

  it('refreshes a long bulk at the cap instead of waiting for quiet', () => {
    vi.useFakeTimers()
    const lists: number[] = []
    const refresh = new SearchRefresh(
      () => lists.push(1),
      () => {},
    )
    const start = 1_000
    refresh.onEvent(JSON.stringify({ op: 'bulk' }), start)
    refresh.onEvent(JSON.stringify({ op: 'bulk_summary', count: 1 }), start + BULK_MAX_MS)
    vi.advanceTimersByTime(0)
    expect(lists).toEqual([1])
    refresh.dispose()
  })
})
