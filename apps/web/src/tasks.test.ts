import { describe, expect, it } from 'vitest'
import { apply, type ActorKind, type Explicit, type Observe } from './tasks'

function event(at: number, operation: string, actor: ActorKind, ids: string[], explicit?: Explicit): Observe {
  return { atMs: at, operation, actor, objectIds: ids, explicit }
}

function play(events: Observe[]) {
  let cursor = null as ReturnType<typeof apply>['cursor'] | null
  let seq = 0
  const out: Array<[string, string]> = []
  for (const item of events) {
    const next = apply(cursor, item, 300_000, () => {
      seq += 1
      return `t${seq}`
    })
    out.push([next.cursor.taskId, next.cursor.taskType])
    cursor = next.cursor
  }
  return out
}

describe('task boundaries', () => {
  it('keeps search then edit as one enrich task', () => {
    const steps = play([
      event(0, 'search', 'human', [], 'search'),
      event(2_000, 'get_object', 'human', ['a']),
      event(4_000, 'patch_object', 'human', ['a']),
    ])
    expect(steps[0]?.[1]).toBe('search')
    expect(steps[1]?.[0]).toBe(steps[0]?.[0])
    expect(steps[1]?.[1]).toBe('enrich')
    expect(steps[2]?.[0]).toBe(steps[0]?.[0])
    expect(steps[2]?.[1]).toBe('enrich')
  })

  it('keeps edits on imported objects in the import task', () => {
    const steps = play([
      event(0, 'bulk', 'integration', ['a', 'b'], 'import'),
      event(5_000, 'patch_object', 'human', ['b']),
      event(6_000, 'ui.open_view', 'human', []),
    ])
    expect(steps[0]?.[1]).toBe('import')
    expect(steps[1]?.[0]).toBe(steps[0]?.[0])
    expect(steps[2]?.[0]).toBe(steps[0]?.[0])
    expect(steps[2]?.[1]).toBe('import')
  })

  it('starts a mass edit as its own task', () => {
    const steps = play([
      event(0, 'bulk', 'human', ['a'], 'import'),
      event(1_000, 'mass_edit', 'human', ['a'], 'mass_edit'),
    ])
    expect(steps[0]?.[0]).not.toBe(steps[1]?.[0])
    expect(steps[1]?.[1]).toBe('mass_edit')
  })

  it('ends the task when the objects are unrelated', () => {
    const steps = play([
      event(0, 'get_object', 'human', ['a']),
      event(1_000, 'patch_object', 'human', ['a']),
      event(2_000, 'get_object', 'human', ['z']),
    ])
    expect(steps[0]?.[0]).toBe(steps[1]?.[0])
    expect(steps[1]?.[0]).not.toBe(steps[2]?.[0])
  })

  it('starts a new task after the inactivity gap', () => {
    const steps = play([
      event(0, 'patch_object', 'human', ['a']),
      event(300_001, 'patch_object', 'human', ['a']),
    ])
    expect(steps[0]?.[0]).not.toBe(steps[1]?.[0])
  })

  it('joins agent steps to the human task', () => {
    const steps = play([
      event(0, 'search', 'human', [], 'search'),
      event(500, 'ui.filter', 'agent', []),
      event(800, 'patch_object', 'agent', ['a']),
    ])
    expect(steps[0]?.[0]).toBe(steps[1]?.[0])
    expect(steps[1]?.[0]).toBe(steps[2]?.[0])
    expect(steps[2]?.[1]).toBe('enrich')
  })

  it('starts a task when a command targets other objects', () => {
    const steps = play([
      event(0, 'get_object', 'human', ['a']),
      event(1_000, 'ui.open_view', 'human', [], 'command'),
    ])
    expect(steps[0]?.[0]).not.toBe(steps[1]?.[0])
  })
})
