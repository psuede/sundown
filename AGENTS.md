# AGENTS.md

Instructions for AI coding agents working on this project.

## Build & Test

```bash
cargo build              # debug build
cargo build --release     # release build
cargo run -- --mock --config /tmp/sundown-test/config.toml  # run without timekpr-next
```

There is no test suite yet. Verify changes by building and running in mock mode.

## Architecture

- Rust workspace with one crate: `crates/sundown-daemon`
- The daemon talks to timekpr-next via D-Bus (system bus, `com.timekpr.server`)
- REST API served by actix-web on port 48800
- Web UI is a single HTML file (`crates/sundown-daemon/static/index.html`) using Alpine.js from CDN, embedded in the binary at compile time via `include_str!`
- Auth: Bearer token on all API endpoints

## Important Conventions

- All D-Bus methods live on object path `/com/timekpr/server`, NOT per-user paths
- `getUserInformation` requires `"F"` as the second argument for full data
- D-Bus values like `LIMITS_PER_WEEKDAYS` are arrays of i32, not semicolon-delimited strings
- `setAllowedHours` expects keys `STARTMIN`, `ENDMIN`, `UACC` (not `from`/`to`)
- `TIME_LEFT_DAY` is 0 when the user has no active session — compute from `limit - TIME_SPENT_BALANCE`
- The web UI requires recompilation after changes (`include_str!`)
- Do not add unnecessary dependencies — keep the binary small
- No TLS — relies on home WiFi encryption

## File Layout

| File | Purpose |
|------|---------|
| `crates/sundown-daemon/src/main.rs` | CLI args, server setup, QR pairing |
| `crates/sundown-daemon/src/bridge.rs` | timekpr-next D-Bus interface + mock mode |
| `crates/sundown-daemon/src/api.rs` | REST endpoints with auth middleware |
| `crates/sundown-daemon/src/auth.rs` | Token generation and loading |
| `crates/sundown-daemon/src/config.rs` | TOML config parsing |
| `crates/sundown-daemon/static/index.html` | Web UI (Alpine.js, single file) |
| `packaging/install.sh` | Single-step installer |
| `packaging/systemd/sundown.service` | Systemd service file |
