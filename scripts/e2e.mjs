// Playwright against its own Postgres database and Typesense alias.
// The demo database used by `just dev` is not written.
//
// BRAIAN_BIN, when set, is the server binary. CI points it at the release
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

function fileWeight(file) {
  const text = readFileSync(join(root, 'apps/web', file), 'utf8')
  const matches = text.match(/^\s*test(?:\.[A-Za-z]+)?\(/gm)
  return matches ? matches.length : 1
}

// Lightest-bin assignment keeps every file's tests on one database, in the
// original relative order, and keeps the processes close in test count.
function partition(list, workers) {
  const bins = Array.from({ length: workers }, () => ({ files: [], weight: 0 }))
  for (const file of list) {
    let lightest = bins[0]
    for (const bin of bins) {
      if (bin.weight < lightest.weight) lightest = bin
    }
    lightest.files.push(file)
    lightest.weight += fileWeight(file)
  }
  return bins.filter((bin) => bin.files.length > 0).map((bin) => bin.files)
}

function withDatabase(url, name) {
  const queryAt = url.indexOf('?')
  const base = queryAt === -1 ? url : url.slice(0, queryAt)
  const query = queryAt === -1 ? '' : url.slice(queryAt)
  const slash = base.lastIndexOf('/')
  return `${base.slice(0, slash + 1)}${name}${query}`
}

function ensureDatabase(name) {
  const user = process.env.POSTGRES_USER || 'braian'
  try {
    execFileSync(
      'docker',
      ['compose', 'exec', '-T', 'postgres', 'psql', '-U', user, '-d', 'postgres', '-c', `CREATE DATABASE ${name}`],
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

const slots = groups.map((files, index) => {
  const db = groups.length === 1 ? 'braian_e2e' : `braian_e2e_${index}`
  const apiPort = 8095 + index
  const webPort = 3010 + index
  const alias = groups.length === 1 ? 'braian_e2e' : `braian_e2e_${index}`
  ensureDatabase(db)
  const apiEnv = { ...process.env }
  delete apiEnv.KAFKA_BROKERS
  apiEnv.DATABASE_URL = withDatabase(source, db)
  apiEnv.BRAIAN_SEARCH_ALIAS = alias
  apiEnv.BRAIAN_API_ADDR = `127.0.0.1:${apiPort}`
  apiEnv.BRAIAN_SEED_COUNT = process.env.BRAIAN_E2E_SEED_COUNT || '250'
  apiEnv.BRAIAN_AGENT_MOCK = process.env.BRAIAN_AGENT_MOCK || '1'
  apiEnv.BRAIAN_E2E = '1'
  const api = spawnLogged(binary, ['all'], apiEnv)
  const web = spawnLogged(pnpm, viteDevArgs(webPort, groups.length > 1), {
    ...process.env,
    BRAIAN_API_PROXY: `http://127.0.0.1:${apiPort}`,
  })
  return { files, db, apiPort, webPort, alias, api, web }
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

let code = 1
try {
  const results = await Promise.all(
    slots.map(async (slot) => {
      await waitFor(`http://127.0.0.1:${slot.apiPort}/healthz`)
      await waitFor(`http://127.0.0.1:${slot.webPort}/`)
      console.log(
        `e2e slot ready db=${slot.db} alias=${slot.alias} api=${slot.apiPort} web=${slot.webPort} files=${slot.files.join(' ')}`,
      )
      return runPlaywright(slot.files, `http://127.0.0.1:${slot.webPort}`)
    }),
  )
  const failed = results.find((status) => status !== 0)
  code = failed ?? 0
} catch (error) {
  code = exitCode(error)
} finally {
  stop()
}
process.exit(code)
