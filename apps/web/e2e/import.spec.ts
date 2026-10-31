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
  await expect(page.getByTestId('import-canvas')).toHaveCount(0)
  await expect(page.getByTestId('import-banner')).not.toContainText('marked')
  await expect(page.getByTestId('import-banner')).toContainText('needs a look')
  await expect(page.getByTestId('import-col-1')).toHaveClass(/low/)
  await expect(page.getByTestId('import-steps')).toBeVisible()
  await page.screenshot({ path: `${shots}/import-mapping-desktop.png` })
  await page.setViewportSize({ width: 420, height: 800 })
  await page.screenshot({ path: `${shots}/import-mapping-narrow.png` })
  await page.setViewportSize({ width: 1440, height: 900 })

  await page.getByTestId('import-target-0').click()
  await expect(page.getByTestId('import-field-mode')).toBeVisible()
  await page.getByTestId('import-field-row-unmapped').click()
  await expect(page.locator('.import-aria')).not.toContainText('Column Number maps to')
  await page.keyboard.press('Control+z')
  await expect(page.locator('.import-aria')).toContainText('Column Number maps to Article number')

  await openPreview(page, 'diff')
  await expect(page.getByTestId('import-row-new')).toBeVisible()
  await expect(page.getByTestId('import-row-changed').locator('.changed')).toBeVisible()
  await expect(page.getByTestId('import-row-failed').locator('.error')).toBeVisible()
  await expect(page.getByTestId('import-save')).toContainText('Approve')
  await expect(page.getByTestId('import-save')).toBeDisabled()
  await page.screenshot({ path: `${shots}/import-preview.png` })
})
