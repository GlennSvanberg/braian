import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/change-flow'
mkdirSync(shots, { recursive: true })

async function login(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('braian.theme', JSON.stringify('dark'))
  })
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('object-table').first()).toBeVisible()
}

test('a person saves directly and reviews an agent draft field by field', async ({ page }) => {
  test.setTimeout(120_000)
  await login(page)
  await page.locator('nav.tabs').getByRole('button', { name: /^Articles/ }).click()
  const table = page.locator('[data-testid=object-table]:visible').first()
  const filter = table.getByTestId('quick-filter').first()
  if ((await filter.inputValue()) !== '') await filter.fill('')
  const row = table.getByTestId('object-row').first()
  await expect(row).toBeVisible()
  const rowId = await row.getAttribute('id')
  const objectId = rowId?.replace('object-row-', '') ?? ''
  expect(objectId).not.toBe('')

  await row.click()
  await page.locator('[data-view-type="object_detail"]').first().click()
  const nummer = page.locator('[data-testid=object-detail] .field', { hasText: 'Number' }).locator('input')
  const original = await nummer.inputValue()
  await nummer.fill(`${original}-s`)
  await page.getByTestId('save-object').click()
  await expect(page.getByTestId('confirm-proposal')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText('Saved')

  const loaded = await page.request.get(`/v1/objects/${objectId}`)
  expect(loaded.ok()).toBeTruthy()
  const object = (await loaded.json()) as {
    key?: string
    type?: string
    version?: number
    values?: { namn?: Record<string, string>; farg?: string[] }
  }
  const namn = object.values?.namn ?? {}
  const colours = object.values?.farg ?? []
  const extra = (['bla', 'gra', 'svart', 'gron'] as const).find((code) => !colours.includes(code)) ?? 'bla'
  const colourLabel = { bla: 'Blue', gra: 'Grey', svart: 'Black', gron: 'Green' }[extra]
  const stamp = Date.now().toString().slice(-5)
  const swedish = `Agenttält ${stamp}`
  const english = `Agent tent ${stamp}`
  const created = await page.request.post('/v1/proposals', {
    data: {
      title: 'Agent draft',
      rows: [
        {
          object_id: objectId,
          object_key: object.key ?? original,
          type_key: object.type ?? 'produkt',
          base_version: object.version ?? 0,
          patch: {
            values: {
              namn: { sv: swedish, en: english },
              farg: [...colours, extra],
            },
          },
        },
      ],
    },
  })
  expect(created.ok()).toBeTruthy()
  const proposal = (await created.json()) as { actor_name?: string; behalf_name?: string }
  // The signed-in person files this draft, so the badge is their name.
  // `source` is the principal, not a field on the request.
  const badge = proposal.behalf_name || proposal.actor_name || 'user'

  const listed = await page.request.get('/v1/proposals')
  expect(listed.ok()).toBeTruthy()
  const pending = (await listed.json()) as { pending?: { total?: number; objects?: Array<{ object_id: string; count: number }> } }
  const total = Number(pending.pending?.total ?? 0)
  const onObject = pending.pending?.objects?.find((item) => item.object_id === objectId)?.count ?? 0
  const paletteTitle = total === 1 ? '1 suggested change' : `${total} suggested changes`

  await page.reload()
  await page.locator('nav.tabs').getByRole('button', { name: /^Articles/ }).click()
  await expect(page.locator('[data-testid=object-table]:visible').first()).toBeVisible()
  const marked = page.locator(`#object-row-${objectId}`)
  await expect.poll(async () => {
    await filter.fill(`${original}-s`)
    return marked.count()
  }, { timeout: 15_000 }).toBe(1)
  await expect(marked.getByTestId('suggestion-count')).toContainText(`${onObject} suggested`)

  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('palette')).toContainText(paletteTitle)
  await page.keyboard.press('Escape')

  await page.locator('nav.tabs').getByRole('button', { name: /Review/ }).click()
  const review = page.getByTestId('proposal')
  await expect(review.getByTestId('source-badge')).toHaveText(badge)
  await expect(review).toContainText(swedish)
  await expect(review.getByTestId('lang-chip').filter({ hasText: 'sv' }).first()).toBeVisible()
  await expect(review.getByTestId('lang-chip').filter({ hasText: 'en' }).first()).toBeVisible()
  await expect(review.getByText(colourLabel, { exact: false }).first()).toBeVisible()
  await expect(review.locator('.diff-old').first()).toBeVisible()
  await expect(review.locator('.diff-new').first()).toBeVisible()

  const englishLine = review.locator('[data-line="namn:en"]')
  await englishLine.getByTestId('reject-field').click()
  await expect(englishLine.getByTestId('reject-field')).toHaveAttribute('aria-pressed', 'true')
  await page.screenshot({ path: `${shots}/review-dark.png` })
  await page.getByRole('button', { name: 'Light' }).click()
  await page.screenshot({ path: `${shots}/review-light.png` })
  await page.getByRole('button', { name: 'Dark' }).click()

  await review.getByTestId('save-all').click()
  await expect(page.getByTestId('activity-log')).toContainText('Saved')

  const after = await page.request.get(`/v1/objects/${objectId}`)
  expect(after.ok()).toBeTruthy()
  const saved = (await after.json()) as { values?: { namn?: Record<string, string>; farg?: string[] } }
  expect(saved.values?.namn?.sv).toBe(swedish)
  expect(saved.values?.namn?.en ?? '').toBe(namn.en ?? '')
  expect(saved.values?.farg ?? []).toEqual(expect.arrayContaining([...colours, extra]))
})
