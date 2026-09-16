use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

const MAX_SECRET_BYTES: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("keyring backend is unavailable: {0}")]
    Unavailable(String),
    #[error("keyring account is invalid")]
    InvalidAccount,
    #[error("keyring secret exceeds the configured size limit")]
    TooLarge,
    #[error("keyring command timed out")]
    Timeout,
    #[error("keyring command failed during {operation} with status {status}")]
    Command {
        operation: &'static str,
        status: i32,
    },
    #[error("keyring returned invalid text")]
    InvalidText,
    #[error("keyring I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
enum Backend {
    Native,
    Fixture(PathBuf),
}

#[derive(Debug, Clone)]
pub struct SecretStore {
    service: String,
    backend: Backend,
}

#[derive(Debug)]
struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

impl SecretStore {
    /// Use the operating system keyring. On Linux this is libsecret's
    /// `secret-tool`; unsupported platforms report unavailable instead of
    /// silently writing a file.
    pub fn native(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            backend: Backend::Native,
        }
    }

    /// Use a small command fixture with the contract:
    /// `fixture get|set|delete <service> <account>`, with the secret sent on
    /// stdin for `set` and returned on stdout for `get`.
    pub fn with_command(service: impl Into<String>, command: impl Into<PathBuf>) -> Self {
        Self {
            service: service.into(),
            backend: Backend::Fixture(command.into()),
        }
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            Backend::Native => {
                #[cfg(target_os = "linux")]
                {
                    "libsecret"
                }
                #[cfg(target_os = "macos")]
                {
                    "keychain"
                }
                #[cfg(target_os = "windows")]
                {
                    "credential-manager"
                }
                #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
                {
                    "unsupported"
                }
            }
            Backend::Fixture(_) => "fixture",
        }
    }

    pub fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        validate_account(account)?;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if matches!(&self.backend, Backend::Native) {
            return native_keyring::get(&self.service, account);
        }
        let output = self.run("get", account, None)?;
        if !output.status.success() {
            // Both secret-tool and the fixture use status 1 for a missing
            // item. Other failures remain actionable and do not expose stderr.
            if output.status.code() == Some(1) {
                return Ok(None);
            }
            return Err(SecretError::Command {
                operation: "get",
                status: output.status.code().unwrap_or(-1),
            });
        }
        if output.stdout.len() > MAX_SECRET_BYTES {
            return Err(SecretError::TooLarge);
        }
        let mut bytes = output.stdout;
        if bytes.ends_with(b"\n") {
            bytes.pop();
            if bytes.ends_with(b"\r") {
                bytes.pop();
            }
        }
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| SecretError::InvalidText)
    }

    pub fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        validate_account(account)?;
        if secret.len() > MAX_SECRET_BYTES {
            return Err(SecretError::TooLarge);
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if matches!(&self.backend, Backend::Native) {
            return native_keyring::set(&self.service, account, secret);
        }
        let output = self.run("set", account, Some(secret.as_bytes().to_vec()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(SecretError::Command {
                operation: "set",
                status: output.status.code().unwrap_or(-1),
            })
        }
    }

    pub fn delete(&self, account: &str) -> Result<(), SecretError> {
        validate_account(account)?;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if matches!(&self.backend, Backend::Native) {
            return native_keyring::delete(&self.service, account);
        }
        let output = self.run("delete", account, None)?;
        if output.status.success() || output.status.code() == Some(1) {
            Ok(())
        } else {
            Err(SecretError::Command {
                operation: "delete",
                status: output.status.code().unwrap_or(-1),
            })
        }
    }

    fn command_spec(
        &self,
        operation: &str,
        account: &str,
    ) -> Result<(PathBuf, Vec<String>), SecretError> {
        match &self.backend {
            Backend::Fixture(command) => Ok((
                command.clone(),
                vec![operation.into(), self.service.clone(), account.into()],
            )),
            Backend::Native => {
                #[cfg(target_os = "linux")]
                {
                    return Ok((
                        PathBuf::from("secret-tool"),
                        match operation {
                            "get" => vec![
                                "lookup".into(),
                                "service".into(),
                                self.service.clone(),
                                "account".into(),
                                account.into(),
                            ],
                            "set" => vec![
                                "store".into(),
                                "--label=Vox provider credential".into(),
                                "service".into(),
                                self.service.clone(),
                                "account".into(),
                                account.into(),
                            ],
                            "delete" => vec![
                                "clear".into(),
                                "service".into(),
                                self.service.clone(),
                                "account".into(),
                            ],
                            _ => {
                                return Err(SecretError::Unavailable(
                                    "unknown keyring operation".into(),
                                ))
                            }
                        },
                    ));
                }
                #[cfg(target_os = "macos")]
                {
                    return match operation {
                        "get" => Ok((
                            PathBuf::from("/usr/bin/security"),
                            vec![
                                "find-generic-password".into(),
                                "-a".into(),
                                account.into(),
                                "-s".into(),
                                self.service.clone(),
                                "-w".into(),
                            ],
                        )),
                        // `security` only accepts a write secret as a process
                        // argument. Refuse that path instead of exposing it in
                        // a process list; a native Security.framework backend
                        // is required before macOS writes are enabled.
                        _ => Err(SecretError::Unavailable(
                            "macOS keychain writes require the native Security.framework backend"
                                .into(),
                        )),
                    };
                }
                #[cfg(target_os = "windows")]
                {
                    return Err(SecretError::Unavailable(
                        "Windows Credential Manager backend is not enabled in this build".into(),
                    ));
                }
                #[allow(unreachable_code)]
                Err(SecretError::Unavailable(
                    "no native keyring backend for this target".into(),
                ))
            }
        }
    }

    fn run(
        &self,
        operation: &'static str,
        account: &str,
        input: Option<Vec<u8>>,
    ) -> Result<CommandOutput, SecretError> {
        let (program, args) = self.command_spec(operation, account)?;
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        apply_minimal_environment(&mut command);
        let mut child = command.spawn().map_err(|error| {
            SecretError::Unavailable(format!("{}: command could not start", error.kind()))
        })?;
        let mut input_writer = input.map(|input| {
            let mut pipe = child
                .stdin
                .take()
                .expect("piped stdin must be available after spawn");
            thread::spawn(move || {
                let _ = pipe.write_all(&input);
            })
        });
        let stdout_reader = child.stdout.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                bytes
            })
        });
        let deadline = Instant::now() + COMMAND_TIMEOUT;
        loop {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                if let Some(writer) = input_writer.take() {
                    let _ = writer.join();
                }
                return Err(SecretError::Timeout);
            }
            if let Some(status) = child.try_wait()? {
                if let Some(writer) = input_writer.take() {
                    let _ = writer.join();
                }
                let stdout = stdout_reader
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default();
                return Ok(CommandOutput { status, stdout });
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn validate_account(account: &str) -> Result<(), SecretError> {
    if account.is_empty()
        || account.len() > 256
        || account
            .chars()
            .any(|character| character == '\0' || character.is_control())
    {
        return Err(SecretError::InvalidAccount);
    }
    Ok(())
}

fn apply_minimal_environment(command: &mut Command) {
    const ALLOWED: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "SystemRoot",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
    ];
    let values = ALLOWED
        .iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (*key, value)))
        .collect::<Vec<_>>();
    command.env_clear();
    for (key, value) in values {
        command.env(key, value);
    }
}

