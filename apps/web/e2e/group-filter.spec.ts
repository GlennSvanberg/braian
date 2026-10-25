import { expect, test, type Locator, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/group-filter'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('structure-tree')).toBeVisible()
  await expect(page.getByTestId('object-row').first()).toBeVisible()
}

async function resetEnrich(page: Page) {
  await page.evaluate(() => sessionStorage.setItem('braian.layout.suppressFlush', '1'))
  const listed = await page.request.get('/v1/workspaces')
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  const enrich = body.workspaces.find((item) => item.name === 'Enrich')
  if (!enrich) throw new Error('missing workspace Enrich')
  const me = await page.request.get('/v1/me')
  const userId = ((await me.json()) as { user_id?: string }).user_id
  if (userId) {
    await page.evaluate((key) => localStorage.removeItem(key), `braian.layout.${userId}.default.${enrich.id}`)
  }
  const deleted = await page.request.delete(`/v1/workspaces/${enrich.id}/state?environment=default`)
  expect(deleted.ok() || deleted.status() === 204).toBeTruthy()
  await page.reload()
  await expect(page.getByTestId('object-table')).toBeVisible()
  await page.evaluate(() => sessionStorage.removeItem('braian.layout.suppressFlush'))
}

async function choose(table: Locator, name: string) {
  const inline = table.locator('.toolbar-actions').getByRole('button', { name, exact: true })
  if (await inline.count()) {
    await inline.click()
    return
  }
  await table.getByTestId('toolbar-more').click()
  await table.page().getByRole('menuitem', { name, exact: true }).click()
}

test('selecting a structure group lists the same articles as its assortment', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await resetEnrich(page)
  const table = page.getByTestId('object-table')
  const count = table.getByTestId('row-count')
  await expect(count).not.toHaveText(/^0 /)
  const filter = table.getByTestId('quick-filter')

  await page.getByTestId('structure-tree').getByRole('treeitem', { name: 'Tents' }).click()
  await filter.fill('ART-TALT')
  await expect(table.getByTestId('object-row').first()).toContainText('ART-TALT')
  await expect(table).not.toContainText('no rows')
  const grouped = (await count.textContent()) ?? ''
  expect(grouped.startsWith('0 ')).toBeFalsy()
  await page.screenshot({ path: `${shots}/group-dark.png` })

  await choose(table, 'All')
  await expect(table.getByTestId('object-row').first()).toBeVisible()
  await choose(table, 'Tents')
  await filter.fill('ART-TALT')
  await expect(table.getByTestId('object-row').first()).toContainText('ART-TALT')
  await expect(count).toHaveText(grouped)

  await page.getByRole('button', { name: 'Light' }).click()
  await page.screenshot({ path: `${shots}/group-light.png` })
  await choose(table, 'All')
  await page.getByRole('button', { name: 'Dark' }).click()
})
