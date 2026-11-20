//! Local log file and the redacted diagnostics bundle.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use chrono::{SecondsFormat, Utc};
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::private_fs;
use crate::state::{AppSettings, AppState};

const LOG_DIRECTORY: &str = "logs";
const LOG_FILE: &str = "core.log";
const LOG_BACKUP: &str = "core.log.1";
const LOG_LIMIT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BUNDLE_LOG_LINES: usize = 500;
const REDACTED: &str = "[redacted]";

/// The file that holds the local core log.
pub fn log_file_path() -> Option<PathBuf> {
    let directory = env::var_os("ELESDEE_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::seed::home_dir().map(|home| home.join(".elesdee")))?;
    Some(directory.join(LOG_DIRECTORY).join(LOG_FILE))
}

/// Opens the log file for the tracing subscriber.
///
/// Returns `None` when the file cannot be prepared. The caller then keeps the
/// stderr writer only.
pub fn writer_from_environment() -> Option<RedactingFileWriter> {
    let path = log_file_path()?;
    match RedactingFileWriter::open(&path) {
        Ok(writer) => Some(writer),
        Err(error) => {
            eprintln!(
                "elesdee: could not open the log file {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// A tracing writer that redacts each line before it reaches the log file.
#[derive(Clone)]
pub struct RedactingFileWriter {
    sink: Arc<Mutex<LogSink>>,
}

struct LogSink {
    path: PathBuf,
    file: File,
    bytes: u64,
}

impl RedactingFileWriter {
    /// Opens or creates the log file and its directory.
    pub fn open(path: &Path) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            private_fs::create_private_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        private_fs::restrict_file_permissions(path)?;
        let bytes = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            sink: Arc::new(Mutex::new(LogSink {
                path: path.to_path_buf(),
                file,
                bytes,
            })),
        })
    }

    /// Writes one redacted line and rotates the file when it grows too large.
    pub fn write_redacted(&self, line: &str) {
        let mut sink = lock(&self.sink);
        if sink.bytes >= LOG_LIMIT_BYTES {
            sink.rotate();
        }
        let redacted = redact_line(line);
        let result = (|| {
            sink.file.write_all(redacted.as_bytes())?;
            sink.file.write_all(b"\n")?;
            sink.file.flush()
        })();
        if result.is_ok() {
            sink.bytes += redacted.len() as u64 + 1;
        }
    }
}

impl LogSink {
    fn rotate(&mut self) {
        let backup = self.path.with_file_name(LOG_BACKUP);
        let _ = fs::remove_file(&backup);
        if fs::rename(&self.path, &backup).is_err() {
            return;
        }
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
        {
            Ok(file) => {
                let _ = private_fs::restrict_file_permissions(&self.path);
                self.file = file;
                self.bytes = 0;
            }
            Err(error) => {
                eprintln!(
                    "elesdee: could not reopen the log file {}: {error}",
                    self.path.display()
                );
            }
        }
    }
}

impl<'a> tracing_subscriber::fmt::writer::MakeWriter<'a> for RedactingFileWriter {
    type Writer = RedactingWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            writer: self,
            buffer: Vec::new(),
        }
    }
}

/// Buffers one event and redacts each complete line.
pub struct RedactingWriter<'a> {
    writer: &'a RedactingFileWriter,
    buffer: Vec<u8>,
}

impl RedactingWriter<'_> {
    fn drain_lines(&mut self) {
        while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line = String::from_utf8_lossy(&self.buffer[..position])
                .trim_end_matches('\r')
                .to_owned();
            self.writer.write_redacted(&line);
            self.buffer.drain(..=position);
        }
    }
}

impl Write for RedactingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(bytes);
        self.drain_lines();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buffer.is_empty() {
            let line = String::from_utf8_lossy(&self.buffer).to_string();
            self.writer.write_redacted(&line);
            self.buffer.clear();
        }
        Ok(())
    }
}

impl Drop for RedactingWriter<'_> {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// Redacts one log line. JSON lines keep their shape; other text goes through
/// the same value patterns.
pub fn redact_line(line: &str) -> String {
    match serde_json::from_str::<Value>(line) {
        Ok(mut value) => {
            redact_value(&mut value);
            serde_json::to_string(&value).unwrap_or_else(|_| redact_text(line))
        }
        Err(_) => redact_text(line),
    }
}

