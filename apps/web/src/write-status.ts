import { useSyncExternalStore } from 'react'

/** In-flight writes (anything but GET/HEAD) and the last write failure. Feeds the status bar. */
export type WriteStatus = { pending: number; error: string | null }

let state: WriteStatus = { pending: 0, error: null }
const listeners = new Set<() => void>()
let clearTimer: ReturnType<typeof setTimeout> | undefined

function set(next: WriteStatus) {
  state = next
  for (const listener of listeners) listener()
}

export function isWrite(method: string): boolean {
  const upper = method.toUpperCase()
  return upper !== 'GET' && upper !== 'HEAD' && upper !== 'OPTIONS'
}

export function writeStarted() {
  set({ ...state, pending: state.pending + 1 })
}

export function writeSettled(failure?: string) {
  if (clearTimer) clearTimeout(clearTimer)
  const pending = Math.max(0, state.pending - 1)
  if (failure) {
    set({ pending, error: failure })
    clearTimer = setTimeout(() => set({ ...state, error: null }), 10_000)
  } else {
    set({ pending, error: null })
  }
}

export function writeStatus(): WriteStatus {
  return state
}

export function resetWriteStatus() {
  if (clearTimer) clearTimeout(clearTimer)
  set({ pending: 0, error: null })
}

export function subscribeWriteStatus(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function useWriteStatus(): WriteStatus {
  return useSyncExternalStore(subscribeWriteStatus, writeStatus, writeStatus)
}
