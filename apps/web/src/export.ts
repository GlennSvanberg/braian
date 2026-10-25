import { api } from './api'

export type ExportColumnInput = { id: string; header: string }

export type ExportRequest = {
  columns: { id: string; header: string }[]
  lang: string
  sort?: string
  q?: string
  filter?: string
  ids?: string[]
}

/** Columns, filters, and selection the table is showing. Always xlsx; no format field. */
export function exportPayload(input: {
  columns: ExportColumnInput[]
  hidden: string[]
  order: string[]
  q: string
  filter: string
  sort: string
  ids: string[]
  lang: string
}): ExportRequest {
  const known = new Set(input.columns.map((column) => column.id))
  const ordered = input.order.filter((id) => known.has(id))
  const shown = (ordered.length > 0 ? ordered : input.columns.map((column) => column.id)).filter(
    (id) => !input.hidden.includes(id),
  )
  const headers = new Map(input.columns.map((column) => [column.id, column.header]))
  const body: ExportRequest = {
    columns: shown.map((id) => ({ id, header: headers.get(id) || id })),
    lang: input.lang.trim() || 'sv',
  }
  if (input.sort.trim()) body.sort = input.sort.trim()
  if (input.ids.length > 0) {
    body.ids = input.ids
    return body
  }
  const q = input.q.trim()
  if (q && q !== '*') body.q = q
  const filter = input.filter.trim()
  if (filter) body.filter = filter
  return body
}

type JobBody = { mode?: string; id?: string; status?: string; total?: number }
type JobRun = { status?: string; log?: string }

const POLL_MS = 2_000
const TIMEOUT_MS = 30 * 60 * 1000

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/** Inline file, or a job whose progress arrives on `/v1/stream` and whose file is downloaded after. */
export async function runExport(body: ExportRequest): Promise<Blob> {
  const result = await api<Blob | JobBody>('/v1/export', {
    method: 'POST',
    body: JSON.stringify(body),
  })
  if (result instanceof Blob) return result
  const id = result.id
  if (!id) throw new Error('Export did not start')
  await waitForJob(id)
  return api<Blob>(`/v1/exports/${id}`)
}

async function waitForJob(id: string): Promise<void> {
  const started = Date.now()
  const source = new EventSource('/v1/stream', { withCredentials: true })
  let sse: 'succeeded' | 'failed' | null = null
  const onMessage = (event: MessageEvent) => {
    try {
      const body = JSON.parse(String(event.data)) as { op?: string; id?: string; status?: string }
      if (body.op !== 'job' || body.id !== id) return
      if (body.status === 'succeeded' || body.status === 'failed') sse = body.status
    } catch {
      /* activity lines that are not JSON */
    }
  }
  source.addEventListener('message', onMessage)
  try {
    for (;;) {
      if (Date.now() - started > TIMEOUT_MS) throw new Error('Export timed out')
      if (sse === 'failed') throw new Error('Export failed')
      const job = await api<JobRun>(`/v1/jobs/${id}`)
      if (job.status === 'succeeded' || sse === 'succeeded') return
      if (job.status === 'failed') throw new Error(job.log || 'Export failed')
      await sleep(POLL_MS)
    }
  } finally {
    source.removeEventListener('message', onMessage)
    source.close()
  }
}

export function saveWorkbook(blob: Blob, filename = 'braian.xlsx') {
  const url = URL.createObjectURL(blob)
  const link = document.createElement('a')
  link.href = url
  link.download = filename
  link.click()
  URL.revokeObjectURL(url)
}
