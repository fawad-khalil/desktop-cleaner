# StorageCleaner

A CleanMyMac-inspired disk analyzer and cleaner for Windows, macOS and Linux, built with
Rust + Tauri v2 and a SvelteKit (TypeScript) frontend.

## Screens

- **Disk**: drive usage pies; drill down by folder, file type and last-used age; largest files per slice.
- **Cleanup**: caches, old files (by last use and size) and update leftovers; dry-run preview,
  then move the selection to the Recycle Bin / Trash.
- **Processes**: live CPU / RAM / disk I/O, grouped by parent or by app, network connections and throughput.
- **Caches**: sizes of browser, developer-tool, app and OS caches.
- **Apps**: installed apps with size and last-used date.
- **Recommendations**: rule-based suggestions linking to the tab that acts on them.
- **Drives**: volumes; format non-system drives (Windows and Linux).

## Safety

- `src-tauri/src/safety.rs` holds per-OS protected paths. Every path is checked when it is listed and again right before deletion.
- Cleanup only moves items to the Recycle Bin / Trash. There is no permanent delete.
- OS-owned leftovers (Windows.old, old kernels) go through the OS's own tool, or through a command the user runs themselves.
- Formatting refuses system, boot and page-file volumes and Linux system mounts. It requires typing `FORMAT <drive>` and always shows the OS elevation prompt.

## Development

```bash
npm install
npm run tauri dev
```

Checks: `npm run check`, `npm run build`, and `cargo test` in `src-tauri`.
