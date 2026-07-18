# Contributing to Token Guard

Thanks for your interest in Token Guard. This project is open source under Apache-2.0.

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) 24+
- [Rust](https://rustup.rs/) stable toolchain
- For Linux: `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`

### Install Dependencies

```bash
npm install
```

### Run the App in Development

```bash
cargo tauri dev
```

This starts the Vite dev server and the Tauri desktop app with hot reload.

## Project Structure

- `src/` — React frontend
- `src-tauri/` — Rust backend and Tauri shell
- `public/` — Static web assets
- `site/` — Public website (deployed separately)
- `docs/` — Markdown documentation (rendered in-app, mirrored to the GitHub wiki)
- `pricing.json` — Built-in model pricing table (see below)

## Updating pricing (`pricing.json`)

Cost estimates come from `pricing.json` at the repo root, embedded into the
binary at build time. Token Guard **never fetches pricing from the internet** —
fresh prices ship with each release, and this file is how the community keeps
them current.

Schema (an ordered array under `models`, longest `pattern` first — the first
match wins):

```json
{
  "pattern": "gpt-4o-mini",
  "match_type": "prefix",
  "input_per_1k": 0.00015,
  "output_per_1k": 0.0006,
  "cached_input_per_1k": 0.000075,
  "provider": "openai",
  "source": "https://openai.com/api/pricing/",
  "updated": "2026-07-18"
}
```

Rules for a pricing PR:

- Prices are **USD per 1K tokens**. `cached_input_per_1k` is optional.
- `match_type` is `prefix` (model name starts with `pattern`) or `contains`
  (model name contains it). Put more specific patterns before less specific
  ones (e.g. `gpt-4o-mini` before `gpt-4o`).
- Every entry **must cite an official pricing page** in `source`.
- The bulk import was seeded from [models.dev](https://models.dev); feel free
  to correct entries against official provider pricing.
- `cargo test` validates the schema (no duplicates, https sources, sane
  values) — CI and the release workflow both run it, so a malformed file
  cannot ship.
- Unknown models still cost $0.00 until added; users can also set exact
  per-provider prices in the app's Settings as an override.

## Making Changes

1. Fork the repository and create a branch.
2. Make your changes.
3. Add or update tests where appropriate.
4. Run the checks below.
5. Open a pull request.

## Checks Before Committing

```bash
# TypeScript / Vite build
npm run build

# Rust tests
cd src-tauri && cargo test

# Rust formatting
cd src-tauri && cargo fmt --check
```

## Reporting Issues

Please include:

- Your operating system and version
- Token Guard version
- Steps to reproduce the problem
- Relevant logs or screenshots

## License

By contributing, you agree that your contributions will be licensed under the Apache-2.0 license.
