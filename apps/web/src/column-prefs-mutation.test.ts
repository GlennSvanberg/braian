// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, cleanup, renderHook, waitFor } from '@testing-library/react'
import { createElement, type ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ColumnPrefsDocument } from './column-prefs'
import {
  columnPrefsMutationOptions,
  columnPrefsMutationScopeId,
  columnPrefsQueryKey,
  useSaveColumnPrefs,
} from './column-prefs-mutation'

const viewId = '018f0000-0000-7000-8000-000000000111'
const userId = 'user-1'

const doc = (path: string, visible: boolean): ColumnPrefsDocument => ({
  version: 2,
  columns: [{ path, visible }],
})

function wrapperFor(client: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return createElement(QueryClientProvider, { client }, children)
  }
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('columnPrefsMutationScopeId', () => {
  it('includes user, environment and view', () => {
    expect(columnPrefsMutationScopeId(userId, viewId)).toBe(
      `column-prefs:${userId}:default:${viewId}`,
    )
  })
})

describe('columnPrefsMutationOptions', () => {
  it('optimistically updates the query cache', async () => {
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
    const key = columnPrefsQueryKey(viewId)
    client.setQueryData(key, { document: doc('sku', true), updated_at: 't0' })
    const opts = columnPrefsMutationOptions(client, viewId, userId)
    const next = doc('sku', false)
    const ctx = await opts.onMutate!({ document: next }, {} as never)
    expect(client.getQueryData(key)).toEqual({
      document: next,
      updated_at: 't0',
      environment: undefined,
      stale_paths: undefined,
    })
    expect(ctx?.previous?.document).toEqual(doc('sku', true))
  })

  it('rolls back the query cache on error', async () => {
    const client = new QueryClient({ defaultOptions: { mutations: { retry: false } } })
    const key = columnPrefsQueryKey(viewId)
    const previous = { document: doc('sku', true), updated_at: 't0' }
    client.setQueryData(key, previous)
    const opts = columnPrefsMutationOptions(client, viewId, userId)
    const ctx = await opts.onMutate!({ document: doc('sku', false) }, {} as never)
    opts.onError?.(new Error('boom'), { document: doc('sku', false) }, ctx, {} as never)
    expect(client.getQueryData(key)).toEqual(previous)
  })
})

describe('useSaveColumnPrefs scope', () => {
  it('runs PUTs for the same view in order', async () => {
    const started: number[] = []
    const finished: number[] = []
    let release1!: () => void
    const gate1 = new Promise<void>((resolve) => {
      release1 = resolve
    })
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        if (
          !String(input).includes('/column-prefs') ||
          (init?.method ?? 'GET').toUpperCase() !== 'PUT'
        ) {
          return new Response('{}', { status: 200, headers: { 'Content-Type': 'application/json' } })
        }
        const body = JSON.parse(String(init?.body)) as { document: ColumnPrefsDocument }
        const n = body.document.columns[0]?.path === 'a' ? 1 : 2
        started.push(n)
        if (n === 1) await gate1
        finished.push(n)
        return new Response(JSON.stringify({ document: body.document, updated_at: `t${n}` }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        })
      }),
    )

    const client = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    })
    const { result } = renderHook(() => useSaveColumnPrefs(viewId, userId), {
      wrapper: wrapperFor(client),
    })

    act(() => {
      result.current.mutate({ document: doc('a', true) })
      result.current.mutate({ document: doc('b', true) })
    })

    await waitFor(() => expect(started).toEqual([1]))
    expect(finished).toEqual([])
    release1()
    await waitFor(() => expect(finished).toEqual([1, 2]))
    expect(started).toEqual([1, 2])
  })
})
