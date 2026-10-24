export type Zone = 'center' | 'left' | 'right' | 'top' | 'bottom'

export type LayoutNode =
  | { tabs: string[]; active?: string }
  | { direction: 'row' | 'column'; ratio: number; first: LayoutNode; second: LayoutNode }

export function isSplit(node: LayoutNode): node is Extract<LayoutNode, { direction: 'row' | 'column' }> {
  return 'direction' in node
}

export function removeTab(node: LayoutNode, id: string): LayoutNode | null {
  if (!isSplit(node)) {
    const tabs = node.tabs.filter((tab) => tab !== id)
    if (tabs.length === 0) return null
    const active = node.active === id ? tabs[0] : node.active
    return { tabs, active }
  }
  const first = removeTab(node.first, id)
  const second = removeTab(node.second, id)
  if (!first) return second
  if (!second) return first
  return { ...node, first, second }
}

export function insertTab(node: LayoutNode, id: string, zone: Zone, target: string): LayoutNode {
  if (!isSplit(node)) {
    if (!node.tabs.includes(target)) return node
    if (zone === 'center') {
      const tabs = node.tabs.includes(id) ? node.tabs : [...node.tabs, id]
      return { tabs, active: id }
    }
    const rest = { tabs: node.tabs.filter((tab) => tab !== id), active: target }
    const leaf = { tabs: [id], active: id }
    const direction = zone === 'left' || zone === 'right' ? 'row' : 'column'
    const first = zone === 'left' || zone === 'top' ? leaf : rest
    const second = zone === 'left' || zone === 'top' ? rest : leaf
    return { direction, ratio: 0.5, first, second }
  }
  return {
    ...node,
    first: insertTab(node.first, id, zone, target),
    second: insertTab(node.second, id, zone, target),
  }
}

export function dockTab(root: LayoutNode, id: string, zone: Zone, target: string): LayoutNode {
  const without = removeTab(root, id) ?? { tabs: [] }
  if (!isSplit(without) && without.tabs.length === 0) {
    return { tabs: [id], active: id }
  }
  return insertTab(without, id, zone, target)
}
