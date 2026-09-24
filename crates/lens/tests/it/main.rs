//! Integration tests over real sockets: a test origin, Lens, and a client.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    unreachable_pub
)]

mod features;
mod folder;
mod gates;
mod proxy;
mod simulation;
mod streaming;
mod support;
