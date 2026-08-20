use serde::Serialize;
use tauri::plugin::PermissionState;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/// Update metadata surfaced to the web app / update notification. `url` is
/// always a human-facing link (the GitHub release page) so non-updatable
/// platforms (mobile) can still send the user somewhere to download.
#[derive(Debug, Clone, Serialize)]
pub struct ShellUpdate {
    pub version: String,
    pub notes: Option<String>,
    pub url: String,
}

/// The desktop updater reads the same `latest.json` the release workflow
/// publishes. Mobile has no in-place updater, so the shell fetches this file
/// directly and compares versions to tell the user an update exists.
#[cfg(not(desktop))]
const UPDATE_ENDPOINT: &str =
    "https://github.com/whyskr-dev/felyne-tauri/releases/latest/download/latest.json";
const RELEASE_PAGE: &str = "https://github.com/whyskr-dev/felyne-tauri/releases/tag";

/// True when `candidate` is a newer semver than the running shell.
fn is_newer(candidate: &str) -> bool {
    let Ok(current) = semver::Version::parse(env!("CARGO_PKG_VERSION")) else {
        return false;
    };
    semver::Version::parse(candidate)
        .map(|c| c > current)
        .unwrap_or(false)
}

/// Check whether a newer shell version exists. Desktop consults the updater
/// plugin (which also drives the actual install); mobile reads the same
/// `latest.json` over HTTP and compares versions.
pub async fn check_for_update(app: AppHandle) -> Result<Option<ShellUpdate>, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;
        let updater = app.updater().map_err(|e| e.to_string())?;
        match updater.check().await.map_err(|e| e.to_string())? {
            Some(update) => {
                let version = update.version.to_string();
                // Defense-in-depth: never surface a downgrade or garbage feed.
                if !is_newer(&version) {
                    return Ok(None);
                }
                let url = format!("{RELEASE_PAGE}/v{version}");
                Ok(Some(ShellUpdate {
                    notes: update.body,
                    version,
                    url,
                }))
            }
            None => Ok(None),
        }
    }
    #[cfg(not(desktop))]
    {
        let body = tauri::async_runtime::spawn_blocking(move || {
            ureq::get(UPDATE_ENDPOINT)
                .call()
                .map_err(|e| e.to_string())?
                .into_string()
                .map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())??;
        let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let version = json.get("version").and_then(|v| v.as_str()).unwrap_or("");
        if !is_newer(version) {
            return Ok(None);
        }
        Ok(Some(ShellUpdate {
            version: version.to_string(),
            notes: json.get("notes").and_then(|n| n.as_str()).map(String::from),
            url: format!("{RELEASE_PAGE}/v{version}"),
        }))
    }
}

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

/// Does a newer shell release exist? Returns `None` when already current or
/// when the check cannot be performed (offline, malformed feed). Mobile uses
/// this to tell the user an update is available for manual download.
#[tauri::command]
pub async fn shell_check_for_update(app: AppHandle) -> Result<Option<ShellUpdate>, String> {
    check_for_update(app).await
}

/// Install and restart into a pending update. Desktop-only: Tauri's updater
/// cannot replace a mobile app in place, so mobile returns an error and the
/// web app directs the user to the release page instead.
#[tauri::command]
pub async fn shell_install_update(app: AppHandle) -> Result<(), String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_updater::UpdaterExt;
        let updater = app.updater().map_err(|e| e.to_string())?;
        let update = updater
            .check()
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no update available".to_string())?;
        update
            .download_and_install(|_current, _total| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        let _ = app.emit_to("main", "shell:update-installed", &update.version.to_string());
        tauri::process::restart(&app.env());
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Err("mobile has no in-place updater; download the new build from the release page".into())
    }
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
        "shell_check_for_update",
        "shell_install_update",
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
            if pending_command
                && (trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn "))
            {
                let name = trimmed
                    .trim_start_matches("pub async fn ")
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
    ///
    /// `plugin:`-prefixed invocations (e.g. `plugin:mobile-push|get_token`)
    /// target third-party plugin commands directly and are excluded here —
    /// they are governed by the plugins' own permission sets, not the shell
    /// command surface.
    fn init_js_invocations() -> Vec<String> {
        let src = std::fs::read_to_string(repo_path("src/init.js")).expect("read init.js");
        let mut names = Vec::new();
        let needle = "invoke('";
        let mut rest = src.as_str();
        while let Some(idx) = rest.find(needle) {
            rest = &rest[idx + needle.len()..];
            if let Some(end) = rest.find('\'') {
                let name = &rest[..end];
                if !name.starts_with("plugin:") {
                    names.push(name.to_string());
                }
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
            matches!(
                platform.as_str(),
                "macos" | "windows" | "linux" | "ios" | "android" | "unknown"
            ),
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
        assert!(validate_external_url("https://felyne.app/").is_ok());
        assert!(validate_external_url("http://example.com").is_ok());
    }

    #[test]
    fn accepts_urls_with_paths_queries_and_fragments() {
        for url in [
            "https://felyne.app/?open=abc",
            "https://felyne.app/invite/token",
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

    // shell_check_for_update / shell_install_update hit the network and OS
    // installers, so they are not exercised directly. The pure version gate
    // below is tested, and the command-surface guards keep the bridge
    // contract pinned.

    #[test]
    fn update_is_newer_compares_semver() {
        let current = env!("CARGO_PKG_VERSION");
        assert!(!is_newer(current), "current is not newer than itself");
        assert!(!is_newer("0.0.1"));
        assert!(!is_newer("garbage"));
        // Parse the current version and prove a patch bump reads as newer.
        let bump = semver::Version::parse(current)
            .map(|mut v| {
                v.patch += 1;
                v.to_string()
            })
            .unwrap();
        assert!(is_newer(&bump), "{bump} should be newer than {current}");
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
        for file in [
            "capabilities/local.json",
            "capabilities/remote.json",
            "capabilities/dev.json",
        ] {
            assert_eq!(
                capability_allow_entries(file),
                sorted_shell_commands(),
                "{file} must grant every shell command"
            );
        }
    }
}