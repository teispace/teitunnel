//! Upgraded connections (WebSocket and other `Upgrade:` protocols): bytes are copied
//! both ways as they arrive, with backpressure (a write completes before the next
//! read), while WebSocket frames are observed for message counts and previews.

use std::{io, sync::atomic::Ordering::Relaxed, time::Duration};

use hyper::upgrade::{OnUpgrade, Upgraded};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::{
    Direction, ExchangeError,
    capture::ErrorKind,
    metrics::TapMetrics,
    recorder::Recorder,
    stream::{Deflate, PreviewLimits, WsObserver},
};

const BUFFER: usize = 16 * 1024;
/// How long a WebSocket's other direction may continue after one side closed.
const WS_LINGER: Duration = Duration::from_secs(1);
/// The same for other upgraded protocols, which may use TCP half-close deliberately.
const UPGRADE_LINGER: Duration = Duration::from_secs(10);

/// Waits for both sides to finish upgrading, then pumps until either side closes or
/// Lens shuts down, and finishes the recording.
pub(crate) async fn run(
    client: OnUpgrade,
    server: OnUpgrade,
    recorder: Recorder,
    websocket: bool,
    deflate: Option<Deflate>,
    limits: PreviewLimits,
    cancel: CancellationToken,
) {
    let metrics = std::sync::Arc::clone(recorder.metrics());
    metrics.active_streams.fetch_add(1, Relaxed);
    let result = tokio::select! {
        result = async {
            let (client, server) = tokio::try_join!(client, server)
                .map_err(|err| io::Error::other(err.to_string()))?;
            pump(client, server, &recorder, websocket, deflate, limits).await
        } => result,
        () = cancel.cancelled() => Ok(()),
    };
    TapMetrics::dec(&metrics.active_streams);
    let error = result.err().map(|err| ExchangeError {
        kind: match err.kind() {
            io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
            | io::ErrorKind::UnexpectedEof => ErrorKind::ConnectionReset,
            _ => ErrorKind::Other,
        },
        message: format!("the upgraded connection failed: {err}"),
    });
    recorder.finish(error);
}

async fn pump(
    client: Upgraded,
    server: Upgraded,
    recorder: &Recorder,
    websocket: bool,
    deflate: Option<Deflate>,
    limits: PreviewLimits,
) -> io::Result<()> {
    let observer = |direction: Direction| {
        websocket.then(|| WsObserver::new(limits, deflate.map(|d| d.resets(direction))))
    };
    let (mut client_read, mut client_write) = tokio::io::split(TokioIo::new(client));
    let (mut server_read, mut server_write) = tokio::io::split(TokioIo::new(server));
    let up = copy(
        &mut client_read,
        &mut server_write,
        Direction::ClientToServer,
        recorder,
        observer(Direction::ClientToServer),
        limits,
    );
    let down = copy(
        &mut server_read,
        &mut client_write,
        Direction::ServerToClient,
        recorder,
        observer(Direction::ServerToClient),
        limits,
    );
    tokio::pin!(up, down);
    // When one side closes, the other gets a short linger to finish (half-close), then
    // the tunnel ends: WebSocket clients often keep their socket open after the close
    // handshake, and nothing meaningful can follow it.
    let linger = if websocket { WS_LINGER } else { UPGRADE_LINGER };
    let rest = tokio::select! {
        result = &mut up => {
            result?;
            tokio::time::timeout(linger, &mut down).await
        }
        result = &mut down => {
            result?;
            tokio::time::timeout(linger, &mut up).await
        }
    };
    match rest {
        Ok(result) => result.map(|_| ()),
        Err(_) => Ok(()),
    }
}

async fn copy<R, W>(
    reader: &mut R,
    writer: &mut W,
    direction: Direction,
    recorder: &Recorder,
    mut observer: Option<WsObserver>,
    limits: PreviewLimits,
) -> io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let counter = match direction {
        Direction::ClientToServer => &recorder.metrics().bytes_in,
        Direction::ServerToClient => &recorder.metrics().bytes_out,
    };
    let mut buf = vec![0u8; BUFFER];
    let mut total = 0u64;
    let mut messages = Vec::new();
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            // Half-close: tell the other side nothing more is coming.
            let _ = writer.shutdown().await;
            return Ok(total);
        }
        if let Some(ws) = observer.as_mut().filter(|ws| !ws.is_broken()) {
            ws.feed(&buf[..n], &mut messages);
            if !messages.is_empty() {
                let at = recorder.elapsed_us();
                let batch = std::mem::take(&mut messages);
                recorder.stream_update(|stats| {
                    let mut previewed = false;
                    for observed in batch {
                        previewed |= observed.record(stats, direction, at, limits);
                    }
                    previewed
                });
            }
        }
        writer.write_all(&buf[..n]).await?;
        writer.flush().await?;
        total += n as u64;
        counter.fetch_add(n as u64, Relaxed);
    }
}
