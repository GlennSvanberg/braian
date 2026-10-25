import { api, type Json } from './api'
import { apply, canonicalTaskType, type ActorKind, type Cursor, type Explicit } from './tasks'

const SESSION_KEY = 'braian.telemetry.session'
const FLUSH_MS = 4_000
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i

type Result = 'ok' | 'error' | 'cancelled'

type Pending = {
  id: string
  occurred_at: string
  actor_type: ActorKind
  task_id: string
  task_type: string
  session_id: string
  operation: string
  object_ids: string[]
  object_count: number
  result: Result
  duration_ms?: number
  ambiguous: boolean
}

type RecordInput = {
  operation: string
  actor?: ActorKind
  objectIds?: string[]
  objectCount?: number
  result?: Result
  durationMs?: number
  explicit?: Explicit
}

let cursor: Cursor | null = null
let gapMs = 300_000
let buffer: Pending[] = []
let armed: Explicit | undefined
let started = false

export function armExplicit(kind: Explicit) {
  armed = kind
}

export function objectIdsFrom(operation: string, args: Json): string[] {
  if (operation.startsWith('ui.') && operation !== 'ui.select' && operation !== 'ui.select_rows') return []
  const out: string[] = []
  const push = (value: unknown) => {
    if (typeof value !== 'string' || !UUID.test(value) || out.includes(value) || out.length >= 64) return
    out.push(value)
  }
  push(args.id)
  push(args.object_id)
  for (const key of ['ids', 'object_ids']) {
    const list = args[key]
    if (!Array.isArray(list)) continue
    for (const item of list) push(item)
  }
  return out
}

/** Fire-and-forget. A failure drops the event. */
export function recordOperation(input: RecordInput) {
  try {
    const operation = input.operation.trim()
    if (!/^[A-Za-z][A-Za-z0-9._-]{0,63}$/.test(operation)) return
    const explicit = input.explicit ?? takeArmed()
    const objectIds = (input.objectIds ?? []).filter((id) => UUID.test(id)).slice(0, 64)
    const at = Date.now()
    const next = apply(
      cursor,
      { atMs: at, operation, actor: input.actor ?? 'human', objectIds, explicit },
      gapMs,
      () => crypto.randomUUID(),
    )
    cursor = next.cursor
    const event: Pending = {
      id: crypto.randomUUID(),
      occurred_at: new Date(at).toISOString(),
      actor_type: input.actor ?? 'human',
      task_id: next.cursor.taskId,
      task_type: canonicalTaskType(next.cursor.taskType),
      session_id: sessionId(),
      operation,
      object_ids: objectIds,
      object_count: Math.max(input.objectCount ?? objectIds.length, objectIds.length),
      result: input.result ?? 'ok',
      ambiguous: next.ambiguous,
    }
    if (input.durationMs != null && input.durationMs >= 0 && input.durationMs <= 86_400_000) {
      event.duration_ms = Math.round(input.durationMs)
    }
    buffer.push(event)
    if (buffer.length >= 50) flush()
  } catch {
    buffer = buffer.slice(-100)
  }
}

export function recordFromUi(name: string, args: Json) {
  recordOperation({ operation: name, objectIds: objectIdsFrom(name, args) })
}

export function startTelemetry() {
  if (started || typeof window === 'undefined') return
  started = true
  void loadGap()
  window.setInterval(() => flush(), FLUSH_MS)
  const idle = () => {
    if ('requestIdleCallback' in window) window.requestIdleCallback(() => flush())
    else flush()
  }
  window.addEventListener('pagehide', () => beacon())
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'hidden') beacon()
    else idle()
  })
}

export function flush() {
  const events = take()
  if (events.length === 0) return
  void api('/v1/telemetry', {
    method: 'POST',
    body: JSON.stringify({ events }),
    keepalive: true,
  }).catch(() => undefined)
}

function beacon() {
  const events = take()
  if (events.length === 0) return
  try {
    const blob = new Blob([JSON.stringify({ events })], { type: 'application/json' })
    const sent = navigator.sendBeacon?.('/v1/telemetry', blob) ?? false
    if (!sent) {
      void fetch('/v1/telemetry', {
        method: 'POST',
        credentials: 'include',
        keepalive: true,
        headers: { 'content-type': 'application/json', 'x-braian-telemetry': 'client' },
        body: JSON.stringify({ events }),
      }).catch(() => undefined)
    }
  } catch {
    /* drop */
  }
}

function take(): Pending[] {
  const events = buffer.slice(0, 100)
  buffer = buffer.slice(events.length)
  return events
}

function takeArmed(): Explicit | undefined {
  const value = armed
  armed = undefined
  return value
}

function sessionId(): string {
  try {
    const stored = sessionStorage.getItem(SESSION_KEY)
    if (stored && UUID.test(stored)) return stored
    const fresh = crypto.randomUUID()
    sessionStorage.setItem(SESSION_KEY, fresh)
    return fresh
  } catch {
    return crypto.randomUUID()
  }
}

async function loadGap() {
  try {
    const settings = await api<Json>('/v1/usage/settings')
    const seconds = Number(settings.inactivity_gap_seconds)
    if (seconds >= 30 && seconds <= 86_400) gapMs = seconds * 1000
  } catch {
    /* keep the default */
  }
}
