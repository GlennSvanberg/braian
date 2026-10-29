/**
 * Column-prefs saves via TanStack Query useMutation.
 * Optimistic cache update + rollback; scope.id serializes PUTs per user/env/view.
 */
import {
  useMutation,
  useQueryClient,
  type QueryClient,
  type UseMutationOptions,
} from '@tanstack/react-query'
import {
  saveColumnPrefs,
  saveColumnPrefsKeepalive,
  type ColumnPrefsDocument,
  type ColumnPrefsResponse,
} from './column-prefs'
import { LAYOUT_ENVIRONMENT } from './workspace-doc'

export function columnPrefsQueryKey(viewId: string) {
  return ['column-prefs', viewId] as const
}

/** Serial mutation scope: one in-flight PUT chain per user + environment + view. */
export function columnPrefsMutationScopeId(userId: string, viewId: string) {
  return `column-prefs:${userId || 'anon'}:${LAYOUT_ENVIRONMENT}:${viewId}`
}

export type SaveColumnPrefsVars = {
  document: ColumnPrefsDocument
  /** Fire-and-forget across unload; still updates the query cache in onMutate. */
  keepalive?: boolean
}

export type SaveColumnPrefsContext = {
  previous: ColumnPrefsResponse | undefined
}

export function columnPrefsMutationOptions(
  queryClient: QueryClient,
  viewId: string,
  userId: string,
): UseMutationOptions<ColumnPrefsResponse | null, Error, SaveColumnPrefsVars, SaveColumnPrefsContext> {
  const queryKey = columnPrefsQueryKey(viewId)
  return {
    mutationKey: ['save-column-prefs', viewId],
    scope: { id: columnPrefsMutationScopeId(userId, viewId) },
    mutationFn: async (vars) => {
      if (vars.keepalive) {
        saveColumnPrefsKeepalive(viewId, vars.document)
        return null
      }
      return (await saveColumnPrefs(viewId, vars.document)) as ColumnPrefsResponse
    },
    onMutate: async (vars) => {
      await queryClient.cancelQueries({ queryKey })
      const previous = queryClient.getQueryData<ColumnPrefsResponse>(queryKey)
      queryClient.setQueryData<ColumnPrefsResponse>(queryKey, {
        document: vars.document,
        updated_at: previous?.updated_at ?? null,
        environment: previous?.environment,
        stale_paths: previous?.stale_paths,
      })
      return { previous }
    },
    onError: (_error, _vars, context) => {
      if (context?.previous !== undefined) {
        queryClient.setQueryData(queryKey, context.previous)
      }
    },
    onSuccess: (data) => {
      if (data?.document && (data.document as { version?: number }).version === 2) {
        queryClient.setQueryData(queryKey, data)
      }
    },
  }
}

export function useSaveColumnPrefs(viewId: string, userId: string) {
  const queryClient = useQueryClient()
  return useMutation(columnPrefsMutationOptions(queryClient, viewId, userId))
}
