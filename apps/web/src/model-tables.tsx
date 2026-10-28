import { useEffect, useMemo, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, api, textValue, type Json } from './api'
import { copy, uiLanguage } from './i18n'
import { objectTypes } from './model-view'
import { onDraftChange, setModelFocus, stageModel } from './model-stage'
import { StandardEditableTable, type StandardColumn } from './standard-editable-table'
import { chooseModelSelection } from './model-selection'
import { StateNote } from './states'

type TypeRow = {
  id: string
  key: string
  label: string
  singular: string
  plural: string
  kind: string
}

type ListRow = { id: string; key: string; label: string; optionCount: number }
type OptionRow = { id: string; code: string; label: string; listKey: string }
type UnitRow = { id: string; family: string; code: string; factor: string }

export function valueListOptions(defs: Json | undefined, listKey: string): OptionRow[] {
  const list = ((defs?.value_lists as Json[] | undefined) ?? []).find((item) => String(item.key) === listKey)
  if (!list) return []
  return ((list.options as Json[] | undefined) ?? []).map((option) => {
    const code = String(option.code ?? option.key ?? '')
    return {
      id: `${listKey}:${code}`,
      code,
      label: textValue(option.labels as Json, uiLanguage) || code,
      listKey,
    }
  })
}

function useModelDefs() {
  const queryClient = useQueryClient()
  const stored = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const draft = useQuery({ queryKey: ['model-draft'], queryFn: () => api<Json>('/v1/model/draft') })
  useEffect(() => onDraftChange(() => {
    void queryClient.invalidateQueries({ queryKey: ['model-draft'] })
  }), [queryClient])
  const preview = draft.data?.conflict === true ? undefined : (draft.data?.definitions as Json | undefined)
  return { stored, defs: preview ?? stored.data, draft }
}

async function reviewDraft() {
  window.dispatchEvent(new Event('braian-review-save'))
}

export function ObjectTypesTable() {
  const { stored, defs } = useModelDefs()
  const [filter, setFilter] = useState('')
  const [draftRows, setDraftRows] = useState<Record<string, TypeRow>>({})
  const rows = useMemo(() => {
    const raw = ((defs?.types as Json[]) ?? []).filter((item) => item.kind !== 'link')
    return objectTypes({ ...defs, types: raw }, uiLanguage).map((item) => ({
      id: item.key,
      key: item.key,
      label: item.label,
      singular: item.singular,
      plural: item.plural || item.label,
      kind: 'object',
    }))
  }, [defs])

  const columns: Array<StandardColumn<TypeRow>> = [
    {
      id: 'singular',
      header: copy.singular,
      editable: true,
      read: (row) => row.singular,
      write: (row, value) => ({ ...row, singular: value, label: value || row.label }),
    },
    {
      id: 'plural',
      header: copy.plural,
      editable: true,
      read: (row) => row.plural,
      write: (row, value) => ({ ...row, plural: value }),
    },
    {
      id: 'key',
      header: copy.key,
      editable: true,
      read: (row) => row.key,
      write: (row, value) => ({ ...row, key: value }),
    },
  ]

  if (stored.isPending) return <StateNote tone="loading" title={copy.loadingModel} />
  if (stored.isError) return <StateNote tone="error" title={copy.errorModel} />

  return (
    <StandardEditableTable
      testId="object-types-table"
      columns={columns}
      rows={rows}
      draftRows={draftRows}
      onDraftRowsChange={setDraftRows}
      createDraftRow={() => ({
        id: `new-${crypto.randomUUID()}`,
        key: '',
        label: '',
        singular: '',
        plural: '',
        kind: 'object',
      })}
      newRowLabel={copy.objectTypes}
      filterValue={filter}
      onFilterChange={setFilter}
      onActivateRow={(row) => {
        if (!row.key) return
        chooseModelSelection({ kind: 'type', typeKey: row.key })
        setModelFocus({ kind: 'type', key: row.key })
      }}
      onReviewAndSave={async () => {
        for (const row of Object.values(draftRows)) {
          const existing = rows.some((item) => item.id === row.id)
          if (!existing) {
            await stageModel('model.create_type', {
              key: row.key || row.singular.toLowerCase().replace(/\s+/g, '_'),
              singular: row.singular,
              plural: row.plural || row.singular,
              language: uiLanguage,
            })
          } else {
            await stageModel('model.update_type', {
              key: row.id,
              singular: row.singular,
              plural: row.plural,
              language: uiLanguage,
              ...(row.key !== row.id ? { new_key: row.key } : {}),
            })
          }
        }
        setDraftRows({})
        await reviewDraft()
      }}
    />
  )
}

