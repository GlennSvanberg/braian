import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('braian.theme', JSON.stringify('dark'))
  })
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('dock')).toBeVisible()
}

test('model structure tree starts collapsed and adds a child under the node', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await page.getByRole('button', { name: 'Data model' }).click()
  const tree = page.getByTestId('data-model').first()
  const dairy = tree.getByTestId('group-dairy')
  await expect(dairy).toBeVisible()
  await expect(dairy.getByRole('button', { name: 'Dairy & eggs' })).toHaveAttribute('aria-expanded', 'false')
  await expect(tree.getByTestId('group-cheese')).toHaveCount(0)
  await expect(tree.getByTestId('group-yoghurt')).toHaveCount(0)
  await expect(tree.getByTestId('group-beverages')).toBeVisible()
  await expect(tree.getByTestId('group-juice')).toHaveCount(0)

  await page.locator('nav.tabs').getByRole('button', { name: /^Products/ }).click()
  await expect(page.getByTestId('structure-tree').getByRole('treeitem', { name: 'Cheese' })).toBeVisible()

  await page.getByRole('button', { name: 'Data model' }).click()
  await dairy.locator('.twist').click()
  await expect(dairy.getByRole('button', { name: 'Dairy & eggs' })).toHaveAttribute('aria-expanded', 'true')
  await expect(tree.getByTestId('group-cheese')).toBeVisible()
  await expect(tree.getByTestId('group-juice')).toHaveCount(0)

  await tree.getByTestId('group-menu-food-cheese').click()
  await page.getByRole('menuitem', { name: 'Add child' }).click()
  const cheese = tree.getByTestId('group-cheese')
  await expect(cheese.getByRole('button', { name: 'Cheese' })).toBeVisible()
  const draft = cheese.getByTestId('group-draft')
  await expect(draft).toBeVisible()
  await expect(draft).toHaveCSS('padding-left', '28px')
  const input = draft.getByTestId('structure-name')
  await expect(input).toBeFocused()
  const order = await cheese.evaluate((node) => {
    const button = node.querySelector('button.model-node')
    const field = node.querySelector('[data-testid="group-draft"]')
    if (!button || !field) return 'missing'
    return (button.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 ? 'below' : 'above'
  })
  expect(order).toBe('below')

  await input.press('Escape')
  await expect(cheese.getByTestId('group-draft')).toHaveCount(0)
  await expect(cheese.getByRole('button', { name: 'Cheese' })).toBeVisible()

  await tree.getByTestId('group-menu-food-cheese').click()
  await page.getByRole('menuitem', { name: 'Add child' }).click()
  const field = cheese.getByTestId('structure-name')
  await expect(field).toBeFocused()
  const suffix = Date.now().toString().slice(-6)
  const name = `Canopy ${suffix}`
  await field.fill(name)
  await field.press('Enter')
  const created = tree.getByTestId(`group-canopy_${suffix}`)
  await expect(created).toBeVisible()
  await expect(created).toContainText(name)
  await expect(cheese.getByTestId('group-draft')).toHaveCount(0)
  const createdOrder = await tree.evaluate((node) => {
    const parent = node.querySelector('[data-testid="group-cheese"]')
    const child = node.querySelector('[data-testid^="group-canopy_"]')
    if (!parent || !child) return 'missing'
    return (parent.compareDocumentPosition(child) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 ? 'below' : 'above'
  })
  expect(createdOrder).toBe('below')
})
