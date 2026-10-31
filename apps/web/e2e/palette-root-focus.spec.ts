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

async function openProducts(page: Page) {
  await page.locator('nav.tabs').getByRole('button', { name: /^Products/ }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
}

test.describe('palette root and first-key focus', () => {
  test('Ctrl+K after Select fields › returns to the root list, not Fields ›', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('command-palette')).toBeVisible()
    await page.getByTestId('palette-input').fill('Select fields')
    await page.keyboard.press('Enter')
    await expect(page.getByTestId('fields-mode')).toBeVisible()
    await expect(page.getByTestId('command-palette')).toHaveAttribute('data-fields-mode', 'true')

    // Ctrl+K must leave Fields › and open the root command list.
    await page.keyboard.press('Control+k')
    await expect(page.getByTestId('palette-input')).toBeVisible()
    await expect(page.getByTestId('fields-mode')).toHaveCount(0)
    await expect(page.getByTestId('command-palette')).not.toHaveAttribute('data-fields-mode', 'true')

    await page.getByTestId('palette-input').fill('Move column')
    const move = page.getByTestId('command-palette').getByRole('option').filter({ hasText: /Move column/i }).first()
    await expect(move).toBeVisible()
    await page.keyboard.press('Enter')
    // Root Enter must do something other than leave Fields stuck open.
    await expect(page.getByTestId('fields-mode')).toHaveCount(0)
  })

  test('first character after Ctrl+K is not dropped', async ({ page }) => {
    await login(page)
    await openProducts(page)
    await expect(page.getByText('Mature cheddar')).toBeVisible({ timeout: 60_000 })

    // Deterministic: the Ctrl+K handler must leave #palette-input focused in the
    // same turn (flushSync) via the document-capture hotkey listener. Without that, this evaluate sees no input yet and
    // Playwright's later type() can still win the race — which is why a plain
    // press→type loop is only sporadically red under automation.
    for (let round = 0; round < 8; round += 1) {
      await page.keyboard.press('Escape')
      await expect(page.getByTestId('command-palette')).toHaveCount(0)
      await page.getByTestId('object-table').focus()

      const sync = await page.evaluate(() => {
        const event = new KeyboardEvent('keydown', {
          key: 'k',
          code: 'KeyK',
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        })
        document.dispatchEvent(event)
        const input = document.querySelector<HTMLInputElement>('[data-testid="palette-input"]')
        if (!input) return { focused: false, hasInput: false }
        return {
          focused: document.activeElement === input,
          hasInput: true,
        }
      })
      expect(sync.hasInput, `round ${round}: palette input missing after Ctrl+K (no flushSync)`).toBe(true)
      expect(sync.focused, `round ${round}: palette input not focused synchronously`).toBe(true)

      const input = page.getByTestId('palette-input')
      await expect(input).toBeVisible()
      // No wait between sync-focus proof and typing — first glyph must land.
      await page.keyboard.type('Move column left', { delay: 0 })
      await expect(input).toHaveValue('Move column left')
      await page.keyboard.press('Escape')
    }
  })
})
