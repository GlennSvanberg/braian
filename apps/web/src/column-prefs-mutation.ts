/**
 * Column-prefs saves via TanStack Query useMutation.
 * Concurrent optimistic updates (TkDodo): onMutate + rollback, invalidate in
 * onSettled only when this is the last in-flight mutation for the key.
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

export function columnPrefsMutationKey(viewId: string) {
  return ['save-column-prefs', viewId] as const
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
  const mutationKey = columnPrefsMutationKey(viewId)
  return {
    mutationKey,
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
    onSettled: () => {
      // Only refetch when no other save for this view is still in flight.
      if (queryClient.isMutating({ mutationKey }) === 1) {
        void queryClient.invalidateQueries({ queryKey })
      }
    },
  }
}

export function useSaveColumnPrefs(viewId: string, userId: string) {
  const queryClient = useQueryClient()
  return useMutation(columnPrefsMutationOptions(queryClient, viewId, userId))
}
