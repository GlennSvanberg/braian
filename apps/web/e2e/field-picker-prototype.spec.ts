import { expect, test } from '@playwright/test'

/**
 * Field-picker prototype flows. Runs against the stack under scripts/e2e.mjs
 * (no sign-in: the prototype route is public mock UI).
 */
test.describe('field picker prototype', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/prototype/field-picker')
    await expect(page.getByTestId('field-picker-prototype')).toBeVisible()
    await expect(page.getByTestId('prototype-product-table')).toBeVisible()
  })

  test('open Fields mode from Columns and Ctrl+K', async ({ page }) => {
    await page.getByTestId('table-select-fields').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await expect(page.getByTestId('fields-crumb')).toContainText('Fields')
    await page.keyboard.press('Escape')
    await expect(page.getByTestId('fields-palette')).toHaveCount(0)

    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('fields-palette')).toBeVisible()
  })

  test('drill into Articles, qualify All + unit, add column', async ({ page }) => {
    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await page.getByTestId('fields-row-articles').click()
    await expect(page.getByTestId('fields-row-all')).toBeVisible()
    await page.getByTestId('fields-row-all').click()
    await expect(page.getByTestId('fields-row-net_volume')).toBeVisible()
    await page.getByTestId('fields-row-net_volume').click()
    await expect(page.getByTestId('fields-row-ml')).toBeVisible()
    await page.getByTestId('fields-row-ml').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › Net volume (all, ml)' })).toBeVisible()
    // palette stays open
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await page.keyboard.press('Escape')
  })

  test('Edit field and Remove column from header menu', async ({ page }) => {
    // Add a First column first
    await page.keyboard.press('Control+k')
    await page.getByTestId('fields-row-articles').click()
    await page.getByTestId('fields-row-first').click()
    await page.getByTestId('fields-row-gtin').click()
    await page.keyboard.press('Escape')
    const header = page.getByRole('columnheader', { name: 'Articles › GTIN (first)' })
    await expect(header).toBeVisible()
    await header.click({ button: 'right' })
    await expect(page.getByTestId('column-header-menu')).toBeVisible()
    await page.getByTestId('edit-field').click()
    await expect(page.getByTestId('fields-palette')).toBeVisible()
    await page.keyboard.press('Escape')

    await header.click({ button: 'right' })
    await page.getByTestId('remove-column').click()
    await expect(page.getByRole('columnheader', { name: 'Articles › GTIN (first)' })).toHaveCount(0)
  })
})
