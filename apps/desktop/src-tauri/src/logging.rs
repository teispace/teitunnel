//! App logging: `tracing` to stderr (debug builds) and a daily-rolling file, with every
//! line passed through [`teitunnel_core::redact::redact`] before it is written.

use std::{
    io::{self, Write},
    path::Path,
};

use teitunnel_core::redact::redact;
use tracing_appender::{non_blocking::WorkerGuard, rolling};
use tracing_subscriber::{
    EnvFilter, fmt::MakeWriter, layer::SubscriberExt, util::SubscriberInitExt,
};

const DEFAULT_FILTER: &str = "info,teitunnel=debug,teitunnel_core=debug,teitunnel_desktop=debug";
const MAX_LOG_FILES: usize = 7;

/// Keeps the background log writer alive; dropping it flushes pending lines.
#[derive(Debug)]
pub(crate) struct LogGuard {
    _file: WorkerGuard,
}

/// Initialises the global subscriber. Call once, at startup.
pub(crate) fn init(log_dir: &Path) -> Result<LogGuard, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(log_dir)?;
    let appender = rolling::Builder::new()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("teitunnel")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)?;
    let (file_writer, guard) = tracing_appender::non_blocking(appender);

    let filter =
        EnvFilter::try_from_env("TEITUNNEL_LOG").unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(Redacting(file_writer));

    let stderr_layer = cfg!(debug_assertions)
        .then(|| tracing_subscriber::fmt::layer().with_writer(Redacting(io::stderr)));

    let init = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();
    // `try_init` installs the subscriber, then bridges the `log` crate. If a plugin
    // already installed a `log` logger (the E2E WebDriver plugin does), only the bridge
    // fails; tracing itself is set up and the app can continue.
    if let Err(err) = init {
        if !tracing::dispatcher::has_been_set() {
            return Err(err.into());
        }
        tracing::warn!(error = %err, "log crate bridge not installed");
    }

    Ok(LogGuard { _file: guard })
}

/// A [`MakeWriter`] whose writers redact secrets from each formatted event.
#[derive(Debug, Clone)]
struct Redacting<M>(M);

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for Redacting<M> {
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter(self.0.make_writer())
    }
}

/// Redacts each buffer before forwarding it. `tracing-subscriber` formats a whole event
/// into one buffer and writes it in a single call, so a secret is never split.
#[derive(Debug)]
struct RedactingWriter<W>(W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.0.write_all(redact(&text).as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Capture {
        type Writer = Self;
        fn make_writer(&'a self) -> Self {
            self.clone()
        }
    }

    #[test]
    fn log_lines_are_redacted() {
        let capture = Capture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_writer(Redacting(capture.clone()))
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                header = "Bearer Xy1aB2cD3eF4gH5iJ6kL7mN8oP9qR0sT1uV2wX3y",
                "calling api with token=abcdef123456"
            );
        });
        let out = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
        assert!(out.contains("calling api"), "{out}");
        assert!(!out.contains("Xy1aB2cD3eF4"), "{out}");
        assert!(!out.contains("abcdef123456"), "{out}");
    }
}
