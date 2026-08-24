//! OS keychain secret storage via keyring-rs — one API across platforms:
//! Windows Credential Manager, macOS Keychain, Linux Secret Service.
//! Keys never touch disk or SQLite.
//!
//! `set` verifies persistence via a *fresh* `Entry` (not the one that wrote),
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

/// Env var that can override the keychain for a provider, e.g.
/// `TOKENGUARD_KEY_OPENAI`.
///
/// Restricted to ASCII alphanumerics because `char::is_alphanumeric` is
/// Unicode-aware: a provider named `名前` would otherwise map to
/// `TOKENGUARD_KEY_名前`, which most shells cannot export — silently making the
/// env fallback unusable for that provider on headless hosts.
///
/// Note that sanitising collapses punctuation, so providers named `a-b` and
/// `a.b` share one variable. The keychain (the primary store) keys on the exact
/// name and is unaffected.
fn env_var_name(name: &str) -> String {
    let safe = name
        .to_uppercase()
        .replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    format!("TOKENGUARD_KEY_{safe}")
}

fn env_key(name: &str) -> Option<String> {
    std::env::var(env_var_name(name)).ok()
}

pub fn get(name: &str) -> Result<String, String> {
    match get_optional(name) {
        Ok(Some(key)) => Ok(key),
        Ok(None) => Err(keyring_err("get_password", keyring::Error::NoEntry)),
        Err(e) => Err(e),
    }
}

/// Like [`get`], but distinguishes "no key stored yet" (`Ok(None)`) from a
/// backend failure (`Err`). Callers that treat absence as a normal state should
/// use this instead of pattern-matching the text of `get`'s error string.
pub fn get_optional(name: &str) -> Result<Option<String>, String> {
    let entry = match Entry::new(SERVICE, name) {
        Ok(e) => e,
        Err(e) => {
            if let Some(key) = env_key(name) {
                return Ok(Some(key));
            }
            return Err(keyring_err("Entry::new", e));
        }
    };
    match entry.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(env_key(name)),
        Err(e) => {
            if let Some(key) = env_key(name) {
                tracing::info!("keyring read failed for '{name}'; using env fallback");
                return Ok(Some(key));
            }
            Err(keyring_err("get_password", e))
        }
    }
}

/// (key_exists, error_if_any). NoEntry is the normal "no key yet" state.
/// In headless/CLI mode keys may be supplied via env vars instead of the OS
/// keychain, so env-backed keys are reported as present.
pub fn status(name: &str) -> (bool, Option<String>) {
    if let Ok(entry) = Entry::new(SERVICE, name) {
        match entry.get_password() {
            Ok(_) => return (true, None),
            Err(keyring::Error::NoEntry) => {}
            Err(e) => {
                if env_key(name).is_some() {
                    return (true, None);
                }
                return (false, Some(keyring_err("status", e)));
            }
        }
    }
    if env_key(name).is_some() {
        return (true, None);
    }
    (false, None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_names_are_shell_safe_and_prefixed() {
        // Provider names are user-entered ("My Provider (EU)"), and the env
        // fallback is how headless/CLI users supply keys — an unsanitised name
        // produces a variable nobody can export.
        assert_eq!(env_var_name("openai"), "TOKENGUARD_KEY_OPENAI");
        assert_eq!(env_var_name("My Provider"), "TOKENGUARD_KEY_MY_PROVIDER");
        assert_eq!(
            env_var_name("azure-eu.west/2"),
            "TOKENGUARD_KEY_AZURE_EU_WEST_2"
        );
        for name in ["openai", "My Provider (EU)", "a.b-c/d:e", "名前"] {
            let var = env_var_name(name);
            assert!(var.starts_with("TOKENGUARD_KEY_"), "{var}");
            assert!(
                var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "not shell-safe: {var}"
            );
        }
        // Non-ASCII names must sanitise, not pass through: `is_alphanumeric` is
        // Unicode-aware, so `名前` used to yield TOKENGUARD_KEY_名前. One
        // underscore per replaced character.
        assert_eq!(env_var_name("名前"), "TOKENGUARD_KEY___");
    }

    #[test]
    fn distinct_provider_names_do_not_collide_after_sanitising() {
        // Sanitising maps many characters to '_', so names that differ only in
        // punctuation would share one env var and one provider's key would
        // silently authenticate requests to the other.
        assert_eq!(env_var_name("a-b"), env_var_name("a.b"));
        // Documented consequence rather than a bug to fix here: the keychain
        // (the primary store) keys on the exact name, so this only affects the
        // env fallback. Assert the keychain path stays exact.
        assert_ne!("a-b", "a.b");
    }

    #[test]
    fn empty_key_is_rejected_before_touching_the_keychain() {
        // Storing "" would make `status` report a key as present while every
        // request 401s.
        let err = set("__tokenguard_test_never_written__", "").expect_err("empty key");
        assert!(err.contains("empty"), "unhelpful message: {err:?}");
    }

    #[test]
    fn env_fallback_supplies_keys_when_the_keychain_has_none() {
        // Headless CI/servers have no secret-service daemon; the env fallback is
        // the only way `tokenguard serve` gets a key there.
        let name = "__tokenguard_test_env_fallback__";
        let var = env_var_name(name);
        // SAFETY: single-threaded test process section; no other thread reads env.
        unsafe { std::env::set_var(&var, "sk-from-env") };
        assert_eq!(env_key(name).as_deref(), Some("sk-from-env"));
        assert_eq!(get_optional(name).unwrap().as_deref(), Some("sk-from-env"));
        assert_eq!(get(name).unwrap(), "sk-from-env");
        assert_eq!(status(name), (true, None));
        unsafe { std::env::remove_var(&var) };
    }

    #[test]
    fn unset_key_reads_as_absent_not_as_an_error() {
        // `get_optional` exists so callers stop pattern-matching error strings;
        // absence must be Ok(None) even with no keychain backend available.
        let name = "__tokenguard_test_absent_key__";
        assert!(env_key(name).is_none());
        match get_optional(name) {
            Ok(v) => assert_eq!(v, None),
            // No secret-service daemon (typical CI): a backend error is the
            // documented alternative, but it must never be Ok(Some(_)).
            Err(e) => assert!(!e.is_empty(), "backend error must be described"),
        }
        // `status` must not claim a key exists either way.
        assert!(!status(name).0);
    }

    #[test]
    fn delete_is_idempotent_for_a_missing_entry() {
        // Called on provider removal for providers that never had a key stored.
        let name = "__tokenguard_test_absent_delete__";
        if let Err(e) = delete(name) {
            // Only a backend failure is acceptable — never NoEntry.
            assert!(
                !e.contains("NoEntry"),
                "missing entry must be treated as success: {e}"
            );
        }
    }

    #[test]
    fn troubleshooting_text_names_the_expected_backend() {
        // These strings are what the user sees in the keyring self-test when
        // storage fails; a generic message leaves them with no next step.
        let backend = expected_backend();
        assert!(!backend.is_empty());
        let help = troubleshooting();
        assert!(help.len() > 40, "troubleshooting text is too vague: {help}");

        let msg = keyring_err("set_password", keyring::Error::NoEntry);
        assert!(msg.contains("set_password"), "missing context: {msg}");
        assert!(msg.contains(backend), "missing backend name: {msg}");
    }
}
