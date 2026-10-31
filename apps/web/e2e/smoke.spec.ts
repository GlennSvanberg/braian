import { setTheme } from './ui'
import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/screenshots'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('structure-tree')).toBeVisible()
  await expect(page.getByTestId('object-table')).toBeVisible()
}

test('login, edit, search, proposal, rollback and live update', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))

  await login(page)
  await expect(page.getByTestId('structure-tree')).toContainText('Cheese')
  const filter = page.getByTestId('quick-filter').first()
  if ((await filter.inputValue()) !== '') await filter.fill('')
  await expect(page.getByTestId('object-row').first()).toBeVisible()
  await expect(page.getByTestId('object-table')).toContainText('Name')
  const named = page.getByTestId('object-row').filter({ hasText: 'Cheddar' }).first()
  if ((await named.count()) > 0) await named.click()
  else await page.getByTestId('object-row').first().click()
  await page.locator('[data-view-type="object_detail"]').first().click()
  await expect(page.getByTestId('object-detail').locator('input').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/workspace-dark.png` })

  await setTheme(page, 'light')
  await page.screenshot({ path: `${shots}/workspace-light.png` })
  await setTheme(page, 'dark')

  await expect(page.getByTestId('object-detail')).toBeVisible()
  const nummer = page.locator('[data-testid=object-detail] .field', { hasText: 'Number' }).locator('input')
  const original = await nummer.inputValue()
  await nummer.fill(`${original}-red`)
  await page.getByTestId('review-and-save').click()
  await expect(page.getByTestId('confirm-proposal')).toHaveCount(0)
  await expect(page.getByTestId('detail-error')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText('Saved')

  await page.getByTestId('table-add-row').click()
  await page.getByTestId('new-number').fill('')
  await page.getByTestId('create-object').click()
  await expect(page.getByTestId('validation-error')).toBeVisible()

  const created = `ART-E2E-${Date.now().toString().slice(-6)}`
  await page.getByTestId('new-number').fill(created)
  await page.getByTestId('new-name').fill(created)
  await page.getByTestId('create-object').click()
  await expect(page.getByTestId('validation-error')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText(created)

  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('command-palette')).toBeVisible()
  await page.getByTestId('palette-input').fill('cheese')
  const hits = page.getByTestId('command-palette').locator('.hit')
  await expect(hits.filter({ hasText: '·' }).first()).toBeVisible()
  await page.keyboard.press('ArrowDown')
  await expect(hits.first()).toHaveClass(/on/)
  await page.keyboard.press('ArrowDown')
  await expect(hits.nth(1)).toHaveClass(/on/)
  await page.keyboard.press('ArrowUp')
  await expect(hits.first()).toHaveClass(/on/)
  await page.screenshot({ path: `${shots}/command-palette.png` })
  await page.keyboard.press('Escape')
  await expect(page.getByTestId('command-palette')).toHaveCount(0)

  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas[data-testid=graph]')).toBeVisible()
  await page.screenshot({ path: `${shots}/graph.png` })

  await page.getByRole('button', { name: 'History' }).click()
  const entry = page.getByTestId('history').locator('article').first()
  await expect(entry).toBeVisible()
  await expect(entry).toContainText('Created by')
  await page.screenshot({ path: `${shots}/history.png` })
  await entry.click()
  await page.getByTestId('rollback').click()

  // Restore opens the consequence review dialog (publishProposal), then Confirm applies it.
  await expect(page.getByTestId('consequence')).toBeVisible()
  await expect(page.getByTestId('activity-log')).toContainText('Proposed')
  await page.screenshot({ path: `${shots}/proposal.png` })
  await page.getByTestId('confirm-model').click()
  await expect(page.getByTestId('consequence')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText(/Confirm|Saved/)

  await page.getByRole('button', { name: 'Data model' }).click()
  await expect(page.getByTestId('data-model').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/data-model.png` })

  await page.locator('nav.tabs').getByRole('button', { name: /^Products/ }).click()
  await expect(page.getByTestId('activity-log')).toHaveAttribute('data-live', /open|message/, { timeout: 20_000 })
  const before = await page.getByTestId('activity-log').innerText()
  const response = await page.request.post('/v1/objects', {
    data: {
      type: 'product',
      groups: { food: 'bread' },
      values: { number: `ART-LIVE-${Date.now().toString().slice(-5)}`, name: { en: 'Live' }, status: 'draft' },
    },
  })
  expect(response.ok()).toBeTruthy()
  await expect.poll(async () => page.getByTestId('activity-log').innerText(), { timeout: 10_000 }).not.toBe(before)
  await expect(page.getByTestId('activity-log')).toContainText('live')

  expect(errors, errors.join('\n')).toEqual([])
})
