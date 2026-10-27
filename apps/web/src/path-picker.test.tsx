// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { afterEach, expect, it } from 'vitest'
import { PathMenu, type ListedPath } from './path-picker'

const paths: ListedPath[] = [
  { text: 'net_weight', label: 'Net weight' },
  { text: 'net_weight.value', label: 'Net weight (value)' },
  { text: 'net_weight.unit', label: 'Net weight (unit)' },
  { text: 'product>brand', label: 'Product › Brand' },
]

afterEach(() => {
  cleanup()
})

function Harness({ onPick }: { onPick: (text: string) => void }) {
  const [open, setOpen] = useState(true)
  if (!open) return null
  return <PathMenu paths={paths} onPick={(path) => onPick(path.text)} onClose={() => setOpen(false)} />
}

it('reaches a part and a link with the keyboard', () => {
  const picked: string[] = []
  render(<Harness onPick={(text) => picked.push(text)} />)
  const menu = screen.getByTestId('path-picker')
  fireEvent.keyDown(menu, { key: 'ArrowRight' })
  expect(screen.getByRole('menuitem', { name: 'Net weight (value)' })).toBeTruthy()
  fireEvent.keyDown(menu, { key: 'ArrowDown' })
  fireEvent.keyDown(menu, { key: 'Enter' })
  expect(picked).toEqual(['net_weight.value'])
})

it('opens the link submenu', () => {
  const picked: string[] = []
  render(<Harness onPick={(text) => picked.push(text)} />)
  const menu = screen.getByTestId('path-picker')
  fireEvent.keyDown(menu, { key: 'ArrowDown' })
  fireEvent.keyDown(menu, { key: 'ArrowDown' })
  fireEvent.keyDown(menu, { key: 'Enter' })
  fireEvent.keyDown(menu, { key: 'Enter' })
  expect(picked).toEqual(['product>brand'])
})
