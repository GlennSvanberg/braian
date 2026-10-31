// Playwright against its own Postgres database and Typesense alias.
// The demo database used by `just dev` is not written.

import { execFileSync, spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { setTimeout as delay } from 'node:timers/promises'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const binary = `${root}/target/debug/braian${process.platform === 'win32' ? '.exe' : ''}`

function withDatabase(url, name) {
  const queryAt = url.indexOf('?')
  const base = queryAt === -1 ? url : url.slice(0, queryAt)
  const query = queryAt === -1 ? '' : url.slice(queryAt)
  const slash = base.lastIndexOf('/')
  return `${base.slice(0, slash + 1)}${name}${query}`
}

function ensureDatabase() {
  const user = process.env.POSTGRES_USER || 'braian'
  try {
    execFileSync(
      'docker',
      ['compose', 'exec', '-T', 'postgres', 'psql', '-U', user, '-d', 'postgres', '-c', 'CREATE DATABASE braian_e2e'],
      { cwd: root, stdio: 'pipe' },
    )
  } catch (error) {
    const text = `${error.stderr ?? ''} ${error.stdout ?? ''}`
    if (!text.includes('already exists')) {
      console.error(text || error.message)
      throw error
    }
  }
}

async function waitFor(url) {
  for (let attempt = 0; attempt < 90; attempt += 1) {
    try {
      const response = await fetch(url)
      if (response.ok) return
    } catch {
      // the process is still starting
    }
    await delay(1000)
  }
  throw new Error(`timed out waiting for ${url}`)
}

const source = process.env.DATABASE_URL
if (!source) {
  console.error('DATABASE_URL is not set. Copy .env.example to .env first.')
  process.exit(1)
}
if (!existsSync(binary)) {
  execFileSync('cargo', ['build', '-p', 'braian-server'], { cwd: root, stdio: 'inherit' })
}

ensureDatabase()

const apiEnv = { ...process.env }
// Older checkouts set this. The process ignores it; drop it so the log stays quiet.
delete apiEnv.KAFKA_BROKERS
apiEnv.DATABASE_URL = withDatabase(source, 'braian_e2e')
apiEnv.BRAIAN_SEARCH_ALIAS = 'braian_e2e'
apiEnv.BRAIAN_API_ADDR = '127.0.0.1:8095'
apiEnv.BRAIAN_SEED_COUNT = process.env.BRAIAN_E2E_SEED_COUNT || '250'
apiEnv.BRAIAN_AGENT_MOCK = process.env.BRAIAN_AGENT_MOCK || '1'
apiEnv.BRAIAN_E2E = '1'

const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'
const windows = process.platform === 'win32'

const api = spawn(binary, ['all'], { cwd: root, env: apiEnv, stdio: 'inherit' })
const web = spawn(
  pnpm,
  ['--dir', 'apps/web', 'exec', 'vite', 'dev', '--host', '127.0.0.1', '--port', '3010', '--strictPort'],
  {
    cwd: root,
    env: { ...process.env, BRAIAN_API_PROXY: 'http://127.0.0.1:8095' },
    stdio: 'inherit',
    shell: windows,
  },
)

function stop() {
  for (const child of [web, api]) {
    if (child.exitCode === null && !child.killed) child.kill('SIGTERM')
  }
}
process.on('SIGINT', () => {
  stop()
  process.exit(1)
})

let code = 1
try {
  await waitFor('http://127.0.0.1:8095/healthz')
  await waitFor('http://127.0.0.1:3010/')
  execFileSync(pnpm, ['--dir', 'apps/web', 'exec', 'playwright', 'test', 'e2e/smoke.spec.ts', 'e2e/drag.spec.ts', 'e2e/layout.spec.ts', 'e2e/graph.spec.ts', 'e2e/history.spec.ts', 'e2e/import.spec.ts', 'e2e/structure.spec.ts', 'e2e/group-filter.spec.ts', 'e2e/type-workspace.spec.ts', 'e2e/pane-scroll.spec.ts', 'e2e/model-tree.spec.ts', 'e2e/change-flow.spec.ts', 'e2e/record.spec.ts', 'e2e/field-picker.spec.ts', 'e2e/column-prefs-multitab.spec.ts', 'e2e/column-drag-and-system-fields.spec.ts', 'e2e/table-flash-sort-palette.spec.ts', 'e2e/move-col-flash-sse.spec.ts', 'e2e/palette-root-focus.spec.ts', 'e2e/legacy-model-workspace.spec.ts', 'e2e/dangling-model-workspace.spec.ts', 'e2e/inline-cell-edit.spec.ts', 'e2e/model-table-row-selection.spec.ts', 'e2e/cell-edit-key-guard.spec.ts', 'e2e/status-select-cell-edit.spec.ts', 'e2e/persist-active-workspace.spec.ts', 'e2e/delete-workspace.spec.ts'], {
    cwd: root,
    stdio: 'inherit',
    shell: windows,
    env: { ...process.env, BRAIAN_WEB_URL: 'http://127.0.0.1:3010' },
  })
  code = 0
} catch (error) {
  const status = error && typeof error === 'object' ? error.status : undefined
  code = typeof status === 'number' ? status : 1
  if (typeof status !== 'number') console.error(error instanceof Error ? error.message : error)
} finally {
  stop()
}
process.exit(code)
