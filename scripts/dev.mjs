// Starts the API (migrations, seed, relay, indexer) and the web app.
// `just` loads .env before this process, on Windows and in the devcontainer.
import { spawn } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const children = []

function run(command, args) {
  const child = spawn(command, args, {
    cwd: root,
    stdio: 'inherit',
    shell: process.platform === 'win32',
  })
  children.push(child)
  child.on('exit', (code, signal) => {
    if (signal) return
    if (code && code !== 0) stop(code)
  })
}

function stop(code = 0) {
  for (const child of children) {
    if (!child.killed) child.kill('SIGTERM')
  }
  setTimeout(() => process.exit(code), 200)
}

process.on('SIGINT', () => stop(0))
process.on('SIGTERM', () => stop(0))

console.log('Braian API on http://127.0.0.1:8090 and web on http://127.0.0.1:3000')
console.log('Demo login: BRAIAN_DEMO_EMAIL and BRAIAN_DEMO_PASSWORD in .env (see .env.example)')
run('cargo', ['run', '-p', 'braian-server', '--', 'all'])
run('pnpm', ['--dir', 'apps/web', 'exec', 'vite', 'dev', '--host', '127.0.0.1', '--port', '3000'])
