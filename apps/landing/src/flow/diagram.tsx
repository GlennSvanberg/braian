import type { CSSProperties } from 'react'
import { arrowHead, roundedPolyline, sketchRect, sketchRoute, type Pt } from '@/flow/sketch'

type Box = {
  id: string
  x: number
  y: number
  w: number
  h: number
  title: string
  sub?: string
  order: number
  seed: number
}

type Wire = {
  id: string
  points: Pt[]
  order: number
  seed: number
  tone?: 'ink' | 'accent' | 'heavy'
  label?: { x: number; y: number; text: string; anchor?: 'start' | 'middle' | 'end' }
  radius?: number
  rough?: number
}

type Route = {
  id: string
  points: Pt[]
  dur: string
  r: number
  delay: number
}

type Layout = {
  width: number
  height: number
  boxes: Box[]
  wires: Wire[]
  routes: Route[]
}

const toneClass = {
  ink: 'flow-ink',
  accent: 'flow-accent',
  heavy: 'flow-heavy',
} as const

function drawStyle(order: number): CSSProperties {
  return { '--i': String(order) } as CSSProperties
}

function BoxNode({ box }: { box: Box }) {
  const cx = box.x + box.w / 2
  const cy = box.y + box.h / 2
  return (
    <g>
      <rect className="flow-fill" x={box.x} y={box.y} width={box.w} height={box.h} rx={8} />
      <path className="flow-ink" data-draw d={sketchRect(box.x, box.y, box.w, box.h, box.seed)} style={drawStyle(box.order)} />
      <text x={cx} y={box.sub ? cy - 9 : cy + 1} textAnchor="middle" dominantBaseline="middle" fontSize={18}>
        {box.title}
      </text>
      {box.sub ? (
        <text className="flow-sub" x={cx} y={cy + 13} textAnchor="middle" dominantBaseline="middle" fontSize={13}>
          {box.sub}
        </text>
      ) : null}
    </g>
  )
}

function WirePath({ wire }: { wire: Wire }) {
  const pts = wire.points
  const end = pts[pts.length - 1]
  const prev = pts[pts.length - 2]
  const d = `${sketchRoute(pts, wire.seed, wire.radius ?? 12, wire.rough ?? 1.05)} ${arrowHead(prev.x, prev.y, end.x, end.y, wire.seed + 9)}`
  return (
    <g>
      <path className={toneClass[wire.tone ?? 'ink']} data-draw d={d} style={drawStyle(wire.order)} />
      {wire.label ? (
        <text
          className="flow-tag"
          x={wire.label.x}
          y={wire.label.y}
          textAnchor={wire.label.anchor ?? 'middle'}
          dominantBaseline="middle"
          fontSize={14}
        >
          {wire.label.text}
        </text>
      ) : null}
    </g>
  )
}

function halfMs(dur: string) {
  return Math.round((Number.parseFloat(dur) * 1000) / 2)
}

function DiagramSvg({ layout, prefix, className }: { layout: Layout; prefix: string; className: string }) {
  return (
    <svg className={className} viewBox={`0 0 ${layout.width} ${layout.height}`} role="presentation">
      {layout.boxes.map((box) => (
        <BoxNode key={box.id} box={box} />
      ))}
      {layout.wires.map((wire) => (
        <WirePath key={wire.id} wire={wire} />
      ))}
      {layout.routes.map((route) => (
        <path key={route.id} id={`${prefix}-${route.id}`} d={roundedPolyline(route.points, 16)} fill="none" stroke="none" />
      ))}
      {layout.routes.flatMap((route) =>
        [0, 1].map((copy) => (
          <circle key={`${route.id}-${copy}`} className="flow-dot" r={route.r} fill="var(--accent)">
            <animateMotion
              dur={route.dur}
              repeatCount="indefinite"
              begin="indefinite"
              data-delay={route.delay + copy * halfMs(route.dur)}
            >
              <mpath href={`#${prefix}-${route.id}`} />
            </animateMotion>
          </circle>
        )),
      )}
    </svg>
  )
}

