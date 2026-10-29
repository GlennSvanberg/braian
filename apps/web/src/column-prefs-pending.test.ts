// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ColumnPrefsDocument } from './column-prefs'
import {
  clearAllPendingColumnPrefs,
  clearPendingColumnPrefs,
  pendingPrefsKey,
  prefsDocumentsEqual,
  readPendingColumnPrefs,
  selectInitialColumnPrefs,
  writePendingColumnPrefs,
} from './column-prefs-pending'

const viewId = '11111111-1111-1111-1111-111111111111'
const doc = (extra?: Partial<ColumnPrefsDocument['columns'][number]>): ColumnPrefsDocument => ({
  version: 2,
  columns: [{ path: 'name', visible: true, ...extra }],
})

beforeEach(() => {
  sessionStorage.clear()
})

afterEach(() => {
  sessionStorage.clear()
  vi.unstubAllGlobals()
})

describe('pending column prefs', () => {
  it('stores a pending entry with seq, document version and basis', () => {
    writePendingColumnPrefs(viewId, doc(), 1, '2026-01-01T00:00:00.000Z')
    expect(readPendingColumnPrefs(viewId)).toEqual({
      viewId,
      seq: 1,
      document: { version: 2, columns: [{ path: 'name', visible: true }] },
      basisUpdatedAt: '2026-01-01T00:00:00.000Z',
    })
    expect(sessionStorage.getItem(pendingPrefsKey(viewId))).toContain('"seq":1')
  })

  it('clears pending after a matching 2xx seq', () => {
    writePendingColumnPrefs(viewId, doc(), 2)
    clearPendingColumnPrefs(viewId, 2)
    expect(readPendingColumnPrefs(viewId)).toBeNull()
  })

  it('keeps pending when clear is called with an older seq', () => {
    writePendingColumnPrefs(viewId, doc(), 1)
    writePendingColumnPrefs(viewId, doc({ path: 'status' }), 2)
    clearPendingColumnPrefs(viewId, 1)
    expect(readPendingColumnPrefs(viewId)?.seq).toBe(2)
    expect(readPendingColumnPrefs(viewId)?.document.columns[0]?.path).toBe('status')
  })

  it('leaves pending in place when not cleared (simulating a failed PUT)', () => {
    writePendingColumnPrefs(viewId, doc(), 3)
    expect(readPendingColumnPrefs(viewId)?.seq).toBe(3)
  })

  it('clearAllPendingColumnPrefs removes every pending key', () => {
    writePendingColumnPrefs(viewId, doc(), 1)
    writePendingColumnPrefs('22222222-2222-2222-2222-222222222222', doc(), 1)
    clearAllPendingColumnPrefs()
    expect(readPendingColumnPrefs(viewId)).toBeNull()
    expect(sessionStorage.length).toBe(0)
  })
})

describe('selectInitialColumnPrefs', () => {
  const server: ColumnPrefsDocument = {
    version: 2,
    columns: [{ path: 'sku', visible: false }],
  }
  const pendingDoc: ColumnPrefsDocument = {
    version: 2,
    columns: [{ path: 'sku', visible: true }],
  }

  it('mount with unacked pending uses it and asks for a retry PUT', () => {
    const pending = {
      viewId,
      seq: 4,
      document: pendingDoc,
      basisUpdatedAt: '2026-01-01T00:00:00.000Z',
    }
    const result = selectInitialColumnPrefs(true, pending, server, '2026-01-01T00:00:00.000Z')
    expect(result.document).toEqual(pendingDoc)
    expect(result.retryPending).toEqual(pending)
    expect(result.discardPending).toBe(false)
  })

  it('mount without pending uses the server document', () => {
    const result = selectInitialColumnPrefs(true, null, server, '2026-01-02T00:00:00.000Z')
    expect(result.document).toEqual(server)
    expect(result.retryPending).toBeNull()
  })

  it('skips pending when hydrateCache is false (vitest)', () => {
    const pending = { viewId, seq: 4, document: pendingDoc, basisUpdatedAt: '2026-01-01T00:00:00.000Z' }
    const result = selectInitialColumnPrefs(false, pending, server, '2026-01-02T00:00:00.000Z')
    expect(result.document).toEqual(server)
    expect(result.retryPending).toBeNull()
  })

  it('discards pending when the server already matches (acked)', () => {
    const pending = { viewId, seq: 4, document: server, basisUpdatedAt: '2026-01-01T00:00:00.000Z' }
    const result = selectInitialColumnPrefs(true, pending, server, '2026-01-02T00:00:00.000Z')
    expect(result.document).toEqual(server)
    expect(result.retryPending).toBeNull()
    expect(result.discardPending).toBe(true)
  })

  it('discards pending older than a newer server ack', () => {
    const pending = {
      viewId,
      seq: 1,
      document: pendingDoc,
      basisUpdatedAt: '2000-01-01T00:00:00.000Z',
    }
    const result = selectInitialColumnPrefs(true, pending, server, '2026-10-29T12:00:00.000Z')
    expect(result.document).toEqual(server)
    expect(result.retryPending).toBeNull()
    expect(result.discardPending).toBe(true)
  })

  it('prefsDocumentsEqual compares columns', () => {
    expect(prefsDocumentsEqual(server, { version: 2, columns: [{ path: 'sku', visible: false }] })).toBe(true)
    expect(prefsDocumentsEqual(server, pendingDoc)).toBe(false)
  })
})
