// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, expect, it } from 'vitest'
import type { ImportTarget } from './controller'
import { TargetPicker } from './target-picker'

afterEach(() => {
  cleanup()
})

const targets: ImportTarget[] = [
  { id: 'type:net_weight', label: 'Net weight', kind: 'number', key: false, source: 'Article' },
  { id: 'type:net_weight.value', label: 'Net weight (value)', kind: 'number', key: false, source: 'Article' },
  { id: 'type:net_weight.unit', label: 'Net weight (unit)', kind: 'text', key: false, source: 'Article' },
  { id: 'product>brand', label: 'Product › Brand', kind: 'text', key: false, source: 'Product' },
]

it('chooses Net weight (value) from the keyboard in single mode', () => {
  const picked: string[] = []
  render(<TargetPicker targets={targets} onPick={(id) => picked.push(id)} onClose={() => undefined} />)
  const picker = screen.getByTestId('import-field-mode')
  expect(picker.getAttribute('data-picker-mode')).toBe('single')
  fireEvent.change(screen.getByPlaceholderText('Search fields'), { target: { value: 'value' } })
  const list = screen.getByTestId('import-field-mode-list').closest('[cmdk-root]') ?? picker
  fireEvent.keyDown(list, { key: 'ArrowDown' })
  fireEvent.keyDown(list, { key: 'Enter' })
  expect(picked).toEqual(['type:net_weight.value'])
})

it('shows the current target as checked and offers Do not import first', () => {
  render(<TargetPicker targets={targets} current="product>brand" onPick={() => undefined} onClose={() => undefined} />)
  expect(screen.getByTestId('import-field-row-unmapped')).toBeTruthy()
  expect(screen.getByTestId('import-field-check-product>brand').getAttribute('aria-label')).toBe('Current')
})
