/** YYYY-MM-DD HH:mm in the browser's time zone. Digits only, 24-hour. */
export function formatTimestamp(value: unknown): string {
  const date = value instanceof Date ? value : new Date(typeof value === 'string' || typeof value === 'number' ? value : '')
  if (Number.isNaN(date.getTime())) return typeof value === 'string' ? value : ''
  const pad = (part: number) => String(part).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}

/** Grouped the same way in every browser locale. */
export function formatCount(value: unknown): string {
  const count = typeof value === 'number' ? value : Number(value)
  if (!Number.isFinite(count)) return '0'
  return new Intl.NumberFormat('en', { maximumFractionDigits: 0 }).format(count)
}
