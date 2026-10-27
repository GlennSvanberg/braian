// @vitest-environment jsdom
import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import type { Json } from './api'
import { formatRecordTime, principalName, RECORD_FIELDS, RecordSection, recordLabel, recordRows } from './record-section'

const created = RECORD_FIELDS.find((field) => field.key === 'created_at')!

const defs: Json = {
  attributes: [
    { id: '018f0000-0000-7000-8000-000000000100', key: 'number', labels: { en: 'Number' } },
    { id: '018f0000-0000-7000-8000-000000000101', key: 'name', labels: { en: 'Name' } },
    { id: '018f0000-0000-7000-8000-00000000a003', key: 'created_at', labels: { en: 'Opened' } },
    { id: '018f0000-0000-7000-8000-00000000a004', key: 'updated_at', labels: { en: 'Updated' } },
    { id: '018f0000-0000-7000-8000-00000000a005', key: 'updated_by', labels: { en: 'Updated by' } },
  ],
  types: [
    {
      key: 'product',
      attributes: [
        '018f0000-0000-7000-8000-000000000100',
        '018f0000-0000-7000-8000-000000000101',
        '018f0000-0000-7000-8000-00000000a003',
        '018f0000-0000-7000-8000-00000000a004',
        '018f0000-0000-7000-8000-00000000a005',
      ],
    },
    {
      key: 'note',
      attributes: ['018f0000-0000-7000-8000-000000000100'],
    },
  ],
}

afterEach(() => cleanup())

describe('record labels', () => {
  it('uses the model label when the type has the attribute', () => {
    expect(recordLabel(defs, 'product', created)).toBe('Opened')
  })

  it('falls back to the English default when the type lacks the attribute', () => {
    expect(recordLabel(defs, 'note', created)).toBe('Created')
    expect(recordLabel(undefined, 'product', created)).toBe('Created')
  })
})

describe('record values', () => {
  it('formats timestamps in the requested time zone and keeps the ISO title', () => {
    const iso = '2026-06-15T18:30:00.000Z'
    const utc = formatRecordTime(iso, 'UTC')
    const york = formatRecordTime(iso, 'America/New_York')
    expect(utc.title).toBe(iso)
    expect(utc.text).toMatch(/18:30|6:30/)
    expect(york.text).toMatch(/14:30|2:30/)
    expect(utc.text).not.toBe(york.text)
    expect(formatRecordTime('not-a-date')).toEqual({ text: 'not-a-date', title: 'not-a-date' })
  })

  it('reads number, name, and the principal the same way as the header chip', () => {
    expect(principalName({ id: 'p', name: 'Demo Admin', kind: 'person' })).toBe('Demo Admin')
    expect(principalName({ id: 'a', name: 'Catalog agent', kind: 'agent' })).toBe('Catalog agent')
    expect(principalName(null)).toBe('')
    const rows = recordRows(defs, 'product', {
      values: { number: 'PROD-1', name: { en: 'Cheddar', sv: 'Cheddar sv' } },
      createdAt: '2026-06-15T18:30:00.000Z',
      updatedAt: '2026-06-15T19:00:00.000Z',
      updatedBy: { name: 'Demo Admin', kind: 'person' },
      timeZone: 'UTC',
    })
    expect(rows.map((row) => row.label)).toEqual(['Number', 'Name', 'Opened', 'Updated', 'Updated by'])
    expect(rows.map((row) => row.text)).toEqual([
      'PROD-1',
      'Cheddar',
      formatRecordTime('2026-06-15T18:30:00.000Z', 'UTC').text,
      formatRecordTime('2026-06-15T19:00:00.000Z', 'UTC').text,
      'Demo Admin',
    ])
    expect(rows[2]?.title).toBe('2026-06-15T18:30:00.000Z')
  })
})

describe('Record section', () => {
  it('renders read-only fields in tab order', () => {
    render(
      <RecordSection
        defs={defs}
        typeKey="product"
        values={{ number: 'PROD-1', name: { en: 'Cheddar' } }}
        createdAt="2026-06-15T18:30:00.000Z"
        updatedAt="2026-06-15T19:00:00.000Z"
        updatedBy={{ name: 'Demo Admin' }}
      />,
    )
    expect(screen.getByRole('heading', { name: 'Record' })).toBeTruthy()
    const number = screen.getByTestId('record-number') as HTMLInputElement
    const createdInput = screen.getByTestId('record-created_at') as HTMLInputElement
    expect(number.readOnly).toBe(true)
    expect(number.disabled).toBe(false)
    expect(number.tabIndex).toBe(0)
    expect(number.value).toBe('PROD-1')
    expect(createdInput.title).toBe('2026-06-15T18:30:00.000Z')
    expect(screen.getByLabelText('Opened')).toBe(createdInput)
    expect((screen.getByTestId('record-updated_by') as HTMLInputElement).value).toBe('Demo Admin')
  })
})
