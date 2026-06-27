//! macOS / Linux keychain backend (keyring crate).

use keyring::Entry;

const SERVICE: &str = "tokenguard";

pub fn set(name: &str, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("empty key".into());
    }
    let entry = Entry::new(SERVICE, name).map_err(|e| format!("Entry::new: {e}"))?;
    entry
        .set_password(key)
        .map_err(|e| format!("set_password: {e}"))?;
    match entry.get_password() {
        Ok(v) if v == key => Ok(()),
        Ok(_) => Err("read-back mismatch".into()),
        Err(e) => Err(format!("saved but unreadable: {e}")),
    }
}

pub fn get(name: &str) -> Result<String, String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| format!("Entry::new: {e}"))?;
    entry
        .get_password()
        .map_err(|e| format!("get_password: {e}"))
}

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
    match set(name, "tokenguard-test") {
        Ok(_) => "set (write + read-back): OK\n".to_string(),
        Err(e) => return format!("set FAILED: {e}"),
    };
    let mut report = match get(name) {
        Ok(v) => format!(
            "set (write + read-back): OK\nfresh read: {}\n",
            if v == "tokenguard-test" { "OK" } else { "MISMATCH" }
        ),
        Err(e) => return format!("fresh read FAILED: {e}"),
    };
    let _ = delete(name);
    report.push_str("(keyring backend)");
    report
}
