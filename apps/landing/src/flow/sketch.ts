export type Pt = { x: number; y: number }

function rng(seed: number) {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function fmt(n: number) {
  return n.toFixed(1)
}

export function sketchLine(x1: number, y1: number, x2: number, y2: number, seed: number, rough = 1.1) {
  const rand = rng(seed)
  const dx = x2 - x1
  const dy = y2 - y1
  const len = Math.hypot(dx, dy) || 1
  const nx = -dy / len
  const ny = dx / len
  const steps = Math.max(1, Math.round(len / 34))
  let d = `M ${fmt(x1)} ${fmt(y1)}`
  for (let i = 1; i <= steps; i++) {
    const t = i / steps
    const end = i === steps
    const j = end ? 0 : (rand() - 0.5) * 2 * rough
    d += ` L ${fmt(x1 + dx * t + nx * j)} ${fmt(y1 + dy * t + ny * j)}`
  }
  return d
}

export function sketchRect(x: number, y: number, w: number, h: number, seed: number) {
  const rand = rng(seed)
  const j = () => (rand() - 0.5) * 1.8
  const tl = { x: x + j(), y: y + j() }
  const tr = { x: x + w + j(), y: y + j() }
  const br = { x: x + w + j(), y: y + h + j() }
  const bl = { x: x + j(), y: y + h + j() }
  return [
    sketchLine(tl.x, tl.y, tr.x, tr.y, seed + 1, 0.85),
    sketchLine(tr.x, tr.y, br.x, br.y, seed + 2, 0.85),
    sketchLine(br.x, br.y, bl.x, bl.y, seed + 3, 0.85),
    sketchLine(bl.x, bl.y, tl.x, tl.y, seed + 4, 0.85),
  ].join(' ')
}

export function arrowHead(x1: number, y1: number, x2: number, y2: number, seed: number) {
  const ang = Math.atan2(y2 - y1, x2 - x1)
  const len = 9
  const spread = 0.45
  const left = ang + Math.PI - spread
  const right = ang + Math.PI + spread
  return [
    sketchLine(x2, y2, x2 + Math.cos(left) * len, y2 + Math.sin(left) * len, seed, 0.35),
    sketchLine(x2, y2, x2 + Math.cos(right) * len, y2 + Math.sin(right) * len, seed + 1, 0.35),
  ].join(' ')
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

function quad(p0: Pt, p1: Pt, p2: Pt, n: number): Pt[] {
  const out: Pt[] = []
  for (let i = 1; i <= n; i++) {
    const t = i / n
    const u = 1 - t
    out.push({
      x: u * u * p0.x + 2 * u * t * p1.x + t * t * p2.x,
      y: u * u * p0.y + 2 * u * t * p1.y + t * t * p2.y,
    })
  }
  return out
}

function lineSamples(a: Pt, b: Pt, step: number): Pt[] {
  const len = Math.hypot(b.x - a.x, b.y - a.y)
  const n = Math.max(1, Math.round(len / step))
  const out: Pt[] = []
  for (let i = 1; i <= n; i++) {
    const t = i / n
    out.push({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t })
  }
  return out
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

export function sketchRoute(points: Pt[], seed: number, radius = 14, rough = 1.15) {
  if (points.length < 2) return ''
  const rand = rng(seed)
  const samples: Pt[] = [points[0]]
  for (let i = 1; i < points.length - 1; i++) {
    const { a, b, c } = corner(points[i - 1], points[i], points[i + 1], radius)
    samples.push(...lineSamples(samples[samples.length - 1], a, 28))
    samples.push(...quad(a, c, b, 4))
  }
  samples.push(...lineSamples(samples[samples.length - 1], points[points.length - 1], 28))
  let d = `M ${fmt(samples[0].x)} ${fmt(samples[0].y)}`
  for (let i = 1; i < samples.length; i++) {
    const end = i === samples.length - 1
    const prev = samples[i - 1]
    const cur = samples[i]
    const len = Math.hypot(cur.x - prev.x, cur.y - prev.y) || 1
    const j = end ? 0 : (rand() - 0.5) * 2 * rough
    d += ` L ${fmt(cur.x + (-(cur.y - prev.y) / len) * j)} ${fmt(cur.y + ((cur.x - prev.x) / len) * j)}`
  }
  return d
}
