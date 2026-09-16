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