#[cfg(target_os = "macos")]
mod native_keyring {
    use super::{SecretError, MAX_SECRET_BYTES};
    use std::ffi::{c_char, c_void, CString};
    use std::ptr;

    type CfIndex = isize;
    type CfTypeRef = *const c_void;
    type CfStringRef = CfTypeRef;
    type CfDataRef = CfTypeRef;
    type CfMutableDictionaryRef = *mut c_void;
    type CfAllocatorRef = CfTypeRef;
    type CfBooleanRef = CfTypeRef;
    type OsStatus = i32;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const ERR_SEC_ITEM_NOT_FOUND: OsStatus = -25_300;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFAllocatorDefault: CfAllocatorRef;
        static kCFBooleanTrue: CfBooleanRef;
        fn CFStringCreateWithCString(
            allocator: CfAllocatorRef,
            c_string: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFDataCreate(allocator: CfAllocatorRef, bytes: *const u8, length: CfIndex) -> CfDataRef;
        fn CFDataGetBytePtr(data: CfDataRef) -> *const u8;
        fn CFDataGetLength(data: CfDataRef) -> CfIndex;
        fn CFDictionaryCreateMutable(
            allocator: CfAllocatorRef,
            capacity: CfIndex,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CfMutableDictionaryRef;
        fn CFDictionarySetValue(
            dictionary: CfMutableDictionaryRef,
            key: CfTypeRef,
            value: CfTypeRef,
        );
        fn CFRelease(value: CfTypeRef);
    }

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        static kSecClass: CfTypeRef;
        static kSecClassGenericPassword: CfTypeRef;
        static kSecAttrService: CfTypeRef;
        static kSecAttrAccount: CfTypeRef;
        static kSecValueData: CfTypeRef;
        static kSecReturnData: CfTypeRef;
        static kSecMatchLimit: CfTypeRef;
        static kSecMatchLimitOne: CfTypeRef;
        fn SecItemAdd(attributes: CfTypeRef, result: *mut CfTypeRef) -> OsStatus;
        fn SecItemCopyMatching(query: CfTypeRef, result: *mut CfTypeRef) -> OsStatus;
        fn SecItemDelete(query: CfTypeRef) -> OsStatus;
        fn SecItemUpdate(query: CfTypeRef, attributes: CfTypeRef) -> OsStatus;
    }

