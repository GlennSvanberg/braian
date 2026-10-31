import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as api from '../api/client'
import { StandardEditableTable, type StandardColumn } from '../standard-editable-table'

type Saved = { id: string; name: string; type: string; headers: unknown; columns: unknown; empty_clears: boolean }

/** Saved import mappings: shared by everyone, listed here so they can be renamed or deleted. */
export function ImportMappingsView() {
  const queryClient = useQueryClient()
  const [filter, setFilter] = useState('')
  const [drafts, setDrafts] = useState<Record<string, Saved>>({})
  const [error, setError] = useState('')
  const list = useQuery({ queryKey: ['import-mappings'], queryFn: () => api.get('/v1/import-mappings') })
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['import-mappings'] })
  const rename = useMutation({
    mutationFn: (item: { id: string; name: string }) =>
      api.patch('/v1/import-mappings/{id}', { params: { path: { id: item.id } }, body: { name: item.name } }),
    onSuccess: () => {
      setError('')
      void refresh()
    },
    onError: (reason: unknown) => setError(reason instanceof Error ? reason.message : 'Rename failed'),
    onSettled: (_data, _error, item) =>
      setDrafts((current) => {
        const { [item.id]: _removed, ...rest } = current
        return rest
      }),
  })
  const remove = useMutation({
    mutationFn: (id: string) => api.del('/v1/import-mappings/{id}', { params: { path: { id } } }),
    onSuccess: () => void refresh(),
    onError: (reason: unknown) => setError(reason instanceof Error ? reason.message : 'Delete failed'),
  })
  const all = (list.data?.mappings ?? []) as Saved[]
  const columns: Array<StandardColumn<Saved>> = [
    {
      id: 'name',
      header: 'Name',
      type: 'text',
      truncate: true,
      read: (row) => row.name,
      write: (row, value) => ({ ...row, name: value }),
    },
    { id: 'type', header: 'Object type', type: 'readonly', truncate: true, read: (row) => row.type },
    {
      id: 'columns',
      header: 'Columns',
      type: 'readonly',
      width: 100,
      read: (row) => (Array.isArray(row.columns) ? row.columns.length : 0),
    },
    {
      id: 'delete',
      header: 'Delete',
      type: 'readonly',
      width: 90,
      read: () => '',
      display: (row) => (
        <button
          type="button"
          aria-label={`Delete ${row.name}`}
          onClick={(event) => {
            event.stopPropagation()
            remove.mutate(row.id)
          }}
        >
          Delete
        </button>
      ),
    },
  ]
  // No Review-and-save step: a cell edit is persisted at once; the draft only bridges until the refetch.
  const onDraftRowsChange = (next: Record<string, Saved>) => {
    for (const [id, row] of Object.entries(next)) {
      const original = all.find((item) => item.id === id)
      const name = row.name.trim()
      if (!original || !name || name === original.name || drafts[id]?.name === row.name) continue
      rename.mutate({ id, name })
    }
    setDrafts(next)
  }
  return (
    <>
      {error ? <p className="error" role="alert">{error}</p> : null}
      <StandardEditableTable<Saved>
        testId="import-mappings"
        columns={columns}
        rows={all}
        draftRows={drafts}
        onDraftRowsChange={onDraftRowsChange}
        filterValue={filter}
        onFilterChange={setFilter}
        getRowTestId={(row) => `import-mapping-${row.id}`}
        emptyLabel="No saved mappings yet. Save one from the mapping step of an import."
      />
    </>
  )
}
