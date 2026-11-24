use log::{LevelFilter, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 512 * 1024;

struct FileStderrLogger {
    file: Mutex<Option<File>>,
}

impl log::Log for FileStderrLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!(
            "[{secs}] [{}] [{}] {}\n",
            record.level(),
            record.target(),
            record.args()
        );
        let _ = std::io::stderr().write_all(line.as_bytes());
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut file) = *guard {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(ref mut file) = *guard {
            let _ = file.flush();
        }
    }
}

pub fn init_logging(log_dir: Option<PathBuf>) {
    let file = log_dir.and_then(|dir| {
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("reticle.log");
        let truncate = std::fs::metadata(&path)
            .map(|m| m.len() > MAX_LOG_BYTES)
            .unwrap_or(false);
        OpenOptions::new()
            .create(true)
            .write(true)
            .append(!truncate)
            .truncate(truncate)
            .open(path)
            .ok()
    });

    let logger = Box::leak(Box::new(FileStderrLogger {
        file: Mutex::new(file),
    }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }

    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "non-string panic payload".to_string()
        };
        let bt = std::backtrace::Backtrace::capture();
        log::error!("PANIC at {location}: {payload}\n{bt}");
        prev_hook(info);
    }));
}
