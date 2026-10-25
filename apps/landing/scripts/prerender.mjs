import { cpSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const serverEntry = resolve(root, 'dist/server/entry-server.js')
const { render } = await import(pathToFileURL(serverEntry).href)

const css = readFileSync(resolve(root, '../web/src/styles.css'), 'utf8')
const bg = css.match(/--bg:\s*([^;]+);/)?.[1]?.trim()
if (!bg) throw new Error('could not read --bg from apps/web/src/styles.css')

const indexPath = resolve(root, 'dist/index.html')
let html = readFileSync(indexPath, 'utf8')
html = html.replace('name="theme-color" content=""', `name="theme-color" content="${bg}"`)
const markup = render()
const placeholder = '<div id="root"></div>'
if (!html.includes(placeholder)) {
  throw new Error('dist/index.html is missing the root placeholder')
}
if (!markup.includes('Try the demo') || !markup.includes('View on GitHub')) {
  throw new Error('prerendered page is missing a call to action')
}
if (/chatbot/i.test(markup)) {
  throw new Error('prerendered page describes the agent as a chatbot')
}
if (!markup.includes('Measured on an 8 vCPU VPS')) {
  throw new Error('performance figures are missing their hardware footnote')
}

writeFileSync(indexPath, html.replace(placeholder, `<div id="root">${markup}</div>`))

const brandDest = resolve(root, 'dist/brand')
rmSync(brandDest, { recursive: true, force: true })
cpSync(resolve(root, '../web/public/brand'), brandDest, { recursive: true })
rmSync(resolve(root, 'dist/server'), { recursive: true, force: true })
