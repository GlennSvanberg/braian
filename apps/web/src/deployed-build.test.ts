import { describe, expect, it, vi } from 'vitest'
import {
  fetchDeployedBuildId,
  isNewVersionAvailable,
  parseDeployedBuild,
} from './deployed-build'

describe('parseDeployedBuild', () => {
  it('reads a non-empty buildId', () => {
    expect(parseDeployedBuild({ buildId: 'abc123' })).toBe('abc123')
    expect(parseDeployedBuild({ buildId: '  abc  ' })).toBe('abc')
  })

  it('rejects missing or empty ids', () => {
    expect(parseDeployedBuild(null)).toBeNull()
    expect(parseDeployedBuild({})).toBeNull()
    expect(parseDeployedBuild({ buildId: '' })).toBeNull()
    expect(parseDeployedBuild({ buildId: '   ' })).toBeNull()
    expect(parseDeployedBuild({ buildId: 12 })).toBeNull()
  })
})

describe('isNewVersionAvailable', () => {
  it('is false when deployed matches the client bundle', () => {
    expect(isNewVersionAvailable('same', 'same')).toBe(false)
  })

  it('is true when deployed differs', () => {
    expect(isNewVersionAvailable('old', 'new')).toBe(true)
  })

  it('is false when deployed is unknown', () => {
    expect(isNewVersionAvailable('old', null)).toBe(false)
    expect(isNewVersionAvailable('', 'new')).toBe(false)
  })
})

describe('fetchDeployedBuildId', () => {
  it('returns the buildId on 200 JSON', async () => {
    const fetchImpl = vi.fn(async () =>
      new Response(JSON.stringify({ buildId: 'sha-1' }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    ) as unknown as typeof fetch
    await expect(fetchDeployedBuildId('/version.json', fetchImpl)).resolves.toBe('sha-1')
    expect(fetchImpl).toHaveBeenCalledWith(
      '/version.json',
      expect.objectContaining({ cache: 'no-store' }),
    )
  })

  it('returns null on HTTP or JSON failure', async () => {
    const notFound = vi.fn(async () => new Response('nope', { status: 404 })) as unknown as typeof fetch
    await expect(fetchDeployedBuildId('/version.json', notFound)).resolves.toBeNull()

    const badJson = vi.fn(async () =>
      new Response('not-json', {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    ) as unknown as typeof fetch
    await expect(fetchDeployedBuildId('/version.json', badJson)).resolves.toBeNull()
  })
})
