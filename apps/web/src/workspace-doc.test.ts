import { describe, expect, it } from 'vitest'
import { tabGroup } from './layout'
import { canonicalDoc, editsArePersonal, reconcileLayout, type ServerLayout, type StoredLayout } from './workspace-doc'

const canonical = canonicalDoc({
  id: 'ws',
  name: 'Enrich',
  owner_id: 'owner',
  layout: { direction: 'row', ratio: 0.5, first: { tabs: ['a'] }, second: { tabs: ['b'] } },
})

describe('reconcileLayout', () => {
  it('paints a newer local document ahead of the server', () => {
    const local: StoredLayout = {
      document: { ...canonical, layout: tabGroup(['a', 'b', 'c']) },
      savedAt: Date.parse('2026-10-25T12:00:00Z'),
    }
    const server: ServerLayout = {
      document: canonical,
      updated_at: '2026-10-25T11:00:00.000Z',
    }
    expect(reconcileLayout(local, server, canonical).layout).toEqual(tabGroup(['a', 'b', 'c']))
  })

  it('uses the server when it is newer, and the canonical layout when neither side has a document', () => {
    const server: ServerLayout = {
      document: { ...canonical, layout: tabGroup(['z']), focus: 'z' },
      updated_at: '2026-10-25T13:00:00.000Z',
    }
    const local: StoredLayout = { document: canonical, savedAt: Date.parse('2026-10-25T12:00:00Z') }
    expect(reconcileLayout(local, server, canonical).focus).toBe('z')
    expect(reconcileLayout(null, { document: null, updated_at: null }, canonical).layout).toEqual(canonical.layout)
  })
})

describe('editsArePersonal', () => {
  it('copies a locked or shared workspace and writes an unlocked private one back', () => {
    expect(editsArePersonal({ locked: true, share_token: null })).toBe(true)
    expect(editsArePersonal({ locked: false, share_token: 'braian' })).toBe(true)
    expect(editsArePersonal({ locked: false, share_token: null })).toBe(false)
  })
})
