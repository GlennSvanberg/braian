import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, MoreHorizontal, Plus } from 'lucide-react'
import { ApiError, api, type Json } from './api'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from './components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from './components/ui/dropdown-menu'
import { copy } from './i18n'
import { groupNodes, structureKeys } from './model-view'
import { structureCaption } from './graph-scene'
import {
  deletionNeedsMove,
  groupKeysToDelete,
  moveTargets,
  renameOnKey,
  structureKeyFromName,
  type MenuNode,
} from './structure-menu'
import type { ModelSelection } from './model-view'

type Draft =
  | { kind: 'structure' }
  | { kind: 'group'; structure: string; parent?: string }
  | { kind: 'rename-structure'; structure: string; value: string }
  | { kind: 'rename-group'; structure: string; group: string; value: string }

type DialogState =
  | { kind: 'move'; structure: string; group: string; label: string; parent: string; error: string }
  | {
      kind: 'delete-structure' | 'delete-group'
      structure: string
      group?: string
      label: string
      count: number | null
      moveTo: string
      error: string
    }

export function StructureSection({
  defs,
  selection,
  onPick,
}: {
  defs: Json | undefined
  selection: ModelSelection | null
  onPick: (next: ModelSelection) => void
}) {
  const client = useQueryClient()
  const [collapsed, setCollapsed] = useState<string[]>([])
  const [draft, setDraft] = useState<Draft | null>(null)
  const [dialog, setDialog] = useState<DialogState | null>(null)
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)

  const fail = (reason: unknown) => setNotice(reason instanceof ApiError ? reason.message : copy.validation)

  const refresh = async () => {
    await client.invalidateQueries({ queryKey: ['definitions'] })
  }

  const run = async (work: () => Promise<void>) => {
    if (busyRef.current) return
    busyRef.current = true
    setBusy(true)
    setNotice('')
    try {
      await work()
      await refresh()
      setDraft(null)
      setDialog(null)
    } catch (reason) {
      if (dialog) setDialog({ ...dialog, error: reason instanceof ApiError ? reason.message : copy.validation })
      else fail(reason)
    } finally {
      busyRef.current = false
      setBusy(false)
    }
  }

  const commitName = (name: string, work: (key: string, label: string) => Promise<void>) => {
    const label = name.trim()
    const key = structureKeyFromName(label)
    if (!key) {
      setNotice(copy.nameRequired)
      return
    }
    void run(() => work(key, label))
  }

  const openDelete = (next: DialogState) => {
    setNotice('')
    setDialog(next)
    const path = next.kind === 'delete-structure'
      ? `/v1/structures/${encodeURIComponent(next.structure)}/usage`
      : `/v1/structures/${encodeURIComponent(next.structure)}/groups/${encodeURIComponent(next.group ?? '')}/usage`
    void api<{ count: number }>(path)
      .then((body) => {
        setDialog((current) => (current && current.kind === next.kind && current.structure === next.structure && current.group === next.group
          ? { ...current, count: body.count }
          : current))
      })
      .catch((reason: unknown) => {
        setDialog((current) => (current && current.kind === next.kind ? { ...current, error: reason instanceof ApiError ? reason.message : copy.validation } : current))
      })
  }

  return (
    <section>
      <div className="model-head">
        <h2>{copy.structures}</h2>
        <button type="button" className="icon-btn" data-testid="new-structure" aria-label={copy.newStructure} onClick={() => setDraft({ kind: 'structure' })}>
          <Plus size={14} />
        </button>
      </div>
      {draft?.kind === 'structure' ? (
        <NameField
          label={copy.newStructure}
          onCommit={(name) => commitName(name, (key, label) => api('/v1/structures:create', { method: 'POST', body: JSON.stringify({ key, label }) }).then(() => undefined))}
          onCancel={() => setDraft(null)}
        />
      ) : null}
      {structureKeys(defs).map((key) => {
        const title = structureCaption(key, defs, key)
        return (
          <div key={key} className="model-tree" data-testid={`structure-${key}`}>
            <div className="model-row">
              {draft?.kind === 'rename-structure' && draft.structure === key ? (
                <NameField
                  label={copy.rename}
                  initial={draft.value}
                  onCommit={(name) => {
                    const label = name.trim()
                    if (!label) {
                      setNotice(copy.nameRequired)
                      return
                    }
                    void run(() => api(`/v1/structures/${encodeURIComponent(key)}`, { method: 'PATCH', body: JSON.stringify({ label }) }).then(() => undefined))
                  }}
                  onCancel={() => setDraft(null)}
                />
              ) : draft?.kind === 'group' && draft.structure === key && !draft.parent ? (
                <NameField
                  label={copy.addChild}
                  onCommit={(name) => commitName(name, (groupKey, label) => api(`/v1/structures/${encodeURIComponent(key)}/groups`, { method: 'POST', body: JSON.stringify({ key: groupKey, label }) }).then(() => undefined))}
                  onCancel={() => setDraft(null)}
                />
              ) : (
                <strong>{title}</strong>
              )}
              <NodeMenu
                testId={`structure-menu-${key}`}
                onAdd={() => setDraft({ kind: 'group', structure: key })}
                onRename={() => setDraft({ kind: 'rename-structure', structure: key, value: title })}
                onDelete={() => openDelete({ kind: 'delete-structure', structure: key, label: title, count: null, moveTo: '', error: '' })}
              />
            </div>
            <GroupList
              defs={defs}
              structure={key}
              collapsed={collapsed}
              setCollapsed={setCollapsed}
              selection={selection}
              onPick={onPick}
              draft={draft}
              setDraft={setDraft}
              onAdd={(parent) => setDraft({ kind: 'group', structure: key, parent })}
              onCreate={(parent, name) => commitName(name, (groupKey, label) => api(`/v1/structures/${encodeURIComponent(key)}/groups`, {
                method: 'POST',
                body: JSON.stringify({ key: groupKey, label, parent }),
              }).then(() => undefined))}
              onRename={(group, label) => setDraft({ kind: 'rename-group', structure: key, group, value: label })}
              onRenameCommit={(group, name) => {
                const label = name.trim()
                if (!label) {
                  setNotice(copy.nameRequired)
                  return
                }
                void run(() => api(`/v1/structures/${encodeURIComponent(key)}/groups/${encodeURIComponent(group)}`, { method: 'PATCH', body: JSON.stringify({ label }) }).then(() => undefined))
              }}
              onMove={(group, label, parentKey) => setDialog({ kind: 'move', structure: key, group, label, parent: parentKey ?? '', error: '' })}
              onDelete={(group, label) => openDelete({ kind: 'delete-group', structure: key, group, label, count: null, moveTo: '', error: '' })}
            />
          </div>
        )
      })}
      {notice ? <p className="model-note" role="alert">{notice}</p> : null}
      <StructureDialog
        defs={defs}
        dialog={dialog}
        busy={busy}
        onChange={setDialog}
        onClose={() => setDialog(null)}
        onConfirm={() => {
          if (!dialog || busy) return
          if (dialog.kind === 'move') {
            const parent = dialog.parent || null
            void run(() => api(`/v1/structures/${encodeURIComponent(dialog.structure)}/groups/${encodeURIComponent(dialog.group)}/parent`, {
              method: 'POST',
              body: JSON.stringify({ parent }),
            }).then(() => undefined))
            return
          }
          if (dialog.kind === 'delete-structure') {
            void run(() => api(`/v1/structures/${encodeURIComponent(dialog.structure)}`, { method: 'DELETE' }).then(() => undefined))
            return
          }
          const group = dialog.group ?? ''
          const query = dialog.moveTo ? `?move_to=${encodeURIComponent(dialog.moveTo)}` : ''
          void run(() => api(`/v1/structures/${encodeURIComponent(dialog.structure)}/groups/${encodeURIComponent(group)}${query}`, { method: 'DELETE' }).then(() => undefined))
        }}
      />
    </section>
  )
}

