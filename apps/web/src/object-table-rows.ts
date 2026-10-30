/**
 * Object-table rows as a TanStack DB query collection.
 *
 * One queryFn loads Typesense hits and pathValues together so a live refresh
 * paints Description and Version in a single sweep (no paint-hold). Cell edits
 * go through collection.update → onUpdate PATCH → refetch. SSE invalidates the
 * collection query key; the server remains the source of truth (no offline).
 */
import { useEffect, useMemo, useRef, useState } from 'react'
import { createCollection, useLiveQuery } from '@tanstack/react-db'
import { useQueryClient } from '@tanstack/react-query'
import { queryCollectionOptions } from '@tanstack/query-db-collection'
import type { QueryClient } from '@tanstack/react-query'
import { ApiError, api, catalogName, freeObject, searchQuery, type Json } from './api'
import { cellEditShouldPatch } from './cell-edit'
import { parseColumnId } from './fields/columns'
import { readIndexed } from './index-fields'
import { uiLanguage } from './i18n'
import { summarizePathValues } from './path-cell'
import { cellPathValues, fetchPathValues, type PathValuesResponse } from './path-values'

export const OBJECT_TABLE_ROWS_KEY = 'object-table-rows' as const
export const objectTablePageSize = 100

export type ObjRow = {
  id: string
  key: string
  type: string
  version: string
  name: string
  status: string
  doc: Json
}

export type ObjectTableRow = ObjRow & {
  /** Stable virtualizer index within the current search result. */
  index: number
  /** Display text for columns that need pathValues (hops, localized, units). */
  pathText: Record<string, string>
}

export type ObjectTableRowsScope = {
  viewId: string
  queryText: string
  filterBy: string
  sortBy: string
  groupField: string
  /** 1-based Typesense pages currently in the virtualizer window. Empty when grouping. */
  pages: number[]
  pathValueColumns: string[]
  typeKey: string
  /** When grouping, pass the flattened group hits instead of paging. */
  groupHits?: Json[]
  defs?: Json
}

function groupHitIds(hits: Json[] | undefined): string {
  if (!hits?.length) return ''
  return hits
    .map((hit) => {
      const doc = ((hit.document as Json | undefined) ?? hit) as Json
      return String(doc.id ?? '')
    })
    .join(',')
}

export function objectTableRowsQueryKey(scope: ObjectTableRowsScope) {
  return [
    OBJECT_TABLE_ROWS_KEY,
    scope.viewId,
    scope.queryText,
    scope.filterBy,
    scope.sortBy,
    scope.groupField,
    scope.pages.join(','),
    scope.pathValueColumns.join('\0'),
    scope.typeKey,
    groupHitIds(scope.groupHits),
  ] as const
}

export function toObjRow(doc: Json, defs?: Json): ObjRow {
  return {
    id: String(doc.id ?? ''),
    key: String(doc.key ?? ''),
    type: String(doc.type_key ?? ''),
    version: String(doc.version ?? ''),
    name: catalogName(doc, defs),
    status: readIndexed(doc, defs, 'status'),
    doc,
  }
}

export async function loadSearchPage(
  q: string,
  filter: string,
  sort: string,
  groupBy: string,
  page: number,
) {
  const params = new URLSearchParams({
    page: String(page),
    per_page: String(objectTablePageSize),
  })
  if (q) params.set('q', q)
  params.set('lang', uiLanguage)
  if (filter) params.set('filter', filter)
  if (sort) params.set('sort', sort)
  if (groupBy) params.set('group', groupBy)
  const body = await api.get('/v1/search', { params: { query: searchQuery(params) } })
  return { ...body, page }
}

function pathTextForRow(
  pathValues: PathValuesResponse | undefined,
  rowId: string,
  columns: string[],
): Record<string, string> {
  const out: Record<string, string> = {}
  if (!pathValues || columns.length === 0) return out
  const byKey = pathValues.values?.[rowId]
  for (const columnId of columns) {
    const values = cellPathValues(byKey, columnId)
    out[columnId] = summarizePathValues(values).text
  }
  return out
}

