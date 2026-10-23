//! `yaac_dev` is set for debug builds and for builds with the `dev-tools` feature. Test machinery
//! (the engine harness, the persistence selftest, the local test site) and directory-backed UI
//! assets exist only in those builds.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(yaac_dev)");
    println!("cargo::rerun-if-changed=build.rs");
    let debug = std::env::var("PROFILE").is_ok_and(|p| p == "debug");
    let dev_tools = std::env::var_os("CARGO_FEATURE_DEV_TOOLS").is_some();
    let embed_ui = std::env::var_os("CARGO_FEATURE_EMBED_UI").is_some();
    // Linux: libcef.so sits next to the executable in the bundle (scripts/bundle-linux.sh), so the
    // binaries look for it there first. `$ORIGIN` is passed to the linker unchanged, no shell.
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux") {
        println!("cargo::rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
    if debug || dev_tools {
        println!("cargo::rustc-cfg=yaac_dev");
    } else if !embed_ui {
        println!(
            "cargo::error=a release build of yaac-app needs the embed-ui feature: the UI is served from the binary. Build with --features embed-ui, or use a debug or dev-tools build."
        );
    }
}
