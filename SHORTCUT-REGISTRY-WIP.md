# Shortcut registry (Notion 107) — WIP parked 2026-10-31 ~12:58 Europe/Stockholm

## Done
- Installed `react-hotkeys-hook@5.3.3` (MIT; fits licenses allowlist).
- `apps/web/src/shortcuts.ts`: typed register (id, chord, scopes, description, commandId, hotkeys, rejectShift, layoutOnly).
- `shortcuts.test.ts`: no-conflict on real list; fails on overlapping-scope chord clash; same chord OK in non-overlapping scopes; `palette:search` owns Ctrl+K; `layout:add-view` has no chord.
- `shortcut-bindings.tsx`: binds existing app/layout shortcuts via `useHotkeys` (capture), with `#132` `isTextEditingTarget` guard and Alt+arrow `rejectShift`.
- Shell `keydown` listener replaced with `useShortcutBindings`.
- `command-registry.ts` / `palette.tsx`: kbd from `chordForCommand`; **Ctrl+K collision fixed** — `layout:add-view` has no shortcut (open via palette/dock).
- Unit: shortcuts + shortcut-bindings (jsdom) green locally (~12:57 Stockholm).
- Spec stub `e2e/shortcut-registry.spec.ts` registered in `scripts/e2e.mjs` (Ctrl+K shows Alt+← kbd; Alt+Shift smoke) — **not run / not verified green yet**.
- `tsc --noEmit` was started but not confirmed finished before park.

## Not done (resume later)
- Confirm tsc clean; fix any type/hook issues.
- Local e2e for shortcut-registry (+ keep #132 cell-edit e2e green with registry).
- **PR 2 scope still open:** Ctrl+Shift+←/→ column move (explicit Control, not meta); F1 Help + `docs/shortcuts.md` generated from registry + sync script/CI; e2e for column move / not-while-editing / Alt+Shift≠Alt focus more thoroughly.
- Open PR(s) after unpark — **no PR opened while parked**.
