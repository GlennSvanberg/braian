// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, expect, it } from 'vitest'
import type { ImportTarget } from './controller'
import { TargetMenu } from './target-menu'

afterEach(() => {
  cleanup()
})

const targets: ImportTarget[] = [
  { id: 'type:net_weight', label: 'Net weight', kind: 'number', key: false, source: 'Article' },
  { id: 'type:net_weight.value', label: 'Net weight (value)', kind: 'number', key: false, source: 'Article' },
  { id: 'type:net_weight.unit', label: 'Net weight (unit)', kind: 'text', key: false, source: 'Article' },
  { id: 'product>brand', label: 'Product › Brand', kind: 'text', key: false, source: 'Product' },
]

it('chooses Net weight (value) from the keyboard', () => {
  const picked: string[] = []
  render(<TargetMenu targets={targets} onPick={(id) => picked.push(id)} onClose={() => undefined} />)
  const menu = screen.getByTestId('import-popover')
  fireEvent.change(screen.getByLabelText('Search fields'), { target: { value: 'value' } })
  fireEvent.keyDown(menu, { key: 'ArrowDown' })
  fireEvent.keyDown(menu, { key: 'Enter' })
  expect(picked).toEqual(['type:net_weight.value'])
  expect(screen.getByRole('menuitem', { name: 'Net weight (value)' })).toBeTruthy()
})
