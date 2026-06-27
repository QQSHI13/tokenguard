//! OS keychain secret storage. Keys never touch disk or SQLite.
//!
//! `set` writes and immediately reads back to verify the backend round-trips;
//! `status` returns (exists, error) so the UI can show *why* a key is missing
//! instead of a bare "no key".

use keyring::Entry;

const SERVICE: &str = "tokenguard";

pub fn set(name: &str, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("empty key".into());
    }
    tracing::info!("keyring: storing key for '{name}' (len={})", key.len());
    let entry = Entry::new(SERVICE, name).map_err(|e| {
        tracing::error!("keyring Entry::new failed for '{name}': {e}");
        format!("Entry::new: {e}")
    })?;
    entry.set_password(key).map_err(|e| {
        tracing::error!("keyring set_password failed for '{name}': {e}");
        format!("set_password: {e}")
    })?;
    // read-back verification — catches backends that silently no-op
    match entry.get_password() {
        Ok(v) if v == key => Ok(()),
        Ok(_) => Err(format!("keyring read-back mismatch for '{name}'")),
        Err(e) => {
            tracing::error!("keyring read-back get failed for '{name}': {e}");
            Err(format!("saved but unreadable: {e}"))
        }
    }
}

pub fn get(name: &str) -> Result<String, String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| format!("Entry::new: {e}"))?;
    entry.get_password().map_err(|e| format!("get_password: {e}"))
}

/// (key_exists, error_if_any). NoEntry is the normal "no key yet" state,
/// not an error — surfaced as (false, None) so the UI shows "no key".
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

/// Step-by-step keychain probe. Returns a human-readable report so the UI
/// can show exactly which step fails (Entry::new / set / read-back / new-entry).
pub fn selftest() -> String {
    let name = "__tokenguard_selftest__";
    let mut report = String::new();
    let entry = match Entry::new(SERVICE, name) {
        Ok(e) => {
            report.push_str("Entry::new: OK\n");
            e
        }
        Err(e) => return format!("Entry::new FAILED: {e}"),
    };
    match entry.set_password("tokenguard-test") {
        Ok(_) => report.push_str("set_password: OK\n"),
        Err(e) => return format!("{report}set_password FAILED: {e}"),
    };
    match entry.get_password() {
        Ok(v) => report.push_str(&format!(
            "read-back (same entry): {}\n",
            if v == "tokenguard-test" { "OK" } else { "MISMATCH" }
        )),
        Err(e) => return format!("{report}read-back FAILED: {e}"),
    };
    match Entry::new(SERVICE, name) {
        Ok(e2) => match e2.get_password() {
            Ok(v) => report.push_str(&format!(
                "new-entry read: {}\n",
                if v == "tokenguard-test" { "OK" } else { "MISMATCH" }
            )),
            Err(e) => report.push_str(&format!("new-entry read FAILED: {e}\n")),
        },
        Err(e) => report.push_str(&format!("new Entry::new FAILED: {e}\n")),
    }
    let _ = entry.delete_credential();
    report.push_str("(selftest credential cleaned up)");
    report
}
