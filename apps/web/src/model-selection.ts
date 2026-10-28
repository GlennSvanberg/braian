import { setModelFocus } from './model-stage'
import type { ModelSelection } from './model-view'

let selection: ModelSelection | null = null
let previewType = ''
const listeners = new Set<() => void>()

function notify() {
  for (const listen of listeners) listen()
}

export function chooseModelSelection(next: ModelSelection) {
  selection = next
  if (next.kind === 'type') {
    previewType = next.typeKey
    setModelFocus({ kind: 'type', key: next.typeKey })
  } else {
    setModelFocus({ kind: 'structure', key: next.structure })
  }
  notify()
}

export function setPreviewType(typeKey: string) {
  previewType = typeKey
  notify()
}

export function getModelSelection() {
  return selection
}

export function getPreviewType() {
  return previewType
}

export function subscribeModelSelection(listen: () => void) {
  listeners.add(listen)
  return () => {
    listeners.delete(listen)
  }
}
