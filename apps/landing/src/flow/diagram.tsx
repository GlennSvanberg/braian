import type { CSSProperties } from 'react'
import { arrowHead, roundedPolyline, roundRectPath, type Pt } from '@/flow/sketch'

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
      <path
        className="flow-node"
        data-draw
        d={roundRectPath(box.x, box.y, box.w, box.h, 14)}
        style={drawStyle(box.order)}
      />
      <text x={cx} y={box.sub ? cy - 8 : cy + 1} textAnchor="middle" dominantBaseline="middle" fontSize={16}>
        {box.title}
      </text>
      {box.sub ? (
        <text className="flow-sub" x={cx} y={cy + 12} textAnchor="middle" dominantBaseline="middle" fontSize={12}>
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
  const d = `${roundedPolyline(pts, wire.radius ?? 14)} ${arrowHead(prev.x, prev.y, end.x, end.y)}`
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
    { id: 'agent', x: 300, y: 58, w: 190, h: 84, title: 'Agent', sub: 'Ctrl+K · suggestions', order: 1, seed: 17 },
    { id: 'api', x: 512, y: 72, w: 116, h: 58, title: 'API', order: 2, seed: 23 },
    { id: 'mcp', x: 648, y: 72, w: 168, h: 58, title: 'MCP clients', order: 3, seed: 29 },
    { id: 'op', x: 150, y: 176, w: 666, h: 54, title: 'Operation registry', sub: 'one shared layer', order: 8, seed: 31 },
    { id: 'pg', x: 292, y: 272, w: 300, h: 86, title: 'Postgres', sub: 'object model + change log', order: 10, seed: 37 },
    { id: 'indexer', x: 760, y: 466, w: 150, h: 90, title: 'Indexer', sub: 'interactive · bulk', order: 14, seed: 47 },
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
      id: 'pg-idx',
      points: [
        { x: 592, y: 315 },
        { x: 680, y: 315 },
        { x: 680, y: 466 },
        { x: 760, y: 466 },
      ],
      order: 11,
      seed: 131,
      label: { x: 608, y: 296, text: 'changes by seq', anchor: 'start' },
    },
    {
      id: 'fast',
      points: [{ x: 442, y: 358 }, { x: 442, y: 420 }, { x: 760, y: 420 }, { x: 760, y: 466 }],
      order: 12,
      seed: 139,
      tone: 'accent',
      label: { x: 548, y: 404, text: 'interactive · fast', anchor: 'start' },
    },
    {
      id: 'bulk',
      points: [{ x: 500, y: 358 }, { x: 500, y: 520 }, { x: 760, y: 520 }, { x: 760, y: 510 }],
      order: 13,
      seed: 149,
      tone: 'heavy',
      rough: 1.35,
      label: { x: 548, y: 544, text: 'bulk · heavier', anchor: 'start' },
    },
    { id: 'idx-ts', points: [{ x: 910, y: 506 }, { x: 960, y: 506 }], order: 18, seed: 151 },
    {
      id: 'sub',
      points: [
        { x: 442, y: 358 },
        { x: 442, y: 634 },
        { x: 700, y: 634 },
      ],
      order: 20,
      seed: 157,
      tone: 'accent',
      label: { x: 560, y: 616, text: 'stream · webhooks' },
    },
    {
      id: 'sse',
      points: [
        { x: 292, y: 330 },
        { x: 36, y: 330 },
        { x: 36, y: 101 },
        { x: 150, y: 101 },
      ],
      order: 22,
      seed: 163,
      tone: 'accent',
      radius: 18,
      label: { x: 52, y: 280, text: 'LISTEN · SSE', anchor: 'start' },
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
        { x: 442, y: 420 },
        { x: 760, y: 420 },
        { x: 760, y: 466 },
        { x: 760, y: 498 },
        { x: 835, y: 498 },
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
        { x: 500, y: 358 },
        { x: 500, y: 520 },
        { x: 760, y: 520 },
        { x: 760, y: 534 },
        { x: 1038, y: 534 },
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
        { x: 292, y: 358 },
        { x: 292, y: 330 },
        { x: 36, y: 330 },
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
        { x: 442, y: 634 },
        { x: 700, y: 634 },
      ],
    },
  ],
}

