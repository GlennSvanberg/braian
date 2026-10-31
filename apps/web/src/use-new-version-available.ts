import { useQuery } from '@tanstack/react-query'
import { appBuildId } from './feedback'
import {
  DEPLOYED_BUILD_QUERY_KEY,
  DEPLOYED_BUILD_REFETCH_MS,
  DEPLOYED_BUILD_URL,
  fetchDeployedBuildId,
  isNewVersionAvailable,
} from './deployed-build'

export type NewVersionAvailable = {
  /** True when /version.json reports a build different from this bundle. */
  available: boolean
  clientBuildId: string
  deployedBuildId: string | null
  /** Last successful poll time, if any. */
  updatedAt: number | undefined
  isFetching: boolean
  isError: boolean
}

/**
 * Detects a newer web deploy while this tab stays open.
 * Polls {@link DEPLOYED_BUILD_URL} with TanStack Query (interval + window focus).
 * No UI — callers (status-bar message after #141) decide how to surface it.
 */
export function useNewVersionAvailable(): NewVersionAvailable {
  const clientBuildId = appBuildId()
  const query = useQuery({
    queryKey: DEPLOYED_BUILD_QUERY_KEY,
    queryFn: () => fetchDeployedBuildId(DEPLOYED_BUILD_URL),
    refetchInterval: DEPLOYED_BUILD_REFETCH_MS,
    refetchOnWindowFocus: true,
    staleTime: 0,
    retry: false,
  })
  const deployedBuildId = query.data ?? null
  return {
    available: isNewVersionAvailable(clientBuildId, deployedBuildId),
    clientBuildId,
    deployedBuildId,
    updatedAt: query.dataUpdatedAt || undefined,
    isFetching: query.isFetching,
    isError: query.isError,
  }
}
