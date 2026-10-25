export function groupKeyFromLabel(value: string) {
  return value.trim().toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '')
}

type GroupRef = { id: string; parent?: string | null }

/** A drop makes the target the new parent. Reject the node itself and its descendants. */
export function dropReparents(groups: GroupRef[], draggedId: string, targetId: string) {
  if (!draggedId || !targetId || draggedId === targetId) return false
  const parentOf = new Map(groups.map((group) => [group.id, group.parent ? String(group.parent) : '']))
  let current = targetId
  const seen = new Set<string>()
  while (current && !seen.has(current)) {
    if (current === draggedId) return false
    seen.add(current)
    current = parentOf.get(current) ?? ''
  }
  return true
}
