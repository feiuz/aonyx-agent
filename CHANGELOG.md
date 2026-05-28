# Changelog

All notable changes to **Aonyx Agent** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial Cargo workspace: 10 crates (`aonyx-core`, `aonyx-memory`, `aonyx-llm`, `aonyx-tools`, `aonyx-skills`, `aonyx-agent`, `aonyx-mcp`, `aonyx-cli`, `aonyx-tui`, `aonyx-adapters`).
- BMAD artefacts under `.bmad/`: brief, PRD, architecture, decision log.
- `SOUL.md` — default agent personality.
- MIT license.
- CI workflow (fmt, clippy, test) on Linux / macOS / Windows.
- `rust-toolchain.toml` pinning stable channel.
