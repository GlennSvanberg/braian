import tailwindcss from '@tailwindcss/vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'
import { fileURLToPath, URL } from 'node:url'
import { defineConfig } from 'vite'

const api = process.env.BRAIAN_API_PROXY ?? 'http://127.0.0.1:8090'

export default defineConfig({
  define: {
    __BRAIAN_BUILD__: JSON.stringify(process.env.BRAIAN_BUILD_ID ?? process.env.GITHUB_SHA ?? 'dev'),
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
  plugins: [tailwindcss(), tanstackStart(), viteReact()],
})
