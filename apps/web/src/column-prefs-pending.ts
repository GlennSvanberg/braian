/**
 * Tab-local pending column-prefs PUT. Survives reload when the in-flight fetch is
 * aborted; never overrides the server once the PUT has been acknowledged.
 */
import type { ColumnPrefsDocument } from './column-prefs'

export const PENDING_PREFS_KEY_PREFIX = 'braian:column-prefs-pending:'

export type PendingColumnPrefsEntry = {
  viewId: string
  /** Monotonic id for this tab's writes — an older 2xx must not clear a newer entry. */
  seq: number
  document: ColumnPrefsDocument
  /**
   * Server `updated_at` we knew when this edit was queued. If the server later
   * reports a newer timestamp, this pending must not revive an older layout.
   */
  basisUpdatedAt?: string | null
}

export function pendingPrefsKey(viewId: string): string {
  return `${PENDING_PREFS_KEY_PREFIX}${viewId}`
}

export function prefsDocumentsEqual(
  left: ColumnPrefsDocument | null | undefined,
  right: ColumnPrefsDocument | null | undefined,
): boolean {
  if (!left || !right) return false
  return JSON.stringify(left.columns) === JSON.stringify(right.columns)
}

export function readPendingColumnPrefs(viewId: string): PendingColumnPrefsEntry | null {
  try {
    const raw = sessionStorage.getItem(pendingPrefsKey(viewId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as PendingColumnPrefsEntry
    if (!parsed || typeof parsed !== 'object') return null
    if (parsed.viewId !== viewId) return null
    if (typeof parsed.seq !== 'number' || !Number.isFinite(parsed.seq)) return null
    if (!parsed.document || parsed.document.version !== 2) return null
    if (!Array.isArray(parsed.document.columns)) return null
    return parsed
  } catch {
    return null
  }
}

export function writePendingColumnPrefs(
  viewId: string,
  document: ColumnPrefsDocument,
  seq: number,
  basisUpdatedAt?: string | null,
): PendingColumnPrefsEntry {
  const entry: PendingColumnPrefsEntry = {
    viewId,
    seq,
    document,
    basisUpdatedAt: basisUpdatedAt ?? null,
  }
  try {
    sessionStorage.setItem(pendingPrefsKey(viewId), JSON.stringify(entry))
  } catch {
    /* private mode / quota */
  }
  return entry
}

/** Clear pending only when `seq` still matches (or when clearing unconditionally). */
export function clearPendingColumnPrefs(viewId: string, seq?: number): void {
  try {
    if (seq != null) {
      const current = readPendingColumnPrefs(viewId)
      if (!current || current.seq !== seq) return
    }
    sessionStorage.removeItem(pendingPrefsKey(viewId))
  } catch {
    /* private mode */
  }
}

export function clearAllPendingColumnPrefs(): void {
  try {
    for (let i = sessionStorage.length - 1; i >= 0; i -= 1) {
      const key = sessionStorage.key(i)
      if (
        key?.startsWith(PENDING_PREFS_KEY_PREFIX) ||
        key?.startsWith('braian:column-prefs:')
      ) {
        sessionStorage.removeItem(key)
      }
    }
  } catch {
    /* private mode */
  }
}

/**
 * Decide which document to apply on mount. `hydrateCache` is false under vitest.
 * Pending never wins over a server document that was acked after the edit basis.
 */
export function selectInitialColumnPrefs(
  hydrateCache: boolean,
  pending: PendingColumnPrefsEntry | null,
  serverDocument: ColumnPrefsDocument | null | undefined,
  serverUpdatedAt?: string | null,
): {
  document: ColumnPrefsDocument | null
  retryPending: PendingColumnPrefsEntry | null
  discardPending: boolean
} {
  if (hydrateCache && pending) {
    const sameAsServer = prefsDocumentsEqual(pending.document, serverDocument)
    if (sameAsServer && serverDocument) {
      return { document: serverDocument, retryPending: null, discardPending: true }
    }
    const basis = pending.basisUpdatedAt ?? null
    const serverMovedOn =
      Boolean(serverUpdatedAt) && (basis == null || String(serverUpdatedAt) > String(basis))
    if (serverMovedOn && serverDocument) {
      // Server acked something newer than the edit we based pending on — drop it.
      return { document: serverDocument, retryPending: null, discardPending: true }
    }
    return { document: pending.document, retryPending: pending, discardPending: false }
  }
  if (serverDocument && serverDocument.version === 2 && Array.isArray(serverDocument.columns)) {
    return { document: serverDocument, retryPending: null, discardPending: false }
  }
  return { document: null, retryPending: null, discardPending: false }
}
