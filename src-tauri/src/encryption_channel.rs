//! Mountain: `encryption` protocol channel (IEncryptionMainService).
//!
//! electron-main exposes safeStorage-based encryption as the `encryption`
//! ProxyChannel (app.ts ~1387) and the desktop renderer registers its
//! IEncryptionService against it
//! (src/vs/workbench/services/encryption/electron-browser/encryptionService.ts
//! — `registerMainProcessRemoteService(IEncryptionService, 'encryption')`).
//!
//! The secret storage service (BaseSecretStorageService) encrypts EVERY
//! secret — including the AI provider API keys the user configures in the
//! editor (the copilot BYOK providers: openai / anthropic / gemini / ollama /
//! openrouter / azure / xai / customoai / customendpoint) — through this
//! channel before it lands in the storage DB (`secret://` keys). Without it
//! the secrets service stays in "in-memory" mode and every API key is lost
//! on restart.
//!
//! Windows implementation: DPAPI (`CryptProtectData`/`CryptUnprotectData`
//! from crypt32.dll) — exactly what Electron's safeStorage uses on Windows,
//! so the ciphertext shape is compatible in spirit (user-scoped, no key
//! material on disk) and the JSON envelope matches upstream:
//! `JSON.stringify({ data: base64 })`.
//!
//! Non-Windows builds (dev parity): a plain-text envelope so the renderer
//! pipeline still works locally. The data folder is dev-only there.

use serde_json::{json, Value};

/// Handle one `encryption` channel request. ProxyChannel semantics: `arg`
/// is the method's first argument; single-string-argument methods.
pub fn handle(command: &str, arg: &Value) -> Result<Value, String> {
    match command {
        "encrypt" => {
            let value = arg.as_str().unwrap_or_default();
            let encrypted = encrypt(value)?;
            // Electron parity: the envelope is JSON.stringify({ data: base64 }).
            Ok(json!(serde_json::to_string(&json!({ "data": encrypted }))
                .map_err(|err| err.to_string())?))
        }
        "decrypt" => {
            let value = arg.as_str().unwrap_or_default();
            let payload: Value =
                serde_json::from_str(value).map_err(|_| "encryption: invalid encrypted envelope".to_string())?;
            let data = payload
                .get("data")
                .and_then(Value::as_str)
                .ok_or_else(|| "encryption: encrypted envelope has no data".to_string())?;
            let plain = decrypt(data)?;
            Ok(json!(plain))
        }
        "isEncryptionAvailable" => Ok(json!(true)),
        // KnownStorageProvider.dplib ("dpapi") on Windows — the value
        // upstream's EncryptionMainService returns for this platform; the
        // renderer's secretStorageService uses it to decide whether secrets
        // persist ("persisted") vs stay in memory.
        "getKeyStorageProvider" => Ok(json!("dpapi")),
        // The Electron custom-build-only plain-text opt-out; the shell
        // always has DPAPI, so this is a no-op.
        "setUsePlainTextEncryption" => Ok(Value::Null),
        other => Err(format!("encryption channel: method not found: {}", other)),
    }
}

