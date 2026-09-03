//! Temporary asynchronous key-path logging for the current Windows debugging cycle.

use std::{
    fmt,
    fs::OpenOptions,
    io::{self, BufWriter, Write},
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, TrySendError, sync_channel},
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

const LOG_QUEUE_CAPACITY: usize = 4096;
const LOG_FILE_NAME: &str = "lastkey-debug.log";

struct LogEntry {
    sequence: u64,
    seconds: u64,
    microseconds: u32,
    thread_name: String,
    message: String,
}

static LOG_SENDER: OnceLock<SyncSender<LogEntry>> = OnceLock::new();
static LOG_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn init() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let path = executable.with_file_name(LOG_FILE_NAME);
    if LOG_SENDER.get().is_some() {
        return Ok(path);
    }

    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let (sender, receiver) = sync_channel::<LogEntry>(LOG_QUEUE_CAPACITY);
    let executable_for_log = executable.clone();
    thread::Builder::new()
        .name("lastkey-debug-log".into())
        .spawn(move || {
            let mut writer = BufWriter::new(file);
            let _ = writeln!(writer, "--- LastKey debug session ---");
            let _ = writeln!(writer, "executable={}", executable_for_log.display());
            let _ = writer.flush();
            while let Ok(entry) = receiver.recv() {
                let _ = writeln!(
                    writer,
                    "{}.{:06} seq={} [{}] {}",
                    entry.seconds,
                    entry.microseconds,
                    entry.sequence,
                    entry.thread_name,
                    entry.message
                );
                let _ = writer.flush();
            }
        })?;
    let _ = LOG_SENDER.set(sender);
    write(format_args!(
        "debug logger initialized path={}",
        path.display()
    ));
    Ok(path)
}

pub fn write(arguments: fmt::Arguments<'_>) {
    let Some(sender) = LOG_SENDER.get() else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let entry = LogEntry {
        sequence: LOG_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        seconds: now.as_secs(),
        microseconds: now.subsec_micros(),
        thread_name: thread::current().name().unwrap_or("unnamed").to_owned(),
        message: arguments.to_string(),
    };
    match sender.try_send(entry) {
        Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
    }
}
