import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, api, type Json } from './api'
import { attributeCards, classRows } from './class-view'
import { copy, uiLanguage } from './i18n'
import { publishProposal } from './proposal-bus'
import { StateNote } from './states'

type View = { state: Json }

let classSelection = ''
let cutClass = ''
const classListeners = new Set<(key: string) => void>()

function chooseClass(key: string) {
  classSelection = key
  for (const listen of classListeners) listen(key)
}

function useClassSelection() {
  const [key, setKey] = useState(classSelection)
  useEffect(() => {
    classListeners.add(setKey)
    return () => {
      classListeners.delete(setKey)
    }
  }, [])
  return key
}

async function propose(name: string, args: Json) {
  const result = await api<Json>(`/v1/operations/${name}`, {
    method: 'POST',
    body: JSON.stringify(args),
  })
  const id = String(result.proposal_id ?? result.id ?? '')
  if (id) publishProposal(id)
  return result
}

export function DataModel({ view }: { view: View }) {
  const defs = useQuery({ queryKey: ['definitions'], queryFn: () => api<Json>('/v1/definitions') })
  const mode = String(view.state.mode ?? '')
  if (Array.isArray(view.state.checklist)) return <Checklist defs={defs.data} items={view.state.checklist as string[]} />
  if (defs.isPending) return <div data-testid="data-model"><StateNote tone="loading" title={copy.loadingModel} /></div>
  if (defs.isError) return <div data-testid="data-model"><StateNote tone="error" title={copy.errorModel} detail={copy.validation} /></div>
  if (mode === 'classes') return <ClassList defs={defs.data} />
  if (mode === 'card' || mode === 'editor') return <ClassCard defs={defs.data} />
  if (mode === 'impact') return <ImpactNote />
  return <StructureTree defs={defs.data} />
}

function Checklist({ defs, items }: { defs: Json | undefined; items: string[] }) {
  const languages = ((defs?.languages as string[]) ?? []).join(', ')
  const units = ((defs?.unit_families as Json[]) ?? []).map((item) => String(item.key)).join(', ')
  const types = ((defs?.types as Json[]) ?? []).map((item) => String(item.key)).join(', ')
  return (
    <div data-testid="data-model">
      <strong>{copy.dataModel}</strong>
      <ul>
        {items.map((item) => (
          <li key={item}>{item}</li>
        ))}
      </ul>
      <p>sv/en/de · {languages}</p>
      <p>{units}</p>
      <p>{types}</p>
    </div>
  )
}

type FlatNode = {
  kind: 'group' | 'class'
  structure: string
  groupId: string
  groupKey: string
  classKey?: string
}

function StructureTree({ defs }: { defs: Json | undefined }) {
  const selectedClass = useClassSelection()
  const [message, setMessage] = useState('')
  const [over, setOver] = useState('')
  const [focus, setFocus] = useState(0)
  const [menu, setMenu] = useState<{ x: number; y: number; classKey: string; structure: string } | null>(null)
  const structures = (defs?.structures as Json[]) ?? []
  const nodes = flatten(structures)
  const focused = nodes[focus]
  const move = (structure: string, classKey: string, groupKey: string) => {
    setMessage('')
    void propose('model.move_class', { structure, class: classKey, group: groupKey }).catch((reason: unknown) => {
      setMessage(reason instanceof ApiError ? reason.message : copy.validation)
    })
  }
  return (
    <div
      data-testid="data-model"
      className="model-tree"
      tabIndex={0}
      onKeyDown={(event) => {
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          event.preventDefault()
          setFocus((current) => {
            const next = event.key === 'ArrowDown' ? current + 1 : current - 1
            return Math.max(0, Math.min(nodes.length - 1, next))
          })
          return
        }
        if (!focused) return
        if (event.key === 'Enter' && focused.kind === 'class' && focused.classKey) {
          event.preventDefault()
          chooseClass(focused.classKey)
        }
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'x' && focused.kind === 'class' && focused.classKey) {
          event.preventDefault()
          cutClass = focused.classKey
          setMessage(`${copy.cut} ${focused.classKey}`)
        }
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v' && focused.kind === 'group' && cutClass) {
          event.preventDefault()
          move(focused.structure, cutClass, focused.groupKey)
          cutClass = ''
        }
      }}
    >
      <p className="hint">{copy.dataModel}</p>
      {structures.length === 0 ? <StateNote tone="empty" title={copy.emptyModel} /> : null}
      {structures.map((structure) => (
        <StructureBranch
          key={String(structure.key)}
          structure={structure}
          selectedClass={selectedClass}
          focus={focused}
          over={over}
          onOver={setOver}
          onFocus={(node) => setFocus(Math.max(0, nodes.findIndex((item) => sameNode(item, node))))}
          onMove={move}
          onMenu={setMenu}
        />
      ))}
      {message ? <p>{message}</p> : null}
      {menu ? (
        <div className="menu" style={{ left: menu.x, top: menu.y }} data-testid="class-menu">
          <p className="hint">{copy.moveTo}</p>
          {groupsOf(structures, menu.structure).map((group) => (
            <button
              key={group.id}
              type="button"
              onClick={() => {
                move(menu.structure, menu.classKey, group.key)
                setMenu(null)
              }}
            >
              {copy.moveTo} {group.key}
            </button>
          ))}
          <button type="button" onClick={() => setMenu(null)}>{copy.close}</button>
        </div>
      ) : null}
    </div>
  )
}

