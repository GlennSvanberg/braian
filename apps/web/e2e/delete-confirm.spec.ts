import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('structure-tree')).toBeVisible()
}

test('the delete dialog is keyboard only and asks for the name', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  const created = await page.request.post('/v1/structures/food/groups', {
    data: { key: 'temp_delete', label: 'Temp delete' },
  })
  expect(created.ok()).toBeTruthy()
  await page.reload()
  const tree = page.getByTestId('structure-tree')
  await expect(tree.getByRole('treeitem', { name: 'Cheese' })).toBeVisible()
  await tree.getByRole('treeitem', { name: 'Cheese' }).click()

  await page.keyboard.press('Control+K')
  const palette = page.getByTestId('palette-input')
  await expect(palette).toBeVisible()
  await palette.fill('Delete the selected')
  await page.keyboard.press('Enter')

  const dialog = page.getByTestId('consequence')
  await expect(dialog).toBeVisible()
  const name = page.getByTestId('confirm-name')
  await expect(name).toBeFocused()
  await expect(dialog.getByTestId('delete-counts')).toContainText('will be')
  await name.fill('nope')
  const remove = dialog.getByTestId('confirm-delete')
  await expect(remove).toBeDisabled()
  await name.press('Enter')
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(tree.getByRole('treeitem', { name: 'Cheese' })).toBeVisible()

  await tree.getByRole('treeitem', { name: 'Temp delete' }).click()
  await page.keyboard.press('Control+K')
  await expect(palette).toBeVisible()
  await palette.fill('Delete the selected')
  await page.keyboard.press('Enter')
  await expect(dialog).toBeVisible()
  await name.fill('Temp delete')
  await expect(remove).toBeEnabled()
  await page.keyboard.press('Tab')
  await page.keyboard.press('Tab')
  await expect(remove).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(dialog).toBeHidden()
  await expect(tree.getByRole('treeitem', { name: 'Temp delete' })).toHaveCount(0)
})
