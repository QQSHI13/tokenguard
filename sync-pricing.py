#!/usr/bin/env python3
"""Sync pricing.json from models.dev catalog plus local overrides.

models.dev exposes the combined provider catalog at:
  https://models.dev/catalog.json

Each provider entry contains models with a "cost" object:
  { "input": $/Mtok, "output": $/Mtok, "cache_read": $/Mtok,
    "cache_write": $/Mtok, "reasoning": $/Mtok, ... }

This script converts prices to USD per 1K tokens (Token Guard's native unit)
and writes a fresh pricing.json. Manual overrides from
pricing.overrides.json are merged on top so context/time-tier examples
and provider corrections survive re-syncs.

Run from the repo root:
  python3 sync-pricing.py
"""

import json
import sys
import urllib.request
from datetime import datetime, timezone

CATALOG_URL = "https://models.dev/catalog.json"
OVERRIDES = "pricing.overrides.json"
OUTPUT = "pricing.json"


def fetch() -> dict:
    # models.dev's CDN appears to reset some HTTP/2 streams; force HTTP/1.1.
    req = urllib.request.Request(
        CATALOG_URL,
        headers={
            "User-Agent": "tokenguard-pricing-sync/1.0",
            "Accept": "application/json",
        },
    )
    last_err: Exception | None = None
    for attempt in range(1, 4):
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                return json.load(resp)
        except Exception as e:  # noqa: BLE001
            last_err = e
            print(f"Fetch attempt {attempt} failed: {e}", file=sys.stderr)
    raise RuntimeError(f"Could not fetch {CATALOG_URL}: {last_err}")


def to_per_1k(per_m: float | None) -> float | None:
    if per_m is None:
        return None
    return per_m / 1000.0


def model_patterns(model_id: str) -> list[str]:
    """Return pattern candidates, most-specific first."""
    patterns = [model_id.lower()]
    # If the id already has a provider prefix (e.g. openai/gpt-4o), also add
    # the unprefixed name so requests that send just "gpt-4o" still match.
    if "/" in model_id:
        patterns.append(model_id.split("/", 1)[1].lower())
    # Deduplicate while preserving order.
    seen: set[str] = set()
    out: list[str] = []
    for p in patterns:
        if p not in seen:
            seen.add(p)
            out.append(p)
    return out


def build_entries(provider_id: str, model: dict) -> list[dict]:
    cost = model.get("cost") or {}
    if not cost.get("input") and not cost.get("output"):
        return []

    base = {
        "match_type": "prefix",
        "input_per_1k": to_per_1k(cost.get("input")),
        "output_per_1k": to_per_1k(cost.get("output")),
        "cached_input_per_1k": to_per_1k(cost.get("cache_read")),
        "provider": provider_id,
        "source": "https://models.dev",
        "updated": datetime.now(timezone.utc).strftime("%Y-%m-%d"),
    }
    # Strip None values.
    base = {k: v for k, v in base.items() if v is not None}

    return [{"pattern": p, **base} for p in model_patterns(model["id"])]


def load_overrides() -> list[dict]:
    try:
        with open(OVERRIDES, "r", encoding="utf-8") as f:
            data = json.load(f)
    except FileNotFoundError:
        return []
    return data.get("models", [])


def merge_entries(generated: list[dict], overrides: list[dict]) -> list[dict]:
    """Replace generated entries by (pattern, match_type) and append new ones."""
    by_key: dict[tuple[str, str], dict] = {
        (e["pattern"], e["match_type"]): e for e in generated
    }
    for entry in overrides:
        key = (entry["pattern"], entry["match_type"])
        by_key[key] = entry
    return list(by_key.values())


def main() -> None:
    catalog = fetch()
    entries = []
    seen: set[tuple[str, str]] = set()

    for provider in catalog.get("providers", {}).values():
        provider_id = provider.get("id", "unknown")
        for model in provider.get("models", {}).values():
            for entry in build_entries(provider_id, model):
                key = (entry["pattern"], entry["match_type"])
                if key in seen:
                    continue
                seen.add(key)
                entries.append(entry)

    entries = merge_entries(entries, load_overrides())

    # Longest-pattern-first so specific ids beat generic prefixes.
    entries.sort(key=lambda e: len(e["pattern"]), reverse=True)

    output = {
        "$comment": (
            "LLM pricing snapshot (USD per 1K tokens) imported from models.dev, "
            "merged with local overrides. Community-maintained: every entry cites "
            "a source. Ordered longest-pattern-first; first match wins."
        ),
        "models": entries,
    }

    with open(OUTPUT, "w", encoding="utf-8") as f:
        json.dump(output, f, indent=2)
        f.write("\n")

    print(f"Wrote {len(entries)} entries to {OUTPUT}")


if __name__ == "__main__":
    main()
