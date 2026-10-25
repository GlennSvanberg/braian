const key = 'braian.frecency'

export type FrecencyEntry = { uses: number; last: number }

export function readFrecency(storage: Storage | null = localStorageOrNull()): Record<string, FrecencyEntry> {
  if (!storage) return {}
  try {
    const raw = storage.getItem(key)
    return raw ? (JSON.parse(raw) as Record<string, FrecencyEntry>) : {}
  } catch {
    return {}
  }
}

export function score(entry: FrecencyEntry | undefined, now = Date.now()) {
  if (!entry || entry.uses <= 0) return 0
  const ageDays = Math.max(0, now - entry.last) / 86_400_000
  return entry.uses * 0.5 ** (ageDays / 7)
}

export function touch(id: string, now = Date.now(), storage: Storage | null = localStorageOrNull()) {
  const all = readFrecency(storage)
  const previous = all[id] ?? { uses: 0, last: now }
  all[id] = { uses: previous.uses + 1, last: now }
  storage?.setItem(key, JSON.stringify(all))
  return all[id]
}

export function mergeFrecency(
  local: Record<string, FrecencyEntry>,
  remote: Record<string, FrecencyEntry>,
): Record<string, FrecencyEntry> {
  const merged = { ...local }
  for (const [id, entry] of Object.entries(remote)) {
    const current = merged[id]
    if (!current || entry.uses > current.uses || (entry.uses === current.uses && entry.last > current.last)) {
      merged[id] = entry
    }
  }
  return merged
}

export function writeFrecency(entries: Record<string, FrecencyEntry>, storage: Storage | null = localStorageOrNull()) {
  storage?.setItem(key, JSON.stringify(entries))
}

export function rankByFrecency<T extends { id: string }>(rows: T[], now = Date.now(), storage: Storage | null = localStorageOrNull()) {
  const all = readFrecency(storage)
  return [...rows].sort((left, right) => score(all[right.id], now) - score(all[left.id], now))
}

function localStorageOrNull() {
  if (typeof localStorage === 'undefined') return null
  return localStorage
}
