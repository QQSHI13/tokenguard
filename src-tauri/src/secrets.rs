//! OS keychain secret storage. Keys never touch disk or SQLite.
//!
//! - Windows: direct Win32 Credential Manager (CredWriteW/CredReadW/CredDeleteW).
//!   The keyring crate's Windows backend was non-persistent in this environment
//!   (write returned Ok but a fresh Entry couldn't read it back), so we bind
//!   the Win32 API directly. Credentials are stored per-user, persist across
//!   reboots, and survive the process.
//! - macOS / Linux: keyring crate (Keychain / Secret Service).

#[cfg(target_os = "windows")]
pub use crate::win_cred::{delete, get, selftest, set, status};

#[cfg(not(target_os = "windows"))]
pub use crate::other::{delete, get, selftest, set, status};
