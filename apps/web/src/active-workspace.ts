/** Persist which workspace tab is active across reload (F5). Per user, like layout. */

const KEY_PREFIX = 'braian.activeWorkspace.'

export function activeWorkspaceStorageKey(userId: string) {
  return `${KEY_PREFIX}${userId}`
}

function storageOrNull(storage?: Storage | null): Storage | null {
  if (storage !== undefined) return storage
  if (typeof localStorage === 'undefined') return null
  return localStorage
}

export function readActiveWorkspaceId(userId: string, storage?: Storage | null): string | null {
  if (!userId) return null
  const store = storageOrNull(storage)
  if (!store) return null
  try {
    const raw = store.getItem(activeWorkspaceStorageKey(userId))
    if (!raw) return null
    const parsed = JSON.parse(raw) as unknown
    return typeof parsed === 'string' && parsed.length > 0 ? parsed : null
  } catch {
    return null
  }
}

export function writeActiveWorkspaceId(userId: string, workspaceId: string, storage?: Storage | null) {
  if (!userId || !workspaceId) return
  storageOrNull(storage)?.setItem(activeWorkspaceStorageKey(userId), JSON.stringify(workspaceId))
}

/** Default when nothing is stored or the stored id is gone — same as shell before persistence. */
export function defaultActiveWorkspaceId(
  ordered: Array<{ id: string; share_token?: string | null }>,
): string | null {
  if (ordered.length === 0) return null
  const preferred = ordered.find((item) => item.share_token?.startsWith('braian-type-'))
  return preferred?.id ?? ordered[1]?.id ?? ordered[0]?.id ?? null
}

/**
 * Pick the active workspace after load or when the list changes.
 * Keep `current` if still listed; else restore `stored` if listed; else default.
 */
export function resolveActiveWorkspaceId(
  ordered: Array<{ id: string; share_token?: string | null }>,
  current: string | null,
  stored: string | null,
): string | null {
  if (ordered.length === 0) return null
  if (current && ordered.some((item) => item.id === current)) return current
  if (stored && ordered.some((item) => item.id === stored)) return stored
  return defaultActiveWorkspaceId(ordered)
}