export function LinkTypesTable() {
  const { stored, defs } = useModelDefs()
  const [filter, setFilter] = useState('')
  const [draftRows, setDraftRows] = useState<Record<string, TypeRow>>({})
  const rows = useMemo(() => {
    const raw = ((defs?.types as Json[]) ?? []).filter((item) => item.kind === 'link')
    return objectTypes({ ...defs, types: raw }, uiLanguage).map((item) => ({
      id: item.key,
      key: item.key,
      label: item.label,
      singular: item.singular,
      plural: item.plural || item.label,
      kind: 'link',
    }))
  }, [defs])

  const columns: Array<StandardColumn<TypeRow>> = [
    {
      id: 'singular',
      header: copy.singular,
      editable: true,
      read: (row) => row.singular,
      write: (row, value) => ({ ...row, singular: value, label: value || row.label }),
    },
    {
      id: 'plural',
      header: copy.plural,
      editable: true,
      read: (row) => row.plural,
      write: (row, value) => ({ ...row, plural: value }),
    },
    {
      id: 'key',
      header: copy.key,
      editable: true,
      read: (row) => row.key,
      write: (row, value) => ({ ...row, key: value }),
    },
  ]

  if (stored.isPending) return <StateNote tone="loading" title={copy.loadingModel} />
  if (stored.isError) return <StateNote tone="error" title={copy.errorModel} />

  return (
    <StandardEditableTable
      testId="link-types-table"
      columns={columns}
      rows={rows}
      draftRows={draftRows}
      onDraftRowsChange={setDraftRows}
      createDraftRow={() => ({
        id: `new-${crypto.randomUUID()}`,
        key: '',
        label: '',
        singular: '',
        plural: '',
        kind: 'link',
      })}
      newRowLabel={copy.linkTypes}
      filterValue={filter}
      onFilterChange={setFilter}
      onActivateRow={(row) => {
        if (!row.key) return
        setModelFocus({ kind: 'type', key: row.key })
      }}
      onReviewAndSave={async () => {
        for (const row of Object.values(draftRows)) {
          const existing = rows.some((item) => item.id === row.id)
          if (!existing) {
            await stageModel('model.create_link_type', {
              key: row.key || row.singular.toLowerCase().replace(/\s+/g, '_'),
              singular: row.singular,
              plural: row.plural || row.singular,
              language: uiLanguage,
            })
          } else {
            await stageModel('model.update_type', {
              key: row.id,
              singular: row.singular,
              plural: row.plural,
              language: uiLanguage,
            })
          }
        }
        setDraftRows({})
        await reviewDraft()
      }}
    />
  )
}

