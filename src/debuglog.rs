//! Optional debug log. Start the app with `--debug-log` and it writes
//! timestamped lines (with the thread id) to  <temp dir>/mchose-g3-debug.log
//! Windows: %TEMP%\mchose-g3-debug.log      Linux: /tmp/mchose-g3-debug.log

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Instant;

static LOG: OnceLock<(Instant, Mutex<File>)> = OnceLock::new();

pub fn init() {
    if !std::env::args().any(|a| a == "--debug-log") {
        return;
    }
    let path = std::env::temp_dir().join("mchose-g3-debug.log");
    if let Ok(file) = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
    {
        let _ = LOG.set((Instant::now(), Mutex::new(file)));
    }
}

pub fn write(line: &str) {
    if let Some((start, file)) = LOG.get() {
        if let Ok(mut f) = file.lock() {
            let _ = writeln!(
                f,
                "[{:9.3}s] [{:?}] {}",
                start.elapsed().as_secs_f64(),
                thread::current().id(),
                line
            );
            // Flush every line so nothing is lost if the app hangs.
            let _ = f.flush();
        }
    }
}

/// Like eprintln!, but also writes to the debug log when enabled.
#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {{
        let line = format!($($arg)*);
        eprintln!("{}", line);
        $crate::debuglog::write(&line);
    }};
}
