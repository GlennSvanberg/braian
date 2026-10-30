import { freeObject } from './api'

/** Body for POST /v1/objects. The name comes from the dialog. Status is a type default. */
export function newObjectBody(input: {
  typeKey: string
  number: string
  name: string
  group: string | null
  /** Missing means any tree. An empty list means the type has no tree. */
  structures?: string[] | null
}) {
  return {
    type: input.typeKey,
    groups: placement(input.group, input.structures),
    values: freeObject({
      number: input.number.trim(),
      name: { en: input.name.trim() },
    }),
  }
}

function placement(group: string | null, structures: string[] | null | undefined): { [key: string]: string } {
  const groups: { [key: string]: string } = {}
  if (!group || structures?.length === 0) return groups
  const splitAt = group.indexOf(':')
  if (splitAt < 1) return groups
  const structure = group.slice(0, splitAt)
  const node = group.slice(splitAt + 1)
  if (!node || node === 'unplaced') return groups
  if (structures && !structures.includes(structure)) return groups
  groups[structure] = node
  return groups
}
