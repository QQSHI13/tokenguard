// Prevents additional console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "gui")]
fn main() {
    tokenguard_lib::run()
}

#[cfg(not(feature = "gui"))]
fn main() {
    eprintln!("Token Guard was built without the GUI feature.");
    eprintln!("Use the tokenguard-cli binary for headless operation.");
    std::process::exit(1);
}
