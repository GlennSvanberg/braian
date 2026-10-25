import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

function readEnvFile(path: string) {
  try {
    return readFileSync(path, 'utf8')
  } catch {
    return ''
  }
}

function fromFile(name: string) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..')
  const text = `${readEnvFile(resolve(root, '.env'))}\n${readEnvFile(resolve(process.cwd(), '.env'))}`
  for (const line of text.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    const eq = trimmed.indexOf('=')
    if (eq < 1 || trimmed.slice(0, eq) !== name) continue
    let value = trimmed.slice(eq + 1).trim()
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1)
    }
    if (value) return value
  }
  return ''
}

export function demoCredentials() {
  const email = process.env.BRAIAN_DEMO_EMAIL || fromFile('BRAIAN_DEMO_EMAIL')
  const password = process.env.BRAIAN_DEMO_PASSWORD || fromFile('BRAIAN_DEMO_PASSWORD')
  if (!email || !password) {
    throw new Error('Set BRAIAN_DEMO_EMAIL and BRAIAN_DEMO_PASSWORD in the environment or .env')
  }
  return { email, password }
}
