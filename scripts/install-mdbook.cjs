#!/usr/bin/env node
/**
 * Preinstall hook: ensure `mdbook` is available.
 *
 * Uses only Node built-ins so it runs before dependencies are installed.
 * Cross-platform: works on Windows, macOS, and Linux as long as Rust/Cargo
 * is on PATH.
 */

const { spawnSync } = require("child_process");

function hasMdbook() {
  const result = spawnSync("mdbook", ["--version"], {
    stdio: "ignore",
    shell: true,
  });
  return result.status === 0;
}

function hasCargo() {
  const result = spawnSync("cargo", ["--version"], {
    stdio: "ignore",
    shell: true,
  });
  return result.status === 0;
}

function installMdbook() {
  console.log("Installing mdbook via cargo...");
  const result = spawnSync("cargo", ["install", "mdbook"], {
    stdio: "inherit",
    shell: true,
  });
  if (result.status !== 0) {
    console.error("Failed to install mdbook.");
    process.exit(result.status ?? 1);
  }
}

if (hasMdbook()) {
  console.log("mdbook is already installed.");
  process.exit(0);
}

if (!hasCargo()) {
  console.error(
    "Cargo not found on PATH. Install Rust from https://rustup.rs/ and re-run npm install."
  );
  process.exit(1);
}

installMdbook();
