//! Tokens in Windows Credential Manager (generic credentials), so they never
//! sit in plain files or reach the webview.

use windows::core::{HSTRING, PWSTR};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

pub fn write(target: &str, secret: &str) -> Result<(), String> {
    let target = HSTRING::from(target);
    let user = HSTRING::from("dock-panel");
    let mut blob = secret.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_ptr() as *mut _),
        UserName: PWSTR(user.as_ptr() as *mut _),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    unsafe { CredWriteW(&cred, 0) }.map_err(|e| e.to_string())
}

pub fn read(target: &str) -> Option<String> {
    let target = HSTRING::from(target);
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        CredReadW(&target, CRED_TYPE_GENERIC, None, &mut cred).ok()?;
        let c = &*cred;
        let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
        let secret = String::from_utf8(bytes.to_vec()).ok();
        CredFree(cred as *const _);
        secret
    }
}

pub fn delete(target: &str) {
    let target = HSTRING::from(target);
    unsafe {
        let _ = CredDeleteW(&target, CRED_TYPE_GENERIC, None);
    }
}
