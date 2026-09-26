# Teitunnel browser extension

Share the local page you're on at a public URL, and see or stop your shares, from Chrome,
Edge, Brave, Arc, Vivaldi, Chromium or Firefox, through the Teitunnel app.

- `pnpm --filter @teitunnel/browser-extension build`: `dist/chrome` and `dist/firefox`.
- `pnpm --filter @teitunnel/browser-extension test` / `typecheck`.

To try it: build, then load `dist/chrome` as an unpacked extension (chrome://extensions,
Developer mode) or `dist/firefox/manifest.json` as a temporary add-on (about:debugging), and
choose **Set Up** in Teitunnel ▸ Settings ▸ Integrations ▸ Browser Extension (or run
`teitunnel browser install`). The unpacked Chromium build has a fixed id thanks to the
manifest's `key`, which the app's host manifest allows.

The extension talks only to the `teitunnel` command the app registers as its native messaging
host (`com.teispace.teitunnel`); see `crates/core/src/browser_host.rs`. Publishing:
`docs/RELEASING.md`, "Browser extension stores".
