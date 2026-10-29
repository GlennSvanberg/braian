// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ColumnPrefsDocument } from './column-prefs'
import {
  clearAllPendingColumnPrefs,
  clearPendingColumnPrefs,
  pendingPrefsKey,
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
  it('stores a pending entry with seq and document version', () => {
    writePendingColumnPrefs(viewId, doc(), 1)
    expect(readPendingColumnPrefs(viewId)).toEqual({
      viewId,
      seq: 1,
      document: { version: 2, columns: [{ path: 'name', visible: true }] },
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
    // caller swallows the error and does not clear
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
    columns: [{ path: 'sku', visible: true }],
  }
  const pendingDoc: ColumnPrefsDocument = {
    version: 2,
    columns: [{ path: 'sku', visible: false }],
  }

  it('mount with pending uses it and asks for a retry PUT', () => {
    const pending = { viewId, seq: 4, document: pendingDoc }
    const result = selectInitialColumnPrefs(true, pending, server)
    expect(result.document).toEqual(pendingDoc)
    expect(result.retryPending).toEqual(pending)
  })

  it('mount without pending uses the server document', () => {
    const result = selectInitialColumnPrefs(true, null, server)
    expect(result.document).toEqual(server)
    expect(result.retryPending).toBeNull()
  })

  it('skips pending when hydrateCache is false (vitest)', () => {
    const pending = { viewId, seq: 4, document: pendingDoc }
    const result = selectInitialColumnPrefs(false, pending, server)
    expect(result.document).toEqual(server)
    expect(result.retryPending).toBeNull()
  })
})
