export type ClientLogEntry = {
  at: number
  kind: 'console' | 'api' | 'navigation' | 'action'
  level?: 'error' | 'warn'
  message: string
  requestId?: string
  method?: string
  route?: string
  status?: number
  durationMs?: number
}

const MAX_EVENTS = 200
const MAX_AGE_MS = 5 * 60 * 1000
const SLOW_API_MS = 2_000

const buffer: ClientLogEntry[] = []
let installed = false

const secretKey = /^(token|access_token|refresh_token|secret|password|auth|session|api[_-]?key|code)$/i
const bearer = /Bearer\s+[A-Za-z0-9._~+/=-]+/gi

export function sanitizeText(raw: string): string {
  return raw
    .replace(bearer, 'Bearer [redacted]')
    .replace(/(authorization|cookie)\s*[:=]\s*[^\s;,]+/gi, '$1=[redacted]')
}

export function sanitizeRoute(path: string): string {
  const base = path.split('?')[0] ?? path
  const query = path.includes('?') ? path.slice(path.indexOf('?') + 1) : ''
  if (!query) return sanitizeText(base)
  const params = new URLSearchParams(query)
  for (const key of [...params.keys()]) {
    if (secretKey.test(key) || /token|secret|password|auth/i.test(key)) {
      params.set(key, '[redacted]')
    }
  }
  const next = params.toString()
  return sanitizeText(next ? `${base}?${next}` : base)
}

function trimBuffer() {
  const cutoff = Date.now() - MAX_AGE_MS
  while (buffer.length > 0 && (buffer.length > MAX_EVENTS || buffer[0].at < cutoff)) {
    buffer.shift()
  }
}

function push(entry: ClientLogEntry) {
  buffer.push(entry)
  trimBuffer()
}

export function recordAction(message: string) {
  push({ at: Date.now(), kind: 'action', message: sanitizeText(message) })
}

export function recordNavigation(route: string) {
  push({ at: Date.now(), kind: 'navigation', message: sanitizeRoute(route) })
}

export function recordApi(event: {
  method: string
  route: string
  status: number
  durationMs: number
  requestId?: string
}) {
  const failed = event.status >= 400
  const slow = event.durationMs >= SLOW_API_MS
  if (!failed && !slow) return
  push({
    at: Date.now(),
    kind: 'api',
    message: `${event.method} ${sanitizeRoute(event.route)} ${event.status} ${event.durationMs}ms`,
    method: event.method,
    route: sanitizeRoute(event.route),
    status: event.status,
    durationMs: event.durationMs,
    requestId: event.requestId,
  })
}

export function snapshotClientLog(): ClientLogEntry[] {
  trimBuffer()
  return buffer.map((entry) => ({ ...entry }))
}

export function formatLogPreview(entries: ClientLogEntry[]): string {
  if (entries.length === 0) return 'Recent log: (empty)'
  return entries
    .slice(-40)
    .map((entry) => {
      const stamp = new Date(entry.at).toISOString().slice(11, 19)
      const id = entry.requestId ? ` id=${entry.requestId}` : ''
      return `${stamp} ${entry.kind}${entry.level ? `:${entry.level}` : ''} ${entry.message}${id}`
    })
    .join('\n')
}

function requestIdFrom(headers: Headers): string | undefined {
  for (const name of ['x-request-id', 'request-id', 'x-braian-request-id', 'traceparent']) {
    const value = headers.get(name)
    if (value) return sanitizeText(value)
  }
  return undefined
}

export function installClientLog() {
  if (installed || typeof window === 'undefined') return
  installed = true
  recordNavigation(sanitizeRoute(`${window.location.pathname}${window.location.search}${window.location.hash}`))

  const wrapConsole = (level: 'error' | 'warn', original: (...args: unknown[]) => void) => {
    return (...args: unknown[]) => {
      const message = sanitizeText(args.map((item) => String(item)).join(' ').slice(0, 500))
      push({ at: Date.now(), kind: 'console', level, message })
      original(...args)
    }
  }
  console.error = wrapConsole('error', console.error.bind(console))
  console.warn = wrapConsole('warn', console.warn.bind(console))

  const { pushState, replaceState } = history
  history.pushState = function pushStatePatched(state, title, url) {
    if (typeof url === 'string') recordNavigation(url)
    return pushState.call(this, state, title, url)
  }
  history.replaceState = function replaceStatePatched(state, title, url) {
    if (typeof url === 'string') recordNavigation(url)
    return replaceState.call(this, state, title, url)
  }
  window.addEventListener('popstate', () => {
    recordNavigation(`${window.location.pathname}${window.location.search}${window.location.hash}`)
  })

  import('./ui-bus').then(({ onUi }) => {
    onUi((event) => {
      if (event.name.startsWith('ui.')) {
        recordAction(`ui ${event.name}`)
      }
    })
  })
}

export function noteApiResponse(method: string, path: string, started: number, response: Response) {
  recordApi({
    method,
    route: path,
    status: response.status,
    durationMs: Math.round(performance.now() - started),
    requestId: requestIdFrom(response.headers),
  })
}
