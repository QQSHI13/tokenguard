//! OS keychain secret storage via keyring-rs — one API across platforms:
//! Windows Credential Manager, macOS Keychain, Linux Secret Service.
//! Keys never touch disk or SQLite.
//!
//! `set` verifies persistence via a *fresh* `Entry` (not the one that wrote),
//! so a non-persistent backend fails loudly instead of silently losing keys.

use keyring::Entry;

const SERVICE: &str = "tokenguard";

pub fn set(name: &str, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("empty key".into());
    }
    tracing::info!("keyring: storing key for '{name}' (len={})", key.len());
    let entry = Entry::new(SERVICE, name).map_err(|e| format!("Entry::new: {e}"))?;
    entry
        .set_password(key)
        .map_err(|e| format!("set_password: {e}"))?;
    // verify it actually persisted by reading from a FRESH entry
    match Entry::new(SERVICE, name).and_then(|e| e.get_password()) {
        Ok(v) if v == key => Ok(()),
        Ok(_) => Err("read-back mismatch".into()),
        Err(e) => Err(format!("saved but unreadable by fresh entry: {e}")),
    }
}

pub fn get(name: &str) -> Result<String, String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| format!("Entry::new: {e}"))?;
    entry
        .get_password()
        .map_err(|e| format!("get_password: {e}"))
}

/// (key_exists, error_if_any). NoEntry is the normal "no key yet" state.
pub fn status(name: &str) -> (bool, Option<String>) {
    let Ok(entry) = Entry::new(SERVICE, name) else {
        return (false, Some("Entry::new failed".into()));
    };
    match entry.get_password() {
        Ok(_) => (true, None),
        Err(keyring::Error::NoEntry) => (false, None),
        Err(e) => (false, Some(e.to_string())),
    }
}

pub fn delete(name: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| format!("Entry::new: {e}"))?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("delete_credential: {e}")),
    }
}

pub fn selftest() -> String {
    let name = "__tokenguard_selftest__";
    let mut report = String::new();
    match set(name, "tokenguard-test") {
        Ok(_) => report.push_str("set (write + fresh-read-back): OK\n"),
        Err(e) => return format!("set FAILED: {e}"),
    }
    match get(name) {
        Ok(v) => report.push_str(&format!(
            "fresh read: {}\n",
            if v == "tokenguard-test" { "OK" } else { "MISMATCH" }
        )),
        Err(e) => return format!("{report}fresh read FAILED: {e}"),
    }
    let _ = delete(name);
    report.push_str("(keyring-rs backend)");
    report
}
