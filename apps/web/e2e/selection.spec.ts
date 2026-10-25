import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
  await expect(page.getByTestId('structure-tree')).toBeVisible()
}

test('click and arrow share one active table row', async ({ page }) => {
  await login(page)
  const table = page.getByTestId('object-table')
  await table.getByTestId('quick-filter').fill('')
  const rows = table.getByTestId('object-row')
  await expect(rows.nth(2)).toBeVisible()
  await rows.nth(1).click()
  await expect(table.locator('[data-active="true"]')).toHaveCount(1)
  await expect(rows.nth(1)).toHaveAttribute('data-active', 'true')
  await page.keyboard.press('ArrowDown')
  await expect(table.locator('[data-active="true"]')).toHaveCount(1)
  await expect(rows.nth(2)).toHaveAttribute('data-active', 'true')
  await expect(rows.nth(1)).not.toHaveAttribute('data-active', 'true')
})

test('click and arrow share one active structure row', async ({ page }) => {
  await login(page)
  const tree = page.getByTestId('structure-tree')
  const rows = tree.locator('.tree-row')
  await expect(rows.nth(2)).toBeVisible()
  await rows.nth(0).click()
  await expect(tree.locator('[data-active="true"]')).toHaveCount(1)
  await page.keyboard.press('ArrowDown')
  await expect(tree.locator('[data-active="true"]')).toHaveCount(1)
  await expect(rows.nth(1)).toHaveAttribute('data-active', 'true')
  await expect(rows.nth(0)).not.toHaveAttribute('data-active', 'true')
})
