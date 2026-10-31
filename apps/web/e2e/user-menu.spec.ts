import { expect, test, type Page } from '@playwright/test'
import { demoCredentials } from './demo'
import { setTheme } from './ui'

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('user-menu')).toBeVisible()
  await expect(page.getByTestId('workspace')).toBeVisible()
}

async function runPaletteCommand(page: Page, query: string, title: string) {
  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('command-palette')).toBeVisible()
  await page.getByTestId('palette-input').fill(query)
  await page.getByTestId('command-palette').locator('.hit', { hasText: title }).first().click()
}

test('the header only holds the avatar, the menu holds the account info', async ({ page }) => {
  const demo = demoCredentials()
  await login(page)
  const header = page.locator('header.app-bar')
  await expect(header.getByRole('button', { name: 'Light' })).toHaveCount(0)
  await expect(header.getByRole('button', { name: 'Agent' })).toHaveCount(0)
  await expect(header.getByRole('button', { name: 'Sign out' })).toHaveCount(0)
  await expect(header).not.toContainText(demo.email)

  await page.getByTestId('user-menu').click()
  await expect(page.getByTestId('user-menu-email')).toHaveText(demo.email)
  await expect(page.getByTestId('user-menu-role')).not.toHaveText('')
  await expect(page.getByTestId('user-menu-status')).toHaveText('Connected')
  await page.keyboard.press('Escape')
  await expect(page.getByTestId('user-menu-content')).toHaveCount(0)
  await expect(page.getByTestId('user-menu')).toBeFocused()
})

test('the menu works from the keyboard and toggles the theme', async ({ page }) => {
  await login(page)
  await setTheme(page, 'dark')
  await page.getByTestId('user-menu').focus()
  await page.keyboard.press('Enter')
  await expect(page.getByTestId('user-menu-content')).toBeVisible()
  await expect(page.getByTestId('user-menu-theme')).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light')
  await page.getByTestId('user-menu').click()
  await expect(page.getByTestId('user-menu-theme')).toHaveText('Dark mode')
  await page.getByTestId('user-menu-theme').click()
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark')
})

test('Ctrl+K toggles the theme', async ({ page }) => {
  await login(page)
  await setTheme(page, 'dark')
  await runPaletteCommand(page, 'theme', 'Light mode')
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'light')
  await setTheme(page, 'dark')
})

test('Ctrl+K Open agent opens the agent panel', async ({ page }) => {
  await login(page)
  await runPaletteCommand(page, 'open agent', 'Open agent')
  await expect(page.getByTestId('ai-badge')).toBeVisible()
  await expect(page.getByTestId('palette-input')).toBeFocused()
})

async function setBrowserOnline(page: Page, online: boolean) {
  await page.evaluate((next) => {
    window.dispatchEvent(new Event(next ? 'online' : 'offline'))
  }, online)
}

test('the status bar is empty when normal and shows offline while disconnected', async ({ page, context }) => {
  await login(page)
  await expect(page.getByTestId('activity-log')).toHaveAttribute('data-live', /open|message/, { timeout: 20_000 })
  const bar = page.getByTestId('status-bar')
  const bottom = page.locator('.bottom-bar')
  await expect(bar).toHaveAttribute('data-kind', '')
  await expect(bar).toHaveText('')
  await expect(bottom).not.toContainText(/connected|admin/i)

  await context.setOffline(true)
  await page.waitForFunction(() => !navigator.onLine)
  await setBrowserOnline(page, false)
  await expect(page.getByTestId('workspace')).toBeVisible()
  await expect(bar).toHaveAttribute('data-kind', 'offline', { timeout: 20_000 })
  await expect(bar).toContainText(/Offline.*reconnecting/i)

  await context.setOffline(false)
  await page.waitForFunction(() => navigator.onLine)
  await setBrowserOnline(page, true)
  await expect(page.getByTestId('activity-log')).toHaveAttribute('data-live', /open|message/, { timeout: 20_000 })
  await expect(bar).toHaveAttribute('data-kind', '', { timeout: 20_000 })
  await expect(bar).toHaveText('')
})

test('Ctrl+K Sign out returns to the login form', async ({ page }) => {
  await login(page)
  const loggedOut = page.waitForResponse(
    (res) => res.url().includes('/v1/logout') && res.request().method() === 'POST' && res.ok(),
  )
  await runPaletteCommand(page, 'sign out', 'Sign out')
  await loggedOut
  await expect(page.getByTestId('email')).toBeVisible({ timeout: 20_000 })
})
