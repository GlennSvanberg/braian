import { api } from './api'
import { parseColumnId } from './fields/columns'

export type PathValueEntry = {
  text: string
  title?: string
  /** Linked peer object id when the path crosses a link. */
  peer_id?: string
  /** True when unit conversion could not be applied. */
  raw?: boolean
}

export type PathValuesResponse = {
  values: Record<string, Record<string, string[]>>
  /** Optional richer hop entries (peer ids, conversion markers). */
  entries?: Record<string, Record<string, PathValueEntry[]>>
}

export type PathSpec = {
  path: string
  cardinality?: 'all' | 'first' | 'specific'
  peer_id?: string
  /** Target display unit for measure attributes. */
  unit?: string
}

export function fetchPathValues(input: {
  ids: string[]
  paths: string[] | PathSpec[]
  typeKey: string
  lang?: string
}) {
  const paths = input.paths.map((item) => {
    if (typeof item === 'string') {
      const { path, qualifiers } = parseColumnId(item)
      const card = qualifiers.find((q) => q.kind === 'cardinality')
      const unit = qualifiers.find((q) => q.kind === 'unit')
      const spec: PathSpec = { path }
      if (card) {
        spec.cardinality = card.optionId as PathSpec['cardinality']
        if (card.optionId === 'specific' && card.value) spec.peer_id = card.value
      }
      if (unit) spec.unit = unit.optionId
      return spec
    }
    return item
  })
  return api<PathValuesResponse>('/v1/objects:pathValues', {
    method: 'POST',
    body: JSON.stringify({
      ids: input.ids,
      paths,
      type: input.typeKey,
      lang: input.lang,
    }),
  })
}