function GroupList({
  defs,
  structure,
  collapsed,
  setCollapsed,
  selection,
  onPick,
  draft,
  setDraft,
  onAdd,
  onCreate,
  onRename,
  onRenameCommit,
  onMove,
  onDelete,
}: {
  defs: Json | undefined
  structure: string
  collapsed: string[]
  setCollapsed: (update: (current: string[]) => string[]) => void
  selection: ModelSelection | null
  onPick: (next: ModelSelection) => void
  draft: Draft | null
  setDraft: (draft: Draft | null) => void
  onAdd: (parent: string) => void
  onCreate: (parent: string, name: string) => void
  onRename: (group: string, label: string) => void
  onRenameCommit: (group: string, name: string) => void
  onMove: (group: string, label: string, parentKey?: string) => void
  onDelete: (group: string, label: string) => void
}) {
  const rows = groupNodes(defs, structure)
  let skipDepth = -1
  const visible = rows.filter((node) => {
    if (skipDepth >= 0) {
      if (node.depth > skipDepth) return false
      skipDepth = -1
    }
    if (collapsed.includes(node.id)) skipDepth = node.depth
    return true
  })
  return (
    <>
      {visible.map((node) => {
        const open = !collapsed.includes(node.id)
        const on = selection?.kind === 'group' && selection.structure === structure && selection.group === node.key
        const hasChildren = rows.some((item) => item.parentKey === node.key)
        const label = structureCaption(node.key, defs, structure)
        const renaming = draft?.kind === 'rename-group' && draft.structure === structure && draft.group === node.key
        return (
          <div key={node.id} data-testid={`group-${node.key}`}>
            <div className="model-row" style={{ paddingLeft: node.depth * 14 }}>
              {renaming ? (
                <NameField label={copy.rename} initial={draft.value} onCommit={(name) => onRenameCommit(node.key, name)} onCancel={() => setDraft(null)} />
              ) : draft?.kind === 'group' && draft.structure === structure && draft.parent === node.key ? (
                <NameField label={copy.addChild} onCommit={(name) => onCreate(node.key, name)} onCancel={() => setDraft(null)} />
              ) : (
                <button type="button" className={on ? 'model-node on' : 'model-node'} onClick={() => onPick({ kind: 'group', structure, group: node.key })}>
                  {hasChildren ? (
                    <span
                      className="twist"
                      onClick={(event) => {
                        event.stopPropagation()
                        setCollapsed((current) => (current.includes(node.id) ? current.filter((id) => id !== node.id) : [...current, node.id]))
                      }}
                    >
                      {open ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
                    </span>
                  ) : <span className="twist" />}
                  {label}
                </button>
              )}
              <NodeMenu
                testId={`group-menu-${structure}-${node.key}`}
                onAdd={() => onAdd(node.key)}
                onRename={() => onRename(node.key, label)}
                onMove={() => onMove(node.key, label, node.parentKey)}
                onDelete={() => onDelete(node.key, label)}
              />
            </div>
          </div>
        )
      })}
    </>
  )
}

function NodeMenu({
  testId,
  onAdd,
  onRename,
  onMove,
  onDelete,
}: {
  testId: string
  onAdd: () => void
  onRename: () => void
  onMove?: () => void
  onDelete: () => void
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="icon-btn" aria-label={copy.moreActions} data-testid={testId}>
        <MoreHorizontal size={14} />
      </DropdownMenuTrigger>
      <DropdownMenuContent>
        <DropdownMenuItem onSelect={onAdd} onClick={onAdd}>{copy.addChild}</DropdownMenuItem>
        <DropdownMenuItem onSelect={onRename} onClick={onRename}>{copy.rename}</DropdownMenuItem>
        {onMove ? <DropdownMenuItem onSelect={onMove} onClick={onMove}>{copy.move}</DropdownMenuItem> : null}
        <DropdownMenuItem onSelect={onDelete} onClick={onDelete}>{copy.delete}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function NameField({
  label,
  initial = '',
  onCommit,
  onCancel,
}: {
  label: string
  initial?: string
  onCommit: (name: string) => void
  onCancel: () => void
}) {
  const [value, setValue] = useState(initial)
  const field = useRef<HTMLInputElement>(null)
  useEffect(() => {
    field.current?.focus()
    field.current?.scrollIntoView({ block: 'nearest' })
  }, [])
  return (
    <input
      ref={field}
      className="model-rename"
      placeholder={label}
      aria-label={label}
      data-testid="structure-name"
      value={value}
      onChange={(event) => setValue(event.target.value)}
      onKeyDown={(event) => {
        const action = renameOnKey(event.key)
        if (action === 'cancel') {
          event.preventDefault()
          onCancel()
        }
        if (action === 'commit') {
          event.preventDefault()
          onCommit(value)
        }
      }}
    />
  )
}

function StructureDialog({
  defs,
  dialog,
  busy,
  onChange,
  onClose,
  onConfirm,
}: {
  defs: Json | undefined
  dialog: DialogState | null
  busy: boolean
  onChange: (next: DialogState) => void
  onClose: () => void
  onConfirm: () => void
}) {
  const nodes: MenuNode[] = dialog && dialog.kind !== 'delete-structure' ? groupNodes(defs, dialog.structure) : []
  const targets = dialog?.kind === 'delete-group' && dialog.group ? moveTargets(nodes, dialog.group) : dialog?.kind === 'move' ? moveTargets(nodes, dialog.group) : []
  const removed = dialog?.kind === 'delete-group' && dialog.group ? groupKeysToDelete(nodes, dialog.group) : []
  const count = dialog && dialog.kind !== 'move' ? dialog.count : null
  const needsMove = dialog?.kind === 'delete-group' && deletionNeedsMove(count ?? 0)
  const blocked = dialog?.kind === 'delete-structure' && deletionNeedsMove(count ?? 0)
  const confirmDisabled = busy || !dialog || (dialog.kind !== 'move' && count === null) || blocked || (needsMove && !dialog.moveTo) || (needsMove && targets.length === 0)
  return (
    <Dialog open={dialog !== null} onOpenChange={(open) => { if (!open) onClose() }}>
      <DialogContent data-testid={dialog?.kind === 'move' ? 'move-dialog' : 'delete-dialog'}>
        <DialogTitle>
          {dialog?.kind === 'move' ? copy.moveGroupTitle : dialog?.kind === 'delete-structure' ? copy.deleteStructureTitle : copy.deleteGroupTitle}
        </DialogTitle>
        <DialogDescription>{dialog?.label ?? ''}</DialogDescription>
        {dialog && dialog.kind !== 'move' ? (
          <p>{count === null ? copy.loadingModel : `${count} ${dialog.kind === 'delete-structure' ? copy.objectsInStructure : copy.objectsInGroup}`}</p>
        ) : null}
        {removed.length > 1 ? <p>{copy.groupsRemovedToo}</p> : null}
        {blocked ? <p>{copy.structureOccupiedHint}</p> : null}
        {needsMove && targets.length === 0 ? <p>{copy.nowhereToMove}</p> : null}
        {dialog?.kind === 'delete-group' && needsMove && targets.length > 0 ? (
          <label className="model-field">
            {copy.moveObjectsTo}
            <select data-testid="move-target" value={dialog.moveTo} onChange={(event) => onChange({ ...dialog, moveTo: event.target.value })}>
              <option value="">{copy.moveTo}</option>
              {targets.map((key) => <option key={key} value={key}>{structureCaption(key, defs, dialog.structure)}</option>)}
            </select>
          </label>
        ) : null}
        {dialog?.kind === 'move' ? (
          <label className="model-field">
            {copy.moveObjectsTo}
            <select data-testid="move-target" value={dialog.parent} onChange={(event) => onChange({ ...dialog, parent: event.target.value })}>
              <option value="">{copy.structureRoot}</option>
              {targets.map((key) => <option key={key} value={key}>{structureCaption(key, defs, dialog.structure)}</option>)}
            </select>
          </label>
        ) : null}
        {dialog && dialog.kind !== 'move' && dialog.error ? <p role="alert">{dialog.error}</p> : null}
        {dialog?.kind === 'move' && dialog.error ? <p role="alert">{dialog.error}</p> : null}
        <div className="dialog-actions">
          <button type="button" onClick={onClose}>{copy.cancel}</button>
          <button type="button" data-testid="structure-confirm" disabled={confirmDisabled} onClick={onConfirm}>{copy.confirm}</button>
        </div>
      </DialogContent>
    </Dialog>
  )
}
