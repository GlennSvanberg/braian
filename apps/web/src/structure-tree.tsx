import { useEffect, useRef, useState, type ReactNode, type RefObject } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { ChevronDown, ChevronRight, MoreHorizontal, Plus } from 'lucide-react'
import { ApiError, api, type Json } from './api'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from './components/ui/dialog'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from './components/ui/dropdown-menu'
import { copy } from './i18n'
import { groupNodes, structureKeys, type GroupNode, type ModelSelection } from './model-view'
import { structureCaption } from './graph-scene'
import {
  deletionNeedsMove,
  groupKeysToDelete,
  moveTargets,
  renameOnKey,
  structureKeyFromName,
  type MenuNode,
} from './structure-menu'

type Draft =
  | { kind: 'structure' }
  | { kind: 'group'; structure: string; parent?: string }
  | { kind: 'rename-structure'; structure: string; value: string }
  | { kind: 'rename-structure-key'; structure: string; value: string }
  | { kind: 'rename-group'; structure: string; group: string; value: string }
  | { kind: 'rename-group-key'; structure: string; group: string; value: string }

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
  const [expanded, setExpanded] = useState<string[]>([])
  const [draft, setDraft] = useState<Draft | null>(null)
  const nameRef = useRef<HTMLInputElement | null>(null)
  const focusName = useRef(false)
  const [dialog, setDialog] = useState<DialogState | null>(null)
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const busyRef = useRef(false)

  const fail = (reason: unknown) => setNotice(reason instanceof ApiError ? reason.message : copy.validation)

  const refresh = async () => {
    await client.invalidateQueries({ queryKey: ['definitions'] })
  }

  const proposeKey = async (name: string, args: Json) => {
    const result = await api<Json>(`/v1/operations/${name}`, {
      method: 'POST',
      body: JSON.stringify(args),
    })
    const id = String(result.proposal_id ?? result.id ?? '')
    if (id) {
      const { publishProposal } = await import('./proposal-bus')
      publishProposal(id)
    }
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

  // The dropdown restores focus to its trigger after close. Hold the name field instead.
  const holdNameFocus = (event: Event) => {
    if (!focusName.current) return
    event.preventDefault()
    focusName.current = false
    nameRef.current?.focus()
  }

  const editName = (next: Draft) => {
    focusName.current = true
    setDraft(next)
  }

  const expandGroup = (structure: string, parent: string) => {
    const node = groupNodes(defs, structure).find((item) => item.key === parent)
    if (!node) return
    setExpanded((current) => (current.includes(node.id) ? current : [...current, node.id]))
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
          inputRef={nameRef}
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
                  inputRef={nameRef}
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
              ) : draft?.kind === 'rename-structure-key' && draft.structure === key ? (
                <NameField
                  label={copy.renameKey}
                  initial={draft.value}
                  warning={copy.keyRenameWarning}
                  inputRef={nameRef}
                  onCommit={(name) => {
                    const next = name.trim()
                    if (!next || next === key) {
                      setDraft(null)
                      return
                    }
                    void run(() => proposeKey('model.rename_structure_key', { structure: key, new_key: next }))
                  }}
                  onCancel={() => setDraft(null)}
                />
              ) : (
                <strong>{title}</strong>
              )}
              <NodeMenu
                testId={`structure-menu-${key}`}
                onCloseAutoFocus={holdNameFocus}
                onAdd={() => editName({ kind: 'group', structure: key })}
                onRename={() => editName({ kind: 'rename-structure', structure: key, value: title })}
                onRenameKey={() => editName({ kind: 'rename-structure-key', structure: key, value: key })}
                onDelete={() => void run(() => proposeKey('model.delete_structure', { structure: key }))}
              />
            </div>
            {draft?.kind === 'group' && draft.structure === key && !draft.parent ? (
              <DraftRow depth={0}>
                <NameField
                  label={copy.addChild}
                  inputRef={nameRef}
                  onCommit={(name) => commitName(name, (groupKey, label) => api(`/v1/structures/${encodeURIComponent(key)}/groups`, { method: 'POST', body: JSON.stringify({ key: groupKey, label }) }).then(() => undefined))}
                  onCancel={() => setDraft(null)}
                />
              </DraftRow>
            ) : null}
            <GroupList
              defs={defs}
              structure={key}
              expanded={expanded}
              setExpanded={setExpanded}
              nameRef={nameRef}
              onCloseAutoFocus={holdNameFocus}
              selection={selection}
              onPick={onPick}
              draft={draft}
              setDraft={setDraft}
              onAdd={(parent) => {
                expandGroup(key, parent)
                editName({ kind: 'group', structure: key, parent })
              }}
              onCreate={(parent, name) => commitName(name, (groupKey, label) => api(`/v1/structures/${encodeURIComponent(key)}/groups`, {
                method: 'POST',
                body: JSON.stringify({ key: groupKey, label, parent }),
              }).then(() => undefined))}
              onRename={(group, label) => editName({ kind: 'rename-group', structure: key, group, value: label })}
              onRenameKey={(group) => editName({ kind: 'rename-group-key', structure: key, group, value: group })}
              onRenameCommit={(group, name) => {
                const label = name.trim()
                if (!label) {
                  setNotice(copy.nameRequired)
                  return
                }
                void run(() => api(`/v1/structures/${encodeURIComponent(key)}/groups/${encodeURIComponent(group)}`, { method: 'PATCH', body: JSON.stringify({ label }) }).then(() => undefined))
              }}
              onMove={(group, label, parentKey) => setDialog({ kind: 'move', structure: key, group, label, parent: parentKey ?? '', error: '' })}
              onDelete={(group) => void run(() => proposeKey('model.delete_group', { structure: key, group }))}
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
  expanded,
  setExpanded,
  nameRef,
  onCloseAutoFocus,
  selection,
  onPick,
  draft,
  setDraft,
  onAdd,
  onCreate,
  onRename,
  onRenameKey,
  onRenameCommit,
  onMove,
  onDelete,
}: {
  defs: Json | undefined
  structure: string
  expanded: string[]
  setExpanded: (update: (current: string[]) => string[]) => void
  nameRef: RefObject<HTMLInputElement | null>
  onCloseAutoFocus: (event: Event) => void
  selection: ModelSelection | null
  onPick: (next: ModelSelection) => void
  draft: Draft | null
  setDraft: (draft: Draft | null) => void
  onAdd: (parent: string) => void
  onCreate: (parent: string, name: string) => void
  onRename: (group: string, label: string) => void
  onRenameKey: (group: string) => void
  onRenameCommit: (group: string, name: string) => void
  onMove: (group: string, label: string, parentKey?: string) => void
  onDelete: (group: string, label: string) => void
}) {
  const rows = groupNodes(defs, structure)
  const visible = visibleGroups(rows, expanded)
  return (
    <>
      {visible.map((node) => {
        const open = expanded.includes(node.id)
        const on = selection?.kind === 'group' && selection.structure === structure && selection.group === node.key
        const hasChildren = rows.some((item) => item.parentKey === node.key)
        const label = structureCaption(node.key, defs, structure)
        const renaming = draft?.kind === 'rename-group' && draft.structure === structure && draft.group === node.key
        const keyDraft = draft?.kind === 'rename-group-key' && draft.structure === structure && draft.group === node.key ? draft : null
        const adding = draft?.kind === 'group' && draft.structure === structure && draft.parent === node.key
        return (
          <div key={node.id} data-testid={`group-${node.key}`}>
            <div className="model-row" style={{ paddingLeft: node.depth * 14 }}>
              {renaming ? (
                <NameField label={copy.rename} initial={draft.value} inputRef={nameRef} onCommit={(name) => onRenameCommit(node.key, name)} onCancel={() => setDraft(null)} />
              ) : keyDraft ? (
                <NameField
                  label={copy.renameKey}
                  initial={keyDraft.value}
                  warning={copy.keyRenameWarning}
                  inputRef={nameRef}
                  onCommit={(name) => {
                    const next = name.trim()
                    setDraft(null)
                    if (!next || next === node.key) return
                    void api<Json>('/v1/operations/model.rename_group_key', {
                      method: 'POST',
                      body: JSON.stringify({ structure, group: node.key, new_key: next }),
                    }).then(async (result) => {
                      const id = String(result.proposal_id ?? result.id ?? '')
                      if (!id) return
                      const { publishProposal } = await import('./proposal-bus')
                      publishProposal(id)
                    })
                  }}
                  onCancel={() => setDraft(null)}
                />
              ) : (
                <button
                  type="button"
                  className={on ? 'model-node on' : 'model-node'}
                  aria-expanded={hasChildren ? open : undefined}
                  onClick={() => onPick({ kind: 'group', structure, group: node.key })}
                >
                  {hasChildren ? (
                    <span
                      className="twist"
                      onClick={(event) => {
                        event.stopPropagation()
                        setExpanded((current) => (current.includes(node.id) ? current.filter((id) => id !== node.id) : [...current, node.id]))
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
                onCloseAutoFocus={onCloseAutoFocus}
                onAdd={() => onAdd(node.key)}
                onRename={() => onRename(node.key, label)}
                onRenameKey={() => onRenameKey(node.key)}
                onMove={() => onMove(node.key, label, node.parentKey)}
                onDelete={() => onDelete(node.key, label)}
              />
            </div>
            {adding ? (
              <DraftRow depth={node.depth + 1}>
                <NameField label={copy.addChild} inputRef={nameRef} onCommit={(name) => onCreate(node.key, name)} onCancel={() => setDraft(null)} />
              </DraftRow>
            ) : null}
          </div>
        )
      })}
    </>
  )
}

/** A node shows when every ancestor is expanded. The expanded set starts empty. */
function visibleGroups(rows: GroupNode[], expanded: string[]) {
  const parents = new Set(rows.flatMap((node) => (node.parentKey ? [node.parentKey] : [])))
  let skipDepth = -1
  return rows.filter((node) => {
    if (skipDepth >= 0) {
      if (node.depth > skipDepth) return false
      skipDepth = -1
    }
    if (parents.has(node.key) && !expanded.includes(node.id)) skipDepth = node.depth
    return true
  })
}

function DraftRow({ depth, children }: { depth: number; children: ReactNode }) {
  return (
    <div className="model-row" data-testid="group-draft" style={{ paddingLeft: depth * 14 }}>
      <span className="twist" aria-hidden="true" />
      {children}
    </div>
  )
}

function NodeMenu({
  testId,
  onAdd,
  onRename,
  onRenameKey,
  onMove,
  onDelete,
  onCloseAutoFocus,
}: {
  testId: string
  onAdd: () => void
  onRename: () => void
  onRenameKey?: () => void
  onMove?: () => void
  onDelete: () => void
  onCloseAutoFocus: (event: Event) => void
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger className="icon-btn" aria-label={copy.moreActions} data-testid={testId}>
        <MoreHorizontal size={14} />
      </DropdownMenuTrigger>
      <DropdownMenuContent onCloseAutoFocus={onCloseAutoFocus}>
        <DropdownMenuItem onSelect={onAdd} onClick={onAdd}>{copy.addChild}</DropdownMenuItem>
        <DropdownMenuItem onSelect={onRename} onClick={onRename}>{copy.rename}</DropdownMenuItem>
        {onRenameKey ? <DropdownMenuItem onSelect={onRenameKey} onClick={onRenameKey}>{copy.renameKey}</DropdownMenuItem> : null}
        {onMove ? <DropdownMenuItem onSelect={onMove} onClick={onMove}>{copy.move}</DropdownMenuItem> : null}
        <DropdownMenuItem onSelect={onDelete} onClick={onDelete}>{copy.deleteMenu}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

function NameField({
  label,
  initial = '',
  warning,
  onCommit,
  onCancel,
  inputRef,
}: {
  label: string
  initial?: string
  warning?: string
  onCommit: (name: string) => void
  onCancel: () => void
  inputRef?: RefObject<HTMLInputElement | null>
}) {
  const [value, setValue] = useState(initial)
  const field = useRef<HTMLInputElement>(null)
  const setField = (node: HTMLInputElement | null) => {
    field.current = node
    if (inputRef) inputRef.current = node
  }
  useEffect(() => {
    const node = field.current
    if (!node) return
    const focus = () => {
      node.focus()
      node.scrollIntoView({ block: 'nearest' })
    }
    focus()
    // Radix returns focus to the menu trigger in a timeout after close.
    const timer = window.setTimeout(focus, 0)
    return () => window.clearTimeout(timer)
  }, [])
  return (
    <>
    <input
      ref={setField}
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
    {warning && value.trim() && value.trim() !== initial ? (
      <p className="warn" data-testid="key-rename-warning">{warning.replace('{key}', initial)}</p>
    ) : null}
    </>
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
