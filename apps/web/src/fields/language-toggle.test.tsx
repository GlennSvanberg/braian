// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { FieldsMode } from './fields-mode'
import { makeViewColumn } from './columns'
import type { FieldDef } from './qualifiers'

const description: FieldDef = {
  id: 'description',
  name: 'Description',
  typeLabel: 'text',
  shape: 'language',
  languages: ['en', 'sv'],
}


beforeEach(() => {
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  )
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('FieldsMode language completion', () => {
  it('toggles an already-visible Description (en) instead of moving it last', () => {
    const existing = makeViewColumn(
      ['description'],
      [{ kind: 'language', optionId: 'en', label: 'EN' }],
      [description],
    )
    expect(existing).toBeTruthy()
    const onToggleColumn = vi.fn()
    const onShowColumn = vi.fn()
    const onReplaceColumn = vi.fn()
    render(
      <FieldsMode
        open={{ kind: 'browse' }}
        columns={[existing!]}
        fields={[description]}
        onClose={() => undefined}
        onToggleColumn={onToggleColumn}
        onShowColumn={onShowColumn}
        onReplaceColumn={onReplaceColumn}
        peerOptionsForField={() => []}
      />,
    )
    fireEvent.click(screen.getByTestId('fields-row-description'))
    // Language step should appear; activate EN
    const enOption = screen.getByText('EN')
    fireEvent.click(enOption)
    expect(onToggleColumn).toHaveBeenCalled()
    expect(onShowColumn).not.toHaveBeenCalled()
  })
})