function StructureBranch({
  structure,
  selectedClass,
  focus,
  over,
  onOver,
  onFocus,
  onMove,
  onMenu,
}: {
  structure: Json
  selectedClass: string
  focus: FlatNode | undefined
  over: string
  onOver: (id: string) => void
  onFocus: (node: FlatNode) => void
  onMove: (structure: string, classKey: string, groupKey: string) => void
  onMenu: (menu: { x: number; y: number; classKey: string; structure: string } | null) => void
}) {
  const groups = (structure.groups as Json[]) ?? []
  const classes = (structure.classes as Json[]) ?? []
  const structureKey = String(structure.key)
  const renderGroup = (group: Json, depth: number) => {
    const groupId = String(group.id)
    const groupKey = String(group.key)
    const node: FlatNode = { kind: 'group', structure: structureKey, groupId, groupKey }
    const here = classes.filter((item) => String(item.group) === groupId)
    return (
      <div key={groupId}>
        <div
          data-testid={`group-${groupKey}`}
          className={over === groupId ? 'drop-on' : undefined}
          style={{ paddingLeft: depth * 12 }}
          onDragOver={(event) => {
            event.preventDefault()
            onOver(groupId)
          }}
          onDragLeave={() => onOver('')}
          onDrop={(event) => {
            const classKey = event.dataTransfer.getData('text/class')
            onOver('')
            if (!classKey) return
            event.preventDefault()
            const current = classes.find((item) => String(item.key) === classKey)
            if (current && String(current.group) === groupId) return
            onMove(structureKey, classKey, groupKey)
          }}
        >
          <span
            className={focus && sameNode(focus, node) ? 'on' : undefined}
            onClick={() => onFocus(node)}
          >
            {groupKey}
          </span>
          {here.map((item) => {
            const classKey = String(item.key)
            const classNode: FlatNode = { kind: 'class', structure: structureKey, groupId, groupKey, classKey }
            return (
              <button
                key={classKey}
                type="button"
                draggable
                className={selectedClass === classKey || (focus && sameNode(focus, classNode)) ? 'on' : ''}
                onClick={() => {
                  chooseClass(classKey)
                  onFocus(classNode)
                }}
                onContextMenu={(event) => {
                  event.preventDefault()
                  chooseClass(classKey)
                  onMenu({ x: event.clientX, y: event.clientY, classKey, structure: structureKey })
                }}
                onDragStart={(event) => {
                  event.dataTransfer.setData('text/class', classKey)
                  event.dataTransfer.effectAllowed = 'move'
                }}
              >
                {classKey}
              </button>
            )
          })}
        </div>
        {groups.filter((item) => String(item.parent ?? '') === groupId).map((child) => renderGroup(child, depth + 1))}
      </div>
    )
  }
  const roots = groups.filter((group) => !group.parent)
  return (
    <div className="tree">
      <strong>{structureKey}</strong>
      {roots.map((group) => renderGroup(group, 0))}
    </div>
  )
}

