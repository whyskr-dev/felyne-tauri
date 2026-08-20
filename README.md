# felyne-tauri

Native desktop shell for [pwa.felyne.app](https://pwa.felyne.app) — a thin
Tauri 2 wrapper that loads the Felyne PWA in a system WebView and bridges the
few native features the web can't reach on its own. Desktop-only by design.

## Why a shell at all?

Felyne ships PWA-first with no App Store / Play Store builds on purpose (see
[why we don't trust the app stores](https://felyne.app/blog/2026-08-08-why-we-dont-trust-the-app-stores)).
A native shell changes none of that. It's a signed desktop binary distributed
via GitHub releases that:

- loads the PWA at its real origin (`https://pwa.felyne.app`), so the app is
  always the latest deploy — no store review lag,
- locks navigation to that origin, so a bad link can't hijack the window,
- routes external links to the system browser,
- shows native OS notifications while the app is running, driven by the web
  app's existing Realtime events (Web Push cannot work inside a WebView).

## Scope

| Capability | Status |
| --- | --- |
| macOS / Windows / Linux | planned |
| iOS / Android native builds | out of scope (PWA covers them) |
| Native push (APNs / FCM) | out of scope |
| Closed-app desktop notifications | out of scope (v1 notifies while running) |
| Notification tap → conversation routing | deferred (desktop backends expose no click callback) |
| Deep links / custom URL scheme | deferred |

## Repo layout

```
├── src-tauri/
│   ├── src/
│   │   ├── main.rs        # entrypoint
│   │   ├── lib.rs         # builder, navigation lock, window setup
│   │   ├── commands.rs    # commands exposed to the bridge
│   │   └── init.js        # injected: __FELYNE_SHELL__ bridge + link routing
│   ├── capabilities/
│   │   ├── local.json     # local splash permissions
│   │   └── remote.json    # narrow surface granted to pwa.felyne.app
│   ├── icons/             # generated from src-tauri/icons/icon-source.png
│   ├── tauri.conf.json    # base config
│   └── tauri.*.conf.json  # per-OS overrides
├── ui/                    # local splash + offline/error screen
└── .github/workflows/     # CI + release
```

## How it works

1. The window loads the bundled `ui/` splash locally.
2. The splash probes `https://pwa.felyne.app`; on success it navigates there,
   on failure it shows an offline state with a Retry button.
3. `on_navigation` allows only the PWA origin, its same-origin paths, local
   bundled assets, and the dev server (`localhost`) in debug builds.
4. An init script injects `window.__FELYNE_SHELL__` with a small fail-closed
   bridge (notify, openExternal, notification permission), and rewrites
   off-origin / `target=_blank` links to the system browser.
5. The web app detects the shell via `window.__FELYNE_SHELL__`, and a thin
   web-app module turns incoming Realtime events into `notify()` calls.

The remote origin only ever receives the permissions in
`capabilities/remote.json`; the shell never holds keys or touches Supabase.

## Development

Prerequisites: Rust (stable), Node 20+, and the Tauri system deps for your OS
([docs](https://tauri.app/start/prerequisites/)).

```bash
# point at the local web app in dev (optional)
FELYNE_APP_URL=http://localhost:5173  # not yet wired; see below

cargo tauri dev
```

> Dev URL: the navigation lock allows `localhost` in debug builds so you can
> point `REMOTE_ORIGIN` at your local Vite server while developing the web app.
> That wiring is left as a follow-up.

Rebuild icons after changing `src-tauri/icons/icon-source.png`:

```bash
npx @tauri-apps/cli icon src-tauri/icons/icon-source.png -o src-tauri/icons
```

## Building / releasing

```bash
cargo tauri build          # debug bundle for the current OS
cargo tauri build --release --target aarch64-apple-darwin   # example target
```

CI (`build-desktop.yml`) builds macOS (arm64 + x86_64), Windows, and Linux on
every push to `main`. Tagging `v*` triggers `release.yml`, which produces
signed bundles, attaches them to a GitHub release, and (once the updater
plugin is wired in) publishes signed updater artifacts.

Required secrets for the release workflow: `TAURI_SIGNING_PRIVATE_KEY`,
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and for macOS notarization
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
`APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`.

## Follow-ups

- Tray icon + close-to-tray so notifications keep working when the window is
  hidden.
- Notification tap → conversation routing via a platform-specific native
  backend (the official plugin's desktop backend has no click callback).
- Self-update via `tauri-plugin-updater` (config is stubbed to inactive).
- `FELYNE_APP_URL` override for pointing the shell at a local web app.