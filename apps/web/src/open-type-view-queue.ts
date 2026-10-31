/** Deferred openTypeView when the workspace document is not painted yet (Notion 112). */

let queuedTypeId: string | null = null

export function queueOpenTypeView(typeId: string) {
  queuedTypeId = typeId
}

export function peekQueuedOpenTypeView() {
  return queuedTypeId
}

export function takeQueuedOpenTypeView() {
  const id = queuedTypeId
  queuedTypeId = null
  return id
}

export function clearQueuedOpenTypeView() {
  queuedTypeId = null
}
