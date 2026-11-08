fn main() {
    let attrs = tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
        "audio_defaults", "audio_fix", "open_external", "app_info", "set_startup", "osd_done",
    ]));
    tauri_build::try_build(attrs).expect("tauri-build");
}
