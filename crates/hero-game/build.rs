//! Build script: WebAssembly link settings.
//!
//! macroquad's web dependencies (`quad-snd`, `sapp-jsutils`) declare their JavaScript functions
//! in plain `extern "C"` blocks and rely on the linker leaving them as imports from the `env`
//! module, which `web/mq_js_bundle.js` provides at load time. Current Rust toolchains no longer
//! pass `--allow-undefined` to `rust-lld` for `wasm32-unknown-unknown` by default, so the game
//! binary asks for it explicitly. Native builds are unaffected.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch == "wasm32" {
        println!("cargo:rustc-link-arg-bins=--allow-undefined");
    }
}
