import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as api from '../api/client'
import { StandardTableChrome } from '../standard-table'

type Saved = { id: string; name: string; type: string; headers: unknown; columns: unknown; empty_clears: boolean }

/** Saved import mappings: shared by everyone, listed here so they can be renamed or deleted. */
export function ImportMappingsView() {
  const queryClient = useQueryClient()
  const [filter, setFilter] = useState('')
  const [editing, setEditing] = useState<{ id: string; name: string } | null>(null)
  const [error, setError] = useState('')
  const list = useQuery({ queryKey: ['import-mappings'], queryFn: () => api.get('/v1/import-mappings') })
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['import-mappings'] })
  const rename = useMutation({
    mutationFn: (item: { id: string; name: string }) =>
      api.patch('/v1/import-mappings/{id}', { params: { path: { id: item.id } }, body: { name: item.name } }),
    onSuccess: () => {
      setEditing(null)
      setError('')
      void refresh()
    },
    onError: (reason: unknown) => setError(reason instanceof Error ? reason.message : 'Rename failed'),
  })
  const remove = useMutation({
    mutationFn: (id: string) => api.del('/v1/import-mappings/{id}', { params: { path: { id } } }),
    onSuccess: () => void refresh(),
    onError: (reason: unknown) => setError(reason instanceof Error ? reason.message : 'Delete failed'),
  })
  const needle = filter.trim().toLowerCase()
  const items = ((list.data?.mappings ?? []) as Saved[]).filter(
    (item) => !needle || `${item.name} ${item.type}`.toLowerCase().includes(needle),
  )
  return (
    <StandardTableChrome testId="import-mappings" rowCount={items.length} filterValue={filter} onFilterChange={setFilter}>
      {error ? <p className="error" role="alert">{error}</p> : null}
      <table className="table" data-testid="import-mappings-table">
        <thead>
          <tr>
            <th>Name</th>
            <th>Object type</th>
            <th>Columns</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <tr key={item.id} data-testid={`import-mapping-${item.id}`}>
              <td>
                {editing?.id === item.id ? (
                  <input
                    autoFocus
                    aria-label="Mapping name"
                    value={editing.name}
                    onChange={(event) => setEditing({ id: item.id, name: event.target.value })}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter') rename.mutate(editing)
                      if (event.key === 'Escape') setEditing(null)
                    }}
                    onBlur={() => setEditing(null)}
                  />
                ) : (
                  <button type="button" className="link" onClick={() => setEditing({ id: item.id, name: item.name })}>{item.name}</button>
                )}
              </td>
              <td>{item.type}</td>
              <td>{Array.isArray(item.columns) ? item.columns.length : 0}</td>
              <td>
                <button type="button" aria-label={`Delete ${item.name}`} onClick={() => remove.mutate(item.id)}>Delete</button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {items.length === 0 && !list.isLoading ? <p className="muted">No saved mappings yet. Save one from the mapping step of an import.</p> : null}
    </StandardTableChrome>
  )
}
