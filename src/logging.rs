// Logging initialisation.
//
// Writes structured logs to **both**:
//   - stdout  → captured by `docker logs`
//   - `./backend.log` → persisted on the host / container volume
//
// The log level is controlled by the `RUST_LOG` environment variable
// (defaults to `info`, suppressing noisy library crates).
//
// To enable debug output:  `RUST_LOG=debug`
// To enable sqlx queries:  `RUST_LOG=info,sqlx=debug`

use tracing_appender::non_blocking;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// Initialise the global tracing subscriber.
//
// Returns a [`WorkerGuard`] that **must** be kept alive for the entire
// duration of the program.  Dropping it early will cause buffered log
// messages to be lost.
pub fn init() -> non_blocking::WorkerGuard {
    // Write to a dedicated logs/ subdirectory so the Docker volume can be a
    // directory mount (much more reliable than file bind-mounts, which Docker
    // creates as a directory when the host path doesn't yet exist).
    // Switch to `rolling::daily` if you want automatic log rotation.
    let file_appender = tracing_appender::rolling::never("./logs", "backend.log");
    let (file_writer, guard) = non_blocking(file_appender);

    // Default filter: info for our code, warn for noisy dependencies.
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,sqlx=warn,hyper=warn,tungstenite=warn,tower=warn,h2=warn")
    });

    // Stdout layer – colourised, intended for docker logs / terminals.
    let stdout_layer = fmt::layer().with_target(true).with_ansi(true);

    // File layer – plain text (no ANSI escape codes).
    let file_layer = fmt::layer()
        .with_target(true)
        .with_ansi(false)
        .with_writer(file_writer);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    guard
}
