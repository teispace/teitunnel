//! Network simulation and fault injection per tap: latency with jitter, bandwidth
//! limits (token buckets on streamed bodies, never buffering), and fault rules that
//! answer with a status, reset the connection, delay the first byte or time out.
//!
//! Randomness (fault percentages, jitter) comes from a [`RandomSource`] the embedder
//! can replace, and time from Tokio's clock, so tests are deterministic (a fixed
//! sequence and `tokio::time::pause`).

use std::{
    fmt,
    future::Future as _,
    pin::Pin,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http::Method;
use http_body::{Body, Frame, SizeHint};
use serde::{Deserialize, Serialize};
use tokio::time::{Instant, Sleep};

use crate::{LensError, PathPattern, body::BoxError};

/// Largest chunk a throttled body emits at once, so pacing stays smooth.
const THROTTLE_CHUNK: usize = 16 * 1024;

/// Added latency: `base_ms` plus a uniform jitter in `±jitter_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Latency {
    /// Base delay in milliseconds.
    pub base_ms: u32,
    /// Maximum deviation in milliseconds, either way.
    pub jitter_ms: u32,
}

impl Latency {
    /// A slow mobile network: 300 ms ± 100 ms.
    pub const THREE_G: Self = Self {
        base_ms: 300,
        jitter_ms: 100,
    };
    /// A typical mobile network: 70 ms ± 20 ms.
    pub const FOUR_G: Self = Self {
        base_ms: 70,
        jitter_ms: 20,
    };
    /// A geostationary satellite link: 600 ms ± 50 ms.
    pub const SATELLITE: Self = Self {
        base_ms: 600,
        jitter_ms: 50,
    };

    /// A delay drawn with `random` (uniform in `[0, 1)`).
    pub fn sample(self, random: f64) -> Duration {
        let jitter = f64::from(self.jitter_ms) * (random.clamp(0.0, 1.0) * 2.0 - 1.0);
        let ms = (f64::from(self.base_ms) + jitter).max(0.0);
        Duration::from_secs_f64(ms / 1_000.0)
    }
}

/// Network conditions for a tap. The default changes nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NetworkConfig {
    /// Delay before each request is handled.
    pub latency: Option<Latency>,
    /// Request bodies (visitor → origin), bytes per second, shared by the tap.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub up_bytes_per_sec: Option<u64>,
    /// Response bodies (origin → visitor), bytes per second, shared by the tap.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub down_bytes_per_sec: Option<u64>,
}

/// What a fault rule does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FaultAction {
    /// Answer with this status (e.g. 500, 502, 503, 504, 429) without forwarding.
    Status {
        /// Status code (400–599).
        status: u16,
        /// `Retry-After` in seconds.
        retry_after_secs: Option<u32>,
    },
    /// Close the connection without a response.
    Reset,
    /// Forward normally, but only after this many milliseconds.
    Delay {
        /// Milliseconds.
        #[cfg_attr(feature = "specta", specta(type = u32))]
        ms: u64,
    },
    /// Hold the request, then answer `504 Gateway Timeout` (as Cloudflare would).
    Timeout {
        /// How long to hold, in milliseconds.
        #[cfg_attr(feature = "specta", specta(type = u32))]
        after_ms: u64,
    },
}

/// A fault applied to a share of matching requests.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FaultRule {
    /// Method to match (case-insensitive); `None` matches any.
    pub method: Option<String>,
    /// Path to match.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub path: PathPattern,
    /// Share of matching requests affected, 0–100.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub percent: f64,
    /// What happens to them.
    pub action: FaultAction,
}

impl FaultRule {
    /// Checks the rule.
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] for a percentage outside 0–100 or a status outside
    /// 400–599.
    pub fn validate(&self) -> Result<(), LensError> {
        if !(0.0..=100.0).contains(&self.percent) {
            return Err(LensError::InvalidConfig(format!(
                "fault percentage {} must be between 0 and 100",
                self.percent
            )));
        }
        if let FaultAction::Status { status, .. } = self.action
            && !(400..=599).contains(&status)
        {
            return Err(LensError::InvalidConfig(format!(
                "fault status {status} must be between 400 and 599"
            )));
        }
        Ok(())
    }

    fn matches(&self, method: &Method, path: &str) -> bool {
        self.method
            .as_deref()
            .is_none_or(|m| m.eq_ignore_ascii_case(method.as_str()))
            && self.path.matches(path)
    }
}

/// The fault applied to a captured exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FaultRecord {
    /// Index of the rule in [`crate::TapConfig::faults`].
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub rule: usize,
    /// What it did.
    pub action: FaultAction,
}

