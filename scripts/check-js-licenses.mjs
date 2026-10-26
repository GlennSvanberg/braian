#!/usr/bin/env node
// Fail if an installed package uses a license outside the allow list.
// Run after `pnpm install --frozen-lockfile`.
//
// Permissive SPDX ids: MIT, ISC, Apache-2.0, BSD-2-Clause, BSD-3-Clause,
// 0BSD, BlueOak-1.0.0, Unlicense, CC0-1.0, Python-2.0.
// Named exceptions the current tree needs:
// MPL-2.0 for lightningcss and its platform packages, CC-BY-4.0 for
// caniuse-lite, MIT-0 for @csstools packages pulled in by the CSS build,
// OFL-1.1 for the two shipped font packages.

import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('..', import.meta.url))

const permissive = new Set([
  'MIT',
  'ISC',
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  '0BSD',
  'BlueOak-1.0.0',
  'Unlicense',
  'CC0-1.0',
  'Python-2.0',
])

function extraAllowed(id, name) {
  if (id === 'MPL-2.0' && (name === 'lightningcss' || name.startsWith('lightningcss-'))) return true
  if (id === 'CC-BY-4.0' && name === 'caniuse-lite') return true
  if (id === 'MIT-0' && name.startsWith('@csstools/')) return true
  if (
    id === 'OFL-1.1' &&
    (name === '@fontsource-variable/inter' || name === '@fontsource/jetbrains-mono')
  ) {
    return true
  }
  return false
}

function parse(expr) {
  const tokens = expr
    .replace(/[()]/g, (ch) => ` ${ch} `)
    .split(/\s+/)
    .filter(Boolean)
  let index = 0
  const peek = () => tokens[index]
  const eat = (token) => {
    if (tokens[index] !== token) throw new Error(`expected ${token} in ${expr}`)
    index += 1
  }
  const parseOr = () => {
    const parts = [parseAnd()]
    while (peek() === 'OR') {
      index += 1
      parts.push(parseAnd())
    }
    return parts.length === 1 ? parts[0] : { op: 'OR', parts }
  }
  const parseAnd = () => {
    const parts = [parseAtom()]
    while (peek() === 'AND') {
      index += 1
      parts.push(parseAtom())
    }
    return parts.length === 1 ? parts[0] : { op: 'AND', parts }
  }
  const parseAtom = () => {
    if (peek() === '(') {
      index += 1
      const node = parseOr()
      eat(')')
      return node
    }
    const id = tokens[index]
    index += 1
    if (!id || id === 'OR' || id === 'AND' || id === ')') throw new Error(`bad token in ${expr}`)
    if (peek() === 'WITH') {
      index += 1
      const exception = tokens[index]
      index += 1
      if (!exception) throw new Error(`missing exception in ${expr}`)
      return { op: 'WITH', id, exception }
    }
    return { op: 'ID', id }
  }
  const node = parseOr()
  if (index !== tokens.length) throw new Error(`trailing tokens in ${expr}`)
  return node
}

function accepted(node, name) {
  if (node.op === 'OR') return node.parts.some((part) => accepted(part, name))
  if (node.op === 'AND') return node.parts.every((part) => accepted(part, name))
  if (node.op === 'WITH') return false
  return permissive.has(node.id) || extraAllowed(node.id, name)
}

const raw = execFileSync('pnpm', ['licenses', 'list', '--json'], {
  cwd: root,
  encoding: 'utf8',
  stdio: ['ignore', 'pipe', 'inherit'],
})
const grouped = JSON.parse(raw)
const violations = []
let packages = 0

for (const [license, entries] of Object.entries(grouped)) {
  if (!Array.isArray(entries)) {
    violations.push(`${license}: unexpected pnpm licenses entry`)
    continue
  }
  let node
  try {
    node = parse(license)
  } catch (error) {
    for (const entry of entries) violations.push(`${entry.name}: ${error.message}`)
    continue
  }
  for (const entry of entries) {
    packages += 1
    if (!accepted(node, entry.name)) {
      const versions = Array.isArray(entry.versions) ? entry.versions.join(', ') : ''
      violations.push(`${entry.name}${versions ? `@${versions}` : ''} — ${license}`)
    }
  }
}

if (violations.length > 0) {
  console.error(`JavaScript license check failed (${violations.length}):`)
  for (const line of violations.sort()) console.error(`  ${line}`)
  process.exit(1)
}

console.log(`JavaScript license check passed (${packages} packages).`)
