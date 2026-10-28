import { describe, expect, it } from 'vitest'
import { copy } from './i18n'
import { commandsForContext, paletteCommandsFromRegistry } from './command-registry'

describe('standard table commands', () => {
  it('keeps the removed product toolbar actions in Ctrl+K and the context menu', () => {
    const palette = paletteCommandsFromRegistry({ typeKey: 'product', singular: 'product' })
    const titles = palette.map((command) =>
      typeof command.title === 'function' ? command.title({ typeKey: 'product', singular: 'product' }) : command.title,
    )
    expect(titles).toContain(copy.massEdit)
    expect(titles).toContain(copy.filterBy)
    expect(titles).toContain(copy.export)
    expect(titles).toContain(copy.exportColumns)
    expect(titles.some((title) => title.toLowerCase().includes('new'))).toBe(true)

    const menu = commandsForContext('row', { objectId: 'id-1', typeKey: 'product', singular: 'product' })
    const menuTitles = menu.map((command) =>
      typeof command.title === 'function' ? command.title({ objectId: 'id-1', typeKey: 'product', singular: 'product' }) : command.title,
    )
    expect(menuTitles).toContain(copy.massEdit)
    expect(menuTitles).toContain(copy.filterBy)
    expect(menuTitles).toContain(copy.export)
    expect(menuTitles).toContain(copy.exportColumns)
    expect(menu.some((command) => command.id === 'object:new')).toBe(true)
  })
})