/// The first rule that matches and fires (each rule draws once, in order).
pub(crate) fn pick_fault(
    rules: &[FaultRule],
    method: &Method,
    path: &str,
    random: &dyn RandomSource,
) -> Option<FaultRecord> {
    rules.iter().enumerate().find_map(|(rule, fault)| {
        (fault.matches(method, path) && random.next_f64() * 100.0 < fault.percent).then(|| {
            FaultRecord {
                rule,
                action: fault.action.clone(),
            }
        })
    })
}

/// A source of uniform random numbers for simulations (not for secrets).
pub trait RandomSource: Send + Sync + fmt::Debug {
    /// The next 64 random bits.
    fn next_u64(&self) -> u64;

    /// A number in `[0, 1)`.
    fn next_f64(&self) -> f64 {
        // 53 random bits: every f64 in [0, 1) this can produce is exact.
        #[allow(clippy::cast_precision_loss)]
        let value = (self.next_u64() >> 11) as f64;
        value / (1u64 << 53) as f64
    }
}

/// SplitMix64, seeded from the OS at start: fast, lock-free, good enough for jitter
/// and percentages.
#[derive(Debug)]
pub struct SplitMix(AtomicU64);

impl SplitMix {
    /// A generator with a fixed seed (reproducible sequences).
    pub fn seeded(seed: u64) -> Self {
        Self(AtomicU64::new(seed))
    }

    /// A generator seeded from the OS (falls back to the clock).
    pub fn from_entropy() -> Self {
        let seed = crate::util::random_bytes::<8>()
            .map_or_else(|_| crate::util::now_unix_ms(), u64::from_le_bytes);
        Self::seeded(seed)
    }
}

impl RandomSource for SplitMix {
    fn next_u64(&self) -> u64 {
        let mut z = self
            .0
            .fetch_add(0x9E37_79B9_7F4A_7C15, Relaxed)
            .wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// A token bucket shared by all bodies in one direction of a tap. Reservations may go
/// into debt; the reserver waits the debt off, which keeps the average rate exact.
#[derive(Debug)]
pub(crate) struct Bucket {
    rate: f64,
    burst: f64,
    state: Mutex<(f64, Instant)>,
}

impl Bucket {
    pub(crate) fn new(bytes_per_sec: u64) -> Self {
        #[allow(clippy::cast_precision_loss)]
        let rate = bytes_per_sec.max(1) as f64;
        let burst = (rate / 10.0).max(THROTTLE_CHUNK as f64);
        Self {
            rate,
            burst,
            state: Mutex::new((burst, Instant::now())),
        }
    }

    /// Takes `bytes` tokens; returns how long to wait before sending them.
    pub(crate) fn reserve(&self, bytes: usize) -> Duration {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        let (tokens, last) = *state;
        let refilled =
            (tokens + now.duration_since(last).as_secs_f64() * self.rate).min(self.burst);
        #[allow(clippy::cast_precision_loss)]
        let after = refilled - bytes as f64;
        *state = (after, now);
        if after >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-after / self.rate)
        }
    }
}

