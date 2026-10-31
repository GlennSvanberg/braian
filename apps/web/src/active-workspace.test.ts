import { describe, expect, it } from 'vitest'
import {
  activeWorkspaceStorageKey,
  defaultActiveWorkspaceId,
  readActiveWorkspaceId,
  resolveActiveWorkspaceId,
  writeActiveWorkspaceId,
} from './active-workspace'

function memory() {
  const data = new Map<string, string>()
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => {
      data.set(key, value)
    },
    removeItem: (key: string) => {
      data.delete(key)
    },
    clear: () => data.clear(),
    key: () => null,
    length: 0,
  } as Storage
}

const rows = [
  { id: 'model', share_token: 'braian-model' },
  { id: 'products', share_token: 'braian-type-product' },
  { id: 'articles', share_token: 'braian-type-article' },
  { id: 'review', share_token: 'braian-review' },
]

describe('active workspace persistence', () => {
  it('keys storage per user', () => {
    expect(activeWorkspaceStorageKey('u1')).toBe('braian.activeWorkspace.u1')
  })

  it('defaults to the first type workspace', () => {
    expect(defaultActiveWorkspaceId(rows)).toBe('products')
    expect(defaultActiveWorkspaceId([])).toBeNull()
  })

  it('keeps the current id when it is still listed', () => {
    expect(resolveActiveWorkspaceId(rows, 'articles', 'products')).toBe('articles')
  })

  it('restores a stored id when current is empty', () => {
    expect(resolveActiveWorkspaceId(rows, null, 'articles')).toBe('articles')
  })

  it('falls back when current and stored are missing from the list', () => {
    expect(resolveActiveWorkspaceId(rows, 'gone', 'also-gone')).toBe('products')
    expect(resolveActiveWorkspaceId(rows, null, 'gone')).toBe('products')
  })

  it('round-trips through storage', () => {
    const store = memory()
    expect(readActiveWorkspaceId('u1', store)).toBeNull()
    writeActiveWorkspaceId('u1', 'articles', store)
    expect(readActiveWorkspaceId('u1', store)).toBe('articles')
    expect(store.getItem('braian.activeWorkspace.u1')).toBe(JSON.stringify('articles'))
  })
})
