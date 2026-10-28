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

test('a type is built in a draft, saved, and deleted from the keyboard', async ({ page }) => {
  test.setTimeout(180_000)
  await login(page)
  await command(page, 'Data model')
  await expect(page.getByTestId('data-model').first()).toBeVisible()

  await command(page, 'New type')
  await page.getByTestId('model-dialog-name').fill('Recipe')
  await page.getByTestId('model-dialog-save').click()
  await expect(page.getByTestId('unsaved-changes')).toBeVisible()

  await command(page, 'New value list')
  await page.getByTestId('model-dialog-name').fill('Difficulty')
  await page.getByTestId('model-dialog-options').fill('Easy, Hard')
  await page.getByTestId('model-dialog-save').click()

  await page.getByTestId('type-recipe').click()
  const add = page.getByTestId('attr-add')
  await add.fill('Servings')
  await page.getByRole('combobox', { name: 'Type' }).last().click()
  await page.getByRole('option', { name: 'Number' }).click()
  await page.getByRole('combobox', { name: 'Unit' }).last().click()
  await page.getByRole('option').nth(1).click()
  await page.getByTestId('attr-min').fill('1')
  await page.getByTestId('attr-max').fill('12')
  await add.press('Enter')

  await expect(page.getByTestId('unsaved-changes')).toContainText('unsaved')
  await page.keyboard.press('Control+S')
  const review = page.getByTestId('review-dialog')
  await expect(review).toBeVisible()
  await expect(review.getByTestId('review-lines')).toContainText('Recipe')
  await page.keyboard.press('Escape')
  await expect(review).toBeHidden()
  await expect(page.getByTestId('type-recipe')).toBeVisible()
  await page.keyboard.press('Control+S')
  await expect(review).toBeVisible()
  await page.getByTestId('confirm-review').click()
  await expect(page.getByTestId('unsaved-changes')).toHaveCount(0)
  await expect(page.getByTestId('type-recipe')).toBeVisible()

  const created = await page.request.post('/v1/operations/model.create_attribute', {
    data: { key: 'recipe_note', label: 'Recipe note', kind: 'text' },
  })
  expect(created.ok()).toBeTruthy()
  const proposal = await created.json()
  expect(proposal.needs_approval).toBe(true)

  await page.getByTestId('type-recipe').click({ button: 'right' })
  await page.getByTestId('context-menu').getByRole('button', { name: 'Delete…' }).click()
  const dialog = page.getByTestId('consequence')
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(page.getByTestId('type-recipe')).toBeVisible()

  await command(page, 'Delete type')
  await expect(dialog).toBeVisible()
  await page.getByTestId('confirm-name').fill('Recipe')
  await page.getByTestId('confirm-delete').click()
  await expect(dialog).toBeHidden()
  await expect(page.getByTestId('type-recipe')).toHaveCount(0)
})
