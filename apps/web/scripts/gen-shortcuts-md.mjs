// Regenerate docs/shortcuts.md from apps/web/src/shortcuts.ts.
import { execFileSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const repoRoot = fileURLToPath(new URL('../../..', import.meta.url))
const pnpm = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'
execFileSync(
  pnpm,
  ['--dir', 'apps/web', 'exec', 'vitest', 'run', 'src/shortcuts-md.test.ts'],
  {
    cwd: repoRoot,
    stdio: 'inherit',
    env: { ...process.env, BRAIAN_WRITE_SHORTCUTS_MD: '1' },
  },
)
