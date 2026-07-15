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
- `docs/` — mdBook documentation

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
