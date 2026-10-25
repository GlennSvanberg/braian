import { attributeKey } from './model-view'

export type MenuNode = { key: string; parentKey?: string }

/** The group and every group under it. */
export function groupKeysToDelete(nodes: MenuNode[], key: string) {
  return [key, ...descendantKeys(nodes, key)]
}

export function descendantKeys(nodes: MenuNode[], key: string) {
  const children = new Map<string, string[]>()
  for (const node of nodes) {
    if (!node.parentKey) continue
    const list = children.get(node.parentKey) ?? []
    list.push(node.key)
    children.set(node.parentKey, list)
  }
  const out: string[] = []
  const walk = (current: string) => {
    for (const child of children.get(current) ?? []) {
      out.push(child)
      walk(child)
    }
  }
  walk(key)
  return out
}

/** Groups the node can move under. Self and descendants are excluded. */
export function moveTargets(nodes: MenuNode[], key: string) {
  const blocked = new Set(groupKeysToDelete(nodes, key))
  return nodes.map((node) => node.key).filter((item) => !blocked.has(item))
}

export function deletionNeedsMove(count: number) {
  return count > 0
}

export function structureKeyFromName(name: string) {
  return attributeKey(name)
}

export function renameOnKey(key: string): 'commit' | 'cancel' | 'type' {
  if (key === 'Enter') return 'commit'
  if (key === 'Escape') return 'cancel'
  return 'type'
}