function ClassList({ defs }: { defs: Json | undefined }) {
  const selected = useClassSelection()
  const rows = classRows(defs)
  if (rows.length === 0) {
    return (
      <div data-testid="data-model">
        <StateNote tone="empty" title={copy.emptyModel} />
      </div>
    )
  }
  return (
    <div data-testid="data-model" className="class-list">
      {rows.map((row) => {
        const cards = attributeCards(defs, row.key, uiLanguage)
        const own = cards.filter((card) => !card.inherited).length
        const inherited = cards.length - own
        return (
          <button
            key={`${row.structure}:${row.key}`}
            type="button"
            className={selected === row.key ? 'class-row on' : 'class-row'}
            style={{ paddingLeft: 8 + row.depth * 14 }}
            onClick={() => chooseClass(row.key)}
          >
            <span>{row.key}</span>
            {row.parentKey ? <small>{copy.parent} {row.parentKey}</small> : null}
            <span className="chip">{copy.own} {own}</span>
            <span className="chip">{copy.inherited} {inherited}</span>
          </button>
        )
      })}
    </div>
  )
}

function ClassCard({ defs }: { defs: Json | undefined }) {
  const selected = useClassSelection()
  const [key, setKey] = useState('')
  const [kind, setKind] = useState('number')
  const [unit, setUnit] = useState('kg')
  const [required, setRequired] = useState(false)
  const [multiple, setMultiple] = useState(false)
  const [languages, setLanguages] = useState(false)
  const [message, setMessage] = useState('')
  const found = findClass(defs, selected)
  if (!found) {
    return (
      <div data-testid="data-model">
        <div data-testid="class-editor">
          <StateNote tone="empty" title={copy.pickClass} />
        </div>
      </div>
    )
  }
  const cards = attributeCards(defs, found.key, uiLanguage)
  const ownKeys = cards.filter((card) => !card.inherited).map((card) => card.key)
  const classes = classRows(defs).filter((row) => row.structure === found.structure && row.key !== found.key)
  const run = (name: string, args: Json) => {
    setMessage('')
    void propose(name, args).catch((reason: unknown) => setMessage(reason instanceof ApiError ? reason.message : copy.validation))
  }
  const add = () => {
    const attributeKey = key.trim()
    if (!attributeKey) return
    const exists = ((defs?.attributes as Json[]) ?? []).some((item) => String(item.key) === attributeKey)
    if (exists) {
      run('model.add_attribute', { structure: found.structure, class: found.key, attribute: attributeKey })
      return
    }
    run('model.draft', {
      title: `${copy.addAttribute} ${attributeKey}`,
      steps: [
        {
          op: 'create_attribute',
          key: attributeKey,
          label: attributeKey,
          kind,
          unit: kind === 'number' || kind === 'range' ? unit.trim() || null : null,
          multiple,
          languages,
          required,
        },
        { op: 'add_attribute', structure: found.structure, class: found.key, attribute: attributeKey },
      ],
    })
  }
  const reorder = (index: number, delta: number) => {
    const next = [...ownKeys]
    const target = index + delta
    if (target < 0 || target >= next.length) return
    const [item] = next.splice(index, 1)
    if (!item) return
    next.splice(target, 0, item)
    run('model.reorder_attributes', { structure: found.structure, class: found.key, attributes: next })
  }
  return (
    <div data-testid="data-model">
      <div data-testid="class-editor" className="class-card">
        <div className="toolbar">
          <strong>{found.key}</strong>
          <span className="chip">{found.structure}</span>
        </div>
        <label className="field">
          <span>{copy.parent}</span>
          <select
            value={found.parentKey ?? ''}
            onChange={(event) =>
              run('model.set_parent', {
                structure: found.structure,
                class: found.key,
                parent: event.target.value || null,
              })
            }
          >
            <option value="" />
            {classes.map((row) => (
              <option key={row.key} value={row.key}>{row.key}</option>
            ))}
          </select>
        </label>
        {cards.map((card) => {
          const ownIndex = ownKeys.indexOf(card.key)
          return (
            <article key={card.key} className="attr-card" data-inherited={card.inherited ? 'true' : 'false'}>
              <header>
                <strong>{card.label}</strong>
                <span className="chip">{card.inherited ? copy.inherited : copy.own}</span>
                {card.from ? <small>{card.from}</small> : null}
              </header>
              <div className="choices">
                <span className="chip">{card.kind}</span>
                {card.unit ? <span className="chip">{card.unit}</span> : null}
                {card.multiple ? <span className="chip">{copy.multiValue}</span> : null}
                {card.languages ? <span className="chip">{copy.languages}</span> : null}
                {card.required ? <span className="chip">{copy.required}</span> : null}
              </div>
              {card.inherited ? null : (
                <div className="toolbar">
                  <button type="button" onClick={() => reorder(ownIndex, -1)}>↑</button>
                  <button type="button" onClick={() => reorder(ownIndex, 1)}>↓</button>
                  <button
                    type="button"
                    onClick={() => run('model.remove_attribute', { structure: found.structure, class: found.key, attribute: card.key })}
                  >
                    {copy.discard}
                  </button>
                </div>
              )}
            </article>
          )
        })}
        <form
          className="toolbar"
          onSubmit={(event) => {
            event.preventDefault()
            add()
          }}
        >
          <input value={key} placeholder={copy.addAttribute} onChange={(event) => setKey(event.target.value)} />
          <select value={kind} onChange={(event) => setKind(event.target.value)}>
            <option value="text">text</option>
            <option value="number">number</option>
            <option value="boolean">boolean</option>
            <option value="date">date</option>
          </select>
          <input value={unit} placeholder={copy.unit} onChange={(event) => setUnit(event.target.value)} />
          <label><input type="checkbox" checked={required} onChange={(event) => setRequired(event.target.checked)} /> {copy.required}</label>
          <label><input type="checkbox" checked={multiple} onChange={(event) => setMultiple(event.target.checked)} /> {copy.multiValue}</label>
          <label><input type="checkbox" checked={languages} onChange={(event) => setLanguages(event.target.checked)} /> {copy.languages}</label>
          <button className="primary" type="submit">{copy.addAttribute}</button>
        </form>
        {message ? <p className="error">{message}</p> : null}
      </div>
    </div>
  )
}