fn redact_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            redact_object(map);
        }
        Value::Array(items) => {
            for item in items {
                redact_value(item);
            }
        }
        Value::String(text) => {
            *text = redact_text(text);
        }
        _ => {}
    }
}

fn redact_object(map: &mut Map<String, Value>) {
    for (key, value) in map.iter_mut() {
        if carries_secret(key) || carries_session_content(key) {
            *value = Value::String(REDACTED.to_owned());
            continue;
        }
        redact_value(value);
    }
}

fn normalized(key: &str) -> String {
    key.chars()
        .filter(|character| !matches!(character, '_' | '-' | '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

fn carries_secret(key: &str) -> bool {
    let key = normalized(key);
    key == "key"
        || key == "keys"
        || key == "auth"
        || key.ends_with("key")
        || key.contains("apikey")
        || key.contains("secret")
        || key.contains("token")
        || key.contains("password")
        || key.contains("passwd")
        || key.contains("credential")
        || key.contains("authorization")
        || key.contains("ticket")
        || key.contains("privatekey")
}

fn carries_session_content(key: &str) -> bool {
    matches!(
        normalized(key).as_str(),
        "output" | "content" | "input" | "stdout" | "stderr" | "preview"
    )
}

fn patterns() -> &'static Vec<Regex> {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]+",
            r#"(?i)((?:api[_-]?key|access[_-]?token|auth[_-]?token|token|secret|password|passwd|ticket|authorization)\s*[=:]\s*)("?)[^\s",}]+"#,
            r"(://)[^/\s@]+@",
            r"\b(?:sk-[A-Za-z0-9_-]{8,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{25,})\b",
            r"\b[0-9a-fA-F]{32,}\b",
            r"\b[A-Za-z0-9+/]{40,}={0,2}\b",
            r"\b[0-9a-z]{40,}\b",
        ]
        .iter()
        .filter_map(|pattern| Regex::new(pattern).ok())
        .collect()
    })
}

fn redact_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let patterns = patterns();
    let Some(bearer) = patterns.first() else {
        return text.to_owned();
    };
    let mut output = bearer.replace_all(text, "${1}[redacted]").into_owned();
    if let Some(markers) = patterns.get(1) {
        output = markers
            .replace_all(&output, "${1}${2}[redacted]")
            .into_owned();
    }
    if let Some(userinfo) = patterns.get(2) {
        output = userinfo
            .replace_all(&output, "${1}[redacted]@")
            .into_owned();
    }
    for pattern in patterns.iter().skip(3) {
        output = pattern.replace_all(&output, REDACTED).into_owned();
    }
    output
}

/// The bundle that the Diagnostics page writes into the chosen folder.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub generated_at: String,
    pub app: AppInfo,
    pub system: SystemInfo,
    pub counts: Counts,
    pub settings: AppSettings,
    pub logs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: &'static str,
    pub family: &'static str,
    pub arch: &'static str,
    pub cpus: usize,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub projects: usize,
    pub providers: usize,
    pub configs: usize,
    pub sessions: usize,
    pub active_sessions: usize,
    pub paired_devices: usize,
}

impl DiagnosticsReport {
    /// Gathers versions, counts, settings and the recent redacted log.
    pub fn collect(state: &AppState) -> Self {
        Self::collect_with_log(state, log_file_path())
    }

