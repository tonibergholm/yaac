//! Subprocess entry point (renderer, GPU, utility). Bundled as "yaac-app Helper*.app".

fn main() {
    std::process::exit(yaac_cef::run_helper());
}
