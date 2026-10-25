import { useEffect, useMemo, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import type { Json } from './api'
import { graphSize, neighbor, placeNodes } from './graph-layout'
import { copy } from './i18n'

type Node = {
  id: string
  label: string
  hint: string
  objectId?: string
  tone: 'object' | 'relation' | 'source'
  rank: number
  x: number
  y: number
}

export function GraphMap({ data, onOpen }: { data: Json; onOpen: (id: string) => void }) {
  const [full, setFull] = useState(false)
  const [selected, setSelected] = useState('self')
  const placed = useMemo(() => layout(data), [data])
  const order = placed.map((node) => node.id)
  const size = graphSize(placed)
  useEffect(() => {
    if (!order.includes(selected)) setSelected(order[0] ?? 'self')
  }, [order, selected])
  const onKey = (event: ReactKeyboardEvent) => {
    if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Enter'].includes(event.key)) return
    event.preventDefault()
    if (event.key === 'Enter') {
      const node = placed.find((item) => item.id === selected)
      if (node?.objectId) onOpen(node.objectId)
      return
    }
    const direction = event.key === 'ArrowUp' ? 'up' : event.key === 'ArrowDown' ? 'down' : event.key === 'ArrowLeft' ? 'left' : 'right'
    setSelected((current) => neighbor(order, current, direction))
  }
  const map = (
    <svg
      className="graph"
      data-testid="graph"
      viewBox={`0 0 ${size.width} ${size.height}`}
      preserveAspectRatio="xMidYMid meet"
      tabIndex={0}
      onKeyDown={onKey}
    >
      {placed.flatMap((node) => {
        if (node.id === 'self') return []
        const self = placed.find((item) => item.id === 'self')
        if (!self) return []
        return [
          <line key={`line-${node.id}`} x1={self.x} y1={self.y} x2={node.x} y2={node.y} stroke="var(--border)" strokeWidth="1.5" />,
        ]
      })}
      {placed.map((node) => (
        <g
          key={node.id}
          transform={`translate(${node.x} ${node.y})`}
          onClick={() => setSelected(node.id)}
          onDoubleClick={() => {
            if (node.objectId) onOpen(node.objectId)
          }}
          style={{ cursor: 'pointer' }}
        >
          <rect
            x={-78}
            y={-18}
            width={156}
            height={36}
            rx="var(--r-lg)"
            fill={node.id === selected ? 'var(--accent-dim)' : node.tone === 'object' ? 'var(--accent-dim)' : 'var(--raised)'}
            stroke={node.id === selected || node.tone === 'object' ? 'var(--accent)' : 'var(--border)'}
          />
          <text textAnchor="middle" y={-1} fill="var(--text)" fontSize="12">
            {node.label}
          </text>
          <text textAnchor="middle" y={12} fill="var(--muted)" fontSize="10">
            {node.hint}
          </text>
        </g>
      ))}
    </svg>
  )
  const attributes = (data.attributes as Json[]) ?? []
  const provenance = (
    <div className="origin" data-testid="attribute-provenance">
      <p className="hint">{copy.attributeProvenance}</p>
      {attributes.map((attribute) => {
        const source = ((attribute.sources as Json[]) ?? [])[0] ?? {}
        const inherited = Boolean(source.inherited)
        const origin = source.class
          ? `${inherited ? copy.inherited : copy.own}: ${String(source.class)} / ${String(source.structure ?? '')}`
          : `${copy.origin}: ${String(source.from ?? 'type')}`
        return (
          <p key={String(attribute.key)}>
            <b>{String(attribute.label ?? attribute.key)}</b> {origin}
          </p>
        )
      })}
    </div>
  )
  return (
    <div className={full ? 'graph-wrap graph-full' : 'graph-wrap'} data-testid={full ? 'graph-full' : undefined}>
      <div className="toolbar">
        <button type="button" onClick={() => setFull((value) => !value)}>
          {full ? copy.closeFull : copy.openFull}
        </button>
      </div>
      {map}
      {provenance}
    </div>
  )
}

function layout(data: Json): Node[] {
  const relations = (data.relations as Json[]) ?? []
  const attributes = (data.attributes as Json[]) ?? []
  const sources = new Map<string, { label: string; hint: string }>()
  for (const attribute of attributes) {
    const source = ((attribute.sources as Json[]) ?? [])[0] ?? {}
    const label = source.class ? String(source.class) : String(source.from ?? 'type')
    const hint = source.inherited ? 'inherited' : source.structure ? String(source.structure) : 'type'
    sources.set(label, { label, hint })
  }
  const ranked = [
    ...[...sources.values()].map((source) => ({
      id: `source-${source.label}`,
      label: source.label,
      hint: source.hint,
      tone: 'source' as const,
      rank: 0,
    })),
    { id: 'self', label: String(data.key ?? ''), hint: String(data.type ?? 'object'), tone: 'object' as const, rank: 1 },
    ...relations.map((relation) => ({
      id: `rel-${String(relation.object_id ?? relation.key)}`,
      label: String(relation.key),
      hint: String(relation.kind),
      objectId: String(relation.object_id ?? ''),
      tone: 'relation' as const,
      rank: 2,
    })),
  ]
  return placeNodes(ranked)
}
