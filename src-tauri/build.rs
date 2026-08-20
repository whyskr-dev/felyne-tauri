fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "shell_platform",
            "shell_version",
            "shell_notify",
            "shell_open_external",
            "shell_notifications_permission",
            "shell_request_notification_permission",
        ])),
    )
    .expect("failed to run tauri build script");
}
