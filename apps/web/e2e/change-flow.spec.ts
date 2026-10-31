import { setTheme } from './ui'
import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { demoCredentials } from './demo'

const shots = '/opt/cursor/artifacts/change-flow'
mkdirSync(shots, { recursive: true })

const STORAGE = [
  { code: 'ambient', label: 'Ambient' },
  { code: 'chilled', label: 'Chilled' },
  { code: 'frozen', label: 'Frozen' },
] as const

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
  // Hero articles sort first by number (ART-). Leave them alone so later
  // specs can open a known product. Synthetic articles are the fixture here.
  // Wait for the search that the filter starts. The table otherwise still
  // shows the unfiltered page, and a click lands on a different object.
  const search = page.waitForResponse(
    (response) => response.url().includes('/v1/search') && response.url().includes('SYN-') && response.ok(),
  )
  await filter.fill('SYN-')
  await search
  const row = table.getByTestId('object-row').filter({ hasText: /SYN-\d+/ }).first()
  await expect(row).toBeVisible()
  const shown = ((await row.innerText()).match(/SYN-\d+/) ?? [''])[0]
  expect(shown).not.toBe('')
  const objectId = ((await row.getAttribute('id')) ?? '').replace('object-row-', '')
  expect(objectId).not.toBe('')

  await row.click()
  await page.locator('[data-view-type="object_detail"]').first().click()
  const numberField = page
    .locator('[data-testid=object-detail] .field')
    .filter({ has: page.locator('label', { hasText: /^Number$/ }) })
    .locator('input')
  await expect(numberField).toHaveValue(shown)
  const original = await numberField.inputValue()
  await numberField.fill(`${original}-s`)
  await page.getByTestId('review-and-save').click()
  await expect(page.getByTestId('confirm-proposal')).toHaveCount(0)
  await expect(page.getByTestId('activity-log')).toContainText('Saved')

  const loaded = await page.request.get(`/v1/objects/${objectId}`)
  expect(loaded.ok()).toBeTruthy()
  const object = (await loaded.json()) as {
    key?: string
    type?: string
    version?: number
    values?: { number?: string; name?: Record<string, string>; storage?: string }
  }
  expect(object.values?.number).toBe(`${original}-s`)
  const name = object.values?.name ?? {}
  const storage = STORAGE.find((item) => item.code !== object.values?.storage) ?? STORAGE[0]
  const stamp = Date.now().toString().slice(-5)
  const english = `Agent cheese ${stamp}`
  const created = await page.request.post('/v1/proposals', {
    data: {
      title: 'Agent draft',
      rows: [
        {
          object_id: objectId,
          object_key: object.key ?? original,
          type_key: object.type ?? 'article',
          base_version: object.version ?? 0,
          patch: {
            values: {
              name: { en: english },
              storage: storage.code,
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
  const articles = page.locator('[data-testid=object-table]:visible').first()
  await expect(articles).toBeVisible()
  const marked = page.locator(`#object-row-${objectId}`)
  // The suffix is a new search token and the row can fall outside the virtual
  // window. The original number's tokens still identify this one article.
  const again = articles.getByTestId('quick-filter')
  await expect.poll(async () => {
    await again.fill(original)
    return marked.count()
  }, { timeout: 15_000 }).toBe(1)
  await expect(marked.getByTestId('suggestion-count')).toContainText(`${onObject} suggested`)

  await page.keyboard.press('Control+k')
  await expect(page.getByTestId('command-palette')).toContainText(paletteTitle)
  await page.keyboard.press('Escape')

  await page.locator('nav.tabs').getByRole('button', { name: /Review/ }).click()
  const review = page.getByTestId('proposal')
  await expect(review.getByTestId('source-badge')).toHaveText(badge)
  await expect(review).toContainText(english)
  await expect(review.getByTestId('lang-chip').filter({ hasText: 'en' }).first()).toBeVisible()
  await expect(review.getByText(storage.label, { exact: false }).first()).toBeVisible()
  await expect(review.locator('.diff-old').first()).toBeVisible()
  await expect(review.locator('.diff-new').first()).toBeVisible()

  const englishLine = review.locator('[data-line="name:en"]')
  await englishLine.getByTestId('reject-field').click()
  await expect(englishLine.getByTestId('reject-field')).toHaveAttribute('aria-pressed', 'true')
  await page.screenshot({ path: `${shots}/review-dark.png` })
  await setTheme(page, 'light')
  await page.screenshot({ path: `${shots}/review-light.png` })
  await setTheme(page, 'dark')

  await review.getByTestId('save-all').click()
  await expect(page.getByTestId('activity-log')).toContainText('Saved')

  const after = await page.request.get(`/v1/objects/${objectId}`)
  expect(after.ok()).toBeTruthy()
  const saved = (await after.json()) as { values?: { name?: Record<string, string>; storage?: string } }
  expect(saved.values?.name?.en ?? '').toBe(name.en ?? '')
  expect(saved.values?.name?.sv).toBeUndefined()
  expect(saved.values?.storage).toBe(storage.code)
})
