//! Provider credentials in the OS secret store (Windows Credential Manager).
//! Keys are written from the UI but never sent back to it, and never logged.

use crate::application::{AppError, AppResult, ErrorKind};

/// Credential Manager target name for the Gemini API key.
pub const GEMINI_KEY: &str = "VoiceFlow/gemini-api-key";

#[cfg(target_os = "windows")]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub fn store(name: &str, secret: &str) -> AppResult<()> {
    use windows::core::PWSTR;
    use windows::Win32::Security::Credentials::{CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC};

    let mut target = wide(name);
    let mut user = wide("voiceflow");
    let mut blob = secret.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        UserName: PWSTR(user.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        // Per-user, persists across logons on this machine; not roamed.
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: all pointers reference live buffers for the duration of the call.
    let result = unsafe { CredWriteW(&cred, 0) };
    blob.fill(0);
    result.map_err(|e| AppError::new(ErrorKind::Internal, format!("Could not save the key securely ({e}).")))
}

/// `Ok(None)` when no key is stored.
#[cfg(target_os = "windows")]
pub fn load(name: &str) -> AppResult<Option<String>> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC};

    let target = wide(name);
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: CredReadW allocates `cred`, which we free with CredFree below.
    match unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut cred) } {
        Ok(()) => {
            // SAFETY: on success `cred` points to a valid CREDENTIALW with a blob of the given size.
            let secret = unsafe {
                let c = &*cred;
                let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
                let s = String::from_utf8_lossy(bytes).into_owned();
                CredFree(cred as *const core::ffi::c_void);
                s
            };
            Ok(Some(secret))
        }
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(None),
        Err(e) => Err(AppError::new(ErrorKind::Internal, format!("Could not read the stored key ({e})."))),
    }
}

#[cfg(target_os = "windows")]
pub fn delete(name: &str) -> AppResult<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target = wide(name);
    // SAFETY: target is a valid NUL-terminated UTF-16 string.
    match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(e) => Err(AppError::new(ErrorKind::Internal, format!("Could not remove the stored key ({e})."))),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn store(_name: &str, _secret: &str) -> AppResult<()> {
    Err(AppError::new(ErrorKind::Internal, "Secure key storage is not implemented on this platform yet."))
}

#[cfg(not(target_os = "windows"))]
pub fn load(_name: &str) -> AppResult<Option<String>> {
    Ok(None)
}

#[cfg(not(target_os = "windows"))]
pub fn delete(_name: &str) -> AppResult<()> {
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn store_load_delete_round_trip() {
        let name = format!("VoiceFlow/test-{}", std::process::id());
        assert_eq!(load(&name).unwrap(), None);
        store(&name, "secret-value-123").unwrap();
        assert_eq!(load(&name).unwrap().as_deref(), Some("secret-value-123"));
        store(&name, "rotated").unwrap();
        assert_eq!(load(&name).unwrap().as_deref(), Some("rotated"));
        delete(&name).unwrap();
        assert_eq!(load(&name).unwrap(), None);
        delete(&name).unwrap(); // idempotent
    }
}