const desktop: Layout = {
  width: 1160,
  height: 700,
  boxes: [
    { id: 'ui', x: 150, y: 72, w: 128, h: 58, title: 'UI', order: 0, seed: 11 },
    { id: 'agent', x: 300, y: 58, w: 190, h: 84, title: 'Agent', sub: 'Ctrl+K · proposals', order: 1, seed: 17 },
    { id: 'api', x: 512, y: 72, w: 116, h: 58, title: 'API', order: 2, seed: 23 },
    { id: 'mcp', x: 648, y: 72, w: 168, h: 58, title: 'MCP clients', order: 3, seed: 29 },
    { id: 'op', x: 150, y: 176, w: 666, h: 54, title: 'Operation', sub: 'one shared layer', order: 8, seed: 31 },
    { id: 'pg', x: 292, y: 272, w: 300, h: 86, title: 'Postgres', sub: 'object model + change log', order: 10, seed: 37 },
    { id: 'relay', x: 352, y: 392, w: 180, h: 52, title: 'Relay', order: 12, seed: 41 },
    { id: 'panda', x: 292, y: 478, w: 300, h: 68, title: 'Redpanda', order: 14, seed: 43 },
    { id: 'indexer', x: 760, y: 466, w: 150, h: 90, title: 'Indexer', order: 17, seed: 47 },
    { id: 'search', x: 960, y: 454, w: 156, h: 104, title: 'Typesense', sub: 'search', order: 19, seed: 53 },
    {
      id: 'ext',
      x: 700,
      y: 596,
      w: 400,
      h: 76,
      title: 'External consumers',
      sub: 'headless integrations',
      order: 21,
      seed: 59,
    },
  ],
  wires: [
    { id: 'ui-op', points: [{ x: 214, y: 130 }, { x: 214, y: 176 }], order: 4, seed: 101 },
    { id: 'agent-op', points: [{ x: 395, y: 142 }, { x: 395, y: 176 }], order: 5, seed: 107 },
    { id: 'api-op', points: [{ x: 570, y: 130 }, { x: 570, y: 176 }], order: 6, seed: 109 },
    { id: 'mcp-op', points: [{ x: 732, y: 130 }, { x: 732, y: 176 }], order: 7, seed: 113 },
    { id: 'op-pg', points: [{ x: 442, y: 230 }, { x: 442, y: 272 }], order: 9, seed: 127 },
    {
      id: 'pg-relay',
      points: [{ x: 442, y: 358 }, { x: 442, y: 392 }],
      order: 11,
      seed: 131,
      label: { x: 548, y: 418, text: 'logical replication', anchor: 'start' },
    },
    { id: 'relay-panda', points: [{ x: 442, y: 444 }, { x: 442, y: 478 }], order: 13, seed: 137 },
    {
      id: 'fast',
      points: [{ x: 592, y: 498 }, { x: 760, y: 498 }],
      order: 15,
      seed: 139,
      tone: 'accent',
      label: { x: 676, y: 478, text: 'interactive · fast' },
    },
    {
      id: 'bulk',
      points: [{ x: 592, y: 534 }, { x: 760, y: 534 }],
      order: 16,
      seed: 149,
      tone: 'heavy',
      rough: 1.35,
      label: { x: 676, y: 558, text: 'bulk · heavier' },
    },
    { id: 'idx-ts', points: [{ x: 910, y: 506 }, { x: 960, y: 506 }], order: 18, seed: 151 },
    {
      id: 'sub',
      points: [
        { x: 442, y: 546 },
        { x: 442, y: 634 },
        { x: 700, y: 634 },
      ],
      order: 20,
      seed: 157,
      tone: 'accent',
      label: { x: 560, y: 616, text: '/subscribe' },
    },
    {
      id: 'sse',
      points: [
        { x: 292, y: 512 },
        { x: 36, y: 512 },
        { x: 36, y: 101 },
        { x: 150, y: 101 },
      ],
      order: 22,
      seed: 163,
      tone: 'accent',
      radius: 18,
      label: { x: 52, y: 300, text: 'SSE · live updates', anchor: 'start' },
    },
    {
      id: 'loop',
      points: [
        { x: 1038, y: 454 },
        { x: 1038, y: 32 },
        { x: 214, y: 32 },
        { x: 214, y: 72 },
      ],
      order: 23,
      seed: 167,
      tone: 'accent',
      radius: 18,
      label: { x: 760, y: 50, text: 'UI search' },
    },
  ],
  routes: [
    {
      id: 'fast',
      dur: '8s',
      r: 3.4,
      delay: 200,
      points: [
        { x: 214, y: 130 },
        { x: 214, y: 203 },
        { x: 442, y: 203 },
        { x: 442, y: 272 },
        { x: 442, y: 358 },
        { x: 442, y: 392 },
        { x: 442, y: 444 },
        { x: 442, y: 512 },
        { x: 592, y: 498 },
        { x: 760, y: 498 },
        { x: 835, y: 506 },
        { x: 910, y: 506 },
        { x: 1038, y: 506 },
        { x: 1038, y: 454 },
        { x: 1038, y: 32 },
        { x: 214, y: 32 },
        { x: 214, y: 72 },
      ],
    },
    {
      id: 'bulk',
      dur: '13s',
      r: 5.2,
      delay: 600,
      points: [
        { x: 570, y: 130 },
        { x: 570, y: 203 },
        { x: 442, y: 203 },
        { x: 442, y: 272 },
        { x: 442, y: 358 },
        { x: 442, y: 392 },
        { x: 442, y: 444 },
        { x: 442, y: 530 },
        { x: 592, y: 534 },
        { x: 760, y: 534 },
        { x: 835, y: 520 },
        { x: 1038, y: 520 },
        { x: 1038, y: 454 },
        { x: 1038, y: 32 },
        { x: 214, y: 32 },
        { x: 214, y: 100 },
      ],
    },
    {
      id: 'sse',
      dur: '9s',
      r: 3.4,
      delay: 900,
      points: [
        { x: 395, y: 142 },
        { x: 395, y: 203 },
        { x: 442, y: 203 },
        { x: 442, y: 272 },
        { x: 442, y: 358 },
        { x: 442, y: 392 },
        { x: 442, y: 444 },
        { x: 442, y: 512 },
        { x: 292, y: 512 },
        { x: 36, y: 512 },
        { x: 36, y: 101 },
        { x: 150, y: 101 },
      ],
    },
    {
      id: 'sub',
      dur: '10s',
      r: 3.4,
      delay: 400,
      points: [
        { x: 732, y: 130 },
        { x: 732, y: 203 },
        { x: 442, y: 203 },
        { x: 442, y: 272 },
        { x: 442, y: 358 },
        { x: 442, y: 392 },
        { x: 442, y: 444 },
        { x: 442, y: 546 },
        { x: 442, y: 634 },
        { x: 700, y: 634 },
      ],
    },
  ],
}

