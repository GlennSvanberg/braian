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
  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('palette')).toBeVisible()
  await page.getByTestId('palette-input').fill('Articles view')
  const entry = page.getByTestId('palette').locator('.hit').filter({ hasText: 'Articles view' }).filter({ hasText: 'Open view' })
  await entry.first().click()
  await expect(page.getByTestId('palette')).toHaveCount(0)
  await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toBeVisible()
}

async function openTent(page: Page) {
  await openArticles(page)
  await page.getByTestId('quick-filter').fill('ART-TALT-2')
  const row = page.getByTestId('object-row').filter({ hasText: 'ART-TALT-2' }).first()
  await expect(row).toBeVisible()
  await row.click()
  await page.locator('.pane-tab', { hasText: 'Graph' }).first().click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  await expect(page.locator('canvas[data-testid=graph]')).toHaveAttribute('data-settled', 'true')
  await expect(page.getByTestId('graph-inspector')).toContainText('2-person tent')
}

test('force-directed graph', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await openTent(page)

  for (const kind of ['type-artikel', 'class', 'group', 'value', 'assortment']) {
    await expect(page.getByTestId(`legend-${kind}`)).toBeVisible()
    await expect(page.getByTestId(`legend-${kind}`)).toHaveAttribute('aria-pressed', 'true')
  }
  const provenance = page.getByTestId('attribute-provenance')
  await expect(provenance).toContainText('Number')
  await expect(provenance).toContainText('Description')
  await expect(provenance).toContainText('Colour')
  await expect(provenance).not.toContainText('Nummer')
  await expect(provenance).not.toContainText('Beschreibung')
  await expect(provenance).not.toContainText('Farbe')
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
  const colour = nodes.getByRole('button', { name: 'Colour', exact: true })
  await expect(colour).toHaveAttribute('data-x', /-?\d+/)
  await colour.dispatchEvent('click')
  await expect(page.getByTestId('graph-inspector')).toContainText('Colour')
  await expect(page.getByTestId('graph-inspector')).toContainText('Green')
  if (process.env.BRAIAN_GRAPH_SHOTS) await page.screenshot({ path: `${shots}/selected.png` })

  const x = Number(await colour.getAttribute('data-x'))
  const y = Number(await colour.getAttribute('data-y'))
  await page.mouse.move(x, y)
  await page.mouse.down()
  await page.mouse.move(x + 70, y + 28, { steps: 10 })
  await page.mouse.up()
  await expect(page.getByTestId('graph-unpin')).toBeVisible()
  await page.getByTestId('graph-unpin').click()
  await expect(page.getByTestId('graph-unpin')).toHaveCount(0)

  await page.getByTestId('legend-value').click()
  await expect(page.getByTestId('legend-value')).toHaveAttribute('aria-pressed', 'false')
  await expect(nodes.getByRole('button', { name: 'Colour', exact: true })).toHaveCount(0)
  await page.getByTestId('legend-value').click()
  await expect(page.getByTestId('graph-inspector')).toContainText('Colour')

  const before = await nodes.locator('button').count()
  const product = nodes.getByRole('button', { name: 'Fjäll tents' })
  await expect(product).toHaveCount(1)
  await product.dispatchEvent('dblclick')
  await expect.poll(async () => nodes.locator('button').count()).toBeGreaterThan(before)
  await expect(nodes.getByRole('button', { name: 'Tent 2 green' })).toHaveCount(1)
  await expect(nodes.getByRole('button', { name: 'VAR-TALT-2-GRON' })).toHaveCount(0)
  await expect(nodes.getByRole('button', { name: 'ART-TALT-3' })).toHaveCount(0)
  await expect(page.locator('canvas[data-testid=graph]')).toHaveAttribute('data-settled', 'true', { timeout: 15_000 })
  if (process.env.BRAIAN_GRAPH_SHOTS) await page.screenshot({ path: `${shots}/expanded.png` })

  const variant = nodes.getByRole('button', { name: 'Tent 2 green' })
  await variant.dispatchEvent('click')
  await expect(page.getByTestId('graph-inspector')).toContainText('Tent 2 green')
  await expect(page.getByTestId('graph-inspector')).toContainText('VAR-TALT-2-GRON')
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
  await page.getByTestId('quick-filter').fill('')
  await page.locator('[data-view-type="object_detail"]').first().click()
})
