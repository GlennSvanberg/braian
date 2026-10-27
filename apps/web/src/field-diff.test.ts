import { describe, expect, it } from 'vitest'
import { diffLines, historyTitle, relativeTime, sourceLabel, suggestionLines, valuesFromKept } from './field-diff'

const defs = {
  attributes: [
    { id: 'name', key: 'name', labels: { en: 'Name', sv: 'Name' } },
    { id: 'status', key: 'status', labels: { en: 'Status' }, kind: { type: 'value_list', list: 'status' } },
  ],
  value_lists: [{ key: 'status', options: [{ code: 'ok', labels: { en: 'OK' } }, { code: 'draft', labels: { en: 'Draft' } }] }],
  structures: [
    {
      key: 'food',
      groups: [
        { id: 'g-cheese', key: 'cheese', labels: { en: 'Cheese' } },
        { id: 'g-juice', key: 'juice', labels: { en: 'Juice' } },
      ],
    },
  ],
}

describe('diff lines', () => {
  it('shows only the language that changed', () => {
    const lines = diffLines(
      {
        name: {
          before: { sv: 'Shell jacket' },
          after: { sv: 'Hooded shell' },
        },
      },
      defs,
    )
    expect(lines).toEqual([
      { id: 'name:sv', label: 'Name (sv)', before: 'Shell jacket', after: 'Hooded shell' },
    ])
  })

  it('keeps each changed language on its own line', () => {
    const lines = diffLines(
      {
        name: {
          before: { sv: 'Shell', en: 'Shell' },
          after: { sv: 'Hooded', en: 'Shell' },
        },
      },
      defs,
    )
    expect(lines.map((line) => line.label)).toEqual(['Name (sv)'])
    expect(lines[0]?.after).toBe('Hooded')
  })

  it('names a create after the person and collapses the values', () => {
    expect(historyTitle({ op: 'create', actor: 'Ada' })).toBe('Created by Ada')
    expect(historyTitle({ op: 'create', actor: 'bot', on_behalf_of: 'Ada', via: 'agent' })).toBe('Created by Ada via agent')
    expect(historyTitle({ op: 'update', actor: 'import-job', via: 'import' })).toBe('import-job via import')
    expect(diffLines({ $created: { version: 1 }, name: { after: { sv: 'Tent' } } }, defs)).toEqual([])
  })

  it('formats a plain value and a group move', () => {
    const lines = diffLines(
      {
        status: { before: 'draft', after: 'ok' },
        $groups: { food: { before: 'g-cheese', after: 'g-juice' } },
      },
      defs,
    )
    expect(lines.map((line) => `${line.label}: ${line.before} -> ${line.after}`)).toEqual([
      'Status: Draft -> OK',
      'Group: Cheese -> Juice',
    ])
  })

  it('prints history times as YYYY-MM-DD HH:mm', () => {
    expect(relativeTime('2026-10-25T10:00:00Z')).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/)
  })
})

describe('suggestion lines', () => {
  const colourDefs = {
    ...defs,
    attributes: [
      ...defs.attributes,
      { id: 'colour', key: 'colour', labels: { en: 'Colour' }, multiple: true, kind: { type: 'value_list', list: 'colour' } },
    ],
    value_lists: [
      ...defs.value_lists,
      { key: 'colour', options: [{ code: 'blue', labels: { en: 'Blue' } }, { code: 'green', labels: { en: 'Green' } }] },
    ],
  }

  it('splits languages and multi-value adds and removes', () => {
    const lines = suggestionLines(
      { name: { sv: 'Tent', en: 'Tent' }, colour: ['green'] },
      { values: { name: { sv: 'Tent new', en: 'Tent' }, colour: ['blue'] } },
      colourDefs,
    )
    expect(lines.map((line) => line.id)).toEqual(['name:sv', 'colour:add:0', 'colour:remove:0'])
    expect(lines[0]).toMatchObject({ language: 'sv', label: 'Name', before: 'Tent', after: 'Tent new' })
    expect(lines[1]).toMatchObject({ change: 'add', after: 'Blue' })
    expect(lines[2]).toMatchObject({ change: 'remove', before: 'Green' })
    expect(lines.some((line) => line.language === 'de')).toBe(false)
  })

  it('keeps only the accepted language and the accepted colour add', () => {
    const current = { name: { sv: 'Tent', en: 'Tent' }, colour: ['green'] }
    const proposed = { name: { sv: 'Tent new', en: 'Tent updated' }, colour: ['blue'] }
    const lines = suggestionLines(current, { values: proposed }, colourDefs)
    const kept = lines.filter((line) => line.id !== 'name:en' && line.change !== 'remove')
    expect(valuesFromKept(current, proposed, lines, kept)).toEqual({
      name: { sv: 'Tent new' },
      colour: ['green', 'blue'],
    })
  })

  it('names an agent and an import job', () => {
    expect(sourceLabel({ source: 'agent', actor_name: 'Demo Admin' })).toBe('Agent Demo Admin')
    expect(sourceLabel({ source: 'import', actor_name: 'Demo Admin' })).toBe('Import job')
  })
})
