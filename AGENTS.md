# Agent Guidance — Token Guard

This file helps AI coding assistants work effectively on the Token Guard repository.

## Project Overview

Token Guard is a local LLM API gateway and cost tracker built with:

- **Frontend:** React + TypeScript + Tailwind CSS + Vite
- **Desktop shell:** Tauri v2 (Rust)
- **Local database:** SQLite (via `rusqlite`)
- **Docs:** Markdown source in `docs/book-en` and `docs/book-zh`, rendered inside the app by `src/components/Docs.tsx`
- **Website:** Static HTML in `site/`, deployed to Cloudflare Pages from the `pages` branch

## Repository Layout

```
src/            React frontend
src-tauri/      Rust backend + Tauri app shell
public/         Static web assets (logo, favicon, etc.)
docs/           Markdown docs (rendered inside the app)
site/           Marketing/purchase website
```

## Build Commands

```bash
# Frontend only
npm run build

# Desktop app in development (requires Rust toolchain)
cargo tauri dev

# Production Tauri build
cargo tauri build

# Run Rust tests
cd src-tauri && cargo test

# Build the website
cd site && npm run build
```

## Conventions

- **Languages:** TypeScript for frontend, Rust for backend.
- **Styling:** Tailwind CSS. Use the neutral/emerald palette already in the components.
- **State:** Rust owns persistent state (SQLite, OS keychain). Frontend invokes commands via Tauri.
- **i18n:** Add new keys to both `en` and `zh-CN` objects in `src/i18n.tsx`.
- **Icons:** Source logo is `public/logo.svg`. Generate platform icon sets locally when needed.

## Testing

- Frontend: `npm run build` (TypeScript + Vite compile check).
- Backend: `cd src-tauri && cargo test`.

## Releases

Releases are built via `.github/workflows/release.yml` (manual workflow dispatch with a `tag` input like `v0.1.6`). The pipeline:

1. **test** — runs the full CI suite: TypeScript type-check, frontend build, site build, `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test`.
2. **bump-version** — writes the version into `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, pushes a `chore(release): bump version` commit (as github-actions[bot]) and the tag.
3. **publish-tauri** — builds binaries in a matrix (macOS, Windows, Linux) and attaches them to the GitHub Release for that tag via `tauri-apps/tauri-action`. Releases are **published immediately** (not drafts).

Trigger a release with:

```bash
gh workflow run release.yml -f tag=v0.1.7 -f releaseName="Token Guard v0.1.7"
```

Note: the tag is created by the workflow from the bumped commit — never tag manually beforehand.

## What NOT to do

- Do not commit secrets, API keys, or personal credentials.
- Do not change public pricing or license terms without explicit confirmation.
- Do not add heavy new dependencies without checking whether the project already uses them.