const mobile: Layout = {
  width: 460,
  height: 540,
  boxes: [
    { id: 'ui', x: 48, y: 36, w: 86, h: 48, title: 'UI', order: 0, seed: 11 },
    { id: 'agent', x: 146, y: 28, w: 112, h: 64, title: 'Agent', sub: 'Ctrl+K', order: 1, seed: 17 },
    { id: 'api', x: 270, y: 36, w: 70, h: 48, title: 'API', order: 2, seed: 23 },
    { id: 'mcp', x: 352, y: 36, w: 72, h: 48, title: 'MCP', order: 3, seed: 29 },
    { id: 'op', x: 48, y: 116, w: 376, h: 48, title: 'Operation registry', sub: 'one shared layer', order: 8, seed: 31 },
    { id: 'pg', x: 64, y: 188, w: 344, h: 66, title: 'Postgres', sub: 'object model + change log', order: 10, seed: 37 },
    { id: 'idx', x: 64, y: 274, w: 344, h: 54, title: 'Indexer', sub: 'interactive · bulk', order: 12, seed: 59 },
    { id: 'ts', x: 64, y: 350, w: 344, h: 58, title: 'Typesense', sub: 'search', order: 14, seed: 61 },
    {
      id: 'ext',
      x: 64,
      y: 440,
      w: 344,
      h: 68,
      title: 'External consumers',
      sub: 'stream · webhooks',
      order: 16,
      seed: 67,
    },
  ],
  wires: [
    { id: 'ui-op', points: [{ x: 91, y: 84 }, { x: 91, y: 116 }], order: 4, seed: 201 },
    { id: 'agent-op', points: [{ x: 202, y: 92 }, { x: 202, y: 116 }], order: 5, seed: 203 },
    { id: 'api-op', points: [{ x: 305, y: 84 }, { x: 305, y: 116 }], order: 6, seed: 205 },
    { id: 'mcp-op', points: [{ x: 388, y: 84 }, { x: 388, y: 116 }], order: 7, seed: 207 },
    { id: 'op-pg', points: [{ x: 236, y: 164 }, { x: 236, y: 188 }], order: 9, seed: 211 },
    {
      id: 'pg-idx',
      points: [{ x: 236, y: 254 }, { x: 236, y: 274 }],
      order: 11,
      seed: 213,
      label: { x: 248, y: 264, text: 'changes by seq', anchor: 'start' },
    },
    { id: 'idx-ts', points: [{ x: 236, y: 328 }, { x: 236, y: 350 }], order: 13, seed: 233 },
    {
      id: 'sub',
      points: [
        { x: 408, y: 221 },
        { x: 432, y: 221 },
        { x: 432, y: 474 },
        { x: 408, y: 474 },
      ],
      order: 15,
      seed: 239,
      tone: 'accent',
      radius: 16,
    },
    {
      id: 'sse',
      points: [
        { x: 64, y: 221 },
        { x: 18, y: 221 },
        { x: 18, y: 60 },
        { x: 48, y: 60 },
      ],
      order: 17,
      seed: 241,
      tone: 'accent',
      radius: 14,
      label: { x: 8, y: 205, text: 'LISTEN · SSE', anchor: 'start' },
    },
    {
      id: 'loop',
      points: [
        { x: 408, y: 379 },
        { x: 448, y: 379 },
        { x: 448, y: 18 },
        { x: 91, y: 18 },
        { x: 91, y: 36 },
      ],
      order: 18,
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
        { x: 236, y: 328 },
        { x: 236, y: 350 },
        { x: 236, y: 379 },
        { x: 408, y: 379 },
        { x: 448, y: 379 },
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
        { x: 236, y: 274 },
        { x: 236, y: 328 },
        { x: 236, y: 350 },
        { x: 236, y: 408 },
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
        { x: 236, y: 221 },
        { x: 64, y: 221 },
        { x: 18, y: 221 },
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
        { x: 236, y: 221 },
        { x: 408, y: 221 },
        { x: 432, y: 221 },
        { x: 432, y: 474 },
        { x: 408, y: 474 },
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
