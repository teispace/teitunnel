fn main() {
    // Windows gives the main thread 1 MiB of stack. The parser for every subcommand and
    // the command futures need more than that in debug builds; 8 MiB matches Linux and
    // macOS.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
}
