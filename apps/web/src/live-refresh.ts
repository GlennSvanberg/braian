import { objectIdFromLive } from './activity'

/** Debounce before refreshing a search list after an interactive edit. */
export const INTERACTIVE_LIST_MS = 500
/** How long a bulk burst must stay quiet before the list refreshes. */
export const BULK_QUIET_MS = 2_000
/** Longest a continuous bulk may go without one list refresh. */
export const BULK_MAX_MS = 8_000

export type LiveClass = 'interactive' | 'bulk' | 'ignore'

export function classifyLive(raw: string): LiveClass {
  try {
    const event = JSON.parse(raw) as Record<string, unknown>
    const op = typeof event.op === 'string' ? event.op : ''
    if (op === 'bulk' || op === 'bulk_summary') return 'bulk'
    if (op === 'create' || op === 'update' || op === 'delete' || op === 'patch') return 'interactive'
    return 'ignore'
  } catch {
    return 'ignore'
  }
}

/**
 * Search refreshes follow the live stream. Interactive edits refresh that
 * object at once and the list after a short debounce. Bulk summaries do not
 * refetch search on every event: they wait for a quiet gap, and at least
 * once per {@link BULK_MAX_MS} during a long import.
 */
export class SearchRefresh {
  private listTimer: ReturnType<typeof setTimeout> | undefined
  private bulkTimer: ReturnType<typeof setTimeout> | undefined
  private bulkSince = 0

  constructor(
    private readonly refreshList: () => void,
    private readonly refreshObject: (id: string) => void,
  ) {}

  onEvent(raw: string, now = Date.now()) {
    const kind = classifyLive(raw)
    if (kind === 'interactive') {
      const id = objectIdFromLive(raw)
      if (id) this.refreshObject(id)
      this.scheduleList(INTERACTIVE_LIST_MS)
      return
    }
    if (kind === 'bulk') {
      if (this.bulkSince === 0) this.bulkSince = now
      const remaining = BULK_MAX_MS - (now - this.bulkSince)
      this.scheduleBulk(Math.max(0, Math.min(BULK_QUIET_MS, remaining)))
    }
  }

  dispose() {
    clearTimeout(this.listTimer)
    clearTimeout(this.bulkTimer)
    this.listTimer = undefined
    this.bulkTimer = undefined
  }

  private scheduleList(delay: number) {
    clearTimeout(this.listTimer)
    this.listTimer = setTimeout(() => {
      this.listTimer = undefined
      this.refreshList()
    }, delay)
  }

  private scheduleBulk(delay: number) {
    clearTimeout(this.bulkTimer)
    this.bulkTimer = setTimeout(() => {
      this.bulkTimer = undefined
      this.bulkSince = 0
      this.refreshList()
    }, delay)
  }
}
