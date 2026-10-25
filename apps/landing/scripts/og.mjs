import { mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { dirname, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const workspace = resolve(dirname(fileURLToPath(import.meta.url)), '../../..')
const pnpm = resolve(workspace, 'node_modules/.pnpm')
const playwrightDir = readdirSync(pnpm).find((name) => name.startsWith('playwright-core@'))
if (!playwrightDir) throw new Error('playwright-core is not installed')
const require = createRequire(import.meta.url)
const { chromium } = require(resolve(pnpm, playwrightDir, 'node_modules/playwright-core'))

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const css = readFileSync(resolve(root, '../web/src/styles.css'), 'utf8')
const tokens = css.match(/:root \{[\s\S]*?\n\}/)
if (!tokens) throw new Error('missing :root tokens')
const mark = readFileSync(resolve(root, '../web/public/brand/braian-mark.svg'), 'utf8')
const fontDir = readdirSync(pnpm).find((name) => name.startsWith('@fontsource-variable+inter@'))
if (!fontDir) throw new Error('Inter is not installed')
const font = resolve(
  pnpm,
  fontDir,
  'node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2',
)

const dir = mkdtempSync(resolve(tmpdir(), 'braian-og-'))
const htmlPath = resolve(dir, 'og.html')
const pngPath = resolve(root, 'public/og.png')

writeFileSync(
  htmlPath,
  `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<style>
@font-face {
  font-family: "Inter Variable";
  src: url("${font}") format("woff2");
  font-weight: 100 900;
  font-style: normal;
  font-display: block;
}
${tokens[0]}
html, body { margin: 0; width: 1200px; height: 630px; background: var(--bg); }
body {
  box-sizing: border-box;
  padding: 36px;
  font-family: "Inter Variable", Inter, sans-serif;
  font-feature-settings: "cv11", "ss01";
  color: var(--text);
}
.sheet {
  height: 100%;
  box-sizing: border-box;
  display: grid;
  grid-template-columns: 1.15fr 0.85fr;
  gap: 36px;
  align-items: center;
  padding: 56px 60px;
  background: var(--panel);
  border-radius: var(--r-xl);
  box-shadow: var(--shadow-md), var(--inset);
}
.brand { display: flex; align-items: center; gap: 12px; font-size: 22px; font-weight: 600; }
.brand svg { width: 36px; height: 36px; }
h1 {
  margin: 18px 0 0;
  font-size: 64px;
  font-weight: 500;
  letter-spacing: -0.035em;
  line-height: 1.02;
}
h1 span { color: var(--accent); }
p { margin: 20px 0 0; font-size: 24px; color: var(--text-2); }
.command {
  background: var(--raised);
  border: 1px solid var(--border);
  border-radius: var(--r-xl);
  box-shadow: var(--shadow-md), var(--inset);
  padding: 10px;
  display: grid;
  gap: 6px;
}
.input, .row {
  border-radius: var(--r-md);
  min-height: 40px;
  display: flex;
  align-items: center;
  padding: 0 12px;
  font-size: 16px;
}
.input { background: var(--panel); color: var(--muted); box-shadow: var(--inset); }
.row { color: var(--text); }
.row small { display: block; color: var(--muted); font-size: 13px; }
.ask { background: var(--accent-dim); color: var(--text); }
</style>
</head>
<body>
  <div class="sheet">
    <div>
      <div class="brand">${mark}<span>Braian</span></div>
      <h1>Product data,<br /><span>built for agents.</span></h1>
      <p>Open source · MIT · Self-hosted</p>
    </div>
    <div class="command">
      <div class="input">Search, or ask the agent</div>
      <div class="row"><span>Items<small>Open the workspace</small></span></div>
      <div class="row ask"><span>Ask AI<small>Works in place</small></span></div>
    </div>
  </div>
</body>
</html>`,
)

const browser = await chromium.launch({
  executablePath: '/usr/bin/google-chrome',
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
})
const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 })
await page.goto(pathToFileURL(htmlPath).href, { waitUntil: 'networkidle' })
await page.screenshot({ path: pngPath })
await browser.close()
console.log(pngPath)
