# rhwp-mcp Desktop Release

The desktop package wraps `rhwp-studio/dist` in Electron and ships the native
`rhwp-mcp` stdio binary as a sidecar. It does not require `vite preview` or any
separate web server at runtime.

## Local Build

```bash
cd rhwp-studio
npm ci
npm run build
cd ..
cargo build --profile desktop-release --bin rhwp-mcp
cd desktop
npm ci
npm run dist
```

Artifacts are written to `desktop/release/`.
The Linux local artifact is an AppImage; macOS and Windows installers are built
by the GitHub Actions matrix on their native runners.

## Runtime Storage

- UI settings and AI provider settings use the app WebView's local storage.
- Autosave and document history use IndexedDB.
- The MCP sidecar is launched by the desktop app over stdio with
  `RHWP_MCP_ROOT` set to the user's Documents directory.
- Gemini requests are proxied through Electron IPC from the main process, so
  `/api/ai/gemini` and `vite preview` are not needed in desktop builds.

OAuth access tokens may still expire. Long-lived OAuth should be backed by a
token broker or OS keychain integration before production distribution.

## GitHub Release

Pushing a tag like `v0.7.17-mcp.1` or running the `rhwp-mcp Desktop Release` workflow manually
builds macOS, Windows, and Linux artifacts and uploads them to a GitHub Release.
