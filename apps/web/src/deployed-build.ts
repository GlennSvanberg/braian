/** Pollable build marker written next to the web client at build time. */
export const DEPLOYED_BUILD_URL = '/version.json'

export const DEPLOYED_BUILD_QUERY_KEY = ['deployed-build'] as const

/** Poll cadence for a parked tab; focus refetch covers the common case sooner. */
export const DEPLOYED_BUILD_REFETCH_MS = 60_000

export type DeployedBuildPayload = {
  buildId: string
}

export function parseDeployedBuild(raw: unknown): string | null {
  if (!raw || typeof raw !== 'object') return null
  const buildId = (raw as { buildId?: unknown }).buildId
  if (typeof buildId !== 'string') return null
  const trimmed = buildId.trim()
  return trimmed.length > 0 ? trimmed : null
}

/**
 * True when the server advertises a different non-empty build than the one
 * baked into this JS bundle. Equal / missing / parse-failure → not available.
 */
export function isNewVersionAvailable(clientBuildId: string, deployedBuildId: string | null): boolean {
  if (!deployedBuildId) return false
  const client = clientBuildId.trim()
  if (!client) return false
  return deployedBuildId !== client
}

export async function fetchDeployedBuildId(
  url: string = DEPLOYED_BUILD_URL,
  fetchImpl: typeof fetch = fetch,
): Promise<string | null> {
  const response = await fetchImpl(url, {
    method: 'GET',
    cache: 'no-store',
    headers: { accept: 'application/json' },
  })
  if (!response.ok) return null
  try {
    return parseDeployedBuild(await response.json())
  } catch {
    return null
  }
}
