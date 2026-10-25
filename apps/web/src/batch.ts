import { api, type Json } from './api'

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
  const body: Json = {
    ids,
    groups: projection.groups ?? true,
  }
  if (projection.parts && projection.parts.length > 0) body.parts = projection.parts
  if (projection.fields && projection.fields.length > 0) body.fields = projection.fields
  if (projection.languages && projection.languages.length > 0) body.languages = projection.languages
  if (projection.values === true && !body.parts && !body.fields) body.values = true
  return body
}

/** Loads many objects in chunks. The default projection is heads and groups. */
export async function batchGetObjects(ids: string[], projection: LeanProjection = {}): Promise<Json[]> {
  const unique = [...new Set(ids.filter((id) => id.length > 0))]
  const objects: Json[] = []
  for (let index = 0; index < unique.length; index += BATCH_GET_LIMIT) {
    const slice = unique.slice(index, index + BATCH_GET_LIMIT)
    const body = await api<{ objects?: Json[] }>('/v1/objects:batchGet', {
      method: 'POST',
      body: JSON.stringify(batchGetBody(slice, projection)),
    })
    objects.push(...(body.objects ?? []))
  }
  return objects
}
