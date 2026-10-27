import type { Json } from './api'

/** Body for POST /v1/objects. The name comes from the dialog. Status is a type default. */
export function newObjectBody(input: {
  typeKey: string
  number: string
  name: string
  group: string | null
  /** Missing means any tree. An empty list means the type has no tree. */
  structures?: string[] | null
}): Json {
  return {
    type: input.typeKey,
    groups: placement(input.group, input.structures),
    values: {
      number: input.number.trim(),
      name: { en: input.name.trim() },
    },
  }
}

function placement(group: string | null, structures: string[] | null | undefined) {
  if (!group || structures?.length === 0) return {}
  const splitAt = group.indexOf(':')
  if (splitAt < 1) return {}
  const structure = group.slice(0, splitAt)
  const node = group.slice(splitAt + 1)
  if (!node || node === 'unplaced') return {}
  if (structures && !structures.includes(structure)) return {}
  return { [structure]: node }
}
