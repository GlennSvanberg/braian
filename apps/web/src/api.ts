export class ApiError extends Error {
  status: number
  body: unknown

  constructor(status: number, detail: string, body: unknown) {
    super(detail)
    this.status = status
    this.body = body
  }
}

export async function api<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  if (init.body && !headers.has('content-type')) {
    headers.set('content-type', 'application/json')
  }
  const method = (init.method ?? 'GET').toUpperCase()
  const started = performance.now()
  const response = await fetch(path, { ...init, headers, credentials: 'include' })
  if (typeof window !== 'undefined') {
    void import('./feedback-log').then(({ noteApiResponse }) => {
      noteApiResponse(method, path, started, response)
    })
  }
  if (response.status === 204) return undefined as T
  const type = response.headers.get('content-type') ?? ''
  if (!type.includes('json')) {
    if (!response.ok) throw new ApiError(response.status, response.statusText, null)
    return (await response.blob()) as T
  }
  const body = await response.json()
  if (!response.ok) {
    const detail = typeof body?.detail === 'string' ? body.detail : response.statusText
    throw new ApiError(response.status, detail, body)
  }
  return body as T
}

export type Json = Record<string, unknown>

export function textValue(value: unknown, language = 'sv'): string {
  if (value == null) return ''
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return String(value)
  }
  if (Array.isArray(value)) return value.map((item) => textValue(item, language)).filter(Boolean).join(', ')
  if (typeof value === 'object') {
    const record = value as Json
    if (language in record) return textValue(record[language], language)
    if ('value' in record) {
      const unit = record.unit ? ` ${record.unit}` : ''
      return `${textValue(record.value, language)}${unit}`
    }
    if ('min' in record || 'max' in record) {
      const unit = record.unit ? ` ${record.unit}` : ''
      return `${textValue(record.min, language)}–${textValue(record.max, language)}${unit}`
    }
    const first = Object.values(record)[0]
    return first == null ? '' : textValue(first, language)
  }
  return ''
}
