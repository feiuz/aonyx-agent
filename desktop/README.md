# Aonyx Desktop (Tauri 2)

A native desktop GUI for Aonyx Agent — **Vague 4**. It is a thin shell over
the **automation API** (`aonyx serve api`): the frontend talks to the API
through Rust-side Tauri commands (so there is no CORS and no bundled npm
toolchain — the UI is plain HTML/CSS/JS under `src/`).

> **Status — V4.6–V4.7.** Connects to a running `aonyx serve api`:
> **streaming** chat (live tokens + tool activity), a **sessions** sidebar
> (switch / new), and **memory-palace search**. The Windows installer builds
> in CI (`.github/workflows/desktop.yml`, manual dispatch). Remaining: an
> embedded local agent (no separate server to launch) and the v0.9.0 cut.

## Prerequisites

- The Rust toolchain + the Tauri CLI (`cargo install tauri-cli --version '^2'`).
- On Windows: the WebView2 runtime (preinstalled on Windows 11).
- A running API to talk to:

  ```bash
  aonyx serve api --port 8788        # from an aonyx build with --features api
  ```

## Run (dev)

```bash
cd desktop
cargo tauri dev
```

The window opens; click **Settings**, set the API URL
(default `http://127.0.0.1:8788`) and a bearer token if your server requires
one, then **Connect**. Ask a question — the agent answers and lists any tools
it invoked.

## Build a Windows installer

```bash
cd desktop
cargo tauri build        # produces an .msi + NSIS .exe under src-tauri/target/release/bundle/
```

## Layout

```
desktop/
├── src/                 static frontend (index.html, app.js, styles.css)
└── src-tauri/           Tauri 2 Rust shell
    ├── src/lib.rs       api_info / api_create_session / api_send commands (reqwest)
    ├── tauri.conf.json  window + bundle config
    └── icons/           generated from the brand logo
```

Design tokens (near-monochrome aerospace look, Saira type) mirror
[agent.aonyx.site](https://agent.aonyx.site).
