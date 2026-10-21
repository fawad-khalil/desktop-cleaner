# DesktopCleaner

A CleanMyMac-inspired disk analyzer and cleaner for Windows, macOS and Linux, built with
Rust + Tauri v2 and a SvelteKit (TypeScript) frontend.

## Screens

- **Disk**: a storage bar per drive (used space by file type, then free) and a two-ring chart of used space
  to click down through folders to single files; breakdowns by file type and last-used age; largest files per slice.
  The scan is saved and shown instantly next time, then brought up to date incrementally (FSEvents history on
  macOS, changed-folder detection elsewhere, a full rescan weekly) and kept live while the app is open.
- **Cleanup**: one card for caches, temporary files and old files (by last use and size) with Select all,
  plus update leftovers; "Show selected" lists exactly what that covers, then the chosen items go to the
  Recycle Bin / Trash.
- **What's running**: apps and background tasks grouped by app, with impact, memory, processor, who started
  them, internet speed per app (macOS) and a confirmed Quit; an Advanced view keeps the raw process tree.
- **Caches**: sizes of browser, developer-tool, app and OS caches.
- **Apps**: installed apps with size and last-used date, multi-column sorting, and Remove (to the Trash on
  macOS, the app's uninstaller on Windows, the package-manager command on Linux).
- **Recommendations**: rule-based suggestions linking to the tab that acts on them.
- **Health**: CPU/GPU/storage temperatures, fans, GPU load, battery health, cycles, condition and power,
  power adapter and system draw, SMART status, per-app power use, a daily battery-health trend with an
  estimate of when it reaches 80%, and a drain estimate. Anything the OS doesn't expose without admin rights
  is shown as not available, with the reason.
- **Search**: every cleaning list has a search box (plain text or regex, optional case-sensitivity) with
  Select matching; Old files and Disk "Find files" search every file on the backend.
- **Drives**: volumes; format non-system drives (Windows and Linux).

## Safety

- `src-tauri/src/safety.rs` holds per-OS protected paths. Every path is checked when it is listed and again right before deletion.
- Cleanup only moves items to the Recycle Bin / Trash. There is no permanent delete.
- OS-owned leftovers (Windows.old, old kernels) go through the OS's own tool, or through a command the user runs themselves.
- Formatting refuses system, boot and page-file volumes and Linux system mounts. It requires typing `FORMAT <drive>` and always shows the OS elevation prompt.

## Building from source

### Prerequisites

- **Rust** via [rustup](https://rustup.rs). The repo's `.tool-versions` pins Rust 1.99.1 for asdf/mise users;
  any recent stable toolchain that supports edition 2024 should work.
- **Node.js 22 LTS and npm 10** (tested with Node 22.15.1 and npm 10.9.2; Vite 8 needs Node 20.19+ or 22.12+).
- **Tauri v2 system dependencies**, per the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/):
  - **macOS**: Xcode Command Line Tools: `xcode-select --install`.
  - **Windows**: Microsoft C++ Build Tools with the "Desktop development with C++" workload, and the
    Microsoft Edge WebView2 runtime (already included in Windows 10 1803+ and Windows 11).
  - **Linux (Debian/Ubuntu)**:
    ```bash
    sudo apt update
    sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
      libssl-dev libayatana-appindicator3-dev librsvg2-dev
    ```
  - **Linux (Fedora)**:
    ```bash
    sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file \
      libappindicator-gtk3-devel librsvg2-devel libxdo-devel
    sudo dnf group install "c-development"
    ```

### Steps

```bash
git clone <repo-url> DesktopCleaner
cd DesktopCleaner
npm install
npm run tauri dev        # run the app with hot reload
```

Tests and checks:

```bash
(cd src-tauri && cargo test)   # Rust unit tests
npm run check                  # svelte-check / TypeScript
npm run build                  # frontend production build
```

Release build:

```bash
npm run tauri build
```

The executable is written to `src-tauri/target/release/` and installers/bundles to
`src-tauri/target/release/bundle/<format>/`, e.g. `bundle/macos/DesktopCleaner.app` on macOS
(`npm run tauri build -- --bundles app` builds only the `.app`; the default also makes a `.dmg`).
With `"targets": "all"`, Tauri's defaults are `bundle/msi` and `bundle/nsis` on Windows and
`bundle/deb`, `bundle/rpm` and `bundle/appimage` on Linux (so far only the macOS build has been run).

### Releases (GitHub Actions)

`.github/workflows/ci.yml` is the whole pipeline:

- **Every push to `main`, pull request and manual run**: `npm run check`, `npm run build` and `cargo test`
  on macOS, Windows and Ubuntu.
- **Pull requests**: once the tests pass, the installers are built and uploaded as workflow artifacts
  (on the run's summary page) so you can try them before merging.
- **Releases and versions are automatic**: every push to `main` whose tests pass is released. The pipeline
  finds the latest `v*` tag, reads the commit messages since then and picks the next version
  ([Conventional Commits](https://www.conventionalcommits.org/)):
  - `feat: …` → minor (0.1.3 → 0.2.0);
  - `type!: …` or a `BREAKING CHANGE:` line → major (1.4.2 → 2.0.0), except that while the major version is
    0 it bumps the minor instead (0.4.1 → 0.5.0), the usual 0.x convention;
  - anything else → patch (0.1.0 → 0.1.1).

  Merge commits count their body too, so a PR titled `feat: …` releases a minor version whether it's merged
  or squashed. The version is stamped into `tauri.conf.json`, `package.json` and `Cargo.toml` for that build
  only, then the installers (macOS universal `.dmg`/`.app`; Windows `.msi` and NSIS `.exe`; Linux `.deb`,
  `.rpm` and `.AppImage`) are attached to GitHub Release `vX.Y.Z` with generated notes, which creates the
  tag. **Tags are the source of truth**: the bump isn't committed back (`main` is protected), so the
  version in the repo files is the last one set by hand and the first release's version. With no new
  commits since the last tag, nothing is released.
- **Pull requests** show the version merging would release in the run's summary, and the installer
  artifacts are named with it.
- **Signing**: builds are unsigned by default, so macOS Gatekeeper ("can't be opened…") and Windows
  SmartScreen warn on first launch. To sign, add these repository secrets and uncomment them in `ci.yml`:
  `APPLE_CERTIFICATE` (base64 .p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, and for
  notarization `APPLE_ID`, `APPLE_PASSWORD` (app-specific password), `APPLE_TEAM_ID`. For Windows, configure
  `bundle.windows.certificateThumbprint` or `bundle.windows.signCommand` in `tauri.conf.json` and import the
  certificate in a step before the build.

### Permissions

- **macOS Full Disk Access**: without it, macOS keeps the app out of protected folders (Mail, Messages,
  other apps' containers), which then show as "System & other" on the Disk screen. Grant it in
  System Settings › Privacy & Security › Full Disk Access (the Disk screen links there), then rescan.
  When running with `npm run tauri dev`, the permission applies to the terminal app that launched it.
- **Formatting drives** (Windows and Linux) always goes through the OS elevation prompt (UAC on Windows,
  pkexec on Linux); nothing is formatted if you decline. The app itself never runs as administrator/root.
