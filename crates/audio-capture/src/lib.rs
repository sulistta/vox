use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use thiserror::Error;

const DEFAULT_SAMPLE_RATE: u32 = 16_000;
const DEFAULT_CHANNELS: u16 = 1;
const BYTES_PER_SAMPLE: usize = 2;
const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    pub command: Option<PathBuf>,
    pub arguments: Vec<String>,
    pub sample_rate: u32,
    pub channels: u16,
    pub max_duration: Duration,
    pub max_bytes: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            command: None,
            arguments: Vec::new(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
            max_duration: Duration::from_secs(60),
            max_bytes: DEFAULT_SAMPLE_RATE as usize
                * DEFAULT_CHANNELS as usize
                * BYTES_PER_SAMPLE
                * 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureResult {
    pub audio: Vec<u8>,
    pub sample_rate: u32,
    pub channels: u16,
    pub elapsed: Duration,
    pub peak_level: u16,
    pub truncated: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CaptureError {
    #[error("voice capture is already active")]
    AlreadyCapturing,
    #[error("voice capture is not active")]
    NotCapturing,
    #[error("audio capture backend is unavailable: {0}")]
    BackendUnavailable(String),
    #[error("microphone permission was denied: {0}")]
    PermissionDenied(String),
    #[error("audio capture exceeded its maximum duration")]
    MaxDuration,
    #[error("audio capture was cancelled")]
    Cancelled,
    #[error("no audio was captured")]
    EmptyAudio,
    #[error("audio capture failed: {0}")]
    Io(String),
}

/// A native program that can provide bounded PCM audio to `AudioCapture`.
///
/// Detecting a backend only inspects the executable search path. It never
/// opens a microphone or requests a permission, so callers can use it to
/// explain availability in their UI before an explicit push-to-talk action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureBackend {
    PipeWire,
    Alsa,
}

impl CaptureBackend {
    pub fn label(self) -> &'static str {
        match self {
            Self::PipeWire => "PipeWire",
            Self::Alsa => "ALSA",
        }
    }
}

