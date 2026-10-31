import { setTheme } from './ui'
import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/history'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table').first()).toBeVisible()
  const filter = page.getByTestId('quick-filter').first()
  if ((await filter.inputValue()) !== '') await filter.fill('')
  await expect(page.getByTestId('object-row').first()).toBeVisible()
}

test('history is a compact timeline of the fields that changed', async ({ page }) => {
  await login(page)
  await page.getByTestId('object-row').first().click()
  await page.locator('[data-view-type="object_detail"]').first().click()
  const nummer = page.locator('[data-testid=object-detail] .field', { hasText: 'Number' }).locator('input')
  const original = await nummer.inputValue()
  const next = `${original}-h`
  await nummer.fill(next)
  await page.getByTestId('review-and-save').click()
  await expect(page.getByTestId('confirm-proposal')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText('Saved')

  await page.getByRole('button', { name: 'History' }).click()
  const history = page.getByTestId('history')
  const first = history.locator('article').first()
  await expect(first).toBeVisible()
  await expect(history.locator('.asof')).toHaveCount(0)
  await expect(history.getByTestId('as-of')).toHaveCount(0)

  const diff = first.getByTestId('field-diff')
  await expect(diff).toContainText('Number')
  await expect(diff).toContainText(original)
  await expect(diff).toContainText(next)
  await expect(diff.locator('.diff-old')).toHaveText(original)
  await expect(diff.locator('.diff-new')).toHaveText(next)
  await expect(diff).not.toContainText('nummer')
  await expect(first.locator('time')).not.toHaveText('')

  const created = history.locator('article', { hasText: 'Created by' }).first()
  await expect(created).toBeVisible()
  await expect(created.getByTestId('field-diff')).toHaveCount(0)
  await page.screenshot({ path: `${shots}/timeline-dark.png` })

  await first.click()
  await expect(first.getByTestId('view-as-of')).toBeVisible()
  await expect(first.getByTestId('rollback')).toBeVisible()
  await page.screenshot({ path: `${shots}/timeline-expanded-dark.png` })

  await first.getByTestId('view-as-of').click()
  await expect(history.getByTestId('as-of')).toBeVisible()
  await expect(history.locator('.asof')).toBeVisible()
  await page.screenshot({ path: `${shots}/time-travel-dark.png` })

  await setTheme(page, 'light')
  await history.getByTestId('time-travel').click()
  await expect(history.locator('.asof')).toHaveCount(0)
  await page.screenshot({ path: `${shots}/timeline-light.png` })
})
