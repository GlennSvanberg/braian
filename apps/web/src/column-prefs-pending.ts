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
}

export function pendingPrefsKey(viewId: string): string {
  return `${PENDING_PREFS_KEY_PREFIX}${viewId}`
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
): PendingColumnPrefsEntry {
  const entry: PendingColumnPrefsEntry = { viewId, seq, document }
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

/** Decide which document to apply on mount. `hydrateCache` is false under vitest. */
export function selectInitialColumnPrefs(
  hydrateCache: boolean,
  pending: PendingColumnPrefsEntry | null,
  serverDocument: ColumnPrefsDocument | null | undefined,
): { document: ColumnPrefsDocument | null; retryPending: PendingColumnPrefsEntry | null } {
  if (hydrateCache && pending) {
    return { document: pending.document, retryPending: pending }
  }
  if (serverDocument && serverDocument.version === 2 && Array.isArray(serverDocument.columns)) {
    return { document: serverDocument, retryPending: null }
  }
  return { document: null, retryPending: null }
}