    struct OwnedQuery {
        dictionary: CfMutableDictionaryRef,
        service: CfStringRef,
        account: CfStringRef,
    }

    impl Drop for OwnedQuery {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.dictionary as CfTypeRef);
                CFRelease(self.service);
                CFRelease(self.account);
            }
        }
    }

    fn unavailable(operation: &str, status: OsStatus) -> SecretError {
        SecretError::Unavailable(format!("macOS Keychain {operation} failed ({status})"))
    }

    fn cf_string(value: &str) -> Result<CfStringRef, SecretError> {
        let value = CString::new(value)
            .map_err(|_| SecretError::Unavailable("Keychain value contains NUL".into()))?;
        let string = unsafe {
            CFStringCreateWithCString(
                kCFAllocatorDefault,
                value.as_ptr(),
                K_CF_STRING_ENCODING_UTF8,
            )
        };
        if string.is_null() {
            Err(SecretError::Unavailable(
                "Keychain could not allocate a string".into(),
            ))
        } else {
            Ok(string)
        }
    }

    fn query(service: &str, account: &str) -> Result<OwnedQuery, SecretError> {
        let service_ref = cf_string(service)?;
        let account_ref = match cf_string(account) {
            Ok(value) => value,
            Err(error) => {
                unsafe { CFRelease(service_ref) };
                return Err(error);
            }
        };
        let dictionary =
            unsafe { CFDictionaryCreateMutable(kCFAllocatorDefault, 4, ptr::null(), ptr::null()) };
        if dictionary.is_null() {
            unsafe {
                CFRelease(service_ref);
                CFRelease(account_ref);
            }
            return Err(SecretError::Unavailable(
                "Keychain could not allocate a query".into(),
            ));
        }
        unsafe {
            CFDictionarySetValue(dictionary, kSecClass, kSecClassGenericPassword);
            CFDictionarySetValue(dictionary, kSecAttrService, service_ref);
            CFDictionarySetValue(dictionary, kSecAttrAccount, account_ref);
        }
        Ok(OwnedQuery {
            dictionary,
            service: service_ref,
            account: account_ref,
        })
    }

    pub fn get(service: &str, account: &str) -> Result<Option<String>, SecretError> {
        let query = query(service, account)?;
        unsafe {
            CFDictionarySetValue(query.dictionary, kSecReturnData, kCFBooleanTrue);
            CFDictionarySetValue(query.dictionary, kSecMatchLimit, kSecMatchLimitOne);
        }
        let mut result = ptr::null();
        let status = unsafe { SecItemCopyMatching(query.dictionary as CfTypeRef, &mut result) };
        if status == ERR_SEC_ITEM_NOT_FOUND {
            return Ok(None);
        }
        if status != 0 {
            return Err(unavailable("read", status));
        }
        if result.is_null() {
            return Err(SecretError::InvalidText);
        }
        let length = unsafe { CFDataGetLength(result) };
        if length < 0 || length as usize > MAX_SECRET_BYTES {
            unsafe { CFRelease(result) };
            return Err(SecretError::TooLarge);
        }
        let value = if length == 0 {
            Ok(String::new())
        } else {
            let bytes =
                unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(result), length as usize) };
            String::from_utf8(bytes.to_vec()).map_err(|_| SecretError::InvalidText)
        };
        unsafe { CFRelease(result) };
        value.map(Some)
    }

    pub fn set(service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        let query = query(service, account)?;
        let data = unsafe {
            CFDataCreate(
                kCFAllocatorDefault,
                secret.as_bytes().as_ptr(),
                secret.len() as CfIndex,
            )
        };
        if data.is_null() {
            return Err(SecretError::Unavailable(
                "Keychain could not allocate secret data".into(),
            ));
        }
        let attributes =
            unsafe { CFDictionaryCreateMutable(kCFAllocatorDefault, 1, ptr::null(), ptr::null()) };
        if attributes.is_null() {
            unsafe { CFRelease(data) };
            return Err(SecretError::Unavailable(
                "Keychain could not allocate update".into(),
            ));
        }
        unsafe { CFDictionarySetValue(attributes, kSecValueData, data) };
        let mut status =
            unsafe { SecItemUpdate(query.dictionary as CfTypeRef, attributes as CfTypeRef) };
        if status == ERR_SEC_ITEM_NOT_FOUND {
            unsafe { CFDictionarySetValue(query.dictionary, kSecValueData, data) };
            status = unsafe { SecItemAdd(query.dictionary as CfTypeRef, ptr::null_mut()) };
        }
        unsafe {
            CFRelease(attributes as CfTypeRef);
            CFRelease(data);
        }
        if status == 0 {
            Ok(())
        } else {
            Err(unavailable("write", status))
        }
    }

    pub fn delete(service: &str, account: &str) -> Result<(), SecretError> {
        let query = query(service, account)?;
        let status = unsafe { SecItemDelete(query.dictionary as CfTypeRef) };
        if status == 0 || status == ERR_SEC_ITEM_NOT_FOUND {
            Ok(())
        } else {
            Err(unavailable("delete", status))
        }
    }
}

