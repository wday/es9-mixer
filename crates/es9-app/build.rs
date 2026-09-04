//! Tauri's build step: bakes the config, icon and capability files into the binary.
//!
//! It also watches the frontend, which Tauri does not do for itself here.
//! `tauri::generate_context!()` in `main.rs` embeds `frontendDist` at compile time, but
//! the `rerun-if-changed` that would notice that directory changing lives in
//! `tauri-build`'s `codegen` feature, which this crate does not enable. Without it cargo
//! has no reason to recompile the crate when only `web/` changed, the macro never
//! re-expands, and a freshly built binary shows the *previous* UI — which looks exactly
//! like a state bug: correct data in the backend log, stale screen.

use std::path::Path;

/// Must match `build.frontendDist` in `tauri.conf.json`, which is resolved relative to
/// that file — the same directory a build script runs in.
const FRONTEND_DIST: &str = "../../web";

fn main() {
    // A path that does not exist would leave cargo rebuilding every time rather than
    // failing, so the stale-frontend trap would come back silently if these two drifted
    // apart. Say so at build time instead.
    assert!(
        Path::new(FRONTEND_DIST).is_dir(),
        "frontendDist {FRONTEND_DIST} is not a directory — has tauri.conf.json moved?"
    );
    // The whole directory, because the whole directory is what gets embedded. That
    // includes `web/pkg`, so rebuilding the WASM bridge rebuilds the shell too; the
    // desktop backend never loads it, but it is baked in, so it is watched.
    println!("cargo:rerun-if-changed={FRONTEND_DIST}");

    tauri_build::build()
}
