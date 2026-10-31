import { describe, expect, it } from 'vitest'
import { isConnected, shortError, statusMessage, type StatusInput } from './status-message'

const text = { reconnecting: 'Offline / reconnecting…', saving: 'Saving…', saveFailed: 'Save failed', locked: 'Locked' }
const normal: StatusInput = { saving: false, online: true, live: 'open', saveError: null, locked: false }

describe('statusMessage', () => {
  it('is empty when saved and connected', () => {
    expect(statusMessage(normal, text)).toBeNull()
    expect(statusMessage({ ...normal, live: 'message' }, text)).toBeNull()
    expect(statusMessage({ ...normal, live: 'connecting' }, text)).toBeNull()
  })

  it('shows offline when the browser or the stream is down', () => {
    expect(statusMessage({ ...normal, online: false }, text)?.kind).toBe('offline')
    expect(statusMessage({ ...normal, live: 'reconnecting' }, text)?.text).toBe('Offline / reconnecting…')
    expect(statusMessage({ ...normal, live: 'closed' }, text)?.kind).toBe('offline')
  })

  it('ranks offline over save error over saving over locked', () => {
    const all: StatusInput = { saving: true, online: false, live: 'reconnecting', saveError: 'boom', locked: true }
    expect(statusMessage(all, text)?.kind).toBe('offline')
    expect(statusMessage({ ...all, online: true, live: 'open' }, text)?.kind).toBe('error')
    expect(statusMessage({ ...all, online: true, live: 'open', saveError: null }, text)?.kind).toBe('saving')
    expect(statusMessage({ ...all, online: true, live: 'open', saveError: null, saving: false }, text)?.kind).toBe('locked')
  })

  it('shortens a save error and keeps the full text as the title', () => {
    const long = 'x'.repeat(200)
    const message = statusMessage({ ...normal, saveError: long }, text)
    expect(message?.text.length).toBeLessThan(80)
    expect(message?.title).toBe(long)
    expect(shortError('a\n  b')).toBe('a b')
  })

  it('treats a missing network as disconnected', () => {
    expect(isConnected(true, 'open')).toBe(true)
    expect(isConnected(false, 'open')).toBe(false)
  })
})
