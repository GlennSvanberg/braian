import { api, catalogName, type Json } from './api'
import { readIndexed } from './index-fields'
import { uiLanguage } from './i18n'

export type PeerPickOption = {
  id: string
  label: string
  subtitle: string
  pickValue: string
  shortLabel: string
}

/** GTIN digit strings: try with and without a leading zero (14↔13). */
export function gtinSearchVariants(raw: string): string[] {
  const digits = raw.replace(/\D/g, '')
  if (digits.length < 8 || digits.length > 14) return []
  const out = new Set<string>([digits])
  const stripped = digits.replace(/^0+/, '') || '0'
  out.add(stripped)
  if (!digits.startsWith('0') && digits.length < 14) out.add(`0${digits}`)
  return [...out]
}

function hitToOption(hit: Record<string, unknown>, defs: Json | undefined): PeerPickOption | null {
  const doc = ((hit.document as Json | undefined) ?? (hit as Json)) as Json
  const id = String(hit.id ?? doc.id ?? '')
  if (!id) return null
  const key = String(doc.key ?? '')
  const name = catalogName(doc, defs) || String(doc.name ?? '')
  const gtin = readIndexed(doc, defs, 'gtin') || String(doc.gtin ?? '')
  const shortLabel = gtin || key || name || id
  return {
    id,
    label: gtin ? `${gtin} · ${key || name}` : key || name || id,
    subtitle: name || key,
    pickValue: id,
    shortLabel,
  }
}

/**
 * Server-side article search for Specific….
 * Digit queries filter on GTIN (with/without leading 0); otherwise Typesense `q` on key/name.
 */
export async function searchPeerArticles(
  query: string,
  defs: Json | undefined,
  opts?: { perPage?: number },
): Promise<PeerPickOption[]> {
  const needle = query.trim()
  const perPage = opts?.perPage ?? 50
  const lang = encodeURIComponent(uiLanguage)
  const variants = gtinSearchVariants(needle)
  let url: string
  if (variants.length > 0) {
    const gtinOr = variants.map((value) => `gtin:=${value}`).join(' || ')
    const filter = encodeURIComponent(`type_key:=article && (${gtinOr})`)
    url = `/v1/search?q=*&filter=${filter}&per_page=${perPage}&lang=${lang}`
  } else {
    const filter = encodeURIComponent('type_key:=article')
    const q = encodeURIComponent(needle || '*')
    url = `/v1/search?q=${q}&filter=${filter}&per_page=${perPage}&lang=${lang}`
  }
  const result = await api<{ hits?: Array<Record<string, unknown>> }>(url)
  const options: PeerPickOption[] = []
  const seen = new Set<string>()
  for (const hit of result.hits ?? []) {
    const option = hitToOption(hit, defs)
    if (!option || seen.has(option.id)) continue
    seen.add(option.id)
    options.push(option)
  }
  return options
}

