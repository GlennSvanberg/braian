import { afterEach, describe, expect, it, vi } from 'vitest'
import { isWrite, resetWriteStatus, writeSettled, writeStarted, writeStatus } from './write-status'

afterEach(() => {
  resetWriteStatus()
  vi.useRealTimers()
})

describe('write status', () => {
  it('only counts writes', () => {
    expect(isWrite('GET')).toBe(false)
    expect(isWrite('post')).toBe(true)
    expect(isWrite('PUT')).toBe(true)
  })

  it('tracks pending writes and a failure that clears by itself', () => {
    vi.useFakeTimers()
    writeStarted()
    writeStarted()
    expect(writeStatus().pending).toBe(2)
    writeSettled()
    writeSettled('500 Internal Server Error')
    expect(writeStatus()).toEqual({ pending: 0, error: '500 Internal Server Error' })
    vi.advanceTimersByTime(10_000)
    expect(writeStatus().error).toBeNull()
  })

  it('clears an old failure when the next write succeeds', () => {
    writeStarted()
    writeSettled('boom')
    writeStarted()
    writeSettled()
    expect(writeStatus().error).toBeNull()
  })
})
