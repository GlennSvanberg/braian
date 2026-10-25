import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { createReadStream, existsSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath, URL } from 'node:url'
import { defineConfig, type Plugin } from 'vite'

const brandDir = fileURLToPath(new URL('../web/public/brand', import.meta.url))

function brand(): Plugin {
  return {
    name: 'brand-assets',
    configureServer(server) {
      server.middlewares.use('/brand', (req, res, next) => {
        const name = decodeURIComponent((req.url ?? '/').split('?')[0]).replace(/^\/+/, '')
        const file = join(brandDir, name)
        if (!name || name.includes('..') || !existsSync(file) || !statSync(file).isFile()) return next()
        res.setHeader('Content-Type', name.endsWith('.svg') ? 'image/svg+xml' : 'image/png')
        createReadStream(file).pipe(res)
      })
    },
  }
}

function entry(): Plugin {
  return {
    name: 'landing-entry',
    transformIndexHtml: {
      order: 'pre',
      handler(html, ctx) {
        const src = ctx.server ? '/src/main.tsx' : '/src/theme.ts'
        return html.replace('<!--entry-->', `<script type="module" src="${src}"></script>`)
      },
    },
  }
}

export default defineConfig({
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  plugins: [tailwindcss(), react(), brand(), entry()],
  server: { port: 4173, strictPort: true },
  preview: { port: 4173, strictPort: true },
})
