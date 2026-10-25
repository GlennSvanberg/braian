export function humanizeLive(raw: string): string {
  const text = raw.trim()
  if (!text.startsWith('{') && !text.startsWith('[')) {
    return text.includes('live') ? text : `live · ${text}`
  }
  try {
    const event = JSON.parse(text) as Record<string, unknown>
    const op = typeof event.op === 'string' ? event.op : 'change'
    const key = typeof event.key === 'string' ? event.key : ''
    const kind = typeof event.type === 'string' ? event.type : ''
    if (op === 'create') return `live · Created ${kind || 'object'}${key ? ` ${key}` : ''}`
    if (op === 'update') return `live · Updated ${key || kind || 'an object'}`
    if (op === 'delete') return `live · Removed ${key || 'an object'}`
    if (op === 'bulk_summary') {
      const count = typeof event.count === 'number' ? event.count : 0
      return `live · Bulk update (${count})`
    }
    if (op === 'lagged') {
      const skipped = typeof event.skipped === 'number' ? event.skipped : 0
      return `live · Skipped ${skipped} events`
    }
    if (op === 'job') {
      const status = typeof event.status === 'string' ? event.status : 'running'
      const done = typeof event.done === 'number' ? event.done : 0
      const total = typeof event.total === 'number' ? event.total : 0
      return `live · Job ${status} ${done}/${total}`
    }
    return `live · ${op}${key ? ` ${key}` : ''}`
  } catch {
    return `live · ${text}`
  }
}

export function objectIdFromLive(raw: string): string | null {
  try {
    const event = JSON.parse(raw) as Record<string, unknown>
    const id = event.object_id ?? event.id
    return typeof id === 'string' ? id : null
  } catch {
    return null
  }
}
