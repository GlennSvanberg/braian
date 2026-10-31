import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/graph'

async function login(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('braian.theme', JSON.stringify('dark'))
  })
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
}

async function openArticles(page: Page) {
  // Notion 112 queues openTypeView until the workspace doc is ready; keep a
  // short retry for unrelated timing flake on the Articles tab paint.
  await expect(async () => {
    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('command-palette')).toBeVisible()
    await page.getByTestId('palette-input').fill('Articles view')
    const entry = page.getByTestId('command-palette').locator('.hit').filter({ hasText: 'Articles view' }).filter({ hasText: 'Open view' })
    await expect(entry.first()).toBeVisible()
    await entry.first().click()
    await expect(page.getByTestId('command-palette')).toHaveCount(0)
    await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toBeVisible({ timeout: 5_000 })
  }).toPass({ timeout: 30_000 })
}

function articlesPane(page: Page) {
  return page.locator('section.pane', { has: page.locator('.pane-tab', { hasText: 'Articles' }) })
}

async function openCheddar(page: Page) {
  await openArticles(page)
  const articles = articlesPane(page)
  await articles.getByTestId('quick-filter').fill('ART-CHED-200')
  const row = articles.getByTestId('object-row').filter({ hasText: 'ART-CHED-200' }).first()
  await expect(row).toBeVisible()
  const objectId = (await row.getAttribute('id'))?.replace(/^object-row-/, '') ?? ''
  expect(objectId, 'article row id').not.toBe('')
  const stored = await page.request.get(`/v1/objects/${objectId}`)
  expect(stored.ok()).toBeTruthy()
  const body = (await stored.json()) as { values?: { storage?: string; name?: { en?: string } } }
  expect(body.values?.storage, JSON.stringify(body.values)).toBe('chilled')
  expect(body.values?.name?.en).toBe('Mature cheddar 200 g')
  await row.click()
  await page.locator('.pane-tab', { hasText: 'Graph' }).first().click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  await expect(page.locator('canvas[data-testid=graph]')).toHaveAttribute('data-settled', 'true')
  await expect(page.getByTestId('graph-inspector')).toContainText('Mature cheddar 200 g')
  return objectId
}

test('force-directed graph', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const objectId = await openCheddar(page)

  for (const kind of ['type-article', 'group', 'value', 'structure']) {
    await expect(page.getByTestId(`legend-${kind}`)).toBeVisible()
    await expect(page.getByTestId(`legend-${kind}`)).toHaveAttribute('aria-pressed', 'true')
  }
  await expect(page.getByTestId('legend-class')).toHaveCount(0)
  const provenance = page.getByTestId('attribute-provenance')
  await expect(provenance).toContainText('Number')
  await expect(provenance).toContainText('Storage')
  await expect(provenance).not.toContainText('Colour')
  await expect(provenance).not.toContainText('Nummer')
  await expect(provenance).not.toContainText('namn')
  await expect(provenance).not.toContainText('friluft')

  await page.getByRole('button', { name: 'Open full size' }).click()
  await expect(page.getByTestId('graph-full')).toBeVisible()
  await expect.poll(async () => (await page.locator('canvas[data-testid=graph]').boundingBox())?.width ?? 0).toBeGreaterThan(800)

  if (process.env.BRAIAN_GRAPH_SHOTS) {
    mkdirSync(shots, { recursive: true })
    await page.screenshot({ path: `${shots}/dark.png` })
  }

  const nodes = page.getByTestId('graph-nodes')
  const storage = nodes.locator(`[data-node-id="value:${objectId}:storage"]`)
  await expect(storage).toHaveAttribute('data-x', /-?\d+/)
  await storage.dispatchEvent('click')
  await expect(page.getByTestId('graph-inspector')).toContainText('Storage')
  await expect(page.getByTestId('graph-inspector')).toContainText('Chilled')
  if (process.env.BRAIAN_GRAPH_SHOTS) await page.screenshot({ path: `${shots}/selected.png` })

  const x = Number(await storage.getAttribute('data-x'))
  const y = Number(await storage.getAttribute('data-y'))
  await page.mouse.move(x, y)
  await page.mouse.down()
  await page.mouse.move(x + 70, y + 28, { steps: 10 })
  await page.mouse.up()
  await expect(page.getByTestId('graph-unpin')).toBeVisible()
  await page.getByTestId('graph-unpin').click()
  await expect(page.getByTestId('graph-unpin')).toHaveCount(0)

  await page.getByTestId('legend-value').click()
  await expect(page.getByTestId('legend-value')).toHaveAttribute('aria-pressed', 'false')
  await expect(nodes.getByRole('button', { name: 'Storage', exact: true })).toHaveCount(0)
  await page.getByTestId('legend-value').click()
  await expect(page.getByTestId('graph-inspector')).toContainText('Storage')

  const before = await nodes.locator('button').count()
  const product = nodes.getByRole('button', { name: 'Mature cheddar', exact: true })
  await expect(product).toHaveCount(1)
  await product.dispatchEvent('dblclick')
  await expect.poll(async () => nodes.locator('button').count()).toBeGreaterThan(before)
  await expect(nodes.getByRole('button', { name: 'Ingredients', exact: true })).toHaveCount(1)
  await expect(nodes.getByRole('button', { name: 'Nutri-Score', exact: true })).toHaveCount(1)
  await expect(page.locator('canvas[data-testid=graph]')).toHaveAttribute('data-settled', 'true', { timeout: 15_000 })
  if (process.env.BRAIAN_GRAPH_SHOTS) await page.screenshot({ path: `${shots}/expanded.png` })

  const recipe = nodes.getByRole('button', { name: 'Mature cheddar', exact: true })
  await recipe.dispatchEvent('click')
  await expect(page.getByTestId('graph-inspector')).toContainText('Mature cheddar')
  await expect(page.getByTestId('graph-inspector')).toContainText('PROD-CHEDDAR')
  if (process.env.BRAIAN_GRAPH_SHOTS) await page.screenshot({ path: `${shots}/selected.png` })

  await page.getByTestId('graph-fit').click()
  await page.getByTestId('graph-relayout').click()
  await expect(page.locator('canvas[data-testid=graph]')).toHaveAttribute('data-settled', 'false')
  await expect(page.locator('canvas[data-testid=graph]')).toHaveAttribute('data-settled', 'true')

  await page.getByRole('button', { name: 'Close full size' }).click()
  await page.getByRole('button', { name: 'Light' }).click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light')
  await page.getByRole('button', { name: 'Open full size' }).click()
  await expect(page.getByTestId('graph-full')).toBeVisible()
  if (process.env.BRAIAN_GRAPH_SHOTS) await page.screenshot({ path: `${shots}/light.png` })
  await page.getByRole('button', { name: 'Close full size' }).click()
  await page.getByRole('button', { name: 'Dark' }).click()
  const filters = page.getByTestId('quick-filter')
  for (let index = 0; index < (await filters.count()); index += 1) await filters.nth(index).fill('')
  await page.locator('[data-view-type="object_detail"]').first().click()
})
