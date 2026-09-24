//! The real control server over the scripted `FakeHost`, for testing clients written in
//! other languages (the editor extensions' shared client runs its interop tests against
//! it): `cargo run -p teitunnel-control --features testing --example fake_app -- <data dir>`.
//!
//! It allows the first change it's asked about and declines the rest, emits
//! `sharesChanged` every 200 ms, prints `listening` once ready and runs until killed.

use std::{path::PathBuf, time::Duration};

use teitunnel_control::{
    Decision, Limits,
    protocol::Event,
    testing::{serve, share},
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        return Err(std::io::Error::other("usage: fake_app <data dir>"));
    };
    let running = serve(&dir, Limits::default()).await?;
    running.host.answer(Decision::Once);
    #[allow(clippy::print_stdout)]
    {
        println!("listening");
    }
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    loop {
        tick.tick().await;
        running.host.emit(Event::SharesChanged {
            id: Some(share("qs-1").id),
        });
    }
}
