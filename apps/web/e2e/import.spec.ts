import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'

const shots = '/opt/cursor/artifacts/screenshots'
mkdirSync(shots, { recursive: true })

async function openPreview(page: Page, kind: 'map' | 'diff') {
  await page.goto(`/?importPreview=${kind}`)
  await expect(page.getByTestId(kind === 'map' ? 'import-mapping' : 'import-preview')).toBeVisible()
}

test('mapping lines can change and the preview paints the diff', async ({ page }) => {
  await openPreview(page, 'map')
  await expect(page.getByTestId('import-canvas')).toBeVisible()
  await expect(page.getByTestId('import-col-1')).toHaveClass(/warn/)
  await page.getByTestId('import-col-0').click()
  await expect(page.getByTestId('import-popover')).toBeVisible()
  await page.getByRole('button', { name: 'Remove line' }).click()
  await expect(page.locator('.import-aria')).not.toContainText('maps to SKU')
  await page.keyboard.press('Control+z')
  await expect(page.locator('.import-aria')).toContainText('Column Artnr maps to SKU')
  await page.screenshot({ path: `${shots}/import-mapping.png` })

  await openPreview(page, 'diff')
  await expect(page.getByTestId('import-row-new')).toBeVisible()
  await expect(page.getByTestId('import-row-changed').locator('.changed')).toBeVisible()
  await expect(page.getByTestId('import-row-failed').locator('.error')).toBeVisible()
  await expect(page.getByTestId('import-row-delete')).toBeVisible()
  await expect(page.getByTestId('import-save')).toContainText('Import the rest')
  await page.screenshot({ path: `${shots}/import-preview.png` })
})
