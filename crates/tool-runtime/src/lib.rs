use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("path is outside the configured scope")]
    OutOfScope,
    #[error("file operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("program failed to start: {0}")]
    Spawn(String),
    #[error("application is not available: {0}")]
    ApplicationUnavailable(String),
    #[error("program timed out")]
    Timeout,
    #[error("program cancelled")]
    Cancelled,
    #[error("program exited with status {0}")]
    Exit(i32),
    #[error("clipboard content exceeds the configured size limit")]
    ClipboardTooLarge,
    #[error("accessibility backend unavailable: {0}")]
    AccessibilityUnavailable(String),
    #[error("destination already exists; explicit overwrite approval is required")]
    DestinationExists,
    #[error("directory is not available: {0}")]
    DirectoryUnavailable(String),
    #[error("process is not available or identity did not match: {0}")]
    ProcessUnavailable(String),
    #[error("refusing to terminate the Vox process")]
    RefuseOwnProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResult {
    pub status: String,
    pub data: serde_json::Value,
    pub error_code: Option<String>,
    pub retryable: bool,
    pub side_effect: String,
    pub duration_ms: u64,
    pub truncated: bool,
    pub verification: Option<serde_json::Value>,
}

impl ToolResult {
    fn success(data: serde_json::Value, side_effect: &str, started: Instant) -> Self {
        Self {
            status: "success".into(),
            data,
            error_code: None,
            retryable: false,
            side_effect: side_effect.into(),
            duration_ms: started.elapsed().as_millis() as u64,
            truncated: false,
            verification: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub allowed_roots: Vec<PathBuf>,
    pub max_output_bytes: usize,
    pub xa11y_command: Option<PathBuf>,
    pub clipboard_read_command: Option<PathBuf>,
    pub clipboard_write_command: Option<PathBuf>,
    pub native_access: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            allowed_roots: vec![std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))],
            max_output_bytes: 64 * 1024,
            xa11y_command: None,
            clipboard_read_command: None,
            clipboard_write_command: None,
            native_access: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolRuntime {
    config: RuntimeConfig,
}

#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // The child becomes the leader of a private process group so timeout and
    // cancellation do not leave shell grandchildren behind.
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut Command) {}

