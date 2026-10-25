export type Pt = { x: number; y: number }

function fmt(n: number) {
  return n.toFixed(1)
}

function corner(prev: Pt, cur: Pt, next: Pt, radius: number) {
  const inLen = Math.hypot(cur.x - prev.x, cur.y - prev.y) || 1
  const outLen = Math.hypot(next.x - cur.x, next.y - cur.y) || 1
  const r = Math.min(radius, inLen / 2.2, outLen / 2.2)
  return {
    a: { x: cur.x - ((cur.x - prev.x) / inLen) * r, y: cur.y - ((cur.y - prev.y) / inLen) * r },
    b: { x: cur.x + ((next.x - cur.x) / outLen) * r, y: cur.y + ((next.y - cur.y) / outLen) * r },
    c: cur,
  }
}

export function roundedPolyline(points: Pt[], radius = 14) {
  if (points.length === 0) return ''
  if (points.length === 1) return `M ${fmt(points[0].x)} ${fmt(points[0].y)}`
  let d = `M ${fmt(points[0].x)} ${fmt(points[0].y)}`
  for (let i = 1; i < points.length - 1; i++) {
    const { a, b, c } = corner(points[i - 1], points[i], points[i + 1], radius)
    d += ` L ${fmt(a.x)} ${fmt(a.y)} Q ${fmt(c.x)} ${fmt(c.y)} ${fmt(b.x)} ${fmt(b.y)}`
  }
  const last = points[points.length - 1]
  d += ` L ${fmt(last.x)} ${fmt(last.y)}`
  return d
}

export function roundRectPath(x: number, y: number, w: number, h: number, r: number) {
  const rr = Math.min(r, w / 2, h / 2)
  return [
    `M ${fmt(x + rr)} ${fmt(y)}`,
    `H ${fmt(x + w - rr)}`,
    `Q ${fmt(x + w)} ${fmt(y)} ${fmt(x + w)} ${fmt(y + rr)}`,
    `V ${fmt(y + h - rr)}`,
    `Q ${fmt(x + w)} ${fmt(y + h)} ${fmt(x + w - rr)} ${fmt(y + h)}`,
    `H ${fmt(x + rr)}`,
    `Q ${fmt(x)} ${fmt(y + h)} ${fmt(x)} ${fmt(y + h - rr)}`,
    `V ${fmt(y + rr)}`,
    `Q ${fmt(x)} ${fmt(y)} ${fmt(x + rr)} ${fmt(y)}`,
    'Z',
  ].join(' ')
}

export function arrowHead(x1: number, y1: number, x2: number, y2: number) {
  const ang = Math.atan2(y2 - y1, x2 - x1)
  const len = 7
  const spread = 0.42
  const left = ang + Math.PI - spread
  const right = ang + Math.PI + spread
  const lx = x2 + Math.cos(left) * len
  const ly = y2 + Math.sin(left) * len
  const rx = x2 + Math.cos(right) * len
  const ry = y2 + Math.sin(right) * len
  return `M ${fmt(lx)} ${fmt(ly)} L ${fmt(x2)} ${fmt(y2)} L ${fmt(rx)} ${fmt(ry)}`
}
