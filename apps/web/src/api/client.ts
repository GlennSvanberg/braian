import createClient from 'openapi-fetch'
import type { FetchResponse, MaybeOptionalInit } from 'openapi-fetch'
import type { paths } from './schema'

import { isWrite, writeSettled, writeStarted } from '../write-status'

const started = new WeakMap<Request, number>()
const tracked = new WeakSet<Request>()

/**
 * Same-origin requests stay relative (`/v1/...`) so cookie credentials and the
 * existing fetch stubs keep seeing the path they already assert on.
 * openapi-fetch builds an absolute URL internally; this fetch strips it.
 */
export const client = createClient<paths>({
  baseUrl: 'http://127.0.0.1',
  credentials: 'include',
  fetch: async (request) => {
    const url = new URL(request.url)
    const method = request.method.toUpperCase()
    const raw = method === 'GET' || method === 'HEAD' ? '' : await request.text()
    // `new Headers` here, not `request.headers`: openapi-fetch's Request is built
    // in the library realm, and column-prefs tests tell a TanStack mutate apart
    // from keepaliveJsonPut by `init.headers instanceof Headers`.
    const headers = new Headers(request.headers)
    return fetch(`${url.pathname}${url.search}`, {
      method,
      headers,
      body: raw ? raw : undefined,
      credentials: 'include',
      keepalive: request.keepalive,
      // Every Request has a signal. Forwarding it makes Chrome abort a
      // keepalive PUT on navigation (column-prefs saves must survive reload).
      ...(request.keepalive ? {} : { signal: request.signal }),
    })
  },
})

client.use({
  onRequest({ request }) {
    started.set(request, performance.now())
    if (isWrite(request.method)) {
      tracked.add(request)
      writeStarted()
    }
    return request
  },
  onError({ request, error }) {
    if (tracked.delete(request)) writeSettled(error instanceof Error ? error.message : 'Network error')
  },
  onResponse({ request, response }) {
    if (tracked.delete(request)) writeSettled(response.ok ? undefined : `${response.status} ${response.statusText}`.trim())
    if (typeof window !== 'undefined') {
      const url = new URL(request.url)
      const path = `${url.pathname}${url.search}`
      const at = started.get(request) ?? performance.now()
      void import('../feedback-log').then(({ noteApiResponse }) => {
        noteApiResponse(request.method, path, at, response)
      })
    }
    return response
  },
})

export class ApiError extends Error {
  status: number
  body: unknown

  constructor(status: number, detail: string, body: unknown) {
    super(detail)
    this.status = status
    this.body = body
  }
}

function problemDetail(body: unknown, fallback: string): string {
  if (body && typeof body === 'object' && 'detail' in body && typeof body.detail === 'string') {
    return body.detail
  }
  return fallback
}

type Settled = { data?: unknown; error?: unknown; response: Response }
type Ok<T> = T extends { data: infer D; error?: never } ? D : never
type Media = `${string}/json`
type Method = 'get' | 'post' | 'put' | 'patch' | 'delete'
type PathsWith<M extends Method> = {
  [P in keyof paths]: paths[P] extends { [K in M]: unknown } ? P : never
}[keyof paths]
type RequiredKeysOfHelper<T> = { [K in keyof T]: {} extends Pick<T, K> ? never : K }[keyof T]
type RequiredKeysOf<T> = RequiredKeysOfHelper<T> extends undefined ? never : RequiredKeysOfHelper<T>
/** Same shape as openapi-fetch's init tuple, so a call can pass it straight through. */
type InitParam<Init> = RequiredKeysOf<Init> extends never
  ? [(Init & { [key: string]: unknown })?]
  : [Init & { [key: string]: unknown }]
type Op<Path extends keyof paths, M extends Method> = NonNullable<paths[Path][M]>
type Data<Path extends keyof paths, M extends Method, Init> = Ok<FetchResponse<Op<Path, M>, Init, Media>>

async function settle<T extends Settled>(pending: Promise<T>): Promise<Ok<T>> {
  const result = await pending
  if (!result.response.ok) {
    throw new ApiError(result.response.status, problemDetail(result.error, result.response.statusText), result.error ?? null)
  }
  return result.data as Ok<T>
}

export function get<Path extends PathsWith<'get'>, Init extends MaybeOptionalInit<paths[Path], 'get'>>(
  path: Path,
  ...init: InitParam<Init>
): Promise<Data<Path, 'get', Init>> {
  return settle(client.GET<Path, Init>(path, ...init))
}

export function post<Path extends PathsWith<'post'>, Init extends MaybeOptionalInit<paths[Path], 'post'>>(
  path: Path,
  ...init: InitParam<Init>
): Promise<Data<Path, 'post', Init>> {
  return settle(client.POST<Path, Init>(path, ...init))
}

export function put<Path extends PathsWith<'put'>, Init extends MaybeOptionalInit<paths[Path], 'put'>>(
  path: Path,
  ...init: InitParam<Init>
): Promise<Data<Path, 'put', Init>> {
  return settle(client.PUT<Path, Init>(path, ...init))
}

export function patch<Path extends PathsWith<'patch'>, Init extends MaybeOptionalInit<paths[Path], 'patch'>>(
  path: Path,
  ...init: InitParam<Init>
): Promise<Data<Path, 'patch', Init>> {
  return settle(client.PATCH<Path, Init>(path, ...init))
}

export function del<Path extends PathsWith<'delete'>, Init extends MaybeOptionalInit<paths[Path], 'delete'>>(
  path: Path,
  ...init: InitParam<Init>
): Promise<Data<Path, 'delete', Init>> {
  return settle(client.DELETE<Path, Init>(path, ...init))
}

/** Fire-and-forget JSON PUT that survives tab close. */
export function keepaliveJsonPut(path: string, body: unknown): void {
  void fetch(path, {
    method: 'PUT',
    credentials: 'include',
    keepalive: true,
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  })
}

/** Copy a typed document into the open object the spec uses for free-form JSON. */
export function freeObject(value: object): { [key: string]: unknown } {
  return Object.fromEntries(Object.entries(value))
}
