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

async function openObject(page: Page, key: string) {
  await page.keyboard.press('Control+K')
  const palette = page.getByTestId('palette-input')
  await palette.fill(key)
  await expect(page.getByTestId('palette-list')).toContainText(key)
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('object-detail')).toBeVisible()
}

test('pairs with links two cheeses from the keyboard and the link type can be deleted', async ({ page }) => {
  test.setTimeout(180_000)
  await login(page)
  await command(page, 'Data model')
  await expect(page.getByTestId('data-model').first()).toBeVisible()
  await expect(page.getByText('Link types').first()).toBeVisible()

  await command(page, 'New link type')
  await page.getByTestId('model-dialog-name').fill('Pairs with')
  await page.getByTestId('link-type-from').selectOption('article')
  await page.getByTestId('link-type-to').selectOption('article')
  await page.getByTestId('link-type-attribute').fill('note')
  await page.getByTestId('model-dialog-save').click()
  await expect(page.getByTestId('unsaved-changes')).toBeVisible()
  await page.keyboard.press('Control+S')
  await page.getByTestId('confirm-review').click()
  await expect(page.getByTestId('unsaved-changes')).toHaveCount(0)
  await expect(page.getByTestId('link-type-pairs_with')).toBeVisible()

  await openObject(page, 'ART-CHED-200')
  await page.getByTestId('add-link').first().focus()
  await page.keyboard.press('Enter')
  await page.getByTestId('link-search').fill('ART-CHED-400')
  await expect(page.getByRole('option').first()).toContainText('ART-CHED-400')
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('link-row').first()).toContainText('ART-CHED-400')

  await openObject(page, 'ART-CHED-400')
  await expect(page.getByTestId('link-row').first()).toContainText('ART-CHED-200')

  await command(page, 'Data model')
  await page.getByTestId('link-type-pairs_with').focus()
  await page.keyboard.press('Enter')
  await command(page, 'Delete link type')
  const dialog = page.getByTestId('consequence')
  await expect(dialog).toBeVisible()
  await page.getByTestId('confirm-name').fill('Pairs with')
  await page.getByTestId('confirm-delete').click()
  await expect(dialog).toBeHidden()
  await expect(page.getByTestId('link-type-pairs_with')).toHaveCount(0)
})
