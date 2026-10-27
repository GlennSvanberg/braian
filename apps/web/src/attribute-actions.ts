export type AttributeAction = 'add-type' | 'add-group' | 'rename' | 'rename-key' | 'delete' | 'new'

const STANDARD = [
  ['018f0000-0000-7000-8000-000000000100', 'number'],
  ['018f0000-0000-7000-8000-000000000101', 'name'],
  ['018f0000-0000-7000-8000-00000000a003', 'created_at'],
  ['018f0000-0000-7000-8000-00000000a004', 'updated_at'],
  ['018f0000-0000-7000-8000-00000000a005', 'updated_by'],
] as const

/** Locked when both the fixed id and the canonical key are in place. */
export function isStandardAttribute(id: string, key: string) {
  return STANDARD.some(([standardId, standardKey]) => standardId === id && standardKey === key)
}

type Handler = (action: AttributeAction) => void

const handlers: Handler[] = []

export function onAttributeAction(handler: Handler) {
  handlers.push(handler)
  return () => {
    const index = handlers.lastIndexOf(handler)
    if (index >= 0) handlers.splice(index, 1)
  }
}

/** The open Attribute definitions view handles the action. */
export function requestAttributeAction(action: AttributeAction) {
  const handler = handlers.at(-1)
  if (!handler) return false
  handler(action)
  return true
}

let queuedViewType: string | null = null

export function queueViewType(type: string) {
  queuedViewType = type
}

export function peekQueuedViewType() {
  return queuedViewType
}

export function clearQueuedViewType() {
  queuedViewType = null
}
