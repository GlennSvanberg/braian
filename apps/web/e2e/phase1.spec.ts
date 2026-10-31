import { setTheme } from './ui'
import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/phase1'
mkdirSync(shots, { recursive: true })

async function signIn(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
}

test('phase 1 surfaces', async ({ page }) => {
  test.skip(!process.env.BRAIAN_PHASE1, 'set BRAIAN_PHASE1=1 against the demo server')
  test.setTimeout(120_000)
  await page.goto('/')
  await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible()
  await page.screenshot({ path: `${shots}/login.png` })
  await signIn(page)

  const workspaces = [
    ['Products', 'object-table'],
    ['Data model', 'data-model'],
    ['Review', 'proposal'],
  ] as const
  for (const [name, testId] of workspaces) {
    await page.getByRole('button', { name }).click()
    await expect(page.getByTestId(testId).first()).toBeVisible()
    await page.screenshot({ path: `${shots}/${name.toLowerCase().replaceAll(' ', '-')}-dark.png` })
  }

  await setTheme(page, 'light')
  await page.locator('nav.tabs').getByRole('button', { name: 'Products' }).click()
  await expect(page.getByTestId('object-table')).toBeVisible()
  await page.screenshot({ path: `${shots}/enrich-objects-light.png` })
  await setTheme(page, 'dark')

  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('command-palette')).toBeVisible()
  await page.screenshot({ path: `${shots}/command-palette.png` })
})
