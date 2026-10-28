import { api } from './api'
import { uiLanguage } from './i18n'
import type { PathValuesResponse } from './path-cell'

export async function fetchPathValues(input: {
  ids: string[]
  paths: string[]
  typeKey?: string
  lang?: string
}): Promise<PathValuesResponse> {
  const ids = [...new Set(input.ids.filter(Boolean))]
  const paths = [...new Set(input.paths.filter(Boolean))]
  if (ids.length === 0 || paths.length === 0) return { values: {} }
  return api<PathValuesResponse>('/v1/objects:pathValues', {
    method: 'POST',
    body: JSON.stringify({
      ids,
      paths,
      type: input.typeKey || undefined,
      lang: input.lang ?? uiLanguage,
    }),
  })
}
