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
    use super::*;
    use std::path::Path;

    /// The authoritative shell bridge surface. A command must appear here, in
    /// `build.rs`'s AppManifest, and as an `allow-shell-*` grant in
    /// `capabilities/*.json`; if the web page calls it, it must also appear in
    /// `init.js`. The regression tests below fail if any of those drift apart,
    /// so the bridge can't silently grow more powerful than the capability
    /// files intend.
    const SHELL_COMMANDS: &[&str] = &[
        "shell_notifications_permission",
        "shell_notify",
        "shell_open_external",
        "shell_platform",
        "shell_request_notification_permission",
        "shell_version",
    ];

    fn repo_path(relative: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
    }

    fn sorted_shell_commands() -> Vec<String> {
        let mut names: Vec<String> = SHELL_COMMANDS.iter().map(|s| s.to_string()).collect();
        names.sort();
        names
    }

    /// All `#[tauri::command]` functions defined in commands.rs.
    fn registered_command_fns() -> Vec<String> {
        let src = std::fs::read_to_string(repo_path("src/commands.rs")).expect("read commands.rs");
        let mut names = Vec::new();
        let mut pending_command = false;
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed == "#[tauri::command]" {
                pending_command = true;
                continue;
            }
            if pending_command && trimmed.starts_with("pub fn ") {
                let name = trimmed
                    .trim_start_matches("pub fn ")
                    .split(['(', ' '])
                    .next()
                    .expect("fn name");
                names.push(name.to_string());
                pending_command = false;
            }
        }
        names.sort();
        names
    }

    /// Command names declared in the build.rs AppManifest.
    fn app_manifest_commands() -> Vec<String> {
        let src = std::fs::read_to_string(repo_path("build.rs")).expect("read build.rs");
        let mut names = Vec::new();
        for line in src.lines() {
            let Some(inner) = line.trim().strip_prefix('"') else {
                continue;
            };
            let name = inner.split('"').next().unwrap_or("");
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                names.push(name.to_string());
            }
        }
        names.sort();
        names
    }

    /// Command names the injected bridge invokes via IPC.
    fn init_js_invocations() -> Vec<String> {
        let src = std::fs::read_to_string(repo_path("src/init.js")).expect("read init.js");
        let mut names = Vec::new();
        let needle = "invoke('";
        let mut rest = src.as_str();
        while let Some(idx) = rest.find(needle) {
            rest = &rest[idx + needle.len()..];
            if let Some(end) = rest.find('\'') {
                names.push(rest[..end].to_string());
            }
        }
        names.sort();
        names.dedup();
        names
    }

    /// `allow-shell-*` grants in a capability file, mapped back to command
    /// names (slug -> underscore).
    fn capability_allow_entries(file: &str) -> Vec<String> {
        let text =
            std::fs::read_to_string(repo_path(file)).expect("read capability file");
        let json: serde_json::Value =
            serde_json::from_str(&text).expect("parse capability json");
        let permissions = json["permissions"]
            .as_array()
            .expect("permissions array in capability");
        let mut names = Vec::new();
        for entry in permissions {
            let id = entry.as_str().expect("permission entry is a string");
            if let Some(slug) = id.strip_prefix("allow-") {
                let name = slug.replace('-', "_");
                if SHELL_COMMANDS.contains(&name.as_str()) {
                    names.push(name);
                }
            }
        }
        names.sort();
        names
    }

    // ------------------------------------------------------------------
    // Pure command behavior
    // ------------------------------------------------------------------

    #[test]
    fn platform_is_stable_and_known() {
        let platform = shell_platform();
        assert_eq!(platform, crate::platform_name());
        assert!(
            matches!(platform.as_str(), "macos" | "windows" | "linux" | "unknown"),
            "unexpected platform: {platform}"
        );
    }

    #[test]
    fn version_matches_package_and_is_semverish() {
        let version = shell_version();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        let parts: Vec<&str> = version.split('.').collect();
        assert!(parts.len() >= 2, "version is not semver-ish: {version}");
        assert!(
            parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
            "version is not semver-ish: {version}"
        );
    }

    // shell_notify / shell_open_external / shell_notifications_permission /
    // shell_request_notification_permission all take an AppHandle and touch
    // real OS surfaces (notification dialogs, system browser, native
    // notifications), so they are not exercised directly by unit tests. Their
    // fail-closed behavior on untrusted input is the validate_external_url
    // gate below (for shell_open_external) plus Tauri's own argument
    // deserialization for the rest; the command-surface regression guards at
    // the bottom keep that trust boundary from growing.

    // ------------------------------------------------------------------
    // shell_open_external fail-closed gate
    // ------------------------------------------------------------------

    #[test]
    fn accepts_http_and_https() {
        assert!(validate_external_url("https://pwa.felyne.app/").is_ok());
        assert!(validate_external_url("http://example.com").is_ok());
    }

    #[test]
    fn accepts_urls_with_paths_queries_and_fragments() {
        for url in [
            "https://pwa.felyne.app/?open=abc",
            "https://pwa.felyne.app/invite/token",
            "https://example.com/x?a=1#frag",
            "https://sub.example.com:8443/path?q=1",
        ] {
            assert!(validate_external_url(url).is_ok(), "should accept {url}");
        }
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

    #[test]
    fn refuses_malformed_input() {
        for url in [
            "",
            "   ",
            "example.com",
            "not a url",
            "http://",
            "https://exa mple.com",
        ] {
            assert!(validate_external_url(url).is_err(), "should refuse {url:?}");
        }
    }

    // ------------------------------------------------------------------
    // Command-surface regression guards
    // ------------------------------------------------------------------

    #[test]
    fn every_registered_command_is_documented_in_shell_commands() {
        assert_eq!(registered_command_fns(), sorted_shell_commands());
    }

    #[test]
    fn build_manifest_lists_every_registered_command() {
        assert_eq!(app_manifest_commands(), sorted_shell_commands());
    }

    #[test]
    fn bridge_invokes_only_registered_commands() {
        let invoked = init_js_invocations();
        assert!(!invoked.is_empty(), "init.js should invoke commands");
        for cmd in &invoked {
            assert!(
                SHELL_COMMANDS.contains(&cmd.as_str()),
                "init.js invokes {cmd}, which is not a registered shell command"
            );
        }
    }

    #[test]
    fn capabilities_grant_every_registered_command() {
        for file in ["capabilities/local.json", "capabilities/remote.json"] {
            assert_eq!(
                capability_allow_entries(file),
                sorted_shell_commands(),
                "{file} must grant every shell command"
            );
        }
    }
}