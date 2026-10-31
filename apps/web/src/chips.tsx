import {
  AppWindow,
  Bookmark,
  Boxes,
  FileText,
  History,
  Layers,
  Library,
  ListChecks,
  ListOrdered,
  ListTree,
  Link2,
  Ruler,
  ScrollText,
  Table2,
  Waypoints,
} from 'lucide-react'
import type { ReactNode } from 'react'
import { textValue, type Json } from './api'
import { singularOf } from './type-labels'

export type ChipTone = 'neutral' | 'ok' | 'warn' | 'danger' | 'info'

export function statusTone(value: string): ChipTone {
  const text = value.trim().toLowerCase()
  if (['ok', 'active', 'published', 'done', 'accepted', 'complete', 'succeeded', 'valid'].includes(text)) return 'ok'
  if (['draft', 'pending', 'warn', 'warning', 'review'].includes(text)) return 'warn'
  if (['error', 'failed', 'danger', 'rejected', 'invalid', 'conflict', 'discontinued', 'deleted'].includes(text)) return 'danger'
  if (['info', 'running', 'queued', 'open'].includes(text)) return 'info'
  return 'neutral'
}

export function MetaChip({ tone = 'neutral', children }: { tone?: ChipTone; children: ReactNode }) {
  return <span className={`meta-chip ${tone}`}>{children}</span>
}

export function optionLabel(defs: Json | undefined, listKey: string, code: string, language = 'en') {
  const list = ((defs?.value_lists as Json[] | undefined) ?? []).find((item) => String(item.key) === listKey)
  const option = ((list?.options as Json[] | undefined) ?? []).find((item) => String(item.code) === code)
  return textValue(option?.labels, language) || code
}

export function typeLabel(defs: Json | undefined, key: string, language = 'en') {
  return singularOf(defs, key, language) || key
}

export function ViewGlyph({ type }: { type: string }) {
  const props = { size: 14, 'aria-hidden': true as const }
  if (type === 'structure_tree') return <ListTree {...props} />
  if (type === 'assortment_list') return <Bookmark {...props} />
  if (type === 'object_table') return <Table2 {...props} />
  if (type === 'object_detail') return <FileText {...props} />
  if (type === 'graph') return <Waypoints {...props} />
  if (type === 'history') return <History {...props} />
  if (type === 'class_attributes') return <Layers {...props} />
  if (type === 'attribute_definitions') return <Library {...props} />
  if (type === 'data_model') return <Boxes {...props} />
  if (type === 'object_types') return <Boxes {...props} />
  if (type === 'link_types') return <Link2 {...props} />
  if (type === 'value_lists') return <ListOrdered {...props} />
  if (type === 'value_list_values') return <ListOrdered {...props} />
  if (type === 'units') return <Ruler {...props} />
  if (type === 'proposal' || type === 'proposal_list') return <ScrollText {...props} />
  if (type === 'batch_log') return <ScrollText {...props} />
  if (type === 'job_history') return <ListChecks {...props} />
  return <AppWindow {...props} />
}
