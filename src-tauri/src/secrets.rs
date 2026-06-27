//! OS keychain secret storage. Keys never touch disk or SQLite.

use keyring::Entry;

const SERVICE: &str = "tokenguard";

pub fn set(name: &str, key: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| e.to_string())?;
    entry.set_password(key).map_err(|e| e.to_string())
}

pub fn get(name: &str) -> Result<String, String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| e.to_string())?;
    entry.get_password().map_err(|e| e.to_string())
}

pub fn has(name: &str) -> bool {
    get(name).is_ok()
}

pub fn delete(name: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE, name).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