// ---------------------------------------------------------------------------
// DPAPI (Windows) / plain envelope (non-Windows dev parity)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod dpapi {
    use std::os::raw::c_void;

    #[repr(C)]
    struct CryptIntegerBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    extern "system" {
        fn CryptProtectData(
            data_in: *const CryptIntegerBlob,
            sz_data_descr: *const u16,
            optional_entropy: *const CryptIntegerBlob,
            pv_reserved: *const c_void,
            pprompt_struct: *const c_void,
            dw_flags: u32,
            data_out: *mut CryptIntegerBlob,
        ) -> i32;
        fn CryptUnprotectData(
            data_in: *const CryptIntegerBlob,
            ppsz_data_descr: *mut *mut u16,
            optional_entropy: *const CryptIntegerBlob,
            pv_reserved: *const c_void,
            pprompt_struct: *const c_void,
            dw_flags: u32,
            data_out: *mut CryptIntegerBlob,
        ) -> i32;
        fn LocalFree(hmem: *mut c_void) -> *mut c_void;
    }

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    fn protect(input: &[u8]) -> Result<Vec<u8>, String> {
        unsafe {
            let in_blob = CryptIntegerBlob {
                cb_data: input.len() as u32,
                pb_data: input.as_ptr() as *mut u8,
            };
            let mut out_blob = CryptIntegerBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
            let ok = CryptProtectData(
                &in_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            );
            if ok == 0 {
                return Err(format!("encryption: CryptProtectData failed (os error {})", std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
            }
            let out = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize).to_vec();
            LocalFree(out_blob.pb_data as *mut c_void);
            Ok(out)
        }
    }

    fn unprotect(input: &[u8]) -> Result<Vec<u8>, String> {
        unsafe {
            let in_blob = CryptIntegerBlob {
                cb_data: input.len() as u32,
                pb_data: input.as_ptr() as *mut u8,
            };
            let mut out_blob = CryptIntegerBlob { cb_data: 0, pb_data: std::ptr::null_mut() };
            let ok = CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            );
            if ok == 0 {
                return Err("encryption: CryptUnprotectData failed (wrong user or corrupted data)".to_string());
            }
            let out = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize).to_vec();
            LocalFree(out_blob.pb_data as *mut c_void);
            Ok(out)
        }
    }

    pub fn encrypt_to_b64(value: &str) -> Result<String, String> {
        let raw = protect(value.as_bytes())?;
        Ok(crate::ipc::base64_encode_public(&raw))
    }

    pub fn decrypt_from_b64(b64: &str) -> Result<String, String> {
        let raw = crate::ipc::base64_decode_public(b64).ok_or_else(|| "encryption: invalid base64 payload".to_string())?;
        let plain = unprotect(&raw)?;
        String::from_utf8(plain).map_err(|_| "encryption: decrypted value is not utf-8".to_string())
    }

    /// Exposed for tests: round-trip through the real DPAPI.
    pub fn roundtrip(value: &str) -> Result<String, String> {
        let b64 = encrypt_to_b64(value)?;
        decrypt_from_b64(&b64)
    }
}

#[cfg(windows)]
fn encrypt(value: &str) -> Result<String, String> {
    dpapi::encrypt_to_b64(value)
}

#[cfg(windows)]
fn decrypt(b64: &str) -> Result<String, String> {
    dpapi::decrypt_from_b64(b64)
}

#[cfg(not(windows))]
fn encrypt(value: &str) -> Result<String, String> {
    // Dev parity on non-Windows hosts: plain base64 envelope. The bundle
    // only ships for Windows; this keeps local bring-up working.
    Ok(crate::ipc::base64_encode_public(value.as_bytes()))
}

#[cfg(not(windows))]
fn decrypt(b64: &str) -> Result<String, String> {
    let raw = crate::ipc::base64_decode_public(b64).ok_or_else(|| "encryption: invalid base64 payload".to_string())?;
    String::from_utf8(raw).map_err(|_| "encryption: decrypted value is not utf-8".to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip_matches_electron_shape() {
        let encrypted = handle("encrypt", &json!("hello secrets")).unwrap();
        let envelope: Value = serde_json::from_str(encrypted.as_str().unwrap()).unwrap();
        assert!(envelope["data"].is_string());

        let plain = handle("decrypt", &encrypted).unwrap();
        assert_eq!(plain, json!("hello secrets"));
    }

    #[test]
    fn provider_metadata_matches_upstream() {
        assert_eq!(handle("isEncryptionAvailable", &Value::Null).unwrap(), json!(true));
        assert_eq!(handle("getKeyStorageProvider", &Value::Null).unwrap(), json!("dpapi"));
        assert_eq!(handle("setUsePlainTextEncryption", &Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn dpapi_roundtrip() {
        // On Windows this exercises the real DPAPI; elsewhere the base64
        // envelope. Either way the value must survive.
        let value = "sk-test-key-0123456789";
        let b64 = encrypt(value).unwrap();
        assert_ne!(b64, value);
        assert_eq!(decrypt(&b64).unwrap(), value);
    }
}
