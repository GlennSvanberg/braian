import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'

const shots = '/opt/cursor/artifacts/screenshots'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  await page.goto('/')
  await page.getByTestId('email').fill('admin@braian.local')
  await page.getByTestId('password').fill('braian-demo')
  await page.getByRole('button', { name: 'Logga in' }).click()
  await expect(page.getByTestId('structure-tree')).toBeVisible()
  await expect(page.getByTestId('object-table')).toBeVisible()
}

test('login, edit, search, proposal, rollback and live update', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (error) => errors.push(error.message))

  await login(page)
  await expect(page.getByTestId('structure-tree')).toContainText('talt')
  await expect(page.getByTestId('object-row').first()).toBeVisible()
  await expect(page.getByTestId('object-table')).toContainText('Namn')
  const named = page.getByTestId('object-row').filter({ hasText: 'Regel' }).first()
  await (await named.count()) > 0 ? named.click() : page.getByTestId('object-row').first().click()
  await expect(page.getByTestId('object-detail').locator('input').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/workspace-dark.png` })

  await page.getByRole('button', { name: 'Ljust' }).click()
  await page.screenshot({ path: `${shots}/workspace-light.png` })
  await page.getByRole('button', { name: 'Mörkt' }).click()

  await expect(page.getByTestId('object-detail')).toBeVisible()
  const nummer = page.locator('[data-testid=object-detail] .field', { hasText: 'Nummer' }).locator('input')
  const original = await nummer.inputValue()
  await nummer.fill(`${original}-red`)
  await page.getByTestId('save-object').click()
  await expect(page.getByTestId('detail-error')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText('Sparat')

  await page.getByRole('button', { name: 'Nytt objekt' }).click()
  await page.getByTestId('new-nummer').fill('')
  await page.getByTestId('create-object').click()
  await expect(page.getByTestId('validation-error')).toBeVisible()

  const created = `ART-E2E-${Date.now().toString().slice(-6)}`
  await page.getByTestId('new-nummer').fill(created)
  await page.getByTestId('create-object').click()
  await expect(page.getByTestId('validation-error')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText(created)

  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('palette')).toBeVisible()
  await page.getByTestId('palette-input').fill('talt')
  await expect(page.getByTestId('palette').locator('.hit').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/command-palette.png` })
  await page.keyboard.press('Escape')
  await expect(page.getByTestId('palette')).toHaveCount(0)

  await page.getByRole('button', { name: 'Graf' }).click()
  await expect(page.locator('svg[data-testid=graph]')).toBeVisible()
  await page.screenshot({ path: `${shots}/graph.png` })

  await page.getByRole('button', { name: 'Historik' }).click()
  await expect(page.getByTestId('history').locator('article').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/history.png` })
  await page.getByTestId('rollback').click()

  await page.getByRole('button', { name: 'Granska' }).click()
  await expect(page.getByTestId('proposal')).toBeVisible()
  await expect(page.getByTestId('accept-all')).toBeVisible()
  await page.screenshot({ path: `${shots}/proposal.png` })
  await page.getByTestId('accept-all').click()
  await expect(page.getByTestId('activity-log')).toContainText('Godkänn alla')

  await page.getByRole('button', { name: 'Datamodell' }).click()
  await expect(page.getByTestId('data-model').first()).toBeVisible()
  await page.screenshot({ path: `${shots}/data-model.png` })

  await page.getByRole('button', { name: 'Berika objekt' }).click()
  await expect(page.getByTestId('activity-log')).toHaveAttribute('data-live', /open|message/, { timeout: 20_000 })
  const before = await page.getByTestId('activity-log').innerText()
  const response = await page.request.post('/v1/objects', {
    data: {
      type: 'artikel',
      groups: { sortiment: 'friluft' },
      values: { nummer: `ART-LIVE-${Date.now().toString().slice(-5)}`, namn: { sv: 'Live' }, status: 'utkast' },
    },
  })
  expect(response.ok()).toBeTruthy()
  await expect.poll(async () => page.getByTestId('activity-log').innerText(), { timeout: 10_000 }).not.toBe(before)
  await expect(page.getByTestId('activity-log')).toContainText('live')

  expect(errors, errors.join('\n')).toEqual([])
})