const mobile: Layout = {
  width: 460,
  height: 820,
  boxes: [
    { id: 'ui', x: 48, y: 36, w: 86, h: 48, title: 'UI', order: 0, seed: 11 },
    { id: 'agent', x: 146, y: 28, w: 112, h: 64, title: 'Agent', sub: 'Ctrl+K', order: 1, seed: 17 },
    { id: 'api', x: 270, y: 36, w: 70, h: 48, title: 'API', order: 2, seed: 23 },
    { id: 'mcp', x: 352, y: 36, w: 72, h: 48, title: 'MCP', order: 3, seed: 29 },
    { id: 'op', x: 48, y: 116, w: 376, h: 48, title: 'Operation', sub: 'one shared layer', order: 8, seed: 31 },
    { id: 'pg', x: 64, y: 188, w: 344, h: 66, title: 'Postgres', sub: 'object model + change log', order: 10, seed: 37 },
    { id: 'relay', x: 104, y: 274, w: 264, h: 48, title: 'Relay', sub: 'logical replication', order: 12, seed: 41 },
    { id: 'panda', x: 64, y: 342, w: 344, h: 54, title: 'Redpanda', order: 14, seed: 43 },
    { id: 'fast', x: 48, y: 422, w: 172, h: 46, title: 'Interactive', sub: 'fast', order: 16, seed: 47 },
    { id: 'bulk', x: 236, y: 422, w: 172, h: 46, title: 'Bulk', sub: 'heavier', order: 17, seed: 53 },
    { id: 'idx', x: 64, y: 494, w: 344, h: 46, title: 'Indexer', order: 19, seed: 59 },
    { id: 'ts', x: 64, y: 560, w: 344, h: 58, title: 'Typesense', sub: 'search', order: 21, seed: 61 },
    {
      id: 'ext',
      x: 64,
      y: 680,
      w: 344,
      h: 68,
      title: 'External consumers',
      sub: '/subscribe',
      order: 23,
      seed: 67,
    },
  ],
  wires: [
    { id: 'ui-op', points: [{ x: 91, y: 84 }, { x: 91, y: 116 }], order: 4, seed: 201 },
    { id: 'agent-op', points: [{ x: 202, y: 92 }, { x: 202, y: 116 }], order: 5, seed: 203 },
    { id: 'api-op', points: [{ x: 305, y: 84 }, { x: 305, y: 116 }], order: 6, seed: 205 },
    { id: 'mcp-op', points: [{ x: 388, y: 84 }, { x: 388, y: 116 }], order: 7, seed: 207 },
    { id: 'op-pg', points: [{ x: 236, y: 164 }, { x: 236, y: 188 }], order: 9, seed: 211 },
    { id: 'pg-relay', points: [{ x: 236, y: 254 }, { x: 236, y: 274 }], order: 11, seed: 213 },
    { id: 'relay-panda', points: [{ x: 236, y: 322 }, { x: 236, y: 342 }], order: 13, seed: 217 },
    { id: 'panda-fast', points: [{ x: 160, y: 396 }, { x: 134, y: 422 }], order: 15, seed: 219, tone: 'accent' },
    { id: 'panda-bulk', points: [{ x: 312, y: 396 }, { x: 322, y: 422 }], order: 15, seed: 223, tone: 'heavy', rough: 1.35 },
    { id: 'fast-idx', points: [{ x: 134, y: 468 }, { x: 180, y: 494 }], order: 18, seed: 227, tone: 'accent' },
    { id: 'bulk-idx', points: [{ x: 322, y: 468 }, { x: 292, y: 494 }], order: 18, seed: 229, tone: 'heavy' },
    { id: 'idx-ts', points: [{ x: 236, y: 540 }, { x: 236, y: 560 }], order: 20, seed: 233 },
    {
      id: 'sub',
      points: [
        { x: 408, y: 369 },
        { x: 432, y: 369 },
        { x: 432, y: 714 },
        { x: 408, y: 714 },
      ],
      order: 22,
      seed: 239,
      tone: 'accent',
      radius: 16,
    },
    {
      id: 'sse',
      points: [
        { x: 64, y: 369 },
        { x: 18, y: 369 },
        { x: 18, y: 60 },
        { x: 48, y: 60 },
      ],
      order: 24,
      seed: 241,
      tone: 'accent',
      radius: 14,
      label: { x: 8, y: 150, text: 'SSE', anchor: 'start' },
    },
    {
      id: 'loop',
      points: [
        { x: 408, y: 589 },
        { x: 448, y: 589 },
        { x: 448, y: 18 },
        { x: 91, y: 18 },
        { x: 91, y: 36 },
      ],
      order: 25,
      seed: 251,
      tone: 'accent',
      radius: 16,
    },
  ],
  routes: [
    {
      id: 'fast',
      dur: '8s',
      r: 3.2,
      delay: 200,
      points: [
        { x: 91, y: 84 },
        { x: 91, y: 140 },
        { x: 236, y: 140 },
        { x: 236, y: 188 },
        { x: 236, y: 254 },
        { x: 236, y: 274 },
        { x: 236, y: 322 },
        { x: 236, y: 369 },
        { x: 134, y: 422 },
        { x: 134, y: 468 },
        { x: 180, y: 494 },
        { x: 236, y: 517 },
        { x: 236, y: 540 },
        { x: 236, y: 589 },
        { x: 408, y: 589 },
        { x: 448, y: 589 },
        { x: 448, y: 18 },
        { x: 91, y: 18 },
        { x: 91, y: 36 },
      ],
    },
    {
      id: 'bulk',
      dur: '13s',
      r: 4.8,
      delay: 700,
      points: [
        { x: 305, y: 84 },
        { x: 305, y: 140 },
        { x: 236, y: 140 },
        { x: 236, y: 188 },
        { x: 236, y: 254 },
        { x: 236, y: 322 },
        { x: 236, y: 396 },
        { x: 322, y: 422 },
        { x: 322, y: 468 },
        { x: 292, y: 494 },
        { x: 236, y: 540 },
        { x: 236, y: 618 },
      ],
    },
    {
      id: 'sse',
      dur: '9s',
      r: 3.2,
      delay: 500,
      points: [
        { x: 202, y: 92 },
        { x: 202, y: 140 },
        { x: 236, y: 140 },
        { x: 236, y: 369 },
        { x: 64, y: 369 },
        { x: 18, y: 369 },
        { x: 18, y: 60 },
        { x: 48, y: 60 },
      ],
    },
    {
      id: 'sub',
      dur: '10s',
      r: 3.2,
      delay: 300,
      points: [
        { x: 388, y: 84 },
        { x: 388, y: 140 },
        { x: 236, y: 140 },
        { x: 236, y: 369 },
        { x: 408, y: 369 },
        { x: 432, y: 369 },
        { x: 432, y: 714 },
        { x: 408, y: 714 },
      ],
    },
  ],
}

export function DataFlow() {
  return (
    <div data-flow>
      <DiagramSvg layout={desktop} prefix="wide" className="flow-wide" />
      <DiagramSvg layout={mobile} prefix="narrow" className="flow-narrow" />
    </div>
  )
}
