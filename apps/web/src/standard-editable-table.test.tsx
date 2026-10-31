// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { StandardEditableTable, type StandardColumn } from './standard-editable-table'
import { copy } from './i18n'

afterEach(() => cleanup())

type Row = { id: string; name: string; kind: string }

const columns: Array<StandardColumn<Row>> = [
  {
    id: 'name',
    header: 'Name',
    editable: true,
    read: (row) => row.name,
    write: (row, value) => ({ ...row, name: value }),
  },
  {
    id: 'kind',
    header: 'Type',
    type: 'enum',
    editable: true,
    options: [
      { value: 'text', label: 'Text' },
      { value: 'value_list', label: 'Value list' },
    ],
    read: (row) => row.kind,
    write: (row, value) => ({ ...row, kind: value }),
    display: (row) => (row.kind === 'value_list' ? 'Value list' : row.kind === 'text' ? 'Text' : row.kind),
  },
]

describe('standard editable table', () => {
  it('adds an inline draft row from plus and focuses the first cell', () => {
    const onDraft = vi.fn()
    render(
      <StandardEditableTable
        columns={columns}
        rows={[{ id: '1', name: 'Colour', kind: 'value_list' }]}
        draftRows={{}}
        onDraftRowsChange={onDraft}
        createDraftRow={() => ({ id: 'draft-1', name: '', kind: 'text' })}
        editable
      />,
    )
    fireEvent.click(screen.getByTestId('table-add-row'))
    expect(onDraft).toHaveBeenCalled()
    const next = onDraft.mock.calls[0][0] as Record<string, Row>
    expect(next['draft-1']).toEqual({ id: 'draft-1', name: '', kind: 'text' })
  })

  it('shows Review and save when drafts exist and runs the save handler', () => {
    const onReview = vi.fn()
    render(
      <StandardEditableTable
        columns={columns}
        rows={[]}
        draftRows={{ 'draft-1': { id: 'draft-1', name: 'New', kind: 'text' } }}
        onDraftRowsChange={() => undefined}
        createDraftRow={() => ({ id: 'x', name: '', kind: 'text' })}
        onReviewAndSave={onReview}
        editable
      />,
    )
    expect(screen.getByTestId('review-and-save')).toBeTruthy()
    fireEvent.click(screen.getByTestId('review-and-save'))
    expect(onReview).toHaveBeenCalledTimes(1)
  })

  it('edits a cell with Enter and commits on blur', () => {
    const onDraft = vi.fn()
    const { rerender } = render(
      <StandardEditableTable
        columns={columns}
        rows={[{ id: '1', name: 'Colour', kind: 'value_list' }]}
        draftRows={{}}
        onDraftRowsChange={onDraft}
        createDraftRow={() => ({ id: 'x', name: '', kind: 'text' })}
        editable
      />,
    )
    const row = screen.getByTestId('standard-grid-row')
    fireEvent.keyDown(row, { key: 'Enter' })
    const editor = screen.getByTestId('standard-cell-editor') as HTMLInputElement
    fireEvent.change(editor, { target: { value: 'Shade' } })
    fireEvent.blur(editor)
    expect(onDraft).toHaveBeenCalled()
    const patched = onDraft.mock.calls.at(-1)?.[0] as Record<string, Row>
    expect(patched['1'].name).toBe('Shade')

    rerender(
      <StandardEditableTable
        columns={columns}
        rows={[{ id: '1', name: 'Colour', kind: 'value_list' }]}
        draftRows={patched}
        onDraftRowsChange={onDraft}
        createDraftRow={() => ({ id: 'x', name: '', kind: 'text' })}
        editable
      />,
    )
    expect(screen.getByText('Shade')).toBeTruthy()
  })

  it('shows readable type labels instead of codes', () => {
    render(
      <StandardEditableTable
        columns={columns}
        rows={[{ id: '1', name: 'Colour', kind: 'value_list' }]}
        editable={false}
      />,
    )
    expect(screen.getByText('Value list')).toBeTruthy()
    expect(screen.queryByText('value_list')).toBeNull()
  })

  it('marks the active row with class on when activeRowId is set', () => {
    render(
      <StandardEditableTable
        columns={columns}
        rows={[
          { id: '1', name: 'Colour', kind: 'value_list' },
          { id: '2', name: 'Size', kind: 'text' },
        ]}
        activeRowId="2"
        editable={false}
      />,
    )
    const rows = screen.getAllByTestId('standard-grid-row')
    expect(rows[0].className).not.toContain(' on')
    expect(rows[0].classList.contains('on')).toBe(false)
    expect(rows[1].classList.contains('on')).toBe(true)
  })

  it('ArrowDown and ArrowUp activate the neighbour row when not editing', () => {
    const onActivate = vi.fn()
    const { rerender } = render(
      <StandardEditableTable
        columns={columns}
        rows={[
          { id: '1', name: 'Colour', kind: 'value_list' },
          { id: '2', name: 'Size', kind: 'text' },
          { id: '3', name: 'Weight', kind: 'text' },
        ]}
        activeRowId="1"
        onActivateRow={onActivate}
        editable={false}
      />,
    )
    const rows = screen.getAllByTestId('standard-grid-row')
    fireEvent.keyDown(rows[0], { key: 'ArrowDown' })
    expect(onActivate).toHaveBeenCalledWith(expect.objectContaining({ id: '2' }))
    rerender(
      <StandardEditableTable
        columns={columns}
        rows={[
          { id: '1', name: 'Colour', kind: 'value_list' },
          { id: '2', name: 'Size', kind: 'text' },
          { id: '3', name: 'Weight', kind: 'text' },
        ]}
        activeRowId="2"
        onActivateRow={onActivate}
        editable={false}
      />,
    )
    const mid = screen.getAllByTestId('standard-grid-row')[1]
    fireEvent.keyDown(mid, { key: 'ArrowUp' })
    expect(onActivate).toHaveBeenCalledWith(expect.objectContaining({ id: '1' }))
  })

  it('click calls onActivateRow for selection', () => {
    const onActivate = vi.fn()
    render(
      <StandardEditableTable
        columns={columns}
        rows={[{ id: '1', name: 'Colour', kind: 'value_list' }]}
        onActivateRow={onActivate}
        editable={false}
      />,
    )
    fireEvent.click(screen.getByTestId('standard-grid-row'))
    expect(onActivate).toHaveBeenCalledWith(expect.objectContaining({ id: '1' }))
  })

})

describe('value list master detail helpers', () => {
  it('exposes list values as a separate row set for the detail pane', async () => {
    const { valueListOptions } = await import('./model-tables')
    const defs = {
      value_lists: [
        {
          key: 'colour',
          labels: { en: 'Colour' },
          options: [
            { code: 'red', labels: { en: 'Red' } },
            { code: 'blue', labels: { en: 'Blue' } },
          ],
        },
      ],
    }
    const rows = valueListOptions(defs, 'colour')
    expect(rows).toHaveLength(2)
    expect(rows[0]?.code).toBe('red')
    expect(rows[0]?.label).toBe('Red')
  })
})
