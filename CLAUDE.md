# CLAUDE.md

## Project Overview

Sundown is a remote parental control frontend for [timekpr-next](https://launchpad.net/timekpr-next). A Rust daemon runs on the child's Linux PC, talks to timekpr-next via D-Bus, and exposes a REST API with a web UI that parents access from their phone on the same WiFi network.

## Architecture

```
Parent's phone (browser)  <──HTTP──>  sundown-daemon (child's PC)
                                         │
                                         ├─ REST API (actix-web, port 48800)
                                         ├─ Web UI (Alpine.js, single HTML file)
                                         └─ D-Bus ──> timekpr-next (timekprd)
```

- **Daemon**: Rust binary (`sundown-daemon`) running as root via systemd
- **D-Bus**: Communicates with timekpr-next on the system bus (`com.timekpr.server`)
- **Web UI**: Single HTML file with Alpine.js (CDN), embedded in the binary at compile time via `include_str!`
- **Auth**: 256-bit random token, Bearer auth on all API endpoints
- **No TLS**: Relies on home WiFi encryption (WPA2/WPA3). Tailscale can be added for remote access.

## Project Structure

```
sundown/
├── Cargo.toml                          # Workspace root
├── crates/
│   └── sundown-daemon/
│       ├── Cargo.toml                  # Dependencies
│       ├── src/
│       │   ├── main.rs                 # CLI, server startup, QR pairing
│       │   ├── bridge.rs               # timekpr-next D-Bus bridge + mock mode
│       │   ├── api.rs                  # REST API endpoints with auth middleware
│       │   ├── auth.rs                 # Token generation and loading
│       │   └── config.rs               # TOML config handling
│       └── static/
│           └── index.html              # Web UI (Alpine.js, embedded at compile time)
├── packaging/
│   ├── install.sh                      # Single-step installer
│   └── systemd/
│       └── sundown.service             # Systemd service file
├── PLAN.md                             # Implementation plan and decisions
└── timekpr-remote-design.md            # Original design document
```

## Development Commands

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run locally in mock mode (no timekpr-next required)
cargo run -- --mock --config /tmp/sundown-test/config.toml

# Run with real timekpr-next (requires root + timekpr.service running)
sudo ./target/release/sundown-daemon --config /etc/sundown/config.toml

# Install on this machine (builds from source, installs timekpr-next if needed)
sudo ./packaging/install.sh

# Uninstall
sudo ./packaging/install.sh --uninstall
```

## Key Technical Details

### timekpr-next D-Bus Interface

- **Bus**: System bus, destination `com.timekpr.server`
- **Object path**: `/com/timekpr/server` (all methods live here, NOT on per-user paths)
- **Admin interface**: `com.timekpr.server.user.admin` (requires root or timekpr group)
- **Limits interface**: `com.timekpr.server.user.limits` (read-only, no auth needed)
- **getUserInformation**: Second argument must be `"F"` (not "full") for full data
- **Return values**: Most methods return `(i32, String)` where 0 = success
- **Data types**: `LIMITS_PER_WEEKDAYS` and `ALLOWED_WEEKDAYS` are arrays of i32, not semicolon-delimited strings
- **setAllowedHours**: Signature `ssa{sa{si}}` — keys must be `STARTMIN`, `ENDMIN`, `UACC`

### Mock Mode

Running with `--mock` simulates a child user in memory. All API operations work without timekpr-next installed. Useful for UI development and testing.

### Web UI

- Single `index.html` file, no build step
- Uses Alpine.js from CDN for reactivity
- Embedded in the binary via `include_str!("../static/index.html")` — changes require recompilation
- QR code encodes a URL with token as query parameter for auto-connect on scan
- 4 tabs: Today (status + quick actions), Limits (per-day/weekly/monthly), Hours (allowed time windows), Settings (lockout type, toggles)

### Installer

- Detects distro family (Debian-based or Arch-based)
- Installs timekpr-next from repos/PPA/AUR if not present
- When run from the repo, builds from source; otherwise downloads pre-built binary from GitHub Releases
- Lists human user accounts and prompts for selection
- Generates config, auth token, enables systemd service
- Detects active firewall (ufw/firewalld) and offers to open port 48800
- Idempotent: safe to run multiple times (upgrades without losing config/token)

### Configuration

- Config file: `/etc/sundown/config.toml`
- Token file: `/etc/sundown/token` (chmod 600)
- Default port: 48800

### Important Patterns

- The daemon runs as root (required for D-Bus admin calls to timekpr-next)
- `TIME_LEFT_DAY` from timekpr is 0 when the child has no active session — compute remaining time from `limit - TIME_SPENT_BALANCE` instead
- Today's limit must be looked up from `LIMITS_PER_WEEKDAYS` array using the current weekday index
- Do NOT restart docker containers or services — the developer manages that manually
- When editing `static/index.html`, the binary must be recompiled to pick up changes

## API Endpoints

All require `Authorization: Bearer <token>` header.

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/status` | Current status (time remaining, spent, limits) |
| GET | `/api/config` | Full configuration (days, hours, limits, settings) |
| POST | `/api/time` | Adjust time: `{seconds, operation: "add"/"subtract"/"set"}` |
| POST | `/api/limits/daily` | Set daily limits: `{daily: [s,s,s,s,s,s,s]}` |
| POST | `/api/limits/weekly` | Set weekly limit: `{seconds}` |
| POST | `/api/limits/monthly` | Set monthly limit: `{seconds}` |
| POST | `/api/allowed-days` | Set allowed days: `{days: [1,2,3,4,5,6,7]}` |
| POST | `/api/allowed-hours` | Set allowed hours: `{day: "1", hours: [0,1,2,...]}` |
| POST | `/api/track-inactive` | Toggle: `{enabled: bool}` |
| POST | `/api/hide-tray` | Toggle: `{hidden: bool}` |
| POST | `/api/lockout-type` | Set lockout: `{lockout_type: "terminate"/"lock"/"suspend"/...}` |
| POST | `/api/lock` | Lock user immediately |
| POST | `/api/unlock` | Unlock user |
| GET | `/` | Web UI (embedded HTML) |
