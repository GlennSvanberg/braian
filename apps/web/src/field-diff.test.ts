import { describe, expect, it } from 'vitest'
import { diffLines, historyTitle, relativeTime, sourceLabel, suggestionLines, valuesFromKept } from './field-diff'

const defs = {
  attributes: [
    { id: 'namn', key: 'namn', labels: { en: 'Name', sv: 'Namn' } },
    { id: 'status', key: 'status', labels: { en: 'Status' }, kind: { type: 'value_list', list: 'status' } },
  ],
  value_lists: [{ key: 'status', options: [{ code: 'ok', labels: { en: 'OK' } }, { code: 'draft', labels: { en: 'Draft' } }] }],
  structures: [
    {
      key: 'sortiment',
      groups: [{ id: 'g-talt', key: 'talt' }, { id: 'g-virke', key: 'virke' }],
    },
  ],
}

describe('diff lines', () => {
  it('shows only the language that changed', () => {
    const lines = diffLines(
      {
        namn: {
          before: { sv: 'Regnjacka skal' },
          after: { sv: 'Regnjacka skalad' },
        },
      },
      defs,
    )
    expect(lines).toEqual([
      { id: 'namn:sv', label: 'Name (sv)', before: 'Regnjacka skal', after: 'Regnjacka skalad' },
    ])
  })

  it('keeps each changed language on its own line', () => {
    const lines = diffLines(
      {
        namn: {
          before: { sv: 'Skal', en: 'Shell' },
          after: { sv: 'Skalad', en: 'Shell' },
        },
      },
      defs,
    )
    expect(lines.map((line) => line.label)).toEqual(['Name (sv)'])
    expect(lines[0]?.after).toBe('Skalad')
  })

  it('names a create after the person and collapses the values', () => {
    expect(historyTitle({ op: 'create', actor: 'Ada' })).toBe('Created by Ada')
    expect(historyTitle({ op: 'create', actor: 'bot', on_behalf_of: 'Ada', via: 'agent' })).toBe('Created by Ada via agent')
    expect(historyTitle({ op: 'update', actor: 'import-job', via: 'import' })).toBe('import-job via import')
    expect(diffLines({ $created: { version: 1 }, namn: { after: { sv: 'Tält' } } }, defs)).toEqual([])
  })

  it('formats a plain value and a group move', () => {
    const lines = diffLines(
      {
        status: { before: 'draft', after: 'ok' },
        $groups: { sortiment: { before: 'g-talt', after: 'g-virke' } },
      },
      defs,
    )
    expect(lines.map((line) => `${line.label}: ${line.before} -> ${line.after}`)).toEqual([
      'Status: Draft -> OK',
      'Group: Tents -> Timber',
    ])
  })

  it('uses a relative time', () => {
    const now = Date.parse('2026-10-25T12:00:00Z')
    expect(relativeTime('2026-10-25T10:00:00Z', now)).toBe('2 hours ago')
  })
})

describe('suggestion lines', () => {
  const colourDefs = {
    ...defs,
    attributes: [
      ...defs.attributes,
      { id: 'farg', key: 'farg', labels: { en: 'Colour' }, multiple: true, kind: { type: 'value_list', list: 'farg' } },
    ],
    value_lists: [
      ...defs.value_lists,
      { key: 'farg', options: [{ code: 'bla', labels: { en: 'Blue' } }, { code: 'gron', labels: { en: 'Green' } }] },
    ],
  }

  it('splits languages and multi-value adds and removes', () => {
    const lines = suggestionLines(
      { namn: { sv: 'Tält', en: 'Tent' }, farg: ['gron'] },
      { values: { namn: { sv: 'Tält nytt', en: 'Tent' }, farg: ['bla'] } },
      colourDefs,
    )
    expect(lines.map((line) => line.id)).toEqual(['namn:sv', 'farg:add:0', 'farg:remove:0'])
    expect(lines[0]).toMatchObject({ language: 'sv', label: 'Name', before: 'Tält', after: 'Tält nytt' })
    expect(lines[1]).toMatchObject({ change: 'add', after: 'Blue' })
    expect(lines[2]).toMatchObject({ change: 'remove', before: 'Green' })
    expect(lines.some((line) => line.language === 'de')).toBe(false)
  })

  it('keeps only the accepted language and the accepted colour add', () => {
    const current = { namn: { sv: 'Tält', en: 'Tent' }, farg: ['gron'] }
    const proposed = { namn: { sv: 'Tält nytt', en: 'Tent new' }, farg: ['bla'] }
    const lines = suggestionLines(current, { values: proposed }, colourDefs)
    const kept = lines.filter((line) => line.id !== 'namn:en' && line.change !== 'remove')
    expect(valuesFromKept(current, proposed, lines, kept)).toEqual({
      namn: { sv: 'Tält nytt' },
      farg: ['gron', 'bla'],
    })
  })

  it('names an agent and an import job', () => {
    expect(sourceLabel({ source: 'agent', actor_name: 'Demo Admin' })).toBe('Agent Demo Admin')
    expect(sourceLabel({ source: 'import', actor_name: 'Demo Admin' })).toBe('Import job')
  })
})
