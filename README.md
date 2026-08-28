# SubBar 🍺

> Your API quota, right in the menu bar — so you never have to open a dashboard again.

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS-lightgrey)
![Version](https://img.shields.io/badge/version-v1.0.32-blue)

A tiny macOS menu-bar app that watches your subscription / quota usage for
**Minimax** (`.com` / `.io`) and **OpenCode Go**. One click. One glance. Done.

<img src="logo-sm.png" width="80" alt="SubBar">

---

## ✨ At a glance

| | |
|---|---|
| 🧲 **Menu-bar ticker** | live 5h usage as `X%`, always visible |
| 📊 **Usage bars** | 5h / week / month quotas at a glance |
| 🪟 **Liquid Glass** | native macOS 26 glass, very pretty |
| 🌗 **Dark / light** | auto-detected, manually toggleable |
| 🔐 **Keychain-safe** | keys, workspace IDs & cookies — never plaintext on disk |
| ⏱️ **Auto-refresh** | every 5 min (configurable) |

---

## 🚀 Install

**Homebrew** (macOS Intel & Apple Silicon — one command, no prompts):

```bash
brew tap MarcoLeongDev/tap
brew install --cask subbar
```

**Manual** — grab the universal `.dmg` from **Releases**, drag `SubBar.app` to `/Applications`.

<details>
<summary>🇨🇳 中文安装说明（中国大陆用户）</summary>

**方式一 · Homebrew**（一条命令，全程无弹窗；二进制经由 jsDelivr 全球 CDN 分发，国内可直连）：

```bash
brew tap MarcoLeongDev/tap
brew install --cask subbar
```

如果 GitHub 克隆较慢，也可以从 Gitee 添加 tap：

```bash
brew tap MarcoLeongDev/tap https://gitee.com/MarcoLeongDev/homebrew-tap
brew install --cask subbar
```

**方式二 · 手动安装** — 从 **Releases** 页面下载 `.dmg`，将 `SubBar.app` 拖入 `/Applications`。

首次启动如提示"无法验证开发者"，请右键点击 App → **打开**，或在
**系统设置 → 隐私与安全性** 中点击 **仍要打开**（未经 Apple 公证签名的构建会出现此提示）。

</details>

### First launch

1. **Run** — a `--` icon appears in the menu bar. Click it → gear ⚙️.
2. **Pick a side** — **OpenCode Go** is the default; or switch to Minimax `.com` / `.io`.
3. **Paste your secrets** — see the table below.
4. **Done.** Close settings and the tray starts doing its thing.

> **Note on signing:** SubBar is signed but **not Apple-notarized yet** (Developer ID
> account coming later). Homebrew 6 quarantines all cask downloads, so our tap
> strips the quarantine flag in a `postflight` hook — `brew install` works with
> zero prompts. A **manual** browser download of the `.dmg` still triggers
> Gatekeeper: right-click the app → **Open**, or use
> **System Settings → Privacy & Security → Open Anyway**.

---

## 🔑 Where to find your credentials

| Endpoint | You need | Where to get it |
|---------|---------|-----------------|
| **OpenCode Go**<br>(default) | **Workspace ID** | Your dashboard URL: `opencode.ai/workspace/<workspace-id>/go` — the `<workspace-id>` bit. |
| | **Auth token** | Browser devtools → **Cookies → `opencode.ai`** → copy the `auth` cookie value. |
| **Minimax** | **API key** (`sk-…`) | [platform.minimaxi.com](https://platform.minimaxi.com) (or `platform.minimax.io` for international). |

All credentials live in the OS keychain and travel only over HTTPS.

---

## 📦 Build from source

```bash
cargo tauri build          # app lands in src-tauri/target/release/bundle/macos/
```

## 🛠️ Development

```bash
cd src-tauri
cargo run                 # dev mode
cargo tauri build                       # release bundle (host arch)
cargo tauri build --target universal-apple-darwin   # universal (Intel + Apple Silicon)
```

## 🧰 Tech stack

**Tauri v2** · **Rust** · vanilla HTML/CSS/JS · **reqwest** · **keyring**

---

## 🔒 Security

Credentials are stored in the OS native credential store (macOS Keychain), sent
only over HTTPS, and never written as plaintext to disk. Full posture →
[SECURITY.md](SECURITY.md).

## 📄 License

GPL-3.0 — see [LICENSE](LICENSE)