fn terminate_process_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        // A negative pid targets the process group created in
        // `isolate_process_group`; failure is harmless because the direct
        // child fallback below still runs.
        unsafe {
            let _ = libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn apply_minimal_environment(command: &mut Command) {
    const ALLOWED: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "LANG",
        "LC_ALL",
        "TMPDIR",
        "TEMP",
        "TMP",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "XDG_SESSION_TYPE",
        "XDG_CURRENT_DESKTOP",
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

fn is_cross_device(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(18)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub kind: String,
    pub bytes: u64,
    pub modified_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessEntry {
    pub pid: u32,
    pub command: String,
    pub start_time: Option<u64>,
}

#[cfg(unix)]
fn read_linux_start_time(pid: u32) -> Option<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = stat.rsplit_once(") ")?.1;
    after_name
        .split_whitespace()
        .nth(19)
        .and_then(|value| value.parse().ok())
}

#[cfg(not(unix))]
fn read_linux_start_time(_pid: u32) -> Option<u64> {
    None
}

impl ToolRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        let allowed_roots = config
            .allowed_roots
            .iter()
            .filter_map(|root| root.canonicalize().ok())
            .collect();
        Self {
            config: RuntimeConfig {
                allowed_roots,
                ..config
            },
        }
    }

    pub fn validate_path(&self, path: impl AsRef<Path>) -> Result<PathBuf, ToolError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() || path.to_string_lossy().contains('\0') {
            return Err(ToolError::InvalidPath(path.display().to_string()));
        }
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_err(ToolError::Io)?.join(path)
        };
        let normalized = if absolute.exists() {
            absolute.canonicalize().map_err(ToolError::Io)?
        } else {
            let parent = absolute
                .parent()
                .ok_or_else(|| ToolError::InvalidPath(absolute.display().to_string()))?;
            let parent = parent.canonicalize().map_err(ToolError::Io)?;
            parent.join(
                absolute
                    .file_name()
                    .ok_or_else(|| ToolError::InvalidPath(absolute.display().to_string()))?,
            )
        };
        if self
            .config
            .allowed_roots
            .iter()
            .any(|root| normalized.starts_with(root))
        {
            Ok(normalized)
        } else {
            Err(ToolError::OutOfScope)
        }
    }

    pub fn read_file(&self, path: impl AsRef<Path>) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let path = self.validate_path(path)?;
        let file = fs::File::open(&path)?;
        let mut bytes = Vec::new();
        file.take((self.config.max_output_bytes + 1) as u64)
            .read_to_end(&mut bytes)?;
        let truncated = bytes.len() > self.config.max_output_bytes;
        if truncated {
            bytes.truncate(self.config.max_output_bytes);
        }
        let text = std::str::from_utf8(&bytes).ok();
        let mut result = ToolResult::success(
            serde_json::json!({
                "path":path,
                "encoding": if text.is_some() { "utf-8" } else { "binary" },
                "content": text,
                "bytes": bytes.len()
            }),
            "none",
            started,
        );
        result.truncated = truncated;
        result.verification = Some(serde_json::json!({"path_exists": true, "bytes": bytes.len()}));
        Ok(result)
    }

    pub fn list_dir(
        &self,
        path: impl AsRef<Path>,
        max_entries: usize,
    ) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let directory = self.validate_path(path)?;
        if !directory.is_dir() {
            return Err(ToolError::DirectoryUnavailable(
                directory.display().to_string(),
            ));
        }
        let limit = max_entries.clamp(1, 10_000);
        let mut entries = Vec::new();
        let mut truncated = false;
        for item in fs::read_dir(&directory)? {
            if entries.len() >= limit {
                truncated = true;
                break;
            }
            let item = item?;
            let metadata = item.metadata()?;
            entries.push(FileEntry {
                path: item.path().display().to_string(),
                kind: if metadata.is_dir() {
                    "directory"
                } else {
                    "file"
                }
                .into(),
                bytes: metadata.len(),
                modified_unix: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs()),
            });
        }
        let mut result = ToolResult::success(
            serde_json::json!({"path":directory,"entries":entries}),
            "none",
            started,
        );
        result.truncated = truncated;
        result.verification = Some(serde_json::json!({
            "directory_exists": true,
            "entry_count": result.data["entries"].as_array().map_or(0, Vec::len)
        }));
        Ok(result)
    }

    pub fn search_files(
        &self,
        root: impl AsRef<Path>,
        query: &str,
        max_entries: usize,
    ) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let root = self.validate_path(root)?;
        if !root.is_dir() {
            return Err(ToolError::DirectoryUnavailable(root.display().to_string()));
        }
        let query = query.to_lowercase();
        let limit = max_entries.clamp(1, 10_000);
        let mut matches = Vec::new();
        let mut pending = vec![root.clone()];
        let mut truncated = false;
        while let Some(directory) = pending.pop() {
            for item in fs::read_dir(&directory)? {
                let item = item?;
                let path = item.path();
                let name = item.file_name().to_string_lossy().to_lowercase();
                let metadata = item.metadata()?;
                if name.contains(&query) {
                    matches.push(FileEntry {
                        path: path.display().to_string(),
                        kind: if metadata.is_dir() {
                            "directory"
                        } else {
                            "file"
                        }
                        .into(),
                        bytes: metadata.len(),
                        modified_unix: metadata
                            .modified()
                            .ok()
                            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|duration| duration.as_secs()),
                    });
                    if matches.len() >= limit {
                        truncated = true;
                        break;
                    }
                }
                if metadata.is_dir() && !path.is_symlink() {
                    pending.push(path);
                }
            }
            if truncated {
                break;
            }
        }
        let mut result = ToolResult::success(
            serde_json::json!({"root":root,"query":query,"matches":matches}),
            "none",
            started,
        );
        result.truncated = truncated;
        result.verification = Some(serde_json::json!({
            "root_exists": true,
            "match_count": result.data["matches"].as_array().map_or(0, Vec::len)
        }));
        Ok(result)
    }

    pub fn write_file(
        &self,
        path: impl AsRef<Path>,
        content: &str,
        overwrite: bool,
    ) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let path = self.validate_path(path)?;
        if path.exists() && !overwrite {
            return Err(ToolError::DestinationExists);
        }
        let parent = path
            .parent()
            .ok_or_else(|| ToolError::InvalidPath(path.display().to_string()))?;
        fs::create_dir_all(parent)?;
        let temporary = path.with_extension(format!("vox-{}.tmp", std::process::id()));
        {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(content.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&temporary, &path)?;
        let mut result = ToolResult::success(
            serde_json::json!({"path":path,"bytes":content.len()}),
            "applied",
            started,
        );
        result.verification =
            Some(serde_json::json!({"path_exists": true, "bytes": content.len()}));
        Ok(result)
    }

    pub fn move_path(
        &self,
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        overwrite: bool,
    ) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let source = self.validate_path(source)?;
        let destination = self.validate_path(destination)?;
        if !source.exists() {
            return Err(ToolError::InvalidPath(source.display().to_string()));
        }
        if destination.exists() && !overwrite {
            return Err(ToolError::DestinationExists);
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        if overwrite && destination.exists() {
            if destination.is_dir() {
                return Err(ToolError::DestinationExists);
            }
            fs::remove_file(&destination)?;
        }
        let mut cross_device = false;
        match fs::rename(&source, &destination) {
            Ok(()) => {}
            Err(error) if is_cross_device(&error) => {
                if source.is_dir() {
                    return Err(ToolError::Io(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "cross-device directory moves are not atomic and are not supported",
                    )));
                }
                fs::copy(&source, &destination)?;
                fs::File::open(&destination)?.sync_all()?;
                fs::remove_file(&source)?;
                cross_device = true;
            }
            Err(error) => return Err(ToolError::Io(error)),
        }
        let mut result = ToolResult::success(
            serde_json::json!({"source":source,"destination":destination}),
            "applied",
            started,
        );
        result.verification = Some(serde_json::json!({
            "source_exists": false,
            "destination_exists": true,
            "atomic": !cross_device,
            "cross_device_fallback": cross_device
        }));
        Ok(result)
    }

    pub fn list_processes(&self, max_entries: usize) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let limit = max_entries.clamp(1, 10_000);
        let mut processes = Vec::new();
        #[cfg(unix)]
        {
            for item in fs::read_dir("/proc").map_err(ToolError::Io)? {
                let item = item?;
                let name = item.file_name().to_string_lossy().into_owned();
                if !name.chars().all(|character| character.is_ascii_digit()) {
                    continue;
                }
                let pid = match name.parse::<u32>() {
                    Ok(pid) => pid,
                    Err(_) => continue,
                };
                let command = fs::read_to_string(item.path().join("comm"))
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                processes.push(ProcessEntry {
                    pid,
                    command,
                    start_time: read_linux_start_time(pid),
                });
                if processes.len() >= limit {
                    break;
                }
            }
        }
        #[cfg(windows)]
        {
            let output = Command::new("tasklist")
                .args(["/FO", "CSV", "/NH"])
                .output()
                .map_err(|error| ToolError::Spawn(error.to_string()))?;
            for line in String::from_utf8_lossy(&output.stdout).lines().take(limit) {
                let fields = line.trim_matches('\"').split("\",\"").collect::<Vec<_>>();
                if let (Some(command), Some(pid)) = (fields.first(), fields.get(1)) {
                    if let Ok(pid) = pid.replace(',', "").parse::<u32>() {
                        processes.push(ProcessEntry {
                            pid,
                            command: (*command).into(),
                            start_time: None,
                        });
                    }
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        return Err(ToolError::ProcessUnavailable(
            "process listing is unsupported on this target".into(),
        ));
        let mut result =
            ToolResult::success(serde_json::json!({"processes":processes}), "none", started);
        result.verification = Some(serde_json::json!({"observed":true,"identity":"pid+command"}));
        Ok(result)
    }

    pub fn terminate_process(
        &self,
        pid: u32,
        expected_command: Option<&str>,
    ) -> Result<ToolResult, ToolError> {
        self.terminate_process_with_identity(pid, expected_command, None)
    }

    pub fn terminate_process_with_identity(
        &self,
        pid: u32,
        expected_command: Option<&str>,
        expected_start_time: Option<u64>,
    ) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        if pid == std::process::id() {
            return Err(ToolError::RefuseOwnProcess);
        }
        let processes = self
            .list_processes(10_000)?
            .data
            .get("processes")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let process = processes
            .iter()
            .find(|process| {
                process.get("pid").and_then(serde_json::Value::as_u64) == Some(pid as u64)
            })
            .ok_or_else(|| ToolError::ProcessUnavailable(pid.to_string()))?;
        let command = process
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if let Some(expected) = expected_command {
            if expected.is_empty() || command != expected {
                return Err(ToolError::ProcessUnavailable(format!(
                    "pid identity mismatch: expected {expected}, observed {command}"
                )));
            }
        }
        let observed_start_time = process
            .get("start_time")
            .and_then(serde_json::Value::as_u64);
        if let Some(expected) = expected_start_time {
            if observed_start_time != Some(expected) {
                return Err(ToolError::ProcessUnavailable(format!(
                    "pid start-time mismatch: expected {expected}, observed {observed_start_time:?}"
                )));
            }
        }
        #[cfg(unix)]
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|error| ToolError::Spawn(error.to_string()))?;
        #[cfg(windows)]
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T"])
            .status()
            .map_err(|error| ToolError::Spawn(error.to_string()))?;
        #[cfg(not(any(unix, windows)))]
        return Err(ToolError::ProcessUnavailable(
            "process termination is unsupported on this target".into(),
        ));
        if !status.success() {
            return Err(ToolError::Exit(status.code().unwrap_or(-1)));
        }
        let deadline = Instant::now() + Duration::from_millis(500);
        let mut observed_absent = false;
        while Instant::now() < deadline {
            let still_present = self
                .list_processes(10_000)
                .ok()
                .and_then(|result| result.data.get("processes").cloned())
                .and_then(|value| value.as_array().cloned())
                .is_some_and(|entries| {
                    entries.iter().any(|entry| {
                        entry.get("pid").and_then(serde_json::Value::as_u64) == Some(pid as u64)
                    })
                });
            if !still_present {
                observed_absent = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let mut result = ToolResult::success(
            serde_json::json!({"pid":pid,"command":command,"start_time":observed_start_time}),
            if observed_absent {
                "applied"
            } else {
                "unknown"
            },
            started,
        );
        result.verification = Some(serde_json::json!({
            "termination_requested": true,
            "observed_absent": observed_absent,
            "postcondition": "process absence observed within deadline"
        }));
        Ok(result)
    }

    pub fn launch_app(&self, program: &str, args: &[String]) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        if program.trim().is_empty() {
            return Err(ToolError::Spawn("program is empty".into()));
        }
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        apply_minimal_environment(&mut command);
        let child = command
            .spawn()
            .map_err(|error| ToolError::Spawn(error.to_string()))?;
        let mut result = ToolResult::success(
            serde_json::json!({"program":program,"pid":child.id()}),
            "unknown",
            started,
        );
        result.verification = Some(serde_json::json!({
            "spawned": true,
            "observed": false,
            "note": "spawn is not proof that the application opened a usable window"
        }));
        Ok(result)
    }

    pub fn resolve_app(&self, name: &str) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let normalized = name.trim().to_lowercase();
        if normalized.is_empty()
            || normalized.len() > 128
            || normalized
                .chars()
                .any(|character| character == '\0' || character.is_control())
        {
            return Err(ToolError::ApplicationUnavailable(
                "application name is invalid".into(),
            ));
        }
        let candidates = match normalized.as_str() {
            "visual studio code" | "vscode" => vec!["code".to_owned()],
            "chrome" | "google chrome" => vec![
                "google-chrome".to_owned(),
                "google-chrome-stable".to_owned(),
                "chromium".to_owned(),
                "chromium-browser".to_owned(),
            ],
            "discord" => vec!["discord".to_owned(), "Discord".to_owned()],
            "firefox" => vec!["firefox".to_owned()],
            _ => vec![name.trim().to_owned()],
        };
        for candidate in candidates {
            if let Some(path) = executable_from_path(&candidate) {
                let mut result = ToolResult::success(
                    serde_json::json!({
                        "name": name,
                        "program": path,
                        "argv": [],
                        "resolved": true
                    }),
                    "none",
                    started,
                );
                result.verification = Some(serde_json::json!({"resolved":true,"observed":true}));
                return Ok(result);
            }
        }
        Err(ToolError::ApplicationUnavailable(format!(
            "{name} was not found in the executable catalog"
        )))
    }

    pub fn open_path(&self, path: impl AsRef<Path>) -> Result<ToolResult, ToolError> {
        let path = self.validate_path(path)?;
        let target = path.to_string_lossy().into_owned();
        #[cfg(target_os = "linux")]
        let (program, args) = ("xdg-open", vec![target]);
        #[cfg(target_os = "macos")]
        let (program, args) = ("open", vec![target]);
        #[cfg(target_os = "windows")]
        let (program, args) = ("explorer", vec![target]);
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        return Err(ToolError::Spawn(
            "opening paths is unsupported on this target".into(),
        ));
        self.launch_app(program, &args)
    }

    pub fn list_windows(&self) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        if self.config.native_access {
            match vox_desktop_access::list_native_windows() {
                Ok(windows) => {
                    let count = windows.len();
                    let mut result = ToolResult::success(
                        serde_json::json!({"windows": windows, "count": count}),
                        "none",
                        started,
                    );
                    result.verification = Some(serde_json::json!({
                        "observed": true,
                        "backend": "xa11y-rust",
                        "fresh": true
                    }));
                    return Ok(result);
                }
                Err(error) if self.config.xa11y_command.is_none() => {
                    return Err(ToolError::AccessibilityUnavailable(error));
                }
                Err(_) => {}
            }
        }
        let command = self
            .config
            .xa11y_command
            .clone()
            .unwrap_or_else(|| PathBuf::from("xa11y"));
        let output = Command::new(&command)
            .arg("windows")
            .output()
            .map_err(|error| {
                ToolError::AccessibilityUnavailable(format!("{}: {error}", command.display()))
            })?;
        if !output.status.success() {
            return Err(ToolError::AccessibilityUnavailable(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        let mut result =
            ToolResult::success(serde_json::json!({"windows_text": text}), "none", started);
        result.verification = Some(serde_json::json!({"observed": true, "backend": "xa11y"}));
        Ok(result)
    }

    fn clipboard_read_spec(&self) -> Result<(String, Vec<String>), ToolError> {
        if let Some(command) = &self.config.clipboard_read_command {
            return Ok((command.to_string_lossy().into_owned(), Vec::new()));
        }
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return Ok(("wl-paste".into(), vec!["--no-newline".into()]));
            }
            return Ok((
                "xclip".into(),
                vec!["-selection".into(), "clipboard".into(), "-o".into()],
            ));
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(("pbpaste".into(), Vec::new()));
        }
        #[cfg(target_os = "windows")]
        {
            return Ok((
                "powershell.exe".into(),
                vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    "Get-Clipboard -Raw".into(),
                ],
            ));
        }
        #[allow(unreachable_code)]
        Err(ToolError::AccessibilityUnavailable(
            "clipboard is unsupported on this target".into(),
        ))
    }

    fn clipboard_write_spec(&self) -> Result<(String, Vec<String>), ToolError> {
        if let Some(command) = &self.config.clipboard_write_command {
            return Ok((command.to_string_lossy().into_owned(), Vec::new()));
        }
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return Ok(("wl-copy".into(), Vec::new()));
            }
            return Ok((
                "xclip".into(),
                vec!["-selection".into(), "clipboard".into()],
            ));
        }
        #[cfg(target_os = "macos")]
        {
            return Ok(("pbcopy".into(), Vec::new()));
        }
        #[cfg(target_os = "windows")]
        {
            return Ok((
                "powershell.exe".into(),
                vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-Command".into(),
                    "$input | Set-Clipboard".into(),
                ],
            ));
        }
        #[allow(unreachable_code)]
        Err(ToolError::AccessibilityUnavailable(
            "clipboard is unsupported on this target".into(),
        ))
    }

    pub fn clipboard_read(&self) -> Result<ToolResult, ToolError> {
        self.clipboard_read_with_cancel(&AtomicBool::new(false))
    }

    pub fn clipboard_read_with_cancel(&self, cancel: &AtomicBool) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let (program, args) = self.clipboard_read_spec()?;
        let command_result =
            self.execute_argv(&program, &args, None, Duration::from_secs(2), cancel)?;
        let text = command_result
            .data
            .get("stdout")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let mut result = ToolResult::success(
            serde_json::json!({
                "text": text,
                "mime": "text/plain",
                "bytes": text.len(),
                "backend": program
            }),
            "none",
            started,
        );
        result.truncated = command_result.truncated;
        result.verification = Some(serde_json::json!({
            "observed": true,
            "backend": "clipboard",
            "mime": "text/plain"
        }));
        Ok(result)
    }

    pub fn clipboard_write(&self, text: &str) -> Result<ToolResult, ToolError> {
        self.clipboard_write_with_cancel(text, &AtomicBool::new(false))
    }

    pub fn clipboard_write_with_cancel(
        &self,
        text: &str,
        cancel: &AtomicBool,
    ) -> Result<ToolResult, ToolError> {
        if text.len() > self.config.max_output_bytes {
            return Err(ToolError::ClipboardTooLarge);
        }
        let started = Instant::now();
        let (program, args) = self.clipboard_write_spec()?;
        self.execute_argv_with_input(
            &program,
            &args,
            None,
            Duration::from_secs(2),
            cancel,
            Some(text.as_bytes().to_vec()),
        )?;
        let mut result = ToolResult::success(
            serde_json::json!({
                "bytes": text.len(),
                "mime": "text/plain",
                "backend": program
            }),
            "applied",
            started,
        );
        result.verification = Some(serde_json::json!({
            "observed": true,
            "backend": "clipboard",
            "mime": "text/plain"
        }));
        Ok(result)
    }

    pub fn execute_argv(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
        timeout: Duration,
        cancel: &AtomicBool,
    ) -> Result<ToolResult, ToolError> {
        self.execute_argv_with_input(program, args, cwd, timeout, cancel, None)
    }

    fn execute_argv_with_input(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
        timeout: Duration,
        cancel: &AtomicBool,
        input: Option<Vec<u8>>,
    ) -> Result<ToolResult, ToolError> {
        let started = Instant::now();
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        apply_minimal_environment(&mut command);
        isolate_process_group(&mut command);
        if let Some(cwd) = cwd {
            command.current_dir(self.validate_path(cwd)?);
        }
        let mut child = command
            .spawn()
            .map_err(|error| ToolError::Spawn(error.to_string()))?;
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
        let stderr_reader = child.stderr.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = pipe.read_to_end(&mut bytes);
                bytes
            })
        });
        let deadline = Instant::now() + timeout;
        loop {
            if cancel.load(Ordering::Relaxed) {
                terminate_process_tree(&mut child);
                if let Some(writer) = input_writer.take() {
                    let _ = writer.join();
                }
                return Err(ToolError::Cancelled);
            }
            if Instant::now() >= deadline {
                terminate_process_tree(&mut child);
                if let Some(writer) = input_writer.take() {
                    let _ = writer.join();
                }
                return Err(ToolError::Timeout);
            }
            if let Some(status) = child.try_wait()? {
                if let Some(writer) = input_writer.take() {
                    let _ = writer.join();
                }
                let mut stdout = stdout_reader
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default();
                let stderr = stderr_reader
                    .and_then(|reader| reader.join().ok())
                    .unwrap_or_default();
                let mut truncated = false;
                stdout.truncate(self.config.max_output_bytes);
                if stdout.len() >= self.config.max_output_bytes {
                    truncated = true;
                }
                let data = serde_json::json!({"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"exit_code":status.code()});
                if !status.success() {
                    return Err(ToolError::Exit(status.code().unwrap_or(-1)));
                }
                let mut result = ToolResult::success(data, "unknown", started);
                result.truncated = truncated;
                result.verification =
                    Some(serde_json::json!({"exit_code": status.code().unwrap_or(-1)}));
                return Ok(result);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn executable_from_path(program: &str) -> Option<PathBuf> {
    let candidate = Path::new(program);
    if candidate.is_absolute() {
        return is_executable(candidate).then(|| candidate.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    #[test]
    fn a04_paths_outside_scope_and_traversal_are_rejected() {
        let root = std::env::temp_dir().join(format!("vox-tool-runtime-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("a.txt");
        fs::write(&path, "hello").unwrap();
        let runtime = ToolRuntime::new(RuntimeConfig {
            allowed_roots: vec![root.clone()],
            ..RuntimeConfig::default()
        });
        assert_eq!(runtime.read_file(&path).unwrap().data["content"], "hello");
        assert!(matches!(
            runtime.validate_path("/etc/passwd"),
            Err(ToolError::OutOfScope)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn executes_argv_without_shell_interpolation() {
        let runtime = ToolRuntime::new(RuntimeConfig::default());
        let result = runtime
            .execute_argv(
                "printf",
                &["safe".into()],
                None,
                Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(result.data["stdout"], "safe");
    }

    #[test]
    fn app_resolution_uses_the_executable_catalog_without_spawning() {
        let runtime = ToolRuntime::new(RuntimeConfig::default());
        let result = runtime.resolve_app("sh").unwrap();
        assert_eq!(result.data["resolved"], true);
        assert!(result.data["program"].as_str().is_some());
        assert!(matches!(
            runtime.resolve_app("vox-command-that-does-not-exist"),
            Err(ToolError::ApplicationUnavailable(_))
        ));
    }

    #[test]
    fn a05_file_write_move_and_collision_boundaries_are_observable() {
        let root = std::env::temp_dir().join(format!("vox-file-runtime-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let runtime = ToolRuntime::new(RuntimeConfig {
            allowed_roots: vec![root.clone()],
            ..RuntimeConfig::default()
        });
        let source = root.join("draft.txt");
        runtime.write_file(&source, "draft", false).unwrap();
        assert_eq!(
            runtime.search_files(&root, "draft", 10).unwrap().data["matches"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let destination = root.join("final.txt");
        runtime.move_path(&source, &destination, false).unwrap();
        assert_eq!(fs::read_to_string(destination).unwrap(), "draft");
        let existing = root.join("existing.txt");
        fs::write(&existing, "keep").unwrap();
        let another = root.join("another.txt");
        fs::write(&another, "new").unwrap();
        assert!(matches!(
            runtime.move_path(&another, &existing, false),
            Err(ToolError::DestinationExists)
        ));
        assert_eq!(fs::read_to_string(existing).unwrap(), "keep");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a07_process_identity_contains_pid_command_and_start_time() {
        let runtime = ToolRuntime::new(RuntimeConfig::default());
        let result = runtime.list_processes(10).unwrap();
        let processes = result.data["processes"].as_array().unwrap();
        assert!(processes.iter().all(|process| {
            process["pid"].as_u64().is_some()
                && process["command"].is_string()
                && process.get("start_time").is_some()
        }));
    }

    #[cfg(unix)]
    #[test]
    fn termination_checks_pid_start_time_when_supplied() {
        let runtime = ToolRuntime::new(RuntimeConfig::default());
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let entry = (0..20)
            .find_map(|_| {
                let processes = runtime.list_processes(10_000).unwrap().data["processes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                processes.into_iter().find(|process| {
                    process["pid"].as_u64() == Some(pid as u64)
                        && process["start_time"].as_u64().is_some()
                })
            })
            .unwrap_or_else(|| panic!("spawned process {pid} was not observable"));
        let start_time = entry["start_time"].as_u64().unwrap();
        let result = runtime
            .terminate_process_with_identity(pid, Some("sleep"), Some(start_time))
            .unwrap();
        assert_eq!(result.data["start_time"], start_time);
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn a07_pid_identity_mismatch_never_terminates_the_process() {
        let runtime = ToolRuntime::new(RuntimeConfig::default());
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let entry = (0..20)
            .find_map(|_| {
                let processes = runtime.list_processes(10_000).unwrap().data["processes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                processes.into_iter().find(|process| {
                    process["pid"].as_u64() == Some(pid as u64)
                        && process["start_time"].as_u64().is_some()
                })
            })
            .unwrap_or_else(|| panic!("spawned process {pid} was not observable"));
        let start_time = entry["start_time"].as_u64().unwrap();
        let result = runtime.terminate_process_with_identity(
            pid,
            Some("sleep"),
            Some(start_time.saturating_add(1)),
        );
        assert!(
            matches!(result, Err(ToolError::ProcessUnavailable(message)) if message.contains("start-time mismatch"))
        );
        assert!(runtime.list_processes(10_000).unwrap().data["processes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|process| process["pid"].as_u64() == Some(pid as u64)));
        child.kill().unwrap();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn a04_symlink_to_outside_scope_is_rejected() {
        let root = std::env::temp_dir().join(format!("vox-symlink-runtime-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let link = root.join("outside");
        std::os::unix::fs::symlink("/etc/passwd", &link).unwrap();
        let runtime = ToolRuntime::new(RuntimeConfig {
            allowed_roots: vec![root.clone()],
            ..RuntimeConfig::default()
        });
        assert!(matches!(
            runtime.read_file(&link),
            Err(ToolError::OutOfScope)
        ));
        let _ = fs::remove_file(link);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn a08_large_output_is_drained_and_marked_truncated() {
        let runtime = ToolRuntime::new(RuntimeConfig {
            max_output_bytes: 1024,
            ..RuntimeConfig::default()
        });
        let result = runtime
            .execute_argv(
                "sh",
                &["-c".into(), "head -c 200000 /dev/zero".into()],
                None,
                Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(result.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn a11_clipboard_text_backend_is_bounded_and_verifiable() {
        let root = std::env::temp_dir().join(format!(
            "vox-tool-clipboard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let read_script = root.join("clipboard-read.sh");
        let write_script = root.join("clipboard-write.sh");
        let output_path = root.join("clipboard.txt");
        fs::write(&read_script, "#!/bin/sh\nprintf 'fixture text'\n").unwrap();
        fs::write(
            &write_script,
            format!("#!/bin/sh\ncat > '{}'\n", output_path.display()),
        )
        .unwrap();
        for script in [&read_script, &write_script] {
            let mut permissions = fs::metadata(script).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
            fs::set_permissions(script, permissions).unwrap();
        }

        let runtime = ToolRuntime::new(RuntimeConfig {
            allowed_roots: vec![root.clone()],
            clipboard_read_command: Some(read_script),
            clipboard_write_command: Some(write_script),
            max_output_bytes: 64,
            ..RuntimeConfig::default()
        });
        let read = runtime.clipboard_read().unwrap();
        assert_eq!(read.data["text"], "fixture text");
        assert_eq!(read.data["mime"], "text/plain");
        assert_eq!(read.verification.as_ref().unwrap()["observed"], true);

        let write = runtime.clipboard_write("written text").unwrap();
        assert_eq!(write.side_effect, "applied");
        assert_eq!(fs::read_to_string(&output_path).unwrap(), "written text");
        assert_eq!(write.verification.as_ref().unwrap()["observed"], true);

        let bounded = ToolRuntime::new(RuntimeConfig {
            allowed_roots: vec![root.clone()],
            clipboard_write_command: Some(root.join("clipboard-write.sh")),
            max_output_bytes: 4,
            ..RuntimeConfig::default()
        });
        assert!(matches!(
            bounded.clipboard_write("12345"),
            Err(ToolError::ClipboardTooLarge)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_terminates_the_private_process_group() {
        let runtime = ToolRuntime::new(RuntimeConfig::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancel);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            trigger.store(true, Ordering::Relaxed);
        });
        let result = runtime.execute_argv(
            "sh",
            &["-c".into(), "sleep 30".into()],
            None,
            Duration::from_secs(3),
            &cancel,
        );
        thread.join().unwrap();
        assert!(matches!(result, Err(ToolError::Cancelled)));
    }
}
