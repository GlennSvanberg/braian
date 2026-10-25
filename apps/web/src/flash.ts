const listeners = new Set<(id: string) => void>()

export function flashObject(id: string) {
  if (!id) return
  for (const listen of listeners) listen(id)
  window.setTimeout(() => {
    for (const listen of listeners) listen('')
  }, 600)
}

export function onFlash(listener: (id: string) => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}