    /// The testable form with an explicit log path.
    pub fn collect_with_log(state: &AppState, log_path: Option<PathBuf>) -> Self {
        let sessions = state.sessions();
        let counts = Counts {
            projects: state.projects().len(),
            providers: state.providers().len(),
            configs: state.configs().len(),
            sessions: sessions.len(),
            active_sessions: sessions.iter().filter(|entry| entry.is_active()).count(),
            paired_devices: state.paired_device_count(),
        };
        let logs = log_path
            .as_deref()
            .map(read_log)
            .unwrap_or_default()
            .into_iter()
            .rev()
            .take(MAX_BUNDLE_LOG_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        Self {
            generated_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            app: AppInfo {
                name: "elesdee",
                version: env!("CARGO_PKG_VERSION"),
            },
            system: SystemInfo {
                os: std::env::consts::OS,
                family: std::env::consts::FAMILY,
                arch: std::env::consts::ARCH,
                cpus: std::thread::available_parallelism()
                    .map(|count| count.get())
                    .unwrap_or(1),
            },
            counts,
            settings: state.settings(),
            logs,
        }
    }
}

fn read_log(path: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(|line| redact_line(line.trim_end_matches('\r')))
        .filter(|line| !line.is_empty())
        .collect()
}

/// Writes the bundle into the given folder and returns its path.
pub fn write_bundle(folder: &Path, report: &DiagnosticsReport) -> io::Result<PathBuf> {
    if !folder.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the folder does not exist",
        ));
    }
    let name = format!(
        "elesdee-diagnostics-{}.json",
        Utc::now().format("%Y%m%d-%H%M%S")
    );
    let path = folder.join(name);
    let bytes = serde_json::to_vec_pretty(report).map_err(io::Error::other)?;
    private_fs::write_private_file_atomic(&path, &bytes)?;
    Ok(path)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "elesdee-diagnostics-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        path
    }

    #[test]
    fn redacts_secret_fields_and_values() {
        let line = r#"{"fields":{"message":"reading","apiKey":"sk-live-abcdef123456","ticket":"ticket-123","authorization":"Bearer abcdef123456","note":"key=sk-other98765432"}}"#;
        let redacted = redact_line(line);
        assert!(!redacted.contains("sk-live"));
        assert!(!redacted.contains("ticket-123"));
        assert!(!redacted.contains("abcdef123456"));
        assert!(!redacted.contains("sk-other"));
        assert!(redacted.contains(REDACTED));
        assert!(redacted.contains("reading"));
    }

    #[test]
    fn redacts_plain_text_markers() {
        let redacted = redact_text("api_key=sk-secret12345 token: abcdefghijklmnopqrstuvwxyz");
        assert!(!redacted.contains("sk-secret"));
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz"));

        let remote = redact_text("https://user:token@example.com/repo.git");
        assert!(!remote.contains("token@"));
        assert!(remote.contains("https://[redacted]@example.com/repo.git"));
    }

    #[test]
    fn bundle_holds_counts_and_no_session_content_or_secrets() {
        let directory = temp_dir("bundle");
        let log_path = directory.join("core.log");
        fs::write(
            &log_path,
            "{\"fields\":{\"message\":\"started\",\"api_key\":\"sk-abcdef1234567890\",\"ticket\":\"ticket-abc\"}}\n{\"fields\":{\"message\":\"output\",\"output\":\"TOP_SECRET_OUTPUT\"}}\n",
        )
        .expect("write log");

        let state = AppState::with_data_dir(directory.join("data"));
        let report = DiagnosticsReport::collect_with_log(&state, Some(log_path));
        let serialized = serde_json::to_string(&report).expect("serialize report");
        assert!(serialized.contains("\"counts\""));
        assert!(serialized.contains("started"));
        assert!(!serialized.contains("sk-abcdef"));
        assert!(!serialized.contains("ticket-abc"));
        assert!(!serialized.contains("TOP_SECRET_OUTPUT"));

        let output = directory.join("out");
        fs::create_dir_all(&output).expect("create output dir");
        let path = write_bundle(&output, &report).expect("write bundle");
        let written = fs::read_to_string(&path).expect("read bundle");
        assert!(written.contains("\"app\""));
        assert!(!written.contains("TOP_SECRET_OUTPUT"));
        assert!(!written.contains("sk-abcdef"));
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn log_file_is_redacted_as_it_is_written() {
        let directory = temp_dir("writer");
        let path = directory.join("core.log");
        let writer = RedactingFileWriter::open(&path).expect("open writer");
        writer.write_redacted(r#"{"fields":{"message":"key sk-live-abcdef123456"}}"#);
        writer.write_redacted("password=hunter2");
        let text = fs::read_to_string(&path).expect("read log");
        assert!(!text.contains("sk-live"));
        assert!(!text.contains("hunter2"));
        assert!(text.contains(REDACTED));
        let _ = fs::remove_dir_all(&directory);
    }
}