function ImpactNote() {
  const queryClient = useQueryClient()
  const [attribute, setAttribute] = useState('status')
  const [result, setResult] = useState<Json | null>(null)
  return (
    <div data-testid="data-model">
      <p className="hint">{copy.reviewChange}</p>
      <form
        className="toolbar"
        onSubmit={(event) => {
          event.preventDefault()
          void api<Json>('/v1/impact', { method: 'POST', body: JSON.stringify({ remove_attribute: attribute }) })
            .then((body) => {
              setResult(body)
              void queryClient.invalidateQueries({ queryKey: ['definitions'] })
            })
        }}
      >
        <input data-testid="impact-attribute" value={attribute} onChange={(event) => setAttribute(event.target.value)} />
        <button type="submit" data-testid="impact-run">{copy.calculate}</button>
      </form>
      {result ? (
        <div data-testid="impact-result">
          <p>{copy.affected}: {String(result.affected_objects)}</p>
          <p>{copy.samples}: {((result.samples as Json[]) ?? []).map((item) => String(item.key)).join(', ')}</p>
          {((result.jobs as Json[]) ?? []).map((job) => (
            <p key={String(job.key)}>{String(job.title)}</p>
          ))}
        </div>
      ) : null}
    </div>
  )
}

function findClass(defs: Json | undefined, classKey: string) {
  if (!classKey) return null
  const row = classRows(defs).find((item) => item.key === classKey)
  return row ?? null
}

function groupsOf(structures: Json[], structureKey: string) {
  const structure = structures.find((item) => String(item.key) === structureKey)
  return ((structure?.groups as Json[]) ?? []).map((group) => ({ id: String(group.id), key: String(group.key) }))
}

function flatten(structures: Json[]): FlatNode[] {
  const nodes: FlatNode[] = []
  for (const structure of structures) {
    const groups = (structure.groups as Json[]) ?? []
    const classes = (structure.classes as Json[]) ?? []
    const structureKey = String(structure.key)
    const walk = (group: Json) => {
      const groupId = String(group.id)
      const groupKey = String(group.key)
      nodes.push({ kind: 'group', structure: structureKey, groupId, groupKey })
      for (const item of classes.filter((row) => String(row.group) === groupId)) {
        nodes.push({ kind: 'class', structure: structureKey, groupId, groupKey, classKey: String(item.key) })
      }
      for (const child of groups.filter((row) => String(row.parent ?? '') === groupId)) walk(child)
    }
    for (const group of groups.filter((row) => !row.parent)) walk(group)
  }
  return nodes
}

function sameNode(left: FlatNode, right: FlatNode) {
  return left.kind === right.kind && left.groupId === right.groupId && left.classKey === right.classKey
}