export function ValueListsTable({ onOpenList }: { onOpenList?: (key: string) => void }) {
  const { stored, defs } = useModelDefs()
  const [filter, setFilter] = useState('')
  const [draftRows, setDraftRows] = useState<Record<string, ListRow>>({})
  const rows = useMemo<ListRow[]>(
    () =>
      ((defs?.value_lists as Json[]) ?? []).map((list) => {
        const key = String(list.key ?? '')
        return {
          id: key,
          key,
          label: textValue(list.labels as Json, uiLanguage) || key,
          optionCount: ((list.options as Json[]) ?? []).length,
        }
      }),
    [defs],
  )

  const columns: Array<StandardColumn<ListRow>> = [
    {
      id: 'label',
      header: copy.name,
      editable: true,
      read: (row) => row.label,
      write: (row, value) => ({ ...row, label: value }),
    },
    {
      id: 'key',
      header: copy.key,
      editable: true,
      read: (row) => row.key,
      write: (row, value) => ({ ...row, key: value }),
    },
    {
      id: 'options',
      header: copy.rows,
      type: 'readonly',
      editable: false,
      read: (row) => row.optionCount,
    },
  ]

  if (stored.isPending) return <StateNote tone="loading" title={copy.loadingModel} />
  if (stored.isError) return <StateNote tone="error" title={copy.errorModel} />

  return (
    <StandardEditableTable
      testId="value-lists-table"
      columns={columns}
      rows={rows}
      draftRows={draftRows}
      onDraftRowsChange={setDraftRows}
      createDraftRow={() => ({ id: `new-${crypto.randomUUID()}`, key: '', label: '', optionCount: 0 })}
      newRowLabel={copy.valueLists}
      filterValue={filter}
      onFilterChange={setFilter}
      onActivateRow={(row) => {
        if (!row.key) return
        setModelFocus({ kind: 'value_list', key: row.key })
        onOpenList?.(row.key)
        window.dispatchEvent(new CustomEvent('braian-open-value-list', { detail: { key: row.key } }))
      }}
      onReviewAndSave={async () => {
        for (const row of Object.values(draftRows)) {
          const existing = rows.some((item) => item.id === row.id)
          if (!existing) {
            await stageModel('model.create_value_list', {
              key: row.key || row.label.toLowerCase().replace(/\s+/g, '_'),
              label: row.label,
              language: uiLanguage,
            })
          } else {
            await stageModel('model.update_value_list', {
              key: row.id,
              label: row.label,
              language: uiLanguage,
            })
          }
        }
        setDraftRows({})
        await reviewDraft()
      }}
    />
  )
}

export function ValueListValuesTable({ listKey }: { listKey: string }) {
  const { stored, defs } = useModelDefs()
  const [filter, setFilter] = useState('')
  const [draftRows, setDraftRows] = useState<Record<string, OptionRow>>({})
  const rows = useMemo(() => valueListOptions(defs, listKey), [defs, listKey])

  const columns: Array<StandardColumn<OptionRow>> = [
    {
      id: 'label',
      header: copy.name,
      editable: true,
      read: (row) => row.label,
      write: (row, value) => ({ ...row, label: value }),
    },
    {
      id: 'code',
      header: copy.key,
      editable: true,
      read: (row) => row.code,
      write: (row, value) => ({ ...row, code: value }),
    },
  ]

  if (!listKey) return <StateNote tone="empty" title={copy.pickModel} />
  if (stored.isPending) return <StateNote tone="loading" title={copy.loadingModel} />

  return (
    <StandardEditableTable
      testId="value-list-values-table"
      columns={columns}
      rows={rows}
      draftRows={draftRows}
      onDraftRowsChange={setDraftRows}
      createDraftRow={() => ({
        id: `${listKey}:new-${crypto.randomUUID()}`,
        code: '',
        label: '',
        listKey,
      })}
      newRowLabel={copy.valueListValues}
      filterValue={filter}
      onFilterChange={setFilter}
      onReviewAndSave={async () => {
        for (const row of Object.values(draftRows)) {
          const existing = rows.some((item) => item.id === row.id)
          if (!existing) {
            await stageModel('model.add_value_option', {
              value_list: listKey,
              option: row.code || row.label.toLowerCase().replace(/\s+/g, '_'),
              label: row.label,
              language: uiLanguage,
            })
          } else {
            await stageModel('model.update_value_option', {
              value_list: listKey,
              option: row.id.split(':').pop(),
              label: row.label,
              language: uiLanguage,
              ...(row.code !== row.id.split(':').pop() ? { new_code: row.code } : {}),
            })
          }
        }
        setDraftRows({})
        await reviewDraft()
      }}
    />
  )
}