#[cfg(target_os = "windows")]
mod native_keyring {
    use super::{SecretError, MAX_SECRET_BYTES};
    use std::ffi::c_void;
    use std::ptr;

    const CRED_TYPE_GENERIC: u32 = 1;
    const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
    const ERROR_NOT_FOUND: u32 = 1168;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[repr(C)]
    struct CredentialAttribute {
        keyword: *mut u16,
        flags: u32,
        value_size: u32,
        value: *mut u8,
    }

    #[repr(C)]
    struct Credential {
        flags: u32,
        type_: u32,
        target_name: *mut u16,
        comment: *mut u16,
        last_written: FileTime,
        credential_blob_size: u32,
        credential_blob: *mut u8,
        persist: u32,
        attribute_count: u32,
        attributes: *mut CredentialAttribute,
        target_alias: *mut u16,
        user_name: *mut u16,
    }

    #[link(name = "Advapi32")]
    unsafe extern "system" {
        fn CredReadW(
            target_name: *const u16,
            type_: u32,
            flags: u32,
            credential: *mut *mut Credential,
        ) -> i32;
        fn CredWriteW(credential: *const Credential, flags: u32) -> i32;
        fn CredDeleteW(target_name: *const u16, type_: u32, flags: u32) -> i32;
        fn CredFree(buffer: *mut c_void);
    }

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetLastError() -> u32;
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn target(service: &str, account: &str) -> Vec<u16> {
        wide(&format!("Vox/{service}/{account}"))
    }

