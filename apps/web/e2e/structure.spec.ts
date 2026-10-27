import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/structure'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('braian.theme', JSON.stringify('dark'))
  })
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('structure-tree')).toBeVisible()
}

test('structure tree proposes group changes and assortments are saved queries', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const tree = page.getByTestId('structure-tree')
  await expect(tree.getByRole('treeitem', { name: 'Cheese' })).toBeVisible()
  await expect(tree.getByText('Food', { exact: true })).toBeVisible()
  await page.screenshot({ path: `${shots}/tree-dark.png` })

  await tree.getByRole('treeitem', { name: 'Cheese' }).click({ button: 'right' })
  const menu = page.getByTestId('context-menu')
  await expect(menu).toBeVisible()
  await expect(menu).toContainText('Add child')
  await expect(menu).toContainText('Rename')
  await expect(menu).toContainText('Delete')
  await page.screenshot({ path: `${shots}/menu-dark.png` })
  await menu.getByRole('button', { name: 'Add child' }).click()
  const edit = page.getByTestId('tree-edit')
  await expect(edit).toBeVisible()
  await edit.fill('canopy')
  await edit.press('Enter')
  const consequence = page.getByTestId('consequence')
  await expect(consequence).toBeVisible()
  await expect(consequence.getByTestId('model-diff')).toContainText('canopy')
  await page.screenshot({ path: `${shots}/proposal-dark.png` })
  await consequence.getByRole('button', { name: 'Cancel' }).click()
  await expect(consequence).toHaveCount(0)

  await tree.getByRole('treeitem', { name: 'Yoghurt' }).dragTo(tree.getByRole('treeitem', { name: 'Beverages' }))
  await expect(page.getByTestId('consequence')).toBeVisible()
  await expect(page.getByTestId('model-diff')).toContainText('Yoghurt')
  await page.getByTestId('consequence').getByRole('button', { name: 'Cancel' }).click()

  await page.locator('.pane-tab', { hasText: 'Selections' }).click()
  const list = page.getByTestId('assortment-list')
  await expect(list.getByTestId('assortment-row').filter({ hasText: 'Cheese' })).toBeVisible()
  await expect(list.getByTestId('assortment-row').filter({ hasText: 'Organic' })).toHaveCount(0)

  const articlesTab = page.locator('nav.tabs').getByRole('button', { name: 'Articles' })
  await articlesTab.click()
  await expect(page.locator('.pane-tab', { hasText: 'Articles' })).toBeVisible()
  await page.locator('.pane-tab', { hasText: 'Selections' }).click()
  const articles = page.getByTestId('assortment-list')
  await expect(articles.getByTestId('assortment-row').filter({ hasText: 'Cheese' })).toBeVisible()
  await expect(articles.getByTestId('assortment-row').filter({ hasText: 'Organic' })).toBeVisible()
  await articles.getByTestId('assortment-row').filter({ hasText: 'Cheese' }).click()
  const open = page.getByTestId('assortment-open')
  await expect(open).toContainText('Cheese')
  await expect(open).toContainText('matching objects')
  await expect(open.locator('.row').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/assortments-dark.png` })

  await page.getByRole('button', { name: 'Light' }).click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light')
  await page.screenshot({ path: `${shots}/assortments-light.png` })
})
