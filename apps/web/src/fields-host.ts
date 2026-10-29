/**
 * Registry so the Ctrl+K Fields › mode can read/write the open object table's columns.
 * ObjectTable publishes a host while mounted (same idea as ui-context columns).
 */
import type { FieldDef } from './fields/qualifiers'
import type { PeerPickOption } from './fields/fields-mode'
import type { ViewColumn } from './fields/columns'

export type FieldsHost = {
  typeKey: string
  fields: FieldDef[]
  columns: ViewColumn[]
  onToggleColumn: (column: ViewColumn) => void
  onReplaceColumn: (previousId: string, next: ViewColumn) => void
  peerOptionsForField: (leafId: string, query: string) => PeerPickOption[]
}

let host: FieldsHost | null = null
let editColumnId: string | null = null
const listeners = new Set<() => void>()

export function publishFieldsHost(next: FieldsHost | null) {
  host = next
  for (const listener of listeners) listener()
}

export function readFieldsHost() {
  return host
}

export function subscribeFieldsHost(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** When opening Fields › on a specific column (Edit field…). */
export function setFieldsEditColumnId(id: string | null) {
  editColumnId = id
}

export function takeFieldsEditColumnId() {
  const id = editColumnId
  editColumnId = null
  return id
}
