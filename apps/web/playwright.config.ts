import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: process.env.BRAIAN_WEB_URL ?? 'http://127.0.0.1:3000',
    viewport: { width: 1440, height: 900 },
    locale: 'sv-SE',
  },
})