/** Build indexed rows from search hits (pathValues filled later via writeUpsert). */
export async function loadObjectTableRows(scope: ObjectTableRowsScope): Promise<ObjectTableRow[]> {
  const byIndex = new Map<number, ObjRow>()

  if (scope.groupField && scope.groupHits) {
    scope.groupHits.forEach((hit, offset) => {
      const doc = ((hit.document as Json | undefined) ?? hit) as Json
      const row = toObjRow(doc, scope.defs)
      if (row.id) byIndex.set(offset, row)
    })
  } else {
    for (const page of scope.pages) {
      const result = await loadSearchPage(
        scope.queryText,
        scope.filterBy,
        scope.sortBy,
        scope.groupField,
        page,
      )
      const hits = (result.hits as Json[] | undefined) ?? []
      hits.forEach((hit, offset) => {
        const doc = ((hit.document as Json) ?? hit) as Json
        const row = toObjRow(doc, scope.defs)
        if (row.id) byIndex.set((page - 1) * objectTablePageSize + offset, row)
      })
    }
  }

  return [...byIndex.entries()]
    .sort(([a], [b]) => a - b)
    .map(([index, row]) => ({ ...row, index, pathText: {} as Record<string, string> }))
}

/** Merge pathValues into collection rows (keeps Typesense fields; fills Description/hops). */
export async function mergePathValuesIntoCollection(
  collection: { utils: { writeBatch: (fn: () => void) => Promise<unknown>; writeUpsert: (row: ObjectTableRow) => unknown } },
  scope: ObjectTableRowsScope,
  rows: ObjectTableRow[],
) {
  if (scope.pathValueColumns.length === 0 || rows.length === 0 || !scope.typeKey) return
  const pathValues = (await fetchPathValues({
    ids: rows.map((row) => row.id),
    paths: scope.pathValueColumns,
    typeKey: scope.typeKey,
  })) as PathValuesResponse
  await collection.utils.writeBatch(() => {
    for (const row of rows) {
      collection.utils.writeUpsert({
        ...row,
        pathText: pathTextForRow(pathValues, row.id, scope.pathValueColumns),
        doc: {
          ...row.doc,
          __pathValues: pathValues.values?.[row.id],
          __pathEntries: pathValues.entries?.[row.id],
        },
      })
    }
  })
}

export type CellEditMeta = {
  columnId: string
  nextText: string
  originalText: string
}

async function persistCellEdit(row: ObjectTableRow, meta: CellEditMeta) {
  if (!cellEditShouldPatch(meta.originalText, meta.nextText)) return
  const path = parseColumnId(meta.columnId).path
  const langMatch = path.includes('@') ? path.split('@') : null
  const attrKey = (langMatch ? langMatch[0]! : path).split('.')[0] ?? path
  const language = langMatch?.[1]
  const bodyValues: Json = language
    ? { [attrKey]: { [language]: meta.nextText } }
    : { [attrKey]: meta.nextText }
  await api.patch('/v1/objects/{id}', {
    params: {
      path: { id: row.id },
      header: { 'If-Match': Number(row.version ?? 0) },
    },
    body: { values: freeObject(bodyValues) },
  })
}

export function createObjectTableRowsCollection(
  queryClient: QueryClient,
  scope: ObjectTableRowsScope,
) {
  const enabled =
    Boolean(scope.groupField)
      ? (scope.groupHits?.length ?? 0) > 0
      : scope.pages.length > 0

  return createCollection(
    queryCollectionOptions<ObjectTableRow>({
      id: `object-table-rows:${scope.viewId}`,
      queryKey: objectTableRowsQueryKey(scope),
      queryClient,
      getKey: (row) => row.id,
      enabled,
      // Server is truth; do not treat stale client rows as fresh across focus.
      staleTime: 0,
      queryFn: () => loadObjectTableRows(scope),
      onUpdate: async ({ transaction, collection }) => {
        for (const mutation of transaction.mutations) {
          const meta = mutation.metadata as CellEditMeta | undefined
          if (!meta?.columnId) continue
          try {
            await persistCellEdit(mutation.original as ObjectTableRow, meta)
          } catch (reason) {
            // Roll back optimistic state by rethrowing; caller shows ApiError text.
            if (reason instanceof ApiError) throw reason
            throw reason
          }
        }
        await collection.utils.refetch()
      },
    }),
  )
}

