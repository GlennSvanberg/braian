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
  const outdoor = tree.getByTestId('group-friluft')
  await expect(outdoor).toBeVisible()
  await expect(outdoor.getByRole('button', { name: 'Outdoor' })).toHaveAttribute('aria-expanded', 'false')
  await expect(tree.getByTestId('group-talt')).toHaveCount(0)
  await expect(tree.getByTestId('group-sovsack')).toHaveCount(0)
  await expect(tree.getByTestId('group-bygg')).toBeVisible()
  await expect(tree.getByTestId('group-virke')).toHaveCount(0)
  await expect(tree.getByTestId('group-climate')).toBeVisible()

  await page.locator('nav.tabs').getByRole('button', { name: /^Products/ }).click()
  await expect(page.getByTestId('structure-tree').getByRole('treeitem', { name: 'Tents' })).toBeVisible()

  await page.getByRole('button', { name: 'Data model' }).click()
  await outdoor.locator('.twist').click()
  await expect(outdoor.getByRole('button', { name: 'Outdoor' })).toHaveAttribute('aria-expanded', 'true')
  await expect(tree.getByTestId('group-talt')).toBeVisible()
  await expect(tree.getByTestId('group-virke')).toHaveCount(0)

  await tree.getByTestId('group-menu-sortiment-talt').click()
  await page.getByRole('menuitem', { name: 'Add child' }).click()
  const tents = tree.getByTestId('group-talt')
  await expect(tents.getByRole('button', { name: 'Tents' })).toBeVisible()
  const draft = tents.getByTestId('group-draft')
  await expect(draft).toBeVisible()
  await expect(draft).toHaveCSS('padding-left', '28px')
  const input = draft.getByTestId('structure-name')
  await expect(input).toBeFocused()
  const order = await tents.evaluate((node) => {
    const button = node.querySelector('button.model-node')
    const field = node.querySelector('[data-testid="group-draft"]')
    if (!button || !field) return 'missing'
    return (button.compareDocumentPosition(field) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 ? 'below' : 'above'
  })
  expect(order).toBe('below')

  await input.press('Escape')
  await expect(tents.getByTestId('group-draft')).toHaveCount(0)
  await expect(tents.getByRole('button', { name: 'Tents' })).toBeVisible()

  await tree.getByTestId('group-menu-sortiment-talt').click()
  await page.getByRole('menuitem', { name: 'Add child' }).click()
  const field = tents.getByTestId('structure-name')
  await expect(field).toBeFocused()
  const suffix = Date.now().toString().slice(-6)
  const name = `Canopy ${suffix}`
  await field.fill(name)
  await field.press('Enter')
  const created = tree.getByTestId(`group-canopy_${suffix}`)
  await expect(created).toBeVisible()
  await expect(created).toContainText(name)
  await expect(tents.getByTestId('group-draft')).toHaveCount(0)
  const createdOrder = await tree.evaluate((node) => {
    const parent = node.querySelector('[data-testid="group-talt"]')
    const child = node.querySelector('[data-testid^="group-canopy_"]')
    if (!parent || !child) return 'missing'
    return (parent.compareDocumentPosition(child) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0 ? 'below' : 'above'
  })
  expect(createdOrder).toBe('below')
})
