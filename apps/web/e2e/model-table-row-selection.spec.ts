import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('workspace')).toBeVisible()
}

async function openDataModelTab(page: Page, label: string, tableTestId: string) {
  await page.getByRole('button', { name: 'Data model' }).click()
  const tab = page.locator('.pane-tab').filter({ hasText: label }).first()
  await expect(tab).toBeVisible({ timeout: 30_000 })
  await tab.click()
  const table = page.getByTestId(tableTestId)
  await expect(table).toBeVisible()
  await expect(table.locator('[data-testid="standard-grid-row"]').first()).toBeVisible()
}

async function assertRowSelection(page: Page, tableTestId: string) {
  const table = page.getByTestId(tableTestId)
  const rows = table.locator('[data-testid="standard-grid-row"]')
  await expect(rows.first()).toBeVisible()
  const count = await rows.count()
  expect(count).toBeGreaterThan(1)

  await rows.nth(0).click()
  await expect(rows.nth(0)).toHaveClass(/\bon\b/)

  // Hover another row: background should change vs idle (Products-like hover).
  const hoverTarget = rows.nth(1)
  await hoverTarget.hover()
  const hoverBg = await hoverTarget.evaluate((el) => getComputedStyle(el).backgroundColor)
  expect(hoverBg).not.toBe('rgba(0, 0, 0, 0)')
  expect(hoverBg).not.toBe('transparent')

  await rows.nth(0).focus()
  await page.keyboard.press('ArrowDown')
  await expect(rows.nth(1)).toHaveClass(/\bon\b/)
  await expect(rows.nth(0)).not.toHaveClass(/\bon\b/)

  await page.keyboard.press('ArrowUp')
  await expect(rows.nth(0)).toHaveClass(/\bon\b/)
  await expect(rows.nth(1)).not.toHaveClass(/\bon\b/)
}

test('Data model Object types and Units share Products-like row selection', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await openDataModelTab(page, 'Object types', 'object-types-table')
  await assertRowSelection(page, 'object-types-table')

  await openDataModelTab(page, 'Units', 'units-table')
  await assertRowSelection(page, 'units-table')
})
