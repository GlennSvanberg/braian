import { expect, type Page } from '@playwright/test'

/** Switch the theme through the user menu and wait until `<html data-theme>` follows. */
export async function setTheme(page: Page, theme: 'light' | 'dark') {
  const html = page.locator('html')
  if ((await html.getAttribute('data-theme')) !== theme) {
    await page.getByTestId('user-menu').click()
    await page.getByTestId('user-menu-theme').click()
  }
  await expect(html).toHaveAttribute('data-theme', theme)
}
