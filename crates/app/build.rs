fn main() {
    // `tauri::generate_context!` embeds `ui/dist`; `cargo test` without a
    // front-end build still needs the directory to exist. The real build
    // (`beforeBuildCommand`) overwrites this placeholder.
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/dist");
    if !dist.join("index.html").exists() {
        let _ = std::fs::create_dir_all(&dist);
        let _ = std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>Model Lab</title><p>Front end not built. Run <code>npm --prefix ui run build</code>.</p>",
        );
    }
    // tauri-build also requires every `externalBin` to exist, even for a
    // debug `cargo test` that never bundles. Debug builds get a zero-byte
    // placeholder (binaries/ is git-ignored); `packaging/sidecar.py`
    // replaces it with the real release binary before any bundle, and its
    // `--check` treats a placeholder as missing. Release builds never get
    // a placeholder: a bundle without the engine job must fail here.
    if std::env::var("PROFILE").as_deref() == Ok("debug") {
        if let Ok(triple) = std::env::var("TARGET") {
            let name = format!("model-lab-engine-job-{triple}{}", if triple.contains("windows") { ".exe" } else { "" });
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries").join(name);
            if !path.exists() {
                let _ = std::fs::create_dir_all(path.parent().unwrap());
                let _ = std::fs::write(&path, b"");
            }
        }
    }
    tauri_build::build()
}
