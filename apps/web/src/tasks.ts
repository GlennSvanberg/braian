/** Task boundaries. The same rules run in `crates/telemetry`. */

export type ActorKind = 'human' | 'agent' | 'integration'
export type Explicit = 'import' | 'mass_edit' | 'command' | 'search'

export type Observe = {
  atMs: number
  operation: string
  actor: ActorKind
  objectIds: string[]
  explicit?: Explicit
}

export type Cursor = {
  taskId: string
  taskType: string
  objects: string[]
  lastAtMs: number
  sawSearch: boolean
  sticky: boolean
}

export const TASK_TYPES = [
  'import',
  'mass_edit',
  'enrich',
  'search',
  'navigate',
  'review',
  'model',
  'agent',
  'other',
] as const

const CHANGES = new Set([
  'create_object',
  'patch_object',
  'mass_edit',
  'rollback',
  'set_relation',
  'accept_proposal',
  'bulk',
  'import',
])

const NAVIGATE = new Set([
  'ui.open_view',
  'ui.open_template',
  'ui.open_workspace',
  'ui.place',
  'ui.link',
  'ui.group',
  'ui.sort',
  'ui.show_column',
  'ui.hide_column',
  'ui.move_column',
  'ui.set_theme',
  'ui.export',
  'open_view',
  'close_view',
  'move_view',
  'move_view_left',
  'move_view_right',
  'move_view_up',
  'move_view_down',
  'split',
  'resize',
  'swap',
  'focus',
  'zoom',
  'get_layout',
  'reset_layout',
])

export function canonicalTaskType(value: string): string {
  return (TASK_TYPES as readonly string[]).includes(value) ? value : 'other'
}

export function isChange(operation: string): boolean {
  return CHANGES.has(operation) || operation.startsWith('model.')
}

type Step = {
  started: boolean
  ambiguous: boolean
  taskType: string
  objects: string[]
  lastAtMs: number
  sawSearch: boolean
  sticky: boolean
}

export function apply(
  cursor: Cursor | null,
  event: Observe,
  gapMs: number,
  newId: () => string,
): { cursor: Cursor; ambiguous: boolean } {
  const step = advance(cursor, event, gapMs)
  const taskId = step.started ? newId() : cursor ? cursor.taskId : newId()
  return {
    cursor: {
      taskId,
      taskType: step.taskType,
      objects: step.objects,
      lastAtMs: step.lastAtMs,
      sawSearch: step.sawSearch,
      sticky: step.sticky,
    },
    ambiguous: step.ambiguous,
  }
}

function advance(cursor: Cursor | null, event: Observe, gapMs: number): Step {
  const proposed = taskTypeFor(event)
  const stale = cursor == null || event.atMs - cursor.lastAtMs > gapMs
  if (event.actor === 'agent') {
    if (cursor && !stale) {
      const taskType = isChange(event.operation) && ['search', 'navigate', 'other'].includes(cursor.taskType)
        ? 'enrich'
        : cursor.taskType
      return cont(cursor, event, taskType, false)
    }
    return start(event, proposed === 'other' ? 'agent' : proposed, true)
  }
  if (stale || !cursor) return start(event, proposed, false)
  const overlap = overlaps(cursor.objects, event.objectIds)
  const explicitBoundary = event.explicit === 'import'
    || event.explicit === 'mass_edit'
    || event.operation === 'bulk'
    || event.operation === 'import'
    || event.operation === 'mass_edit'
  if (explicitBoundary) {
    const followUp = cursor.sticky
      && overlap
      && event.operation !== 'bulk'
      && event.operation !== 'import'
      && event.operation !== 'mass_edit'
    if (followUp) return cont(cursor, event, cursor.taskType, false)
    return start(event, proposed, false)
  }
  if (event.explicit === 'command') {
    if (overlap || (event.objectIds.length === 0 && cursor.sticky)) {
      return cont(cursor, event, mergeType(cursor.taskType, proposed), false)
    }
    return start(event, proposed === 'navigate' ? 'agent' : proposed, false)
  }
  if (
    cursor.sawSearch
    && cursor.objects.length === 0
    && (isChange(event.operation) || event.operation === 'get_object' || event.operation === 'ui.select')
  ) {
    return cont(cursor, event, 'enrich', false)
  }
  if (cursor.sticky) {
    if (event.objectIds.length === 0 || overlap || cursor.objects.length === 0) {
      const ambiguous = event.operation === 'search' && event.objectIds.length > 0 && !overlap
      return cont(cursor, event, cursor.taskType, ambiguous)
    }
    return start(event, proposed, true)
  }
  if (overlap || event.objectIds.length === 0) {
    let taskType = cursor.taskType
    if (isChange(event.operation) && ['search', 'navigate', 'other'].includes(cursor.taskType)) taskType = 'enrich'
    else if (proposed === 'navigate') taskType = cursor.taskType
    else if (cursor.taskType === 'other') taskType = proposed
    return cont(cursor, event, taskType, false)
  }
  const ambiguous = event.operation === 'get_object' || event.operation === 'ui.select'
  return start(event, proposed, ambiguous)
}

function start(event: Observe, taskType: string, ambiguous: boolean): Step {
  return {
    started: true,
    ambiguous,
    taskType,
    objects: event.objectIds.slice(0, 64),
    lastAtMs: event.atMs,
    sawSearch: isSearch(event),
    sticky: taskType === 'import',
  }
}

function cont(cursor: Cursor, event: Observe, taskType: string, ambiguous: boolean): Step {
  const objects = [...cursor.objects]
  for (const id of event.objectIds) {
    if (!objects.includes(id)) objects.push(id)
  }
  return {
    started: false,
    ambiguous,
    taskType,
    objects: objects.slice(0, 64),
    lastAtMs: event.atMs,
    sawSearch: cursor.sawSearch || isSearch(event),
    sticky: cursor.sticky || taskType === 'import',
  }
}

function taskTypeFor(event: Observe): string {
  if (event.explicit === 'import' || event.operation === 'bulk' || event.operation === 'import') return 'import'
  if (event.explicit === 'mass_edit' || event.operation === 'mass_edit') return 'mass_edit'
  if (event.operation.startsWith('model.')) return 'model'
  if (isSearch(event)) return 'search'
  if (['history', 'rollback', 'accept_proposal', 'discard_proposal', 'impact'].includes(event.operation)) return 'review'
  if (NAVIGATE.has(event.operation)) return 'navigate'
  if (event.operation.startsWith('agent.')) return 'agent'
  if (['patch_object', 'create_object', 'set_relation', 'get_object', 'ui.select', 'ui.select_rows'].includes(event.operation)) {
    return 'enrich'
  }
  if (event.actor === 'agent') return 'agent'
  return 'other'
}

function isSearch(event: Observe): boolean {
  return event.explicit === 'search' || event.operation === 'search' || event.operation === 'ui.filter' || event.operation === 'list_objects'
}

function overlaps(left: string[], right: string[]): boolean {
  if (left.length === 0 || right.length === 0) return false
  return right.some((id) => left.includes(id))
}

function mergeType(current: string, proposed: string): string {
  if (current === 'import' || current === 'mass_edit') return current
  if (proposed === 'navigate') return current
  if (['search', 'navigate', 'other'].includes(current) && ['enrich', 'model', 'review', 'agent'].includes(proposed)) return proposed
  return current
}
