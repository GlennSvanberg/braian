import { useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import type { Json } from './api'
import { copy } from './i18n'

type Node = {
  id: string
  label: string
  hint: string
  x: number
  y: number
  objectId?: string
  tone: 'object' | 'relation' | 'source'
}

export function GraphMap({ data, onOpen }: { data: Json; onOpen: (id: string) => void }) {
  const relations = (data.relations as Json[]) ?? []
  const attributes = (data.attributes as Json[]) ?? []
  const initial = useMemo(() => layoutNodes(data, relations, attributes), [data, relations, attributes])
  const [nodes, setNodes] = useState<Node[]>(initial)
  const drag = useRef<{ id: string; dx: number; dy: number } | null>(null)
  const host = useRef<SVGSVGElement>(null)
  const placed = nodes.length === initial.length ? nodes : initial
  const point = (event: ReactPointerEvent) => {
    const rect = host.current?.getBoundingClientRect()
    if (!rect) return { x: 0, y: 0 }
    return { x: ((event.clientX - rect.left) / rect.width) * 640, y: ((event.clientY - rect.top) / rect.height) * 360 }
  }
  return (
    <div className="graph-wrap">
      <svg
        ref={host}
        className="graph"
        data-testid="graph"
        viewBox="0 0 640 360"
        preserveAspectRatio="xMidYMid meet"
        onPointerMove={(event) => {
          if (!drag.current) return
          const at = point(event)
          const current = drag.current
          setNodes((items) => items.map((node) => (node.id === current.id ? { ...node, x: at.x - current.dx, y: at.y - current.dy } : node)))
        }}
        onPointerUp={() => {
          drag.current = null
        }}
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
            onPointerDown={(event) => {
              const at = point(event)
              drag.current = { id: node.id, dx: at.x - node.x, dy: at.y - node.y }
              event.currentTarget.setPointerCapture(event.pointerId)
            }}
            onDoubleClick={() => {
              if (node.objectId) onOpen(node.objectId)
            }}
            style={{ cursor: 'grab' }}
          >
            <rect
              x={-78}
              y={-18}
              width={156}
              height={36}
              rx={8}
              fill={node.tone === 'object' ? 'var(--accent-dim)' : 'var(--raised)'}
              stroke={node.tone === 'object' ? 'var(--accent)' : 'var(--border)'}
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
      <div className="origin">
        {attributes.map((attribute) => {
          const source = ((attribute.sources as Json[]) ?? [])[0] ?? {}
          const origin = source.class
            ? `${copy.origin}: ${String(source.class)} / ${String(source.structure ?? '')}`
            : `${copy.origin}: ${String(source.from ?? 'type')}`
          return (
            <p key={String(attribute.key)}>
              <b>{String(attribute.label ?? attribute.key)}</b> {origin}
            </p>
          )
        })}
      </div>
    </div>
  )
}

function layoutNodes(data: Json, relations: Json[], attributes: Json[]): Node[] {
  const sources = new Map<string, string>()
  for (const attribute of attributes) {
    const source = ((attribute.sources as Json[]) ?? [])[0] ?? {}
    const label = source.class ? String(source.class) : String(source.from ?? 'type')
    const hint = source.structure ? String(source.structure) : 'type'
    sources.set(`${label}:${hint}`, hint)
  }
  const sourceNodes = [...sources.entries()].slice(0, 6).map(([label], index) => ({
    id: `source-${label}`,
    label: label.split(':')[0] ?? label,
    hint: 'attribute',
    x: 110,
    y: 48 + index * 48,
    tone: 'source' as const,
  }))
  const relationNodes = relations.slice(0, 6).map((relation, index) => ({
    id: `rel-${String(relation.object_id ?? relation.key)}`,
    label: String(relation.key),
    hint: String(relation.kind),
    x: 520,
    y: 48 + index * 48,
    objectId: String(relation.object_id ?? ''),
    tone: 'relation' as const,
  }))
  return [
    { id: 'self', label: String(data.key ?? ''), hint: String(data.type ?? 'object'), x: 320, y: 180, tone: 'object' },
    ...sourceNodes,
    ...relationNodes,
  ]
}
