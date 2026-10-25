export type RankedNode = {
  id: string
  rank: number
}

export type PlacedNode<T extends RankedNode> = T & { x: number; y: number }

const columnGap = 220
const rowGap = 64

/** Layered layout: sources on the left, the object in the middle, relations on the right. */
export function placeNodes<T extends RankedNode>(nodes: T[]): Array<PlacedNode<T>> {
  const byRank = new Map<number, T[]>()
  for (const node of nodes) {
    const list = byRank.get(node.rank) ?? []
    list.push(node)
    byRank.set(node.rank, list)
  }
  const ranks = [...byRank.keys()].sort((left, right) => left - right)
  const placed: Array<PlacedNode<T>> = []
  ranks.forEach((rank, column) => {
    const columnNodes = byRank.get(rank) ?? []
    columnNodes.forEach((node, row) => {
      placed.push({ ...node, x: 120 + column * columnGap, y: 56 + row * rowGap })
    })
  })
  return placed
}

export function graphSize(nodes: Array<{ x: number; y: number }>) {
  const width = Math.max(640, ...nodes.map((node) => node.x + 140))
  const height = Math.max(360, ...nodes.map((node) => node.y + 48))
  return { width, height }
}

/** Keyboard move. Left/up go to the previous node, right/down to the next, in rank order. */
export function neighbor(order: string[], current: string, direction: 'up' | 'down' | 'left' | 'right') {
  if (order.length === 0) return current
  const index = Math.max(0, order.indexOf(current))
  const forward = direction === 'down' || direction === 'right'
  const next = forward ? Math.min(order.length - 1, index + 1) : Math.max(0, index - 1)
  return order[next] ?? current
}
