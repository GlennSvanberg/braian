// Playwright against its own Postgres database and Typesense alias.
// The demo database used by `just dev` is not written.
//
// BRAIAN_BIN, when set, is the server binary. CI points it at the profile.ci
// binary built for Phase 2 so this script does not compile.
// BRAIAN_E2E_WORKERS (default 1) starts one database, API and Vite per worker
// and runs that worker's files against only that server. Workers stay at 1
// inside each Playwright process so tests in one file keep their order.

import { execFileSync, spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import { fileURLToPath, pathToFileURL } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))
const debugBinary = `${root}/target/debug/braian${process.platform === 'win32' ? '.exe' : ''}`

const specs = [
  'e2e/smoke.spec.ts',
  'e2e/drag.spec.ts',
  'e2e/layout.spec.ts',
  'e2e/graph.spec.ts',
  'e2e/history.spec.ts',
  'e2e/import.spec.ts',
  'e2e/structure.spec.ts',
  'e2e/group-filter.spec.ts',
  'e2e/type-workspace.spec.ts',
  'e2e/pane-scroll.spec.ts',
  'e2e/model-tree.spec.ts',
  'e2e/change-flow.spec.ts',
  'e2e/record.spec.ts',
  'e2e/field-picker.spec.ts',
  'e2e/column-prefs-multitab.spec.ts',
  'e2e/column-drag-and-system-fields.spec.ts',
  'e2e/table-flash-sort-palette.spec.ts',
  'e2e/move-col-flash-sse.spec.ts',
  'e2e/palette-root-focus.spec.ts',
  'e2e/legacy-model-workspace.spec.ts',
  'e2e/dangling-model-workspace.spec.ts',
  'e2e/inline-cell-edit.spec.ts',
  'e2e/cell-editor-unloaded-value.spec.ts',
  'e2e/model-table-row-selection.spec.ts',
  'e2e/cell-edit-key-guard.spec.ts',
  'e2e/status-select-cell-edit.spec.ts',
  'e2e/persist-active-workspace.spec.ts',
  'e2e/delete-workspace.spec.ts',
  'e2e/template-picker.spec.ts',
  'e2e/shortcut-registry.spec.ts',
  'e2e/user-menu.spec.ts',
]

function workerCount() {
  const raw = process.env.BRAIAN_E2E_WORKERS || '1'
  const parsed = Number(raw)
  if (!Number.isInteger(parsed) || parsed < 1) {
    console.error(`BRAIAN_E2E_WORKERS must be a positive integer, got ${raw}`)
    process.exit(1)
  }
  return parsed
}

// Playwright's own duration on a 2-core GitHub runner (run 41194243730),
// plus a few seconds to drop and recreate the database and start the API.
// Test count is a poor weight: layout is 3 tests and 51s, field-picker is
// 10 tests and 66s, and a 1-test file is often ~6s. A file missing from
// this map uses 12s per test, which matched the short specs on that run.
const measuredSeconds = {
  'e2e/smoke.spec.ts': 16,
  'e2e/drag.spec.ts': 12,
  'e2e/layout.spec.ts': 51,
  'e2e/graph.spec.ts': 26,
  'e2e/history.spec.ts': 6,
  'e2e/import.spec.ts': 4,
  'e2e/structure.spec.ts': 6,
  'e2e/group-filter.spec.ts': 7,
  'e2e/type-workspace.spec.ts': 6,
  'e2e/pane-scroll.spec.ts': 5,
  'e2e/model-tree.spec.ts': 6,
  'e2e/change-flow.spec.ts': 20,
  'e2e/record.spec.ts': 6,
  'e2e/field-picker.spec.ts': 66,
  'e2e/column-prefs-multitab.spec.ts': 10,
  'e2e/column-drag-and-system-fields.spec.ts': 11,
  'e2e/table-flash-sort-palette.spec.ts': 16,
  'e2e/move-col-flash-sse.spec.ts': 9,
  'e2e/palette-root-focus.spec.ts': 10,
  'e2e/legacy-model-workspace.spec.ts': 17,
  'e2e/dangling-model-workspace.spec.ts': 10,
  'e2e/inline-cell-edit.spec.ts': 7,
  'e2e/cell-editor-unloaded-value.spec.ts': 25,
  'e2e/model-table-row-selection.spec.ts': 5,
  'e2e/cell-edit-key-guard.spec.ts': 4,
  'e2e/template-picker.spec.ts': 25,
  'e2e/user-menu.spec.ts': 25,
  'e2e/delete-workspace.spec.ts': 28,
  'e2e/shortcut-registry.spec.ts': 20,
}

const reseedSeconds = 4

