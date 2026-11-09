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
    tauri_build::build()
}
