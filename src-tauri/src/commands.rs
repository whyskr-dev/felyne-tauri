use tauri::plugin::PermissionState;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub fn shell_platform() -> String {
    crate::platform_name().to_string()
}

#[tauri::command]
pub fn shell_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Is the OS-level notification permission already granted for this shell?
#[tauri::command]
pub fn shell_notifications_permission(app: AppHandle) -> bool {
    app.notification()
        .permission_state()
        .map(|state| state == PermissionState::Granted)
        .unwrap_or(false)
}

/// Ask the OS for notification permission (the Settings toggle calls this).
#[tauri::command]
pub fn shell_request_notification_permission(app: AppHandle) -> bool {
    match app.notification().request_permission() {
        Ok(state) => state == PermissionState::Granted,
        Err(_) => false,
    }
}

/// Show a native OS notification.
///
/// `data` is reserved for tap routing. The underlying desktop backend has no
/// click callback today, so routing lives in the web app once the window
/// regains focus — kept in the signature for bridge-contract stability.
#[tauri::command]
pub fn shell_notify(
    app: AppHandle,
    title: String,
    body: String,
    data: Option<serde_json::Value>,
) -> Result<(), String> {
    let _ = data;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| e.to_string())
}

/// Open a URL in the system browser. Refuses anything that is not http(s).
#[tauri::command]
pub fn shell_open_external(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = validate_external_url(&url)?;
    app.opener()
        .open_url(parsed.as_str(), None::<String>)
        .map_err(|e| e.to_string())
}

fn validate_external_url(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("refusing to open a non-http(s) URL".to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::validate_external_url;

    #[test]
    fn accepts_http_and_https() {
        assert!(validate_external_url("https://pwa.felyne.app/").is_ok());
        assert!(validate_external_url("http://example.com").is_ok());
    }

    #[test]
    fn refuses_other_schemes() {
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "mailto:hi@example.com",
            "data:text/html,hi",
        ] {
            assert!(validate_external_url(url).is_err(), "should refuse {url}");
        }
    }
}