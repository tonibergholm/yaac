//! `yaac_dev` is set for debug builds and for builds with the `dev-tools` feature. Test machinery
//! (the engine harness, the persistence selftest, the local test site) and directory-backed UI
//! assets exist only in those builds.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(yaac_dev)");
    println!("cargo::rerun-if-changed=build.rs");
    let debug = std::env::var("PROFILE").is_ok_and(|p| p == "debug");
    let dev_tools = std::env::var_os("CARGO_FEATURE_DEV_TOOLS").is_some();
    if debug || dev_tools {
        println!("cargo::rustc-cfg=yaac_dev");
    }
}
