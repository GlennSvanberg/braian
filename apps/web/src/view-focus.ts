const listeners = new Set<(token: string) => void>()

export function requestView(token: string) {
  for (const listener of listeners) listener(token)
}

export function onViewRequest(listener: (token: string) => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
