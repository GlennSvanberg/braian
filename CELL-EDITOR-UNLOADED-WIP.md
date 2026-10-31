# Cell editor unloaded value (Notion 111) — WIP parked 2026-10-31 Europe/Stockholm

## Done
- `resolveCellEditInitial` in `cell-edit.ts`: block path-values columns until batch has the cell; seed from path-values/query data (not painted "…").
- Unit tests for the helper.
- `beginCellEdit` in `panes.tsx` wired for both grid layouts.
- e2e stub `e2e/cell-editor-unloaded-value.spec.ts` + registered in `scripts/e2e.mjs` (delayed pathValues via route; block while loading; seed after; unchanged Enter skips PATCH).
- Pattern choice: **block open until loaded** (safest vs empty overwrite) + init from path-values cache when ready.

## Not done
- `pnpm install` done; vitest/tsc/e2e not confirmed green (paused for #136 membership fix).
- No PR opened while parked.
