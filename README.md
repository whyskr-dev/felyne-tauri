# felyne-tauri

Native shell for [felyne.app](https://felyne.app) — a thin Tauri 2 wrapper
that loads the Felyne PWA in a system WebView and bridges the few native
features the web can't reach on its own. Ships desktop binaries (macOS /
Windows / Linux) and sideloadable mobile builds (Android `.apk` + iOS `.ipa`),
all built in CI — never on a personal device.

## Why a shell at all?

Felyne ships PWA-first with no App Store / Play Store builds on purpose (see
[why we don't trust the app stores](https://felyne.app/blog/2026-08-08-why-we-dont-trust-the-app-stores)).
A native shell changes none of that. It's a signed desktop binary — and, for
people who want an app icon, a sideloaded mobile build — distributed via GitHub
releases that:

- loads the PWA at its real origin (`https://felyne.app`), so the app is
  always the latest deploy — no store review lag,
- locks navigation to that origin, so a bad link can't hijack the window,
- routes external links to the system browser,
- shows native OS notifications while the app is running, driven by the web
  app's existing Realtime events (Web Push cannot work inside a WebView),
- on mobile, registers the device for **real push** (APNs on iOS / FCM on
  Android) and delivers notifications even when the app is closed — the PWA
  backend sends pushes, the shell only registers the device token and forwards
  taps,
- checks for updates and, on desktop, installs them in place (self-updater).

## Scope

| Capability | Status |
| --- | --- |
| macOS / Windows / Linux | done (v1), updater added in v2.0 |
| iOS / Android sideload builds (`.ipa` / `.apk` from GitHub releases) | done (v2.0) |
| Native push (APNs / FCM) — closed-app notifications on mobile | done (v2.0); needs Firebase + Apple signing setup |
| Closed-app desktop notifications | out of scope (desktop notifies while running) |
| Notification tap → conversation routing | iOS: deferred (plugin doesn't deliver iOS tap events yet); Android: event wired, backend needs to handle it |
| Deep links / custom URL scheme | deferred |

## Repo layout

```
├── src-tauri/
│   ├── src/
│   │   ├── main.rs        # entrypoint
│   │   ├── lib.rs         # builder, navigation lock, window setup, update check
│   │   ├── commands.rs    # commands exposed to the bridge
│   │   └── init.js        # injected: __FELYNE_SHELL__ bridge + link routing
│   ├── capabilities/
│   │   ├── local.json     # local splash permissions
│   │   └── remote.json    # narrow surface granted to felyne.app
│   ├── gen/
│   │   ├── apple/         # committed iOS Xcode project (XcodeGen)
│   │   └── android/       # committed Android Studio project (Gradle)
│   ├── icons/             # generated from src-tauri/icons/icon-source.png
│   ├── tauri.conf.json    # base config
│   └── tauri.*.conf.json  # per-OS overrides
├── package.json           # pins @tauri-apps/cli (required by the iOS build)
├── ui/                    # local splash + offline/error screen
└── .github/workflows/     # CI + release
```

## How it works

1. The window loads the bundled `ui/` splash locally.
2. The splash probes `https://felyne.app`; on success it navigates there,
   on failure it shows an offline state with a Retry button.
3. `on_navigation` allows only the PWA origin, its same-origin paths, local
   bundled assets, and the dev server (`localhost`) in debug builds.
4. An init script injects `window.__FELYNE_SHELL__` with a small fail-closed
   bridge (notify, openExternal, notification permission, update check/install,
   and — on mobile — push token registration), and rewrites off-origin /
   `target=_blank` links to the system browser.
5. The web app detects the shell via `window.__FELYNE_SHELL__`, and a thin
   web-app module turns incoming Realtime events into `notify()` calls. On
   mobile it also requests push permission and registers the APNs/FCM device
   token with felyne's backend, which then sends closed-app pushes.

**Updates.** On desktop the shell checks the same `latest.json` the updater
publishes: at startup it emits `shell:update-available` (and shows a native
notification); the web app can call `bridge.checkForUpdate()` /
`bridge.installUpdate()`, and the updater installs the new version in place and
restarts. Mobile has no in-place updater, so the shell fetches `latest.json`,
compares versions, and tells the web app an update exists via
`shell:update-available` (the web app points the user at the release page).

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
> env var and always load `https://felyne.app`, so a tampered environment
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
npx @tauri-apps/cli android build --apk            # Android (needs SDK/NDK)
npx @tauri-apps/cli ios build --target aarch64-apple-ios   # iOS (needs Xcode)
```

> The committed native projects live in `src-tauri/gen/`. Regenerate them only
> if the Tauri CLI version or config changes:
> `npx @tauri-apps/cli ios init` and `npx @tauri-apps/cli android init`
> (Android init needs the Android SDK present).

### CI / releasing

**GitLab is the development host; GitHub does the free building.**

- **GitLab** (`.gitlab-ci.yml`) — runs `cargo test` + clippy on pushes, merge
  requests, and tags. Kept minimal so the free-tier minutes stay available for
  development.
- **GitHub Actions** (`.github/workflows/`) — the repo is mirrored to a public
  GitHub repo, where Actions runs all three OSes for free (public repos get
  unlimited standard-runner minutes):
  - `ci.yml` — test + clippy + bundle build on macOS, Ubuntu, Windows for every
    push to `main` and every PR, plus a `mobile-check` job that compiles the
    mobile-only Rust code against `aarch64-linux-android`.
  - `release.yml` — on a `v*` tag: tests, then builds and attaches to the
    release (in order): desktop bundles (macOS universal, Ubuntu
    `.deb`/`.AppImage`, Windows `.msi`/`.exe`), the Android universal APK, the
    iOS ad-hoc `.ipa`, and finally mirrors every asset to GitLab. The Android
    and iOS jobs **skip themselves when their secrets aren't set yet**
    (`ANDROID_FIREBASE_JSON` and `IOS_MOBILE_PROVISION` respectively), so a
    release still succeeds without them.

### Making a release

1. Push to GitHub (GitLab remains the main remote):
   ```bash
   git push gitlab main
   git push github main
   git tag -a v2.0.0 -m "felyne v2.0.0"
   git push github v2.0.0
   ```
2. `release.yml` builds and uploads assets to the release.
3. Sanity-check the `.dmg`, `.msi`/`.exe`, `.deb`/`.AppImage`, `.apk`, and
   `.ipa` assets. iOS testers install the `.ipa` via the ad-hoc provisioning
   profile (their device UDID must be in it); Android testers sideload the
   `.apk`.

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
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | Codesigning the macOS app (optional) |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | macOS notarization (optional) |
| `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`, `ANDROID_KEY_BASE64` | Signing the Android APK (`.jks` keystore as base64) |
| `ANDROID_FIREBASE_JSON` | `google-services.json` (base64) — FCM on Android |
| `IOS_CERTIFICATE`, `IOS_CERTIFICATE_PASSWORD`, `IOS_MOBILE_PROVISION` | iOS ad-hoc signing (`.p12` + `.mobileprovision` as base64) |

Windows and Linux artifacts are unsigned. macOS builds are ad-hoc signed
(Gatekeeper will warn); add the `APPLE_*` secrets + env lines to `release.yml`
to sign + notarize properly.

### Android (FCM) setup

Push on Android uses FCM via `tauri-plugin-mobile-push`:

1. Create a Firebase project at [console.firebase.google.com](https://console.firebase.google.com/)
   and register an Android app with package name **`app.felyne`**.
2. Download `google-services.json`, base64 it, and store it as the
   `ANDROID_FIREBASE_JSON` secret. CI writes it to
   `src-tauri/gen/android/app/google-services.json` before building.
3. Generate an upload keystore (`keytool -genkey -v -keystore upload.jks -keyalg RSA -keysize 2048 -validity 10000 -alias upload`), base64 it
   for `ANDROID_KEY_BASE64`, and set `ANDROID_KEY_ALIAS` / `ANDROID_KEY_PASSWORD`.
   (A keystore was generated for this repo at
   `~/.config/felyne-tauri/felyne-upload.jks`, with its password alongside in
   `android-keystore.txt` — back those up; losing the keystore means future
   APKs can't update over installed ones. The three `ANDROID_KEY_*` secrets are
   already set on GitHub.)
4. The web/PWA backend sends pushes to the FCM registration token the shell
   registers.

### iOS (APNs) setup

Ad-hoc `.ipa` distribution needs an Apple Developer account ($99/yr):

1. Create a distribution certificate + an **ad-hoc** provisioning profile for
   bundle id **`app.felyne`** with the **Push Notifications** capability,
   including each tester's device UDID.
2. Export the cert as `.p12` and the profile as `.mobileprovision`; store both
   as base64 in `IOS_CERTIFICATE` / `IOS_MOBILE_PROVISION` and the cert password
   in `IOS_CERTIFICATE_PASSWORD`. CI imports them and signs with
   `aps-environment = production` (see the committed entitlements file).
3. The web/PWA backend sends pushes to the APNs token the shell registers.

## Follow-ups

- Tray icon + close-to-tray so notifications keep working when the window is
  hidden.
- Notification tap → conversation routing on iOS once
  `tauri-plugin-mobile-push` delivers iOS tap/received events (known plugin
  limitation; Android already emits them).
- Deep links / custom URL scheme.