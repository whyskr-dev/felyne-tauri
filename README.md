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
# point the shell at a local/staging build of the PWA (debug builds only)
FELYNE_APP_URL=http://localhost:5173 cargo tauri dev
```

> `FELYNE_APP_URL` is honored in **debug builds only** — the splash navigates to
> it and the navigation lock allow-lists its origin. Release builds ignore the
> env var and always load `https://pwa.felyne.app`, so a tampered environment
> can't redirect the shipped app. The matching shell-bridge grants for the dev
> server live in `capabilities/dev.json` (localhost / 127.0.0.1); that
> capability is inert in release because the navigation lock denies `localhost`
> except in debug builds.

Rebuild icons after changing `src-tauri/icons/icon-source.png`:

```bash
npx @tauri-apps/cli icon src-tauri/icons/icon-source.png -o src-tauri/icons
```

## Building / releasing

Run the Tauri CLI from `src-tauri/` (it resolves bundle paths against the
current directory):

```bash
cd src-tauri
cargo tauri build                                   # bundle for this OS
npx @tauri-apps/cli build --target aarch64-apple-darwin   # other targets
```

### CI / releasing

**GitLab is the development host; GitHub does the free building.**

- **GitLab** (`.gitlab-ci.yml`) — runs `cargo test` + clippy on pushes, merge
  requests, and tags. Kept minimal so the free-tier minutes stay available for
  development.
- **GitHub Actions** (`.github/workflows/`) — the repo is mirrored to a public
  GitHub repo, where Actions runs all three OSes for free (public repos get
  unlimited standard-runner minutes):
  - `ci.yml` — test + clippy + bundle build on macOS, Ubuntu, Windows for every
    push to `main` and every PR.
  - `release.yml` — on a `v*` tag: tests, then builds macOS (arm64 + x86_64),
    Ubuntu (`.deb` + `.AppImage`), and Windows (`.msi`/`.exe`), and publishes a
    **draft** release with the signed updater artifacts attached.

### Making a release

1. Push to GitHub (GitLab remains the main remote):
   ```bash
   git push gitlab main
   git push github main
   git tag -a v0.1.0 -m "felyne v0.1.0"
   git push github v0.1.0
   ```
2. `release.yml` builds and uploads assets to a **draft** release.
3. Open the draft release, sanity-check the `.dmg`, `.msi`/`.exe`, and
   `.deb`/`.AppImage` assets, then publish.

### Signing & secrets

The updater signing key pair lives at `~/.config/felyne-tauri/felyne-updater.key`
(+ `.pub`). The **public** key is committed in `tauri.conf.json`
(`plugins.updater.pubkey`); the **private** key stays out of the repo and is
only needed in CI as a secret. Back it up — if it's lost, published update
signatures can't be produced again.

Set these as **GitHub Actions secrets** (Settings → Secrets and variables →
Actions) on the GitHub mirror repo:

| Secret | Needed for |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Signing updater artifacts (content of the `.key` file) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Updater key password (empty here) |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | Codesigning the macOS app |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | macOS notarization |

Windows and Linux artifacts are unsigned (acceptable for v1). macOS builds are
signed + notarized only when the Apple secrets above are set; without them the
builds still succeed unsigned.

## Follow-ups

- Tray icon + close-to-tray so notifications keep working when the window is
  hidden.
- Notification tap → conversation routing via a platform-specific native
  backend (the official plugin's desktop backend has no click callback).
- Self-update via `tauri-plugin-updater` (config is stubbed to inactive; the
  signing key is already generated).