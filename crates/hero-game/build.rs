//! Build script: WebAssembly link settings and the Windows executable icon.
//!
//! macroquad's web dependencies (`quad-snd`, `sapp-jsutils`) declare their JavaScript functions
//! in plain `extern "C"` blocks and rely on the linker leaving them as imports from the `env`
//! module, which `web/mq_js_bundle.js` provides at load time. Current Rust toolchains no longer
//! pass `--allow-undefined` to `rust-lld` for `wasm32-unknown-unknown` by default, so the game
//! binary asks for it explicitly. Native builds are unaffected.
//!
//! On Windows the executable carries `icon/eiketsuden.ico` as its icon resource, which Explorer,
//! shortcuts and the taskbar show (the window icon itself is set at start-up, `main.rs`). The
//! icon is drawn by `tools/assets/build_icon.py`.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    if arch == "wasm32" {
        println!("cargo:rustc-link-arg-bins=--allow-undefined");
    }
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=icon/eiketsuden.ico");
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("icon/eiketsuden.ico");
        if let Err(e) = resource.compile() {
            panic!("cannot embed icon/eiketsuden.ico into the executable: {e}");
        }
    }
}
