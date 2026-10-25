import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/screenshots'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
}

async function tour(page: Page, theme: 'dark' | 'light') {
  await page.keyboard.press('Control+k')
  await page.getByTestId('palette-input').fill('Articles view')
  await page.getByTestId('palette').locator('.hit').filter({ hasText: 'Articles view' }).filter({ hasText: 'Open view' }).first().click()
  await expect(page.getByTestId('palette')).toHaveCount(0)
  await page.getByRole('button', { name: 'All', exact: true }).click()
  await expect(page.getByTestId('row-count')).toBeVisible()
  await page.screenshot({ path: `${shots}/workspace-${theme}.png` })

  await page.keyboard.press('Control+k')
  await page.getByTestId('palette-input').fill('talt')
  await expect(page.getByTestId('palette').locator('.hit').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/command-palette-${theme}.png` })
  await page.keyboard.press('Escape')
  await expect(page.getByTestId('palette')).toHaveCount(0)

  await page.getByTestId('assortment').filter({ hasText: 'Tents' }).click()
  await expect(page.getByTestId('object-row').first()).toContainText('ART-TALT')
  const tent = page.getByTestId('object-row').filter({ hasText: 'ART-TALT-3' })
  await ((await tent.count()) > 0 ? tent.first() : page.getByTestId('object-row').first()).click()
  await page.locator('[data-view-type="object_detail"]').first().click()
  await expect(page.locator('.langs b').first()).toHaveText('sv')
  await expect(page.getByTestId('object-ref')).toHaveValue(/ART-TALT-2/)
  await page.screenshot({ path: `${shots}/detail-${theme}.png` })

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  await page.screenshot({ path: `${shots}/graph-${theme}.png` })

  await page.getByRole('button', { name: 'History' }).click()
  await expect(page.getByTestId('field-diff')).toContainText('nummer')
  await expect(page.getByTestId('history')).toContainText('create')
  await page.screenshot({ path: `${shots}/history-${theme}.png` })

  await page.getByRole('button', { name: 'Data model' }).click()
  const tree = page.getByTestId('data-model').first()
  await tree.getByTestId('group-talt').getByRole('button').click()
  await expect(page.getByTestId('attribute-table')).toContainText('Weight')
  await expect(page.getByTestId('attribute-table')).toContainText('from Outdoor')
  await page.screenshot({ path: `${shots}/data-model-${theme}.png` })

  await page.getByRole('button', { name: 'Set up the system' }).click()
  await expect(page.getByTestId('dashboard')).toBeVisible()
  await page.screenshot({ path: `${shots}/dashboard-${theme}.png` })

  await page.getByRole('button', { name: 'Review' }).click()
  await expect(page.getByTestId('accept-all')).toBeVisible()
  await expect(page.getByTestId('proposal')).toContainText('ART-TALT-3')
  await page.screenshot({ path: `${shots}/proposal-${theme}.png` })
  await page.getByRole('button', { name: 'Jobs', exact: true }).click()
  await expect(page.getByTestId('jobs')).toBeVisible()
  await page.getByRole('button', { name: 'Bulk write' }).click()
  await expect(page.getByTestId('jobs')).toContainText('succeeded')
  await page.screenshot({ path: `${shots}/jobs-${theme}.png` })

  await page.getByRole('button', { name: 'Enrich' }).click()
}

test('clean catalog screenshots', async ({ page }) => {
  test.skip(!process.env.BRAIAN_SHOTS, 'set BRAIAN_SHOTS=1 against the demo server')
  test.setTimeout(120_000)
  await login(page)
  await expect(page.getByTestId('row-count')).toContainText('3013')
  await tour(page, 'dark')
  await page.getByRole('button', { name: 'Light' }).click()
  await tour(page, 'light')
  await page.getByRole('button', { name: 'Dark' }).click()
})
