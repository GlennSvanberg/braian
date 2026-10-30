import { api, jsonRecord, type Json } from './api'
import { publishProposal } from './proposal-bus'

export type ModelFocus =
  | { kind: 'type'; key: string }
  | { kind: 'attribute'; key: string }
  | { kind: 'structure'; key: string }
  | { kind: 'value_list'; key: string }
  | { kind: 'option'; list: string; code: string }
  | { kind: 'unit_family'; key: string }
  | { kind: 'unit'; family: string; code: string }

let focus: ModelFocus | null = null
const listeners = new Set<() => void>()

export function modelFocus() {
  return focus
}

export function setModelFocus(next: ModelFocus) {
  focus = next
}

export function onDraftChange(listen: () => void) {
  listeners.add(listen)
  return () => {
    listeners.delete(listen)
  }
}

function emitDraft() {
  for (const listen of listeners) listen()
}

export function modelDeletes(name: string) {
  return name.includes('delete')
}

/** A person's edit joins the draft. A delete opens the consequence dialog. */
export async function stageModel(name: string, args: Json = {}) {
  if (modelDeletes(name)) {
    const result = await api.post('/v1/operations/{name}', { params: { path: { name: name } }, body: args })
    const record = jsonRecord(result)
    const id = String(record?.proposal_id ?? record?.id ?? '')
    if (id) publishProposal(id)
    return result
  }
  const draft = await api.post('/v1/model/draft/steps', { body: { operation: name, args } })
  emitDraft()
  return draft
}

export function openModelDialog(kind: 'type' | 'list' | 'family' | 'tree' | 'link') {
  window.dispatchEvent(new CustomEvent('braian-model-dialog', { detail: kind }))
}

export function requestReview() {
  window.dispatchEvent(new Event('braian-review-save'))
}

export function requestDiscardDraft() {
  window.dispatchEvent(new Event('braian-discard-draft'))
}
