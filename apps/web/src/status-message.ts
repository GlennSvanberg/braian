export type LiveState = 'connecting' | 'open' | 'message' | 'reconnecting' | 'closed'

export type StatusMessage = { kind: 'offline' | 'error' | 'saving' | 'locked'; text: string; title?: string }

export type StatusInput = {
  saving: boolean
  online: boolean
  live: LiveState
  saveError: string | null
  locked: boolean
}

/** Connected means: the browser is online and the event stream is not broken. */
export function isConnected(online: boolean, live: LiveState): boolean {
  return online && live !== 'reconnecting' && live !== 'closed'
}

const MAX_ERROR = 60

export function shortError(text: string): string {
  const one = text.replace(/\s+/g, ' ').trim()
  return one.length > MAX_ERROR ? `${one.slice(0, MAX_ERROR - 1)}…` : one
}

/** Null in the normal state (saved and connected): the status bar stays empty. */
export function statusMessage(input: StatusInput, text: { reconnecting: string; saving: string; saveFailed: string; locked: string }): StatusMessage | null {
  if (!isConnected(input.online, input.live)) return { kind: 'offline', text: text.reconnecting }
  if (input.saveError) {
    return { kind: 'error', text: `${text.saveFailed}: ${shortError(input.saveError)}`, title: input.saveError }
  }
  if (input.saving) return { kind: 'saving', text: text.saving }
  if (input.locked) return { kind: 'locked', text: text.locked }
  return null
}
