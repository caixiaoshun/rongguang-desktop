fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "backend_ready", "app_info", "open_data_dir", "open_url",
            ]),
        ),
    ).expect("failed to build desktop command permissions");
}
