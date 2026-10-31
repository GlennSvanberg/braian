import { setTheme } from './ui'
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
  await page.getByTestId('command-palette').locator('.hit').filter({ hasText: 'Articles view' }).filter({ hasText: 'Open view' }).first().click()
  await expect(page.getByTestId('command-palette')).toHaveCount(0)
  await page.getByRole('button', { name: 'All', exact: true }).click()
  await expect(page.getByTestId('row-count')).toBeVisible()
  await page.screenshot({ path: `${shots}/workspace-${theme}.png` })

  await page.keyboard.press('Control+k')
  await page.getByTestId('palette-input').fill('cheese')
  await expect(page.getByTestId('command-palette').locator('.hit').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/command-palette-${theme}.png` })
  await page.keyboard.press('Escape')
  await expect(page.getByTestId('command-palette')).toHaveCount(0)

  await page.getByTestId('assortment').filter({ hasText: 'Cheese' }).click()
  await expect(page.getByTestId('object-row').first()).toContainText('ART-CHED')
  const tent = page.getByTestId('object-row').filter({ hasText: 'ART-CHED-200' })
  await ((await tent.count()) > 0 ? tent.first() : page.getByTestId('object-row').first()).click()
  await page.locator('[data-view-type="object_detail"]').first().click()
  await expect(page.locator('.langs b').first()).toHaveText('en')
  await expect(page.getByTestId('object-ref')).toHaveValue(/PROD-CHEDDAR/)
  await page.screenshot({ path: `${shots}/detail-${theme}.png` })

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  await page.screenshot({ path: `${shots}/graph-${theme}.png` })

  await page.getByRole('button', { name: 'History' }).click()
  await expect(page.getByTestId('history')).toContainText('Created by')
  await expect(page.getByTestId('history').locator('.asof')).toHaveCount(0)
  await page.screenshot({ path: `${shots}/history-${theme}.png` })

  await page.getByRole('button', { name: 'Data model' }).click()
  const tree = page.getByTestId('data-model').first()
  await tree.getByTestId('group-dairy').locator('.twist').click()
  await tree.getByTestId('group-cheese').locator('.model-node').click()
  await expect(page.getByTestId('attribute-table')).toContainText('Fat content')
  await expect(page.getByTestId('attribute-table')).toContainText('from Dairy')
  await page.screenshot({ path: `${shots}/data-model-${theme}.png` })

  await page.locator('nav.tabs').getByRole('button', { name: /^Review/ }).click()
  await expect(page.getByTestId('save-all')).toBeVisible()
  await expect(page.getByTestId('proposal')).toContainText('ART-CHED-400')
  await page.screenshot({ path: `${shots}/proposal-${theme}.png` })
  await page.getByRole('button', { name: 'Jobs', exact: true }).click()
  await expect(page.getByTestId('jobs')).toBeVisible()
  await page.getByRole('button', { name: 'Bulk write' }).click()
  await expect(page.getByTestId('jobs')).toContainText('succeeded')
  await page.screenshot({ path: `${shots}/jobs-${theme}.png` })

  await page.locator('nav.tabs').getByRole('button', { name: 'Products' }).click()
}

test('clean catalog screenshots', async ({ page }) => {
  test.skip(!process.env.BRAIAN_SHOTS, 'set BRAIAN_SHOTS=1 against the demo server')
  test.setTimeout(120_000)
  await login(page)
  await expect(page.getByTestId('row-count')).toContainText('3013')
  await tour(page, 'dark')
  await setTheme(page, 'light')
  await tour(page, 'light')
  await setTheme(page, 'dark')
})