/// Return the preferred platform capture backend without starting it.
pub fn native_capture_backend() -> Result<CaptureBackend, CaptureError> {
    #[cfg(target_os = "linux")]
    {
        if executable_available("pw-record") {
            return Ok(CaptureBackend::PipeWire);
        }
        if executable_available("arecord") {
            return Ok(CaptureBackend::Alsa);
        }
    }
    Err(CaptureError::BackendUnavailable(
        "no supported native capture backend was found".into(),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopReason {
    User,
    Cancelled,
}

struct ReaderOutput {
    bytes: Vec<u8>,
    truncated: bool,
    stderr: String,
    read_error: Option<String>,
}

pub struct AudioCapture {
    config: CaptureConfig,
    child: Arc<Mutex<Option<Child>>>,
    reader: Option<JoinHandle<ReaderOutput>>,
    started_at: Option<Instant>,
    watchdog_cancel: Arc<AtomicBool>,
    watchdog: Option<JoinHandle<()>>,
    max_duration_hit: Arc<AtomicBool>,
}

impl AudioCapture {
    pub fn new(config: CaptureConfig) -> Self {
        Self {
            config,
            child: Arc::new(Mutex::new(None)),
            reader: None,
            started_at: None,
            watchdog_cancel: Arc::new(AtomicBool::new(false)),
            watchdog: None,
            max_duration_hit: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn platform_default() -> Self {
        Self::new(CaptureConfig::default())
    }

    pub fn is_capturing(&self) -> bool {
        self.child
            .lock()
            .map(|child| child.is_some())
            .unwrap_or(false)
    }

    pub fn start(&mut self) -> Result<(), CaptureError> {
        if self.is_capturing() {
            return Err(CaptureError::AlreadyCapturing);
        }
        if self.config.sample_rate == 0 || self.config.channels == 0 {
            return Err(CaptureError::BackendUnavailable(
                "sample rate and channel count must be positive".into(),
            ));
        }
        let (program, arguments) = resolve_command(&self.config)?;
        let mut command = Command::new(&program);
        command
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        isolate_process_group(&mut command);
        let mut child = command.spawn().map_err(|error| {
            let message = error.to_string();
            if is_permission_error(&message) {
                CaptureError::PermissionDenied(message)
            } else {
                CaptureError::BackendUnavailable(format!("{program}: {message}"))
            }
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CaptureError::Io("capture stdout is unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| CaptureError::Io("capture stderr is unavailable".into()))?;
        let max_bytes = self.config.max_bytes.clamp(1, MAX_CAPTURE_BYTES);
        let reader = thread::Builder::new()
            .name("vox-audio-reader".into())
            .spawn(move || read_capture(stdout, stderr, max_bytes))
            .map_err(|error| CaptureError::Io(error.to_string()))?;

        self.watchdog_cancel.store(false, Ordering::Relaxed);
        self.max_duration_hit.store(false, Ordering::Relaxed);
        let child_slot = Arc::clone(&self.child);
        let watchdog_cancel = Arc::clone(&self.watchdog_cancel);
        let max_duration_hit = Arc::clone(&self.max_duration_hit);
        let max_duration = self
            .config
            .max_duration
            .clamp(Duration::from_millis(100), Duration::from_secs(300));
        if let Ok(mut slot) = self.child.lock() {
            *slot = Some(child);
        } else {
            terminate_process_group(&mut child);
            let _ = reader.join();
            return Err(CaptureError::Io("capture process lock is poisoned".into()));
        }
        let watchdog = match thread::Builder::new()
            .name("vox-audio-watchdog".into())
            .spawn(move || {
                let deadline = Instant::now() + max_duration;
                while Instant::now() < deadline {
                    if watchdog_cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    thread::sleep(remaining.min(Duration::from_millis(50)));
                }
                if watchdog_cancel.load(Ordering::Relaxed) {
                    return;
                }
                max_duration_hit.store(true, Ordering::Relaxed);
                if let Ok(mut slot) = child_slot.lock() {
                    if let Some(child) = slot.as_mut() {
                        terminate_process_group(child);
                    }
                }
            }) {
            Ok(handle) => handle,
            Err(error) => {
                self.watchdog_cancel.store(true, Ordering::Relaxed);
                self.kill_child();
                let _ = reader.join();
                return Err(CaptureError::Io(error.to_string()));
            }
        };
        self.reader = Some(reader);
        self.watchdog = Some(watchdog);
        self.started_at = Some(Instant::now());
        Ok(())
    }

    pub fn stop(&mut self) -> Result<CaptureResult, CaptureError> {
        self.finish(StopReason::User)
    }

    pub fn cancel(&mut self) -> Result<(), CaptureError> {
        if !self.is_capturing() && self.reader.is_none() {
            return Err(CaptureError::NotCapturing);
        }
        match self.finish(StopReason::Cancelled) {
            Err(CaptureError::Cancelled) => Ok(()),
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub fn key_down(&mut self) -> Result<(), CaptureError> {
        self.start()
    }

    pub fn key_up(&mut self) -> Result<CaptureResult, CaptureError> {
        self.stop()
    }

    pub fn focus_lost(&mut self) -> Result<(), CaptureError> {
        self.cancel()
    }

    fn finish(&mut self, reason: StopReason) -> Result<CaptureResult, CaptureError> {
        if !self.is_capturing() && self.reader.is_none() {
            return Err(CaptureError::NotCapturing);
        }
        self.watchdog_cancel.store(true, Ordering::Relaxed);
        let _ = self.kill_child();
        self.join_watchdog();
        let reader = self
            .reader
            .take()
            .ok_or_else(|| CaptureError::Io("audio reader is unavailable".into()))?;
        let output = reader
            .join()
            .map_err(|_| CaptureError::Io("audio reader panicked".into()))?;
        let elapsed = self
            .started_at
            .take()
            .map(|started| started.elapsed())
            .unwrap_or_default();
        let duration_hit = self.max_duration_hit.load(Ordering::Relaxed);
        if duration_hit {
            return Err(CaptureError::MaxDuration);
        }
        if reason == StopReason::Cancelled {
            return Err(CaptureError::Cancelled);
        }
        if let Some(error) = output.read_error {
            return Err(CaptureError::Io(error));
        }
        if output.bytes.is_empty() {
            if is_permission_error(&output.stderr) {
                return Err(CaptureError::PermissionDenied(output.stderr));
            }
            return Err(CaptureError::EmptyAudio);
        }
        Ok(CaptureResult {
            peak_level: peak_s16(&output.bytes),
            audio: output.bytes,
            sample_rate: self.config.sample_rate,
            channels: self.config.channels,
            elapsed,
            truncated: output.truncated,
        })
    }

    fn kill_child(&self) -> bool {
        let Ok(mut slot) = self.child.lock() else {
            return false;
        };
        let Some(mut child) = slot.take() else {
            return false;
        };
        terminate_process_group(&mut child);
        true
    }

    fn join_watchdog(&mut self) {
        if let Some(handle) = self.watchdog.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        if self.is_capturing() || self.reader.is_some() {
            let _ = self.cancel();
        } else {
            self.watchdog_cancel.store(true, Ordering::Relaxed);
            self.join_watchdog();
        }
    }
}

fn resolve_command(config: &CaptureConfig) -> Result<(String, Vec<String>), CaptureError> {
    if let Some(command) = &config.command {
        if command.as_os_str().is_empty() {
            return Err(CaptureError::BackendUnavailable(
                "capture command is empty".into(),
            ));
        }
        let arguments = if config.arguments.is_empty() {
            raw_pcm_arguments(config)
        } else {
            config.arguments.clone()
        };
        return Ok((command.to_string_lossy().into_owned(), arguments));
    }
    Ok(command_for_backend(native_capture_backend()?, config))
}

fn command_for_backend(backend: CaptureBackend, config: &CaptureConfig) -> (String, Vec<String>) {
    match backend {
        CaptureBackend::PipeWire => (
            "pw-record".into(),
            vec![
                "--media-type".into(),
                "Audio".into(),
                "--media-category".into(),
                "Capture".into(),
                "--rate".into(),
                config.sample_rate.to_string(),
                "--channels".into(),
                config.channels.to_string(),
                "--format".into(),
                "s16".into(),
                "-".into(),
            ],
        ),
        CaptureBackend::Alsa => ("arecord".into(), raw_pcm_arguments(config)),
    }
}

fn raw_pcm_arguments(config: &CaptureConfig) -> Vec<String> {
    vec![
        "-q".into(),
        "-t".into(),
        "raw".into(),
        "-f".into(),
        "S16_LE".into(),
        "-r".into(),
        config.sample_rate.to_string(),
        "-c".into(),
        config.channels.to_string(),
        "-".into(),
    ]
}

fn executable_available(name: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .any(|candidate| candidate.is_file())
}

fn read_capture<R: Read, E: Read>(mut stdout: R, mut stderr: E, max_bytes: usize) -> ReaderOutput {
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    let mut read_error = None;
    loop {
        match stdout.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let remaining = max_bytes.saturating_sub(bytes.len());
                if read <= remaining {
                    bytes.extend_from_slice(&buffer[..read]);
                } else {
                    bytes.extend_from_slice(&buffer[..remaining]);
                    truncated = true;
                }
            }
            Err(error) => {
                read_error = Some(error.to_string());
                break;
            }
        }
    }
    let mut stderr_bytes = Vec::with_capacity(2048);
    let mut stderr_buffer = [0u8; 1024];
    loop {
        match stderr.read(&mut stderr_buffer) {
            Ok(0) => break,
            Ok(read) => {
                let remaining = 2048usize.saturating_sub(stderr_bytes.len());
                stderr_bytes.extend_from_slice(&stderr_buffer[..read.min(remaining)]);
            }
            Err(_) => break,
        }
    }
    ReaderOutput {
        bytes,
        truncated,
        stderr: String::from_utf8_lossy(&stderr_bytes)
            .chars()
            .take(2048)
            .collect(),
        read_error,
    }
}

fn peak_s16(bytes: &[u8]) -> u16 {
    bytes
        .chunks(2)
        .filter(|chunk| chunk.len() == 2)
        .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]).unsigned_abs())
        .max()
        .unwrap_or(0)
}

fn is_permission_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    [
        "permission denied",
        "access denied",
        "not permitted",
        "no such device",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
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

fn terminate_process_group(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_config(script: &str) -> CaptureConfig {
        #[cfg(unix)]
        {
            CaptureConfig {
                command: Some(PathBuf::from("/bin/sh")),
                arguments: vec!["-c".into(), script.into()],
                max_duration: Duration::from_secs(2),
                max_bytes: 32,
                ..CaptureConfig::default()
            }
        }
        #[cfg(not(unix))]
        {
            let _ = script;
            CaptureConfig::default()
        }
    }

    #[cfg(unix)]
    #[test]
    fn key_down_and_key_up_capture_bounded_pcm_and_level() {
        let mut capture =
            AudioCapture::new(fixture_config("printf '\\001\\000\\377\\177'; sleep 5"));
        capture.key_down().unwrap();
        thread::sleep(Duration::from_millis(50));
        let result = capture.key_up().unwrap();
        assert_eq!(result.audio, vec![1, 0, 255, 127]);
        assert_eq!(result.peak_level, 32767);
        assert!(!result.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn max_bytes_drains_the_child_without_unbounded_memory() {
        let mut capture = AudioCapture::new(fixture_config(
            "printf '1234567890123456789012345678901234567890'; sleep 5",
        ));
        capture.start().unwrap();
        thread::sleep(Duration::from_millis(50));
        let result = capture.stop().unwrap();
        assert_eq!(result.audio.len(), 32);
        assert!(result.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn focus_loss_cancels_and_discards_audio() {
        let mut capture = AudioCapture::new(fixture_config("printf 'audio'; sleep 5"));
        capture.start().unwrap();
        capture.focus_lost().unwrap();
        assert!(!capture.is_capturing());
        assert!(matches!(capture.stop(), Err(CaptureError::NotCapturing)));
    }

    #[cfg(unix)]
    #[test]
    fn duplicate_start_is_rejected() {
        let mut capture = AudioCapture::new(fixture_config("sleep 5"));
        capture.start().unwrap();
        assert_eq!(capture.start(), Err(CaptureError::AlreadyCapturing));
        capture.cancel().unwrap();
    }

    #[test]
    fn unavailable_backend_is_reported_without_starting_a_process() {
        let mut capture = AudioCapture::new(CaptureConfig {
            command: Some(PathBuf::from("definitely-not-a-real-vox-audio-command")),
            ..CaptureConfig::default()
        });
        let _ = capture.start();
        assert!(!capture.is_capturing());
    }

    #[cfg(unix)]
    #[test]
    fn watchdog_reports_max_duration_and_terminates_the_capture() {
        let mut config = fixture_config("sleep 5");
        config.max_duration = Duration::from_millis(100);
        let mut capture = AudioCapture::new(config);
        capture.start().unwrap();
        thread::sleep(Duration::from_millis(180));
        assert_eq!(capture.stop(), Err(CaptureError::MaxDuration));
        assert!(!capture.is_capturing());
    }

    #[test]
    fn peak_level_uses_signed_little_endian_samples() {
        assert_eq!(peak_s16(&[0, 0, 0xff, 0x7f, 0, 0x80]), 32768);
    }

    #[test]
    fn backend_metadata_and_commands_are_safe_to_show_before_capture() {
        let config = CaptureConfig::default();
        let (pipewire, pipewire_arguments) = command_for_backend(CaptureBackend::PipeWire, &config);
        assert_eq!(CaptureBackend::PipeWire.label(), "PipeWire");
        assert_eq!(pipewire, "pw-record");
        assert_eq!(pipewire_arguments.last().map(String::as_str), Some("-"));
        assert!(pipewire_arguments
            .iter()
            .any(|argument| argument == "--rate"));

        let (alsa, alsa_arguments) = command_for_backend(CaptureBackend::Alsa, &config);
        assert_eq!(CaptureBackend::Alsa.label(), "ALSA");
        assert_eq!(alsa, "arecord");
        assert_eq!(alsa_arguments.last().map(String::as_str), Some("-"));
    }
}
