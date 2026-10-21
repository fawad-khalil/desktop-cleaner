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

- **macOS Full Disk Access (recommended, once)**: macOS guards some places with a prompt (Contacts,
  Calendars, Reminders, Photos, iCloud Drive, other apps' data in `~/Library/Containers` and
  `Group Containers`, removable and network drives) and others with Full Disk Access only (Mail, Messages,
  Safari). On first launch the app checks for Full Disk Access without prompting (it tries to read the
  per-user `TCC.db`) and, if it's missing, shows **Grant Full Disk Access once** with a button to
  System Settings › Privacy & Security › Full Disk Access. **Not now** is remembered.
  - Without it, scans skip those folders entirely, so no prompts fire; their space counts as
    "System & other" and the Disk screen says how many were skipped. Other drives are only read when you
    open them yourself. Desktop, Documents and Downloads are still scanned, so macOS asks for those.
  - Once it's granted, the next index update rescans the drive to pick up what was skipped.
  - When running with `npm run tauri dev`, macOS attributes the app to the terminal that launched it, so
    grant Full Disk Access to the terminal (or your editor) instead.
- **Why macOS keeps asking again**: permission answers are tied to the app's code signature. Unsigned
  builds are ad-hoc signed, which gives every build a new signature, so macOS forgets the answers after
  each rebuild or update. A stable signing identity fixes that (see below).
- **Formatting drives** (Windows and Linux) always goes through the OS elevation prompt (UAC on Windows,
  pkexec on Linux); nothing is formatted if you decline. The app itself never runs as administrator/root.

### Keeping macOS permissions across rebuilds and updates

macOS remembers permissions (Full Disk Access and the prompts above) per code signature. With a stable
identity, the signature's requirement stays the same from build to build, so answers stick.

**Local builds: a self-signed code-signing certificate** (one-time, nothing leaves your Mac):

1. Open **Keychain Access** › menu **Keychain Access › Certificate Assistant › Create a Certificate…**
2. Name it e.g. `DesktopCleaner Dev`, Identity Type **Self Signed Root**, Certificate Type **Code Signing**,
   then **Create**. It goes to your login keychain.
3. Check it's usable: `security find-identity -v -p codesigning` should list `DesktopCleaner Dev`.
4. Build with it:
   ```bash
   APPLE_SIGNING_IDENTITY="DesktopCleaner Dev" npm run tauri build -- --bundles app
   ```
   Tauri signs the `.app` with that identity. Grant Full Disk Access to that build once; later builds signed
   the same way keep it. Keep the bundle identifier (`com.fawadkhalil.desktopcleaner`) unchanged, since it's
   part of the requirement too. A self-signed build is for your own Mac only: Gatekeeper still warns
   elsewhere.

**CI releases: a Developer ID certificate** (Apple Developer Program), which also clears Gatekeeper once
notarized:

1. Create a **Developer ID Application** certificate in your Apple Developer account, install it, and
   export it from Keychain Access as a `.p12` with a password.
2. Add repository secrets: `APPLE_CERTIFICATE` (`base64 -i cert.p12 | pbcopy`), `APPLE_CERTIFICATE_PASSWORD`,
   `APPLE_SIGNING_IDENTITY` (e.g. `Developer ID Application: Your Name (TEAMID)`), and for notarization
   `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.
3. Uncomment those variables in the `tauri-action` step of `.github/workflows/ci.yml`. It imports
   the certificate into a temporary keychain, signs, and notarizes.

Every release signed with the same Developer ID and bundle identifier keeps the permissions users granted
to the previous one.

## Support

If DesktopCleaner is useful to you, you can support its development on
[thanks.dev](https://thanks.dev/u/gh/fawad-khalil).

## License

[MIT](LICENSE) © 2026 Fawad Khalil
