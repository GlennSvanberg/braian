import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  // scripts/e2e.mjs owns parallelism: one database, API and Vite per process.
  // Raising workers here would share that database across browsers.
  workers: 1,
  use: {
    baseURL: process.env.BRAIAN_WEB_URL ?? 'http://127.0.0.1:3000',
    viewport: { width: 1440, height: 900 },
    locale: 'sv-SE',
  },
})
