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

/** Pre-#99 Data model: Model/tree, Attributes/classes, Preview/card. */
async function seedLegacyModelWorkspace(page: Page, name: string) {
  const workspaceId = crypto.randomUUID()
  const modelId = crypto.randomUUID()
  const attrsId = crypto.randomUUID()
  const previewId = crypto.randomUUID()
  const layout = {
    type: 'tabs',
    views: [modelId, attrsId, previewId],
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
  expect(created.ok(), `create workspace ${created.status()}`).toBeTruthy()
  const views = [
    { id: modelId, type: 'data_model', title: 'Model', pane: 'main', position: 0, state: { mode: 'tree' } },
    { id: attrsId, type: 'data_model', title: 'Attributes', pane: 'main', position: 1, state: { mode: 'classes' } },
    { id: previewId, type: 'data_model', title: 'Preview', pane: 'main', position: 2, state: { mode: 'card' } },
  ]
  for (const view of views) {
    const response = await page.request.post('/v1/views', {
      data: { ...view, workspace_id: workspaceId },
    })
    expect(response.ok(), `create view ${view.title} ${response.status()}`).toBeTruthy()
  }
  const listed = await page.request.get('/v1/workspaces')
  expect(listed.ok()).toBeTruthy()
  const body = (await listed.json()) as { workspaces: Array<{ id: string; name: string }> }
  expect(body.workspaces.some((row) => row.id === workspaceId)).toBe(true)
  return workspaceId
}

async function openWorkspace(page: Page, name: string) {
  await page.evaluate(() => sessionStorage.setItem('braian.layout.suppressFlush', '1'))
  await page.reload()
  await expect(page.getByTestId('dock')).toBeVisible()
  await page.evaluate(() => sessionStorage.removeItem('braian.layout.suppressFlush'))
  const tab = page.locator('nav.tabs').getByRole('button', { name: new RegExp(name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')) })
  await expect(tab).toBeVisible({ timeout: 30_000 })
  await tab.click()
}

test('legacy Data model workspace resets to the standard template once', async ({ page }) => {
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
  await expect(paneTabs.filter({ hasText: /^Model$/ })).toHaveCount(0)
  await expect(paneTabs.filter({ hasText: /^Preview$/ })).toHaveCount(0)
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
  await expect(paneTabs.filter({ hasText: 'Object types' })).toHaveCount(1)
  await expect(paneTabs.filter({ hasText: 'Structures' })).toHaveCount(1)
  await page.waitForTimeout(1500)
  expect(secondWrites, `unexpected writes on reload: ${secondWrites.join(', ')}`).toEqual([])
})
