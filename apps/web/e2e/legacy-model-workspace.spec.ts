import { expect, test, type Page, type Request } from '@playwright/test'
import { demoCredentials } from './demo'

async function login(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('braian.theme', JSON.stringify('dark'))
  })
  const demo = demoCredentials()
  await page.goto('/')
  await page.getByTestId('email').fill(demo.email)
  await page.getByTestId('password').fill(demo.password)
  await page.getByRole('button', { name: 'Sign in' }).click()
  await expect(page.getByTestId('dock')).toBeVisible()
}

function isWorkspaceWrite(request: Request) {
  if (!['POST', 'PUT', 'PATCH', 'DELETE'].includes(request.method())) return false
  return /\/v1\/(workspaces|views)(\/|$|\?)/.test(request.url())
}

/** Pre-#99 Data model: Model/tree, Attributes/classes, Preview/card + a user embed. */
async function seedLegacyModelWorkspace(page: Page, name: string) {
  const workspaceId = crypto.randomUUID()
  const modelId = crypto.randomUUID()
  const attrsId = crypto.randomUUID()
  const previewId = crypto.randomUUID()
  const notesId = crypto.randomUUID()
  const layout = {
    type: 'tabs',
    views: [modelId, attrsId, previewId, notesId],
    active: 0,
  }
  const created = await page.request.post('/v1/workspaces', {
    data: {
      id: workspaceId,
      name,
      locked: false,
      share_token: `legacy-model-${workspaceId.slice(0, 8)}`,
      layout,
    },
  })
  expect(created.ok()).toBeTruthy()
  const views = [
    { id: modelId, type: 'data_model', title: 'Model', pane: 'main', position: 0, state: { mode: 'tree' } },
    { id: attrsId, type: 'data_model', title: 'Attributes', pane: 'main', position: 1, state: { mode: 'classes' } },
    { id: previewId, type: 'data_model', title: 'Preview', pane: 'main', position: 2, state: { mode: 'card' } },
    { id: notesId, type: 'embed', title: 'My notes', pane: 'main', position: 3, state: { url: 'https://example.com/{object}' } },
  ]
  for (const view of views) {
    const response = await page.request.post('/v1/views', {
      data: { ...view, workspace_id: workspaceId },
    })
    expect(response.ok()).toBeTruthy()
  }
  return { workspaceId, notesId }
}

async function openWorkspace(page: Page, name: string) {
  await page.evaluate(() => sessionStorage.setItem('braian.layout.suppressFlush', '1'))
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await page.evaluate(() => sessionStorage.removeItem('braian.layout.suppressFlush'))
  await page.getByTestId('dock').getByRole('button', { name }).click()
}

test('legacy Data model workspace migrates once and keeps custom panes', async ({ page }) => {
  test.setTimeout(180_000)
  await login(page)
  const name = `Legacy model ${Date.now()}`
  await seedLegacyModelWorkspace(page, name)

  const firstWrites: string[] = []
  const onFirst = (request: Request) => {
    if (isWorkspaceWrite(request)) firstWrites.push(`${request.method()} ${new URL(request.url()).pathname}`)
  }
  page.on('request', onFirst)
  await openWorkspace(page, name)

  const paneTabs = page.locator('.pane-tab')
  await expect(paneTabs.filter({ hasText: 'Structures' })).toBeVisible({ timeout: 30_000 })
  await expect(paneTabs.filter({ hasText: 'Object types' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Link types' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Value lists' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Units' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Type attributes' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'My notes' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Model' })).toHaveCount(0)
  await expect(paneTabs.filter({ hasText: 'Preview' })).toHaveCount(0)
  // Migration must persist the rewritten views/layout.
  await expect.poll(() => firstWrites.length).toBeGreaterThan(0)
  page.off('request', onFirst)

  const secondWrites: string[] = []
  const onSecond = (request: Request) => {
    if (isWorkspaceWrite(request)) secondWrites.push(`${request.method()} ${new URL(request.url()).pathname}`)
  }
  page.on('request', onSecond)
  await openWorkspace(page, name)
  await expect(paneTabs.filter({ hasText: 'Structures' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Object types' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'My notes' })).toBeVisible()
  // No duplicated standard tables after reload.
  await expect(paneTabs.filter({ hasText: 'Object types' })).toHaveCount(1)
  await expect(paneTabs.filter({ hasText: 'Structures' })).toHaveCount(1)
  await expect(paneTabs.filter({ hasText: 'My notes' })).toHaveCount(1)
  // Already modern: migration is a no-op and must not write.
  await page.waitForTimeout(1500)
  expect(secondWrites, `unexpected writes on reload: ${secondWrites.join(', ')}`).toEqual([])
})
