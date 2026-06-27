//! Windows Credential Manager backend (direct Win32, no keyring middleman).

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
};

const CRED_TYPE_GENERIC: u32 = 1;
const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
const ERROR_NOT_FOUND: u32 = 1168;

/// Credential target name, UTF-16, null-terminated.
fn target(name: &str) -> Vec<u16> {
    format!("tokenguard:{name}")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn last_err() -> u32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32
}

pub fn set(name: &str, key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("empty key".into());
    }
    tracing::info!("credman: storing key for '{name}' (len={})", key.len());
    let target = target(name);
    let blob = key.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr() as *mut u16,
        Comment: std::ptr::null_mut(),
        LastWritten: FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: std::ptr::null_mut(),
        TargetAlias: std::ptr::null_mut(),
        UserName: std::ptr::null_mut(),
    };
    unsafe {
        if CredWriteW(&cred as *const _, 0) == 0 {
            return Err(format!("CredWriteW failed (err {})", last_err()));
        }
    }
    // verify it persisted and is readable from a fresh lookup
    match get(name) {
        Ok(v) if v == key => Ok(()),
        Ok(_) => Err("read-back mismatch".into()),
        Err(e) => Err(format!("saved but unreadable: {e}")),
    }
}

pub fn get(name: &str) -> Result<String, String> {
    let target = target(name);
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut ptr as *mut _) == 0 {
            let code = last_err();
            if code == ERROR_NOT_FOUND {
                return Err("NoEntry".into());
            }
            return Err(format!("CredReadW failed (err {code})"));
        }
        let cred = &*ptr;
        let n = cred.CredentialBlobSize as usize;
        let bytes = if n == 0 || cred.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(cred.CredentialBlob, n).to_vec()
        };
        CredFree(ptr as *const _);
        String::from_utf8(bytes).map_err(|e| format!("utf8: {e}"))
    }
}

/// (key_exists, error_if_any). NoEntry is the normal "no key yet" state.
pub fn status(name: &str) -> (bool, Option<String>) {
    match get(name) {
        Ok(_) => (true, None),
        Err(e) if e == "NoEntry" => (false, None),
        Err(e) => (false, Some(e)),
    }
}

pub fn delete(name: &str) -> Result<(), String> {
    let target = target(name);
    unsafe {
        if CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) == 0 {
            let code = last_err();
            if code == ERROR_NOT_FOUND {
                return Ok(());
            }
            return Err(format!("CredDeleteW failed (err {code})"));
        }
    }
    Ok(())
}

pub fn selftest() -> String {
    let name = "__tokenguard_selftest__";
    let mut report = String::new();
    match set(name, "tokenguard-test") {
        Ok(_) => report.push_str("set (write + read-back): OK\n"),
        Err(e) => return format!("set FAILED: {e}"),
    }
    match get(name) {
        Ok(v) => report.push_str(&format!(
            "fresh read: {}\n",
            if v == "tokenguard-test" { "OK" } else { "MISMATCH" }
        )),
        Err(e) => return format!("{report}fresh read FAILED: {e}"),
    }
    match delete(name) {
        Ok(_) => report.push_str("delete: OK\n"),
        Err(e) => report.push_str(&format!("delete FAILED: {e}\n")),
    }
    report.push_str("(win32 Credential Manager)");
    report
}
