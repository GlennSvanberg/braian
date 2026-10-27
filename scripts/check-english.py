#!/usr/bin/env python3
"""Fail when known Swedish words show up in the app or crate sources.

Language-tag parsing fixtures and the agent's recognition phrases are skipped.
A block marked `english-check: allow` / `english-check: end`, or a line marked
`english-check: allow-line`, is skipped.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCAN = [ROOT / "apps" / "web" / "src", ROOT / "crates"]
SKIP = {
    Path("crates/model/src/language.rs"),
    Path("crates/api/src/import/parse.rs"),
    Path("crates/synth"),
    Path("crates/bench"),
}
WORDS = (
    "artikel",
    "artiklar",
    "artikelnummer",
    "produkt",
    "produkter",
    "leverantör",
    "leverantor",
    "sortiment",
    "utkast",
    "namn",
    "nummer",
    "färg",
    "farg",
    "översikt",
    "oversikt",
    "granska",
    "tält",
    "talt",
    "friluft",
    "giltig",
    "beskrivning",
    "datamodell",
    "förslag",
    "forslag",
    "historik",
    "batchlogg",
    "ändringar",
    "andringar",
    "berika",
    "regnjacka",
    "sovsack",
    "objekt",
)
WORD_RE = re.compile(r"(?i)\b(?:" + "|".join(WORDS) + r")\b")
CHAR_RE = re.compile(r"[åäöÅÄÖ]")
SOURCE = {".rs", ".ts", ".tsx", ".js", ".jsx", ".css", ".sql", ".json"}


def skipped(path: Path) -> bool:
    relative = path.relative_to(ROOT)
    return any(relative == item or item in relative.parents for item in SKIP)


def lines_outside_allows(text: str) -> list[tuple[int, str]]:
    kept: list[tuple[int, str]] = []
    skipping = False
    for number, line in enumerate(text.splitlines(), start=1):
        if "english-check: end" in line:
            skipping = False
            continue
        if "english-check: allow-line" in line:
            continue
        if "english-check: allow" in line:
            skipping = True
            continue
        if skipping:
            continue
        kept.append((number, line))
    return kept


def main() -> int:
    hits: list[str] = []
    for base in SCAN:
        for path in base.rglob("*"):
            if not path.is_file() or path.suffix not in SOURCE or skipped(path):
                continue
            relative = path.relative_to(ROOT)
            # Only crate sources, not their integration tests or benches.
            if relative.parts[0] == "crates" and "src" not in relative.parts:
                continue
            text = path.read_text(encoding="utf-8", errors="replace")
            for number, line in lines_outside_allows(text):
                if WORD_RE.search(line) or CHAR_RE.search(line):
                    hits.append(f"{relative}:{number}: {line.strip()}")
    if hits:
        print("Swedish copy is still in the app:", file=sys.stderr)
        print("\n".join(hits), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
