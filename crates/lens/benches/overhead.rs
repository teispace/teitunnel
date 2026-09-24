//! Lens's added latency and streaming throughput on loopback.
//!
//! `cargo bench -p teitunnel-lens --bench overhead` (plain `main`, no framework): sends
//! the same small requests over one keep-alive connection straight to an origin and
//! through Lens (capture on), and reports percentiles of each and the difference; then
//! downloads 256 MiB both ways and reports throughput. Numbers are recorded in the
//! crate README.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::print_stdout,
    clippy::cast_precision_loss,
    missing_docs
)]

use std::{
    convert::Infallible,
    net::SocketAddr,
    time::{Duration, Instant},
};

use bytes::Bytes;
use http::{Request, Response};
use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
use lens::{Lens, LensBody, LensOptions, TapConfig, Upstream};
use tokio::net::{TcpListener, TcpStream};

const WARMUP: usize = 2_000;
const SAMPLES: usize = 20_000;
const DOWNLOAD: usize = 256 * 1024 * 1024;

async fn origin() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = stream.set_nodelay(true);
            tokio::spawn(async move {
                let service = service_fn(|request: Request<Incoming>| async move {
                    let body: LensBody = if request.uri().path() == "/download" {
                        let chunk = Bytes::from(vec![0u8; 64 * 1024]);
                        let frames = (0..DOWNLOAD / chunk.len())
                            .map(move |_| Ok::<_, lens::BoxError>(Frame::data(chunk.clone())));
                        StreamBody::new(futures_util::stream::iter(frames)).boxed_unsync()
                    } else {
                        lens::full(Bytes::from_static(b"{\"ok\":true}"))
                    };
                    let mut response = Response::new(body);
                    response
                        .headers_mut()
                        .insert("content-type", "application/json".parse().unwrap());
                    Ok::<_, Infallible>(response)
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    addr
}

async fn client(addr: SocketAddr) -> hyper::client::conn::http1::SendRequest<LensBody> {
    let stream = TcpStream::connect(addr).await.unwrap();
    stream.set_nodelay(true).unwrap();
    let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    sender
}

async fn one(sender: &mut hyper::client::conn::http1::SendRequest<LensBody>, path: &str) {
    sender.ready().await.unwrap();
    let request = Request::builder()
        .uri(path)
        .header("host", "bench.test")
        .header("user-agent", "lens-bench")
        .header("accept", "application/json")
        .body(lens::empty())
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    let _ = response.into_body().collect().await.unwrap();
}

fn percentile(sorted: &[Duration], q: f64) -> Duration {
    let index = ((sorted.len() as f64 - 1.0) * q).round() as usize;
    sorted[index]
}

async fn latencies(addr: SocketAddr) -> Vec<Duration> {
    let mut sender = client(addr).await;
    for _ in 0..WARMUP {
        one(&mut sender, "/small").await;
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        one(&mut sender, "/small").await;
        samples.push(start.elapsed());
    }
    samples.sort();
    samples
}

async fn throughput(addr: SocketAddr) -> f64 {
    let mut sender = client(addr).await;
    sender.ready().await.unwrap();
    let request = Request::builder()
        .uri("/download")
        .header("host", "bench.test")
        .body(lens::empty())
        .unwrap();
    let start = Instant::now();
    let mut body = sender.send_request(request).await.unwrap().into_body();
    let mut total = 0usize;
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.unwrap().into_data() {
            total += data.len();
        }
    }
    assert_eq!(total, DOWNLOAD);
    total as f64 / (1024.0 * 1024.0) / start.elapsed().as_secs_f64()
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let origin = origin().await;
        let lens = Lens::new(LensOptions::default()).unwrap();
        let tap = lens
            .start_tap(TapConfig::new(
                Upstream::origin(&format!("http://{origin}")).unwrap(),
            ))
            .await
            .unwrap();

        let direct = latencies(origin).await;
        let proxied = latencies(tap.addr).await;
        println!("small GET, {SAMPLES} sequential requests on one keep-alive connection");
        println!("            p50        p95        p99");
        for (name, samples) in [("direct", &direct), ("via Lens", &proxied)] {
            println!(
                "{name:<10} {:>8.1?} {:>10.1?} {:>10.1?}",
                percentile(samples, 0.50),
                percentile(samples, 0.95),
                percentile(samples, 0.99)
            );
        }
        for q in [0.50, 0.95, 0.99] {
            let added = percentile(&proxied, q).saturating_sub(percentile(&direct, q));
            println!("added p{:<3}   {added:>8.1?}", (q * 100.0) as u32);
        }

        let direct_mb = throughput(origin).await;
        let proxied_mb = throughput(tap.addr).await;
        println!("256 MiB download: direct {direct_mb:.0} MiB/s, via Lens {proxied_mb:.0} MiB/s");
        let snapshot = lens.metrics(&tap.id).unwrap();
        println!(
            "Lens's own view: {} requests, p50 {:.3} ms, p99 {:.3} ms (time to response head)",
            snapshot.requests,
            snapshot.latency.p50_ms.unwrap_or_default(),
            snapshot.latency.p99_ms.unwrap_or_default()
        );
        lens.shutdown().await;
    });
}
