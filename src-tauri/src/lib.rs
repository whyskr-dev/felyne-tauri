mod commands;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
use url::Url;

/// The remote app origin. All in-app navigation is locked to this origin
/// (plus the localhost dev server while running a debug build).
const REMOTE_ORIGIN: &str = "https://pwa.felyne.app";

/// Parses a `FELYNE_APP_URL`-style value into a valid http(s) URL, or `None`.
#[cfg(any(test, debug_assertions))]
fn parse_app_url(env_value: Option<&str>) -> Option<String> {
    let url = env_value?;
    let parsed = Url::parse(url).ok()?;
    if matches!(parsed.scheme(), "http" | "https") {
        Some(parsed.to_string())
    } else {
        None
    }
}

/// The URL the shell should load the app from. Debug builds honor the
/// `FELYNE_APP_URL` env var (e.g. a local Vite server or a staging deploy) so
/// the PWA can be developed against the shell. Release builds ignore the env
/// var entirely, so a tampered environment can never redirect the shipped app.
fn app_url() -> String {
    #[cfg(debug_assertions)]
    {
        std::env::var("FELYNE_APP_URL")
            .ok()
            .and_then(|v| parse_app_url(Some(&v)))
            .unwrap_or_else(|| REMOTE_ORIGIN.to_string())
    }
    #[cfg(not(debug_assertions))]
    {
        REMOTE_ORIGIN.to_string()
    }
}

/// Origin (scheme://host[:port]) of the app URL, for navigation allow-listing.
fn app_url_origin() -> String {
    Url::parse(&app_url())
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| REMOTE_ORIGIN.to_string())
}

pub fn platform_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "macos"
    }
    #[cfg(target_os = "windows")]
    {
        "windows"
    }
    #[cfg(target_os = "linux")]
    {
        "linux"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown"
    }
}

/// Navigation lock: only the PWA origin, its same-origin paths, and local
/// bundled assets may load in the shell webview. Everything else is denied so
/// a malicious or off-site link cannot hijack the whole window.
fn allowed_navigation(url: &Url) -> bool {
    match url.scheme() {
        // Local bundled assets. macOS/Linux use a custom `tauri` scheme;
        // Windows serves them over http://tauri.localhost.
        "tauri" => true,
        "http" | "https" => {
            let host = url.host_str().unwrap_or("");
            if host == "tauri.localhost" {
                return true;
            }
            if host == "localhost" || host == "127.0.0.1" {
                // Dev web app only; never allow these in release.
                return cfg!(debug_assertions);
            }
            url.origin().ascii_serialization() == REMOTE_ORIGIN
                || (cfg!(debug_assertions)
                    && url.origin().ascii_serialization() == app_url_origin())
        }
        _ => false,
    }
}

/// Injected into every page the shell loads. Exposes the narrow
/// `window.__FELYNE_SHELL__` bridge and routes external links to the system
/// browser. Fail-closed: without the Tauri IPC the page behaves as a normal
/// webview and the app keeps working without shell features.
fn init_script() -> String {
    include_str!("./init.js")
        .replace("__SHELL_VERSION__", env!("CARGO_PKG_VERSION"))
        .replace("__SHELL_PLATFORM__", platform_name())
        .replace("__FELYNE_APP_URL__", &app_url())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch: focus the existing window instead of starting a
            // duplicate process.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(tauri::generate_handler![
            commands::shell_platform,
            commands::shell_version,
            commands::shell_notify,
            commands::shell_open_external,
            commands::shell_notifications_permission,
            commands::shell_request_notification_permission,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("felyne")
                .inner_size(1200.0, 800.0)
                .min_inner_size(380.0, 600.0)
                .center()
                .on_navigation(allowed_navigation)
                .on_new_window(move |url, _features| {
                    // window.open() / target=_blank → system browser.
                    let _ = handle.opener().open_url(url.as_str(), None::<String>);
                    tauri::webview::NewWindowResponse::Deny
                })
                .initialization_script(init_script())
                .build()?;
            window.show()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running felyne shell");
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn nav(url: &str) -> bool {
        allowed_navigation(&Url::parse(url).expect("valid test url"))
    }

    /// Serializes tests that read or mutate `FELYNE_APP_URL`.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn allows_remote_origin_and_its_paths() {
        assert!(nav("https://pwa.felyne.app/"));
        assert!(nav("https://pwa.felyne.app/?open=abc"));
        assert!(nav("https://pwa.felyne.app/invite/token"));
    }

    #[test]
    fn denies_foreign_origins() {
        assert!(!nav("https://evil.example.com/"));
        assert!(!nav("https://felyne.app/"));
        assert!(!nav("https://pwa.felyne.app.evil.com/"));
        // http downgrade of the PWA origin is not the PWA origin.
        assert!(!nav("http://pwa.felyne.app/"));
    }

    #[test]
    fn denies_non_http_schemes() {
        assert!(!nav("javascript:alert(1)"));
        assert!(!nav("data:text/html,<h1>hi</h1>"));
        assert!(!nav("file:///etc/passwd"));
    }

    #[test]
    fn allows_local_bundled_assets() {
        assert!(nav("tauri://localhost/index.html"));
        assert!(nav("http://tauri.localhost/index.html"));
    }

    #[test]
    fn dev_server_only_in_debug_builds() {
        assert_eq!(nav("http://localhost:5173/"), cfg!(debug_assertions));
    }

    #[test]
    fn init_script_is_fully_interpolated() {
        let _guard = ENV_LOCK.lock().unwrap();
        let script = init_script();
        assert!(!script.contains("__SHELL_VERSION__"));
        assert!(!script.contains("__SHELL_PLATFORM__"));
        assert!(!script.contains("__FELYNE_APP_URL__"));
        assert!(script.contains(env!("CARGO_PKG_VERSION")));
        assert!(script.contains(platform_name()));
        assert!(script.contains(&app_url()));
        assert!(script.contains("__FELYNE_SHELL__"));
    }

    #[test]
    fn parse_app_url_accepts_http_and_https() {
        assert_eq!(
            parse_app_url(Some("http://localhost:5173/")),
            Some("http://localhost:5173/".to_string())
        );
        assert_eq!(
            parse_app_url(Some("https://staging.felyne.app/")),
            Some("https://staging.felyne.app/".to_string())
        );
    }

    #[test]
    fn parse_app_url_rejects_non_http_and_malformed() {
        assert_eq!(parse_app_url(None), None);
        assert_eq!(parse_app_url(Some("file:///etc/passwd")), None);
        assert_eq!(parse_app_url(Some("javascript:alert(1)")), None);
        assert_eq!(parse_app_url(Some("not a url")), None);
        assert_eq!(parse_app_url(Some("")), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn felyne_app_url_origin_is_navigable_when_set() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("FELYNE_APP_URL", "https://staging.felyne.app/");
        assert!(nav("https://staging.felyne.app/"));
        assert!(nav("https://staging.felyne.app/invite/token"));
        std::env::remove_var("FELYNE_APP_URL");
    }
}