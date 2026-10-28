// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { StandardTableChrome } from './standard-table'
import { copy } from './i18n'

afterEach(() => cleanup())

describe('standard table chrome', () => {
  it('uses the plus control to create a row, not to add a view', () => {
    const onNewRow = vi.fn()
    render(
      <StandardTableChrome newRowLabel="New product" onNewRow={onNewRow} rowCount={3}>
        <div data-testid="grid-body" />
      </StandardTableChrome>,
    )
    fireEvent.click(screen.getByTestId('table-add-row'))
    expect(onNewRow).toHaveBeenCalledTimes(1)
    expect(screen.getByTestId('table-add-row').getAttribute('aria-label')).toBe('New product')
    expect(screen.queryByLabelText(copy.addView)).toBeNull()
  })

  it('does not render an empty trailing row in the chrome', () => {
    const { container } = render(
      <StandardTableChrome newRowLabel="New row" onNewRow={() => undefined} rowCount={0}>
        <div className="grid" data-testid="grid-body" />
      </StandardTableChrome>,
    )
    expect(container.querySelectorAll('.row')).toHaveLength(0)
    expect(screen.getByTestId('grid-body').childElementCount).toBe(0)
  })

  it('shows Review and save only when there is an unsaved change and runs the save handler', () => {
    const onReview = vi.fn()
    const { rerender } = render(
      <StandardTableChrome newRowLabel="New row" onNewRow={() => undefined} reviewVisible={false} onReview={onReview}>
        <div />
      </StandardTableChrome>,
    )
    expect(screen.queryByTestId('review-and-save')).toBeNull()
    rerender(
      <StandardTableChrome newRowLabel="New row" onNewRow={() => undefined} reviewVisible onReview={onReview}>
        <div />
      </StandardTableChrome>,
    )
    fireEvent.click(screen.getByTestId('review-and-save'))
    expect(onReview).toHaveBeenCalledTimes(1)
    expect(screen.getByTestId('review-and-save').textContent).toBe(copy.reviewAndSave)
  })

  it('opens the field picker from the columns button next to plus', () => {
    const onSelectFields = vi.fn()
    render(
      <StandardTableChrome newRowLabel="New row" onNewRow={() => undefined} onSelectFields={onSelectFields}>
        <div />
      </StandardTableChrome>,
    )
    const button = screen.getByTestId('table-select-fields')
    expect(button.getAttribute('aria-label')).toBe(copy.selectFields)
    fireEvent.click(button)
    expect(onSelectFields).toHaveBeenCalledTimes(1)
  })
})
