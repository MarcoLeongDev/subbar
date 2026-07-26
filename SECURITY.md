# Security Policy

## Reporting a Vulnerability

Please report security issues **privately** — do not open a public issue.

- Open a GitHub Security Advisory, or
- Email the maintainer directly.

We aim to acknowledge reports within 72 hours.

## Tested Versions

| Version | Status |
| ------- | ------ |
| 1.0.0  | Current |

## Security Posture

### API key storage
- API keys are stored in the OS native credential store via the `keyring` crate:
  **macOS Keychain**, **Windows Credential Manager**, **Linux Secret Service**.
- Keys are **never** written as plaintext to disk. The legacy plaintext `config.json`
  (pre-v1.2.0) is migrated to the keychain and deleted on first launch.
- Keys are loaded from the keychain into an in-memory `Mutex<String>` at startup and
  only sent to the Minimax API over HTTPS with a `Bearer` token. The frontend receives
  a redacted value by default and receives plaintext only after an explicit reveal action.
- Keyring entry creation failures are handled gracefully (fall back to empty key,
  log a warning) — no panic on headless or unsupported platforms.

### Network
- All Minimax API calls use **HTTPS** to `api.minimaxi.com` / `api.minimax.io`.
- Backend applies a **15s request timeout** so a stalled endpoint cannot hang the UI.
- A **shared `reqwest::Client`** (initialised once via `std::sync::OnceLock`) is used
  for all outgoing requests so TLS sessions are reused.
- API keys are **redacted** in debug-level logs (`Bearer sk-...XXXX`).
- No third-party telemetry, analytics, or beaconing.
- Default log level is `info`; no key material or secrets are emitted at that level.
  Set `RUST_LOG=debug` only when actively troubleshooting, and review logs before sharing.
- The backend contacts only two outbound hosts: `https://api.minimaxi.com` and `https://api.minimax.io`.
  These are compile-time constants and are visible in the binary; they are not secrets, but
  rotating endpoints in a future release requires a binary update.
- Certificate validation uses the Mozilla CA roots bundled by `webpki-roots` through
  `reqwest` + `rustls-tls`. No certificate pinning is implemented.

### Frontend / Webview
- A **strict Content Security Policy** is enforced in `tauri.conf.json`:
  `default-src 'self'`, with IPC-only `connect-src`.
  Remote script and resource loads are blocked entirely.

> **Resolved [L1] (v1.2.6+):** The app no longer uses inline `<script>` or `<style>`
> tags. JavaScript and CSS are externalized to `src/main.js` and `src/style.css`,
> referenced via `<script src="main.js">` and `<link rel="stylesheet" href="style.css">`.
> The CSP therefore uses `script-src 'self'` and `style-src 'self'` with no
> `'unsafe-inline'` token. This was the recommended migration path from the original
> trade-off.

- Error messages from the API and from exceptions are rendered with textContent and
  createElement-based DOM APIs (not innerHTML), preventing DOM XSS via server-controlled
  response fields.
- i18n strings in this codebase are developer-controlled and never sourced from user input
  or API responses. Rendering paths that consume i18n strings isolate them from
  external or user-editable sources.
- API-key input is validated for the `sk-` prefix before it is persisted.
- localStorage is used only for non-sensitive UI preferences
  (theme, language, refresh interval, marker toggles, endpoint selection).
  API keys are never stored in localStorage.
- Zero use of eval(), new Function(), exec(), setTimeout(string), or any
  other dynamic code-execution vector.

### Capabilities (least privilege)
Only the permission needed by the webview is granted in `capabilities/default.json`:
- `core:window:allow-start-dragging`

The `shell` plugin is **not loaded** and no `shell:*` permission is granted.

> **Anchored design decision [L3]:** `macOSPrivateApi: true` is set in `tauri.conf.json`
> because the `tauri-plugin-liquid-glass` plugin requires it. This prevents distribution
> via the Mac App Store. Direct distribution via GitHub Releases or direct download is
> unaffected. Revisit if the plugin is ever replaced with a MAS-safe alternative.

### macOS entitlements
- `hardenedRuntime: true` is enabled in `tauri.conf.json`.

  > **Anchored trade-off [L4]:** Two entitlements are required by the liquid-glass
  > plugin and **reduce macOS code-signing enforcement**:
  > - `com.apple.security.cs.allow-unsigned-executable-memory`
  > - `com.apple.security.cs.disable-library-validation`
  >
  > These are accepted for direct distribution. Re-evaluate before attempting
  > notarization for Mac App Store submission.

### Binary hygiene
- All build artifacts (`target/`, `*.dmg`, `*.app`) are excluded by `.gitignore`.
- No binary blobs or compiled objects are committed to the repository.

## Dependency Auditing

To audit Rust dependencies for known CVEs from the `src-tauri/` directory:

```bash
cargo install cargo-audit
cd src-tauri && cargo audit
```

This repo ships a pinned `Cargo.lock` to ensure reproducible builds.

### Known upstream deprecation

`block@0.1.6` is a transitive dependency of `objc@0.2.7` (locked via `Cargo.lock`).
Rust emits a future-incompat warning for this crate, which will become a hard error
in a future compiler release. The upstream `objc` crate has not yet released a
`0.2.8` that removes the dependency. Mitigation: pinning is in place in
`Cargo.lock`; upgrade `objc` as soon as a compatible release is available,
and revisit if replacement Objective-C bindings become necessary.

### Dependency advisory warnings

An offline, locally cached `cargo audit` scan reports no known vulnerabilities and
17 advisory warnings in the complete cross-platform lockfile.

The following unmaintained crates are present in the macOS dependency graph through
`tauri-utils` → `urlpattern`:

- **`unic-*@0.9.0` — RUSTSEC-2025-0075/0080/0081/0098/0100**:
  `unic-char-property`, `unic-char-range`, `unic-common`, `unic-ucd-ident`,
  `unic-ucd-version`. Upgrade Tauri's dependency chain when a maintained replacement
  becomes available.

The remaining warnings are reachable only through Linux/Windows dependencies and are
not present in the macOS normal dependency graph:

- **`glib@0.18.5` — RUSTSEC-2024-0429** (`unsound`): unsound `Iterator`
  impls for `glib::VariantStrIter`. Fixed in `glib ≥ 0.20.0`.
- **gtk-rs GTK3 bindings — RUSTSEC-2024-0411..0420** (`unmaintained`):
  `atk`, `atk-sys`, `gdk`, `gdk-sys`, `gdkwayland-sys`, `gdkx11`,
  `gdkx11-sys`, `gtk`, `gtk-sys`, `gtk3-macros`.
- **`proc-macro-error@1.0.4` — RUSTSEC-2024-0370** (`unmaintained`).

These cross-platform warnings become release-relevant if Linux or Windows artifacts
are published. Before doing so, update the affected dependency chain or split the tray
backend into target-specific dependencies.
