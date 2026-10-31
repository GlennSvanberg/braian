import tailwindcss from '@tailwindcss/vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'
import { writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath, URL } from 'node:url'
import type { Plugin } from 'vite'
import { defineConfig } from 'vite'

const api = process.env.BRAIAN_API_PROXY ?? 'http://127.0.0.1:8090'

/** Same id is baked into the JS bundle and written to /version.json for polling. */
const braianBuildId =
  process.env.BRAIAN_BUILD_ID ??
  process.env.GITHUB_SHA ??
  // Prod autodeploy does not set the env vars today; a unique id per Vite
  // process still lets open tabs notice the next deploy.
  `build-${Date.now().toString(36)}`

function braianVersionJsonPlugin(buildId: string): Plugin {
  const body = `${JSON.stringify({ buildId })}\n`
  return {
    name: 'braian-version-json',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const pathname = req.url?.split('?')[0]
        if (pathname !== '/version.json') {
          next()
          return
        }
        res.setHeader('Content-Type', 'application/json; charset=utf-8')
        res.setHeader('Cache-Control', 'no-store')
        res.end(body)
      })
    },
    writeBundle(options) {
      if (!options.dir) return
      // TanStack Start emits client + server; only the client tree is static-served.
      if (options.dir.split(path.sep).includes('server')) return
      writeFileSync(path.join(options.dir, 'version.json'), body)
    },
  }
}

export default defineConfig({
  define: {
    __BRAIAN_BUILD__: JSON.stringify(braianBuildId),
  },
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 3000,
    strictPort: true,
    proxy: {
      '/v1': {
        target: api,
        changeOrigin: true,
        timeout: 0,
        proxyTimeout: 0,
        configure: (proxy) => {
          proxy.on('proxyRes', (proxyRes) => {
            const type = proxyRes.headers['content-type']
            if (typeof type === 'string' && type.includes('text/event-stream')) {
              proxyRes.headers['cache-control'] = 'no-cache, no-transform'
              delete proxyRes.headers['content-length']
            }
          })
        },
      },
      '/mcp': { target: api, changeOrigin: true, timeout: 0 },
      '/healthz': { target: api, changeOrigin: true },
    },
  },
  plugins: [tailwindcss(), tanstackStart(), viteReact(), braianVersionJsonPlugin(braianBuildId)],
})
