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

async function command(page: Page, title: string) {
  await page.keyboard.press('Control+K')
  const palette = page.getByTestId('palette-input')
  await expect(palette).toBeVisible()
  await palette.fill(title)
  await page.keyboard.press('Enter')
}

test('attribute definitions open from the keyboard, show usage, and attach in a draft', async ({ page }) => {
  test.setTimeout(180_000)
  await login(page)
  await command(page, 'Attribute definitions')
  const view = page.getByTestId('attribute-definitions')
  await expect(view).toBeVisible()
  await expect(page.getByTestId('attribute-definition-row')).toHaveCount(54)

  const search = page.getByTestId('attribute-search')
  await search.fill('number')
  await search.press('ArrowDown')
  const number = page.locator('[data-testid="attribute-definition-row"][data-key="number"]')
  await expect(number).toBeVisible()
  await expect(number).toHaveAttribute('data-standard', 'true')
  await page.keyboard.press('Control+Enter')
  const menu = page.getByTestId('attribute-menu')
  await expect(menu.getByRole('button', { name: 'Rename…' })).toBeVisible()
  await expect(menu.getByRole('button', { name: 'Rename key…' })).toHaveCount(0)
  await expect(menu.getByRole('button', { name: 'Delete…' })).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(menu).toHaveCount(0)

  await search.fill('weight')
  await search.press('ArrowDown')
  const weight = page.getByTestId('attribute-definition-row').first()
  await expect(weight).toBeFocused()
  const key = await weight.getAttribute('data-key')
  const objects = await weight.getByTestId('used-in').getAttribute('data-objects')
  const usage = await page.request.post('/v1/operations/model.attribute_usage', { data: { attribute: key } })
  expect(usage.ok()).toBeTruthy()
  const body = await usage.json()
  expect(String(body.used_in.objects)).toBe(objects)
  await expect(weight.getByTestId('used-in')).not.toHaveText('—')

  await page.keyboard.press('Control+Enter')
  await expect(menu.getByRole('button', { name: 'Add to type…' })).toBeFocused()
  await page.keyboard.press('Enter')
  const picker = page.getByTestId('add-to-type-search')
  await expect(picker).toBeVisible()
  await picker.fill('Supplier')
  await picker.press('Enter')
  await expect(page.getByTestId('unsaved-changes')).toBeVisible()

  await command(page, 'Discard draft')
  await expect(page.getByTestId('unsaved-changes')).toHaveCount(0)
})
