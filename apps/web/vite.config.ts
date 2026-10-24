import { defineConfig } from 'vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'

const api = process.env.BRAIAN_API_PROXY ?? 'http://127.0.0.1:8090'

export default defineConfig({
  server: {
    port: 3000,
    strictPort: true,
    proxy: {
      '/v1': { target: api, changeOrigin: true, timeout: 0 },
      '/mcp': { target: api, changeOrigin: true, timeout: 0 },
      '/healthz': { target: api, changeOrigin: true },
    },
  },
  plugins: [tanstackStart(), viteReact()],
})