/** Apply a cell edit through the collection (optimistic + PATCH + refetch). */
export function commitObjectTableCellEdit(
  collection: ReturnType<typeof createObjectTableRowsCollection>,
  row: ObjectTableRow,
  columnId: string,
  nextText: string,
  originalText: string,
) {
  if (!cellEditShouldPatch(originalText, nextText)) return undefined
  const meta: CellEditMeta = { columnId, nextText, originalText }
  return collection.update(
    row.id,
    { metadata: meta },
    (draft) => {
      draft.pathText[columnId] = nextText
      const path = parseColumnId(columnId).path
      const base = (path.split('@')[0] ?? path).split('.')[0] ?? path
      if (base === 'name') draft.name = nextText
      if (base === 'status') draft.status = nextText
      if (base === 'key' || base === 'number') draft.key = nextText
    },
  )
}

export function useObjectTableRows(scope: ObjectTableRowsScope) {
  const queryClient = useQueryClient()
  const scopeRef = useRef(scope)
  scopeRef.current = scope
  const keyStamp = JSON.stringify(objectTableRowsQueryKey(scope))

  // One collection per view. Recreating with the same id while useLiveQuery is
  // mounted throws; keep the instance and refetch when the scope stamp changes.
  const collection = useMemo(
    () =>
      createCollection(
        queryCollectionOptions<ObjectTableRow>({
          id: `object-table-rows:${scope.viewId}`,
          queryKey: [OBJECT_TABLE_ROWS_KEY, scope.viewId],
          queryClient,
          getKey: (row) => row.id,
          staleTime: 0,
          queryFn: () => loadObjectTableRows(scopeRef.current),
          onUpdate: async ({ transaction, collection: col }) => {
            for (const mutation of transaction.mutations) {
              const meta = mutation.metadata as CellEditMeta | undefined
              if (!meta?.columnId) continue
              await persistCellEdit(mutation.original as ObjectTableRow, meta)
            }
            await col.utils.refetch()
          },
        }),
      ),
    // viewId is stable for the mounted ObjectTable; queryClient is app-wide.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [queryClient, scope.viewId],
  )

  useEffect(() => {
    return () => {
      // useLiveQuery may still hold a subscriber in the same commit; drop dependents too.
      void collection.cleanup({ dependents: true }).catch(() => undefined)
    }
  }, [collection])

  useEffect(() => {
    const enabled = scope.groupField
      ? (scope.groupHits?.length ?? 0) > 0
      : scope.pages.length > 0
    if (!enabled) return
    void collection.utils.refetch()
  }, [collection, keyStamp, scope.groupField, scope.pages.length, scope.groupHits?.length])

  const live = useLiveQuery((q) =>
    q.from({ row: collection }).orderBy(({ row }) => row.index, 'asc'),
  )

  const byIndex = useMemo(() => {
    const map = new Map<number, ObjectTableRow>()
    for (const row of live.data ?? []) map.set(row.index, row)
    return map
  }, [live.data])

  const rows = live.data ?? []

  const rowIdsStamp = rows.map((row) => row.id).join(',')
  const pathColsStamp = scope.pathValueColumns.join('\0')
  const needsPathMerge =
    scope.pathValueColumns.length > 0 &&
    Boolean(scope.typeKey) &&
    rows.length > 0 &&
    rows.some((row) => scope.pathValueColumns.some((columnId) => !(columnId in row.pathText)))

  const [pathValuesBusy, setPathValuesBusy] = useState(false)
  useEffect(() => {
    if (!needsPathMerge) {
      setPathValuesBusy(false)
      return
    }
    // Snapshot rows for this merge; do not cancel on live-query identity churn.
    const snapshot = rows
    const scopeSnap = scopeRef.current
    let cancelled = false
    setPathValuesBusy(true)
    void mergePathValuesIntoCollection(collection, scopeSnap, snapshot)
      .catch(() => undefined)
      .finally(() => {
        if (!cancelled) setPathValuesBusy(false)
      })
    return () => {
      cancelled = true
    }
    // rowIdsStamp / pathColsStamp / needsPathMerge — not `rows` identity
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [collection, keyStamp, rowIdsStamp, pathColsStamp, needsPathMerge])

  return {
    collection,
    rows,
    byIndex,
    isLoading: Boolean(live.isLoading),
    isPending: Boolean(live.isLoading) && rows.length === 0,
    pathValuesBusy,
    status: live.status,
  }
}
