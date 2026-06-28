//! OS keychain secret storage via keyring-rs — one API across platforms:
//! Windows Credential Manager, macOS Keychain, Linux Secret Service.
//! Keys never touch disk or SQLite.
//!
//! `set` verifies persistence via a *fresh` `Entry` (not the one that wrote),
//! so a non-persistent backend fails loudly instead of silently losing keys.

use keyring::Entry;

const SERVICE: &str = "tokenguard";

fn os_name() -> &'static str {
    std::env::consts::OS
}

fn expected_backend() -> &'static str {
    match os_name() {
        "windows" => "Windows Credential Manager",
        "macos" => "macOS Keychain",
        _ => "Linux Secret Service (D-Bus)",
    }
}

fn troubleshooting() -> String {
    match os_name() {
        "windows" => {
            "On Windows: make sure Credential Manager (the 'Credential Manager' Windows service) is running, \
             and that you are not running inside a sandbox/container that blocks Win32 credential APIs. \
             If you are running from WSL, the binary is Linux and needs a Linux secret-service provider instead."
                .to_string()
        }
        "macos" => {
            "On macOS: make sure Keychain Access is available and the app has the required keychain entitlements."
                .to_string()
        }
        _ => {
            "On Linux/WSL: install and start a secret-service provider such as GNOME Keyring \
             (`gnome-keyring-daemon --start --components=secrets`), KeePassXC with secret-service integration, \
             or KWallet with the secret-service interface.".to_string()
        }
    }
}

/// Turn a keyring error into an actionable message.
fn keyring_err(context: &str, e: keyring::Error) -> String {
    let detail = match &e {
        keyring::Error::NoEntry => format!(
            "the entry disappeared after being saved. Expected backend: {}. {}",
            expected_backend(),
            troubleshooting()
        ),
        keyring::Error::PlatformFailure(_) => format!(
            "OS keychain backend failure. Expected backend: {}. {}",
            expected_backend(),
            troubleshooting()
        ),
        _ => "see https://github.com/hwchen/keyring-rs for backend requirements".to_string(),
    };
    format!("{context}: {e} — {detail}")
}

pub fn set(name: &str, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("empty key".into());
    }
    tracing::info!(
        "keyring: storing key for '{name}' (len={}) on {} / expected backend: {}",
        key.len(),
        os_name(),
        expected_backend()
    );
    let entry = Entry::new(SERVICE, name).map_err(|e| keyring_err("Entry::new", e))?;
    entry
        .set_password(key)
        .map_err(|e| keyring_err("set_password", e))?;

    // First verify with the same entry object (catches target-name normalization bugs).
    match entry.get_password() {
        Ok(v) if v == key => {}
        Ok(_) => return Err("same-entry read-back mismatch".into()),
        Err(e) => {
            return Err(format!(
                "same-entry read-back failed: {}. The backend did not persist the credential.",
                keyring_err("same-entry read-back", e)
            ))
        }
    }

    // Then verify with a fresh entry object (catches stale-handle issues).
    match Entry::new(SERVICE, name).and_then(|e| e.get_password()) {
        Ok(v) if v == key => Ok(()),
        Ok(_) => Err("fresh read-back mismatch".into()),
        Err(e) => Err(keyring_err("fresh read-back", e)),
    }
}

pub fn get(name: &str) -> Result<String, String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| keyring_err("Entry::new", e))?;
    entry
        .get_password()
        .map_err(|e| keyring_err("get_password", e))
}

/// (key_exists, error_if_any). NoEntry is the normal "no key yet" state.
pub fn status(name: &str) -> (bool, Option<String>) {
    let Ok(entry) = Entry::new(SERVICE, name) else {
        return (false, Some("Entry::new failed".into()));
    };
    match entry.get_password() {
        Ok(_) => (true, None),
        Err(keyring::Error::NoEntry) => (false, None),
        Err(e) => (false, Some(keyring_err("status", e))),
    }
}

pub fn delete(name: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| keyring_err("Entry::new", e))?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(keyring_err("delete_credential", e)),
    }
}

pub fn selftest() -> String {
    let name = "__tokenguard_selftest__";
    let mut report = format!(
        "OS: {}\nexpected backend: {}\nservice: {}\nusername: {}\n",
        os_name(),
        expected_backend(),
        SERVICE,
        name
    );
    match set(name, "tokenguard-test") {
        Ok(_) => report.push_str("set (write + same-entry + fresh-read-back): OK\n"),
        Err(e) => return format!("{report}set FAILED: {e}"),
    }
    match get(name) {
        Ok(v) => report.push_str(&format!(
            "fresh read: {}\n",
            if v == "tokenguard-test" {
                "OK"
            } else {
                "MISMATCH"
            }
        )),
        Err(e) => return format!("{report}fresh read FAILED: {e}"),
    }
    let _ = delete(name);
    report.push_str("(keyring-rs backend)");
    report
}
