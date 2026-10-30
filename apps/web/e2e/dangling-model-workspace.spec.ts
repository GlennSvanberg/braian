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

/**
 * Reconstructs the demo "Data model 8" failure: layout references view ids that
 * were never persisted (create failed after layout write), so tabs paint as UUIDs.
 */
async function seedDanglingModelWorkspace(page: Page, name: string) {
  const workspaceId = crypto.randomUUID()
  const structuresId = crypto.randomUUID()
  const ghostLeft = crypto.randomUUID()
  const ghostMid = crypto.randomUUID()
  const ghostRight = crypto.randomUUID()
  const layout = {
    type: 'split',
    dir: 'horizontal',
    ratios: [0.22, 0.78],
    children: [
      { type: 'tabs', views: [structuresId, ghostLeft], active: 0 },
      {
        type: 'split',
        dir: 'horizontal',
        ratios: [0.55, 0.45],
        children: [
          { type: 'tabs', views: [ghostMid], active: 0 },
          { type: 'tabs', views: [ghostRight], active: 0 },
        ],
      },
    ],
  }
  const created = await page.request.post('/v1/workspaces', {
    data: {
      id: workspaceId,
      name,
      locked: false,
      share_token: `dangling-model-${workspaceId.slice(0, 8)}`,
      layout,
    },
  })
  expect(created.ok(), `create workspace ${created.status()}`).toBeTruthy()
  const structures = await page.request.post('/v1/views', {
    data: {
      id: structuresId,
      workspace_id: workspaceId,
      type: 'data_model',
      title: 'Structures',
      pane: 'main',
      position: 0,
      state: { mode: 'tree' },
    },
  })
  expect(structures.ok(), `create Structures ${structures.status()}`).toBeTruthy()
  // Intentionally do not create ghostLeft/Mid/Right — that is the bug.
  return { workspaceId, ghostLeft, ghostMid, ghostRight }
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

test('dangling Data model panes heal to the standard template without UUID tabs', async ({ page }) => {
  test.setTimeout(180_000)
  await login(page)
  const name = `Data model dangling ${Date.now()}`
  const { ghostLeft, ghostMid, ghostRight } = await seedDanglingModelWorkspace(page, name)

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
  for (const ghost of [ghostLeft, ghostMid, ghostRight]) {
    await expect(paneTabs.filter({ hasText: ghost })).toHaveCount(0)
  }
  await expect(paneTabs.filter({ hasText: /[0-9a-f]{8}-[0-9a-f]{4}-/i })).toHaveCount(0)
  await expect.poll(() => firstWrites.length).toBeGreaterThan(0)
  page.off('request', onFirst)

  const secondWrites: string[] = []
  const onSecond = (request: Request) => {
    if (isWorkspaceWrite(request)) secondWrites.push(`${request.method()} ${new URL(request.url()).pathname}`)
  }
  page.on('request', onSecond)
  await openWorkspace(page, name)
  await expect(paneTabs.filter({ hasText: 'Structures' })).toBeVisible()
  await expect(paneTabs.filter({ hasText: 'Object types' })).toHaveCount(1)
  await page.waitForTimeout(1500)
  expect(secondWrites, `unexpected writes on reload: ${secondWrites.join(', ')}`).toEqual([])
})
