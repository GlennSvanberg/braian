#!/usr/bin/env bash
# Table-driven check for scripts/ci-changes.sh.
# A Rust path must never come out as mode=web.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
classify() {
  printf '%s\n' "$@" | bash "$root/scripts/ci-changes.sh"
}

fail=0
check() {
  local name="$1"
  local want="$2"
  shift 2
  local got
  got="$(classify "$@")"
  if [ "$got" = "mode=$want" ]; then
    printf 'ok  %s -> %s\n' "$name" "$want"
  else
    printf 'FAIL %s: want mode=%s got %s\n' "$name" "$want" "$got" >&2
    fail=1
  fi
}

# web-only: stack still runs e2e, and the Rust jobs do no compile.
check web-app web apps/web/src/shell.tsx
check packages web packages/ui/package.json
check pnpm-lock web pnpm-lock.yaml
check pnpm-workspace web pnpm-workspace.yaml
check root-package web package.json
check e2e-runner web scripts/e2e.mjs
check web-and-docs web apps/web/src/main.tsx docs/dev.md

# docs and landing do no heavy CI.
check readme skip README.md
check docs skip docs/dev.md
check landing skip apps/landing/index.html
check empty skip

# Rust, manifests, workflows, and the published API are full.
# Mixing any of these with a web path stays full.
check crate full crates/store/src/lib.rs
check cargo-toml full Cargo.toml
check cargo-lock full Cargo.lock
check toolchain full rust-toolchain.toml
check testdata full testdata/demo/article.json
check workflow full .github/workflows/ci.yml
check classifier full scripts/ci-changes.sh
check docs-api full docs/api/openapi.json
check docs-api-md full docs/api/readme.md
check web-and-crate full apps/web/src/shell.tsx crates/api/src/lib.rs
check web-and-cargo full apps/web/package.json Cargo.lock
check web-and-workflow full apps/web/src/main.tsx .github/workflows/ci.yml
check web-and-docs-api full packages/foo/index.ts docs/api/openapi.json

if [ "$fail" -ne 0 ]; then
  exit 1
fi
printf 'ci-changes: all cases passed\n'