/// Paces a body through a [`Bucket`], frame by frame (large frames are split), without
/// buffering beyond the frame in hand.
pub(crate) struct ThrottledBody<B> {
    inner: B,
    bucket: std::sync::Arc<Bucket>,
    /// Data waiting to be sent: the chunk that's paid for (after `sleep`), and the rest
    /// of a split frame.
    ready: Option<Bytes>,
    rest: Option<Bytes>,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl<B> ThrottledBody<B> {
    pub(crate) fn new(inner: B, bucket: std::sync::Arc<Bucket>) -> Self {
        Self {
            inner,
            bucket,
            ready: None,
            rest: None,
            sleep: None,
        }
    }

    /// Splits off the next chunk of `data`, reserves it, and schedules its release.
    fn schedule(&mut self, mut data: Bytes) {
        let rest = (data.len() > THROTTLE_CHUNK).then(|| data.split_off(THROTTLE_CHUNK));
        self.rest = rest.filter(|rest| !rest.is_empty());
        let wait = self.bucket.reserve(data.len());
        self.ready = Some(data);
        self.sleep = (!wait.is_zero()).then(|| Box::pin(tokio::time::sleep(wait)));
    }
}

impl<B> Body for ThrottledBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        loop {
            if this.ready.is_some() {
                if let Some(sleep) = this.sleep.as_mut() {
                    if sleep.as_mut().poll(cx).is_pending() {
                        return Poll::Pending;
                    }
                    this.sleep = None;
                }
                if let Some(data) = this.ready.take() {
                    return Poll::Ready(Some(Ok(Frame::data(data))));
                }
            }
            if let Some(rest) = this.rest.take() {
                this.schedule(rest);
                continue;
            }
            match Pin::new(&mut this.inner).poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err.into()))),
                Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                    Ok(data) if data.is_empty() => {}
                    Ok(data) => this.schedule(data),
                    Err(frame) => return Poll::Ready(Some(Ok(frame))),
                },
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.ready.is_none() && self.rest.is_none() && self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        let pending =
            self.ready.as_ref().map_or(0, Bytes::len) + self.rest.as_ref().map_or(0, Bytes::len);
        let mut hint = self.inner.size_hint();
        let pending = pending as u64;
        hint.set_lower(hint.lower() + pending);
        if let Some(upper) = hint.upper() {
            hint.set_upper(upper + pending);
        }
        hint
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::{BodyExt, Full};

    use super::*;

    /// Returns the given values in order, then repeats the last.
    #[derive(Debug)]
    pub(crate) struct Sequence(Mutex<Vec<f64>>);

    impl RandomSource for Sequence {
        fn next_u64(&self) -> u64 {
            0
        }
        fn next_f64(&self) -> f64 {
            let mut values = self.0.lock().unwrap();
            if values.len() > 1 {
                values.remove(0)
            } else {
                values[0]
            }
        }
    }

    fn rule(percent: f64, action: FaultAction) -> FaultRule {
        FaultRule {
            method: None,
            path: PathPattern::parse("/api/*").unwrap(),
            percent,
            action,
        }
    }

    #[test]
    fn latency_presets_and_jitter() {
        assert_eq!(Latency::THREE_G.sample(0.5), Duration::from_millis(300));
        assert_eq!(Latency::THREE_G.sample(0.0), Duration::from_millis(200));
        assert_eq!(Latency::THREE_G.sample(1.0), Duration::from_millis(400));
        assert_eq!(
            Latency {
                base_ms: 10,
                jitter_ms: 50
            }
            .sample(0.0),
            Duration::ZERO
        );
        assert_eq!(Latency::FOUR_G.sample(0.5), Duration::from_millis(70));
        assert_eq!(Latency::SATELLITE.sample(0.5), Duration::from_millis(600));
    }

    #[test]
    fn fault_percentages_use_the_random_source() {
        let rules = vec![
            rule(
                30.0,
                FaultAction::Status {
                    status: 503,
                    retry_after_secs: Some(5),
                },
            ),
            rule(100.0, FaultAction::Reset),
        ];
        // 0.2 → first rule fires (20 < 30).
        let random = Sequence(Mutex::new(vec![0.2]));
        let fault = pick_fault(&rules, &Method::GET, "/api/x", &random).unwrap();
        assert_eq!(fault.rule, 0);
        // 0.5 → first rule doesn't (50 ≥ 30); the second always does.
        let random = Sequence(Mutex::new(vec![0.5]));
        assert_eq!(
            pick_fault(&rules, &Method::GET, "/api/x", &random)
                .unwrap()
                .rule,
            1
        );
        assert!(pick_fault(&rules, &Method::GET, "/other", &random).is_none());
        let never = vec![rule(0.0, FaultAction::Reset)];
        let random = Sequence(Mutex::new(vec![0.0]));
        assert!(pick_fault(&never, &Method::GET, "/api/x", &random).is_none());
    }

    #[test]
    fn validation() {
        assert!(rule(101.0, FaultAction::Reset).validate().is_err());
        assert!(
            rule(
                10.0,
                FaultAction::Status {
                    status: 200,
                    retry_after_secs: None
                }
            )
            .validate()
            .is_err()
        );
        assert!(
            rule(
                10.0,
                FaultAction::Status {
                    status: 429,
                    retry_after_secs: Some(1)
                }
            )
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn splitmix_is_uniform_enough() {
        let random = SplitMix::seeded(42);
        let mut sum = 0.0;
        for _ in 0..10_000 {
            let v = random.next_f64();
            assert!((0.0..1.0).contains(&v));
            sum += v;
        }
        assert!((sum / 10_000.0 - 0.5).abs() < 0.02);
        assert_ne!(
            SplitMix::seeded(1).next_u64(),
            SplitMix::seeded(2).next_u64()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn bucket_paces_to_the_rate() {
        let bucket = std::sync::Arc::new(Bucket::new(100_000));
        let data = Bytes::from(vec![0u8; 300_000]);
        let start = Instant::now();
        let body = ThrottledBody::new(Full::new(data), bucket);
        let out = body.collect().await.unwrap().to_bytes();
        assert_eq!(out.len(), 300_000);
        let elapsed = start.elapsed();
        // 300 kB at 100 kB/s, less the initial 16 kB burst.
        assert!(elapsed >= Duration::from_millis(2_800), "{elapsed:?}");
        assert!(elapsed <= Duration::from_millis(3_100), "{elapsed:?}");
    }
}
