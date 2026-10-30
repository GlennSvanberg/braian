import { api } from './api'

/** Matches `BATCH_GET_LIMIT` on `POST /v1/objects:batchGet`. */
export const BATCH_GET_LIMIT = 200

export type LeanProjection = {
  groups?: boolean
  values?: boolean
  parts?: string[]
  fields?: string[]
  languages?: string[]
}

/**
 * Body for a batch read. Omitting `values`, `fields` and `parts` asks for
 * heads and groups only.
 */
export function batchGetBody(ids: string[], projection: LeanProjection = {}) {
  const parts = projection.parts && projection.parts.length > 0 ? projection.parts : undefined
  const fields = projection.fields && projection.fields.length > 0 ? projection.fields : undefined
  return {
    ids,
    groups: projection.groups ?? true,
    ...(parts ? { parts } : {}),
    ...(fields ? { fields } : {}),
    ...(projection.languages && projection.languages.length > 0 ? { languages: projection.languages } : {}),
    ...(projection.values === true && !parts && !fields ? { values: true } : {}),
  }
}

/** Loads many objects in chunks. The default projection is heads and groups. */
export async function batchGetObjects(ids: string[], projection: LeanProjection = {}) {
  const unique = [...new Set(ids.filter((id) => id.length > 0))]
  const objects = []
  for (let index = 0; index < unique.length; index += BATCH_GET_LIMIT) {
    const slice = unique.slice(index, index + BATCH_GET_LIMIT)
    const body = await api.post('/v1/objects:batchGet', { body: batchGetBody(slice, projection) })
    objects.push(...(body.objects ?? []))
  }
  return objects
}
