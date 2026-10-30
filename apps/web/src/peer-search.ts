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

/**
 * Typesense filter for exact GTIN match.
 * Use `field:=[a,b]` (same-field OR) — `(a || b)` breaks translate_filter_expression
 * which only splits on `&&` and then cannot parse a parenthesized OR clause.
 * Backticks keep leading zeros as strings.
 */
export function gtinExactFilter(variants: string[]): string {
  const list = variants.map((value) => `\`${value}\``).join(',')
  return `type_key:=article && gtin:=[${list}]`
}

function hitToOption(hit: Record<string, unknown>, defs: Json | undefined): PeerPickOption | null {
  const doc = ((hit.document as Json | undefined) ?? (hit as Json)) as Json
  const id = String(hit.id ?? doc.id ?? '')
  if (!id) return null
  const key = String(doc.key ?? '')
  const name = catalogName(doc, defs) || String(doc.name ?? '')
  const gtin = readIndexed(doc, defs, 'gtin') || String(doc.gtin ?? '')
  const shortLabel = key || gtin || name || id
  return {
    id,
    label: gtin ? `${gtin} · ${key || name}` : key || name || id,
    subtitle: name || key,
    pickValue: id,
    shortLabel,
  }
}

function pushHits(
  result: { hits?: Array<Record<string, unknown>> },
  defs: Json | undefined,
  options: PeerPickOption[],
  seen: Set<string>,
) {
  for (const hit of result.hits ?? []) {
    const option = hitToOption(hit, defs)
    if (!option || seen.has(option.id)) continue
    seen.add(option.id)
    options.push(option)
  }
}

/**
 * Server-side article search for Specific….
 * Digit queries exact-match GTIN (with/without leading 0) via filter, then also
 * `q=` so prefixes hit the searchable blob. Text uses `q` on key/name.
 */
export async function searchPeerArticles(
  query: string,
  defs: Json | undefined,
  opts?: { perPage?: number },
): Promise<PeerPickOption[]> {
  const needle = query.trim()
  const perPage = opts?.perPage ?? 50
  const variants = gtinSearchVariants(needle)
  const options: PeerPickOption[] = []
  const seen = new Set<string>()

  if (variants.length > 0) {
    try {
      const exact = await api.get('/v1/search', { params: { query: { q: '*', filter: gtinExactFilter(variants), per_page: perPage, lang: uiLanguage } } })
      pushHits(exact, defs, options, seen)
    } catch {
      // Fall through to q= — filter translation / Typesense errors must not blank the list.
    }
  }

  // Text and digit prefixes: searchable blob includes gtin when indexed searchable.
  try {
    const text = await api.get('/v1/search', { params: { query: { q: needle || '*', filter: 'type_key:=article', per_page: perPage, lang: uiLanguage } } })
    pushHits(text, defs, options, seen)
  } catch {
    /* ignore */
  }
  return options
}