export function UnitsTable() {
  const { stored, defs } = useModelDefs()
  const [filter, setFilter] = useState('')
  const [draftRows, setDraftRows] = useState<Record<string, UnitRow>>({})
  const rows = useMemo<UnitRow[]>(() => {
    const out: UnitRow[] = []
    for (const family of (defs?.unit_families as Json[] | undefined) ?? []) {
      const familyKey = String(family.key ?? '')
      for (const unit of (family.units as Json[] | undefined) ?? []) {
        const code = String(unit.code ?? '')
        out.push({
          id: `${familyKey}:${code}`,
          family: familyKey,
          code,
          factor: String(unit.factor ?? '1'),
        })
      }
      if (((family.units as Json[] | undefined) ?? []).length === 0) {
        out.push({ id: `${familyKey}:`, family: familyKey, code: '', factor: '' })
      }
    }
    return out
  }, [defs])

  const columns: Array<StandardColumn<UnitRow>> = [
    {
      id: 'family',
      header: copy.unitFamilies,
      editable: true,
      read: (row) => row.family,
      write: (row, value) => ({ ...row, family: value }),
    },
    {
      id: 'code',
      header: copy.unit,
      editable: true,
      read: (row) => row.code,
      write: (row, value) => ({ ...row, code: value }),
    },
    {
      id: 'factor',
      header: copy.number,
      type: 'number',
      editable: true,
      read: (row) => row.factor,
      write: (row, value) => ({ ...row, factor: value }),
    },
  ]

  if (stored.isPending) return <StateNote tone="loading" title={copy.loadingModel} />
  if (stored.isError) return <StateNote tone="error" title={copy.errorModel} />

  return (
    <StandardEditableTable
      testId="units-table"
      columns={columns}
      rows={rows}
      draftRows={draftRows}
      onDraftRowsChange={setDraftRows}
      createDraftRow={() => ({
        id: `new-${crypto.randomUUID()}`,
        family: '',
        code: '',
        factor: '1',
      })}
      newRowLabel={copy.unitsView}
      filterValue={filter}
      onFilterChange={setFilter}
      onReviewAndSave={async () => {
        for (const row of Object.values(draftRows)) {
          const existing = rows.some((item) => item.id === row.id)
          if (!existing) {
            const hasFamily = ((defs?.unit_families as Json[]) ?? []).some((item) => String(item.key) === row.family)
            if (!hasFamily && row.family) {
              await stageModel('model.create_unit_family', {
                key: row.family,
                base_unit: row.code || row.family,
                units: row.code ? [{ code: row.code, factor: row.factor || '1' }] : [],
              })
            } else if (row.code) {
              await stageModel('model.add_unit', {
                unit_family: row.family,
                code: row.code,
                factor: row.factor || '1',
              })
            }
          } else if (row.code) {
            await stageModel('model.update_unit', {
              unit_family: row.family,
              unit: row.id.split(':').pop(),
              factor: row.factor,
            })
          }
        }
        setDraftRows({})
        await reviewDraft()
      }}
    />
  )
}

/** Thin wrapper kept for shell routing. */
export function ModelTableView({ view }: { view: { type: string; state: Json } }) {
  if (view.type === 'object_types') return <ObjectTypesTable />
  if (view.type === 'link_types') return <LinkTypesTable />
  if (view.type === 'value_lists') return <ValueListsTable />
  if (view.type === 'value_list_values') return <ValueListValuesTable listKey={String(view.state.list_key ?? '')} />
  if (view.type === 'units') return <UnitsTable />
  return <StateNote tone="empty" title={copy.pickModel} />
}