    fn unavailable(operation: &str, code: u32) -> SecretError {
        SecretError::Unavailable(format!(
            "Windows Credential Manager {operation} failed ({code})"
        ))
    }

    pub fn get(service: &str, account: &str) -> Result<Option<String>, SecretError> {
        let target = target(service, account);
        let mut raw = ptr::null_mut();
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
        if ok == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(unavailable("read", code));
        }
        if raw.is_null() {
            return Err(SecretError::InvalidText);
        }
        let credential = unsafe { &*raw };
        let length = credential.credential_blob_size as usize;
        let result = if length > MAX_SECRET_BYTES
            || (length > 0 && credential.credential_blob.is_null())
        {
            Err(SecretError::TooLarge)
        } else if length == 0 {
            Ok(String::new())
        } else {
            let bytes = unsafe { std::slice::from_raw_parts(credential.credential_blob, length) };
            String::from_utf8(bytes.to_vec()).map_err(|_| SecretError::InvalidText)
        };
        unsafe { CredFree(raw as *mut c_void) };
        result.map(Some)
    }

    pub fn set(service: &str, account: &str, secret: &str) -> Result<(), SecretError> {
        let target = target(service, account);
        let mut username = wide(account);
        let mut blob = secret.as_bytes().to_vec();
        let credential = Credential {
            flags: 0,
            type_: CRED_TYPE_GENERIC,
            target_name: target.as_ptr() as *mut u16,
            comment: ptr::null_mut(),
            last_written: FileTime { low: 0, high: 0 },
            credential_blob_size: blob.len() as u32,
            credential_blob: blob.as_mut_ptr(),
            persist: CRED_PERSIST_LOCAL_MACHINE,
            attribute_count: 0,
            attributes: ptr::null_mut(),
            target_alias: ptr::null_mut(),
            user_name: username.as_mut_ptr(),
        };
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(unavailable("write", unsafe { GetLastError() }));
        }
        Ok(())
    }

    pub fn delete(service: &str, account: &str) -> Result<(), SecretError> {
        let target = target(service, account);
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok != 0 {
            return Ok(());
        }
        let code = unsafe { GetLastError() };
        if code == ERROR_NOT_FOUND {
            Ok(())
        } else {
            Err(unavailable("delete", code))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    #[test]
    fn keyring_fixture_round_trip_never_writes_to_the_session_store() {
        let root = std::env::temp_dir().join(format!(
            "vox-secrets-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let state = root.join("secret.state");
        let fixture = root.join("keyring-fixture.sh");
        fs::write(
            &fixture,
            format!(
                "#!/bin/sh\ncase \"$1\" in\nget) test -f '{0}' && cat '{0}' || exit 1 ;;\nset) cat > '{0}' ;;\ndelete) rm -f '{0}' ;;\nesac\n",
                state.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&fixture).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(&fixture, permissions).unwrap();

        let store = SecretStore::with_command("vox-test", fixture);
        assert_eq!(store.backend_name(), "fixture");
        assert_eq!(store.get("provider").unwrap(), None);
        store.set("provider", "secret-value").unwrap();
        assert_eq!(
            store.get("provider").unwrap().as_deref(),
            Some("secret-value")
        );
        store.delete("provider").unwrap();
        assert_eq!(store.get("provider").unwrap(), None);
        assert!(matches!(
            store.get("bad\naccount"),
            Err(SecretError::InvalidAccount)
        ));
        assert!(matches!(
            store.set("provider", &"x".repeat(MAX_SECRET_BYTES + 1)),
            Err(SecretError::TooLarge)
        ));
        let _ = fs::remove_dir_all(root);
    }
}