function fileWeight(file) {
  const measured = measuredSeconds[file]
  if (measured) return measured + reseedSeconds
  const text = readFileSync(join(root, 'apps/web', file), 'utf8')
  const matches = text.match(/^\s*test(?:\.[A-Za-z]+)?\(/gm)
  const tests = matches ? matches.length : 1
  return tests * 12 + reseedSeconds
}

// Longest-first assignment. Each file still runs on one database, in its
// original relative order. The previous list-order pack left field-picker
// and layout on the same worker, so that process ran about a minute longer.
function partition(list, workers) {
  const bins = Array.from({ length: workers }, () => ({ files: [], weight: 0 }))
  const ranked = [...list].sort((a, b) => fileWeight(b) - fileWeight(a) || list.indexOf(a) - list.indexOf(b))
  for (const file of ranked) {
    let lightest = bins[0]
    for (const bin of bins) {
      if (bin.weight < lightest.weight) lightest = bin
    }
    lightest.files.push(file)
    lightest.weight += fileWeight(file)
  }
  const order = new Map(list.map((file, index) => [file, index]))
  return bins
    .filter((bin) => bin.files.length > 0)
    .map((bin) => bin.files.sort((a, b) => order.get(a) - order.get(b)))
}

function withDatabase(url, name) {
  const queryAt = url.indexOf('?')
  const base = queryAt === -1 ? url : url.slice(0, queryAt)
  const query = queryAt === -1 ? '' : url.slice(queryAt)
  const slash = base.lastIndexOf('/')
  return `${base.slice(0, slash + 1)}${name}${query}`
}

function psql(sql) {
  const user = process.env.POSTGRES_USER || 'braian'
  execFileSync(
    'docker',
    ['compose', 'exec', '-T', 'postgres', 'psql', '-U', user, '-d', 'postgres', '-v', 'ON_ERROR_STOP=1', '-c', sql],
    { cwd: root, stdio: 'pipe' },
  )
}

function databaseNameOk(name) {
  return /^[a-z][a-z0-9_]*$/.test(name)
}

function ensureDatabase(name) {
  if (!databaseNameOk(name)) throw new Error(`refusing database name ${name}`)
  try {
    psql(`CREATE DATABASE ${name}`)
  } catch (error) {
    const text = `${error.stderr ?? ''} ${error.stdout ?? ''}`
    if (!text.includes('already exists')) {
      console.error(text || error.message)
      throw error
    }
  }
}

function resetDatabase(name) {
  if (!databaseNameOk(name)) throw new Error(`refusing database name ${name}`)
  psql(`DROP DATABASE IF EXISTS ${name} WITH (FORCE)`)
  psql(`CREATE DATABASE ${name}`)
}

async function waitFor(url) {
  for (let attempt = 0; attempt < 120; attempt += 1) {
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

function resolveBinary() {
  if (process.env.BRAIAN_BIN) {
    if (!existsSync(process.env.BRAIAN_BIN)) {
      console.error(`BRAIAN_BIN does not exist: ${process.env.BRAIAN_BIN}`)
      process.exit(1)
    }
    console.log(`e2e binary ${process.env.BRAIAN_BIN} (cargo not invoked)`)
    return process.env.BRAIAN_BIN
  }
  if (!existsSync(debugBinary)) {
    execFileSync('cargo', ['build', '-p', 'braian-server'], { cwd: root, stdio: 'inherit' })
  }
  return debugBinary
}

const source = process.env.DATABASE_URL
if (!source) {
  console.error('DATABASE_URL is not set. Copy .env.example to .env first.')
  process.exit(1)
}

const binary = resolveBinary()
const workers = workerCount()
const groups = partition(specs, workers)
console.log(`e2e workers ${groups.length} (Playwright processes, 1 browser worker each)`)
for (const [index, files] of groups.entries()) {
  const weight = files.reduce((sum, file) => sum + fileWeight(file), 0)
  console.log(`e2e slot ${index} weight=${weight}s files=${files.join(' ')}`)
}
if (process.env.BRAIAN_E2E_PLAN === '1') process.exit(0)

const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'
const windows = process.platform === 'win32'
const children = []

function stop() {
  for (const child of children) {
    if (child.exitCode === null && !child.killed) child.kill('SIGTERM')
  }
}
process.on('SIGINT', () => {
  stop()
  process.exit(1)
})

function spawnLogged(command, args, env) {
  const child = spawn(command, args, { cwd: root, env, stdio: 'inherit', shell: windows })
  children.push(child)
  return child
}

// Parallel Vite processes in one project share node_modules/.vite and deadlock
// or corrupt the optimizer. Give each worker its own cacheDir. The generated
// config sits beside that cache so Vite's cleanup cannot delete it, and it
// only overrides root and cacheDir.
function viteDevArgs(webPort, isolated) {
  const args = [
    '--dir',
    'apps/web',
    'exec',
    'vite',
    'dev',
    '--host',
    '127.0.0.1',
    '--port',
    String(webPort),
    '--strictPort',
  ]
  if (!isolated) return args
  const parent = join(root, 'apps/web/node_modules/.cache')
  mkdirSync(parent, { recursive: true })
  const cacheDir = mkdtempSync(join(parent, 'braian-e2e-vite-'))
  const webRoot = join(root, 'apps/web')
  const configPath = join(parent, `braian-e2e-vite-${webPort}.config.mjs`)
  writeFileSync(
    configPath,
    `import { mergeConfig } from 'vite'
import base from ${JSON.stringify(pathToFileURL(join(webRoot, 'vite.config.ts')).href)}
export default mergeConfig(base, {
  root: ${JSON.stringify(webRoot)},
  cacheDir: ${JSON.stringify(cacheDir)},
})
`,
  )
  args.push('--config', configPath)
  return args
}

function apiEnvFor(db, apiPort, alias) {
  const apiEnv = { ...process.env }
  delete apiEnv.KAFKA_BROKERS
  apiEnv.DATABASE_URL = withDatabase(source, db)
  apiEnv.BRAIAN_SEARCH_ALIAS = alias
  apiEnv.BRAIAN_API_ADDR = `127.0.0.1:${apiPort}`
  apiEnv.BRAIAN_SEED_COUNT = process.env.BRAIAN_E2E_SEED_COUNT || '250'
  apiEnv.BRAIAN_AGENT_MOCK = process.env.BRAIAN_AGENT_MOCK || '1'
  apiEnv.BRAIAN_E2E = '1'
  return apiEnv
}

const slots = groups.map((files, index) => {
  const db = groups.length === 1 ? 'braian_e2e' : `braian_e2e_${index}`
  const apiPort = 8095 + index
  const webPort = 3010 + index
  const alias = groups.length === 1 ? 'braian_e2e' : `braian_e2e_${index}`
  const web = spawnLogged(pnpm, viteDevArgs(webPort, groups.length > 1), {
    ...process.env,
    BRAIAN_API_PROXY: `http://127.0.0.1:${apiPort}`,
  })
  return { files, db, apiPort, webPort, alias, web }
})

function exitCode(error) {
  const status = error && typeof error === 'object' ? error.status : undefined
  if (typeof status === 'number') return status
  console.error(error instanceof Error ? error.message : error)
  return 1
}

function runPlaywright(files, webUrl) {
  return new Promise((resolve) => {
    const child = spawn(pnpm, ['--dir', 'apps/web', 'exec', 'playwright', 'test', ...files], {
      cwd: root,
      stdio: 'inherit',
      shell: windows,
      env: { ...process.env, BRAIAN_WEB_URL: webUrl },
    })
    children.push(child)
    child.on('error', (error) => resolve(exitCode(error)))
    child.on('exit', (code) => resolve(code ?? 1))
  })
}

function waitExit(child) {
  return new Promise((resolve) => {
    if (child.exitCode !== null) return resolve()
    const timer = setTimeout(() => child.kill('SIGKILL'), 10_000)
    child.on('exit', () => {
      clearTimeout(timer)
      resolve()
    })
  })
}

async function withApi(slot, files, alias) {
  const api = spawnLogged(binary, ['all'], apiEnvFor(slot.db, slot.apiPort, alias))
  try {
    await waitFor(`http://127.0.0.1:${slot.apiPort}/healthz`)
    console.log(`e2e db=${slot.db} alias=${alias} api=${slot.apiPort} web=${slot.webPort} files=${files.join(' ')}`)
    return await runPlaywright(files, `http://127.0.0.1:${slot.webPort}`)
  } finally {
    if (api.exitCode === null && !api.killed) api.kill('SIGTERM')
    await waitExit(api)
    await delay(200)
  }
}

async function runSlot(slot) {
  await waitFor(`http://127.0.0.1:${slot.webPort}/`)
  if (groups.length === 1) {
    ensureDatabase(slot.db)
    return withApi(slot, slot.files, slot.alias)
  }
  // Each file gets a reseeded database and its own Typesense alias. Files that
  // share a database leave a second object table, and the next file's strict
  // lookup fails.
  for (let i = 0; i < slot.files.length; i += 1) {
    resetDatabase(slot.db)
    const alias = `${slot.alias}_${i}`
    const status = await withApi(slot, [slot.files[i]], alias)
    if (status !== 0) return status
  }
  return 0
}

let code = 1
try {
  const results = await Promise.all(slots.map((slot) => runSlot(slot)))
  const failed = results.find((status) => status !== 0)
  code = failed ?? 0
} catch (error) {
  code = exitCode(error)
} finally {
  stop()
}
process.exit(code)
