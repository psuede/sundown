# Sundown

Remote parental controls for Linux. Manage your child's screen time from any device.

Sundown is a lightweight daemon that sits on top of [timekpr-next](https://launchpad.net/timekpr-next) and gives parents a mobile-friendly web interface to monitor and control their child's computer time — without needing to be at the computer.

## Features

- **View time remaining** — see how much screen time your child has left today
- **Grant or remove time** — reward extra time or cut it short, from any device
- **Lock / unlock** — immediately lock the computer or restore access
- **Per-day limits** — set different time limits for each day of the week
- **Allowed hours** — define which hours of the day the computer can be used
- **Weekly & monthly limits** — cap total usage across longer periods
- **Lockout behavior** — choose what happens when time runs out (lock screen, suspend, terminate session, shut down)
- **QR pairing** — scan a QR code to connect your device, no manual setup
- **No cloud, no account** — everything runs locally on your home network

## How It Works

```
Parent's device (browser)  <──WiFi──>  sundown-daemon (child's PC)
                                      │
                                      └─ D-Bus ──> timekpr-next
```

Sundown installs alongside timekpr-next on the child's Linux computer. It talks to timekpr-next via D-Bus and serves a web UI on your local network. Open it from any device on the same WiFi — no app install needed.

## Install

### Quick install (no Rust required)

One command installs everything — timekpr-next, the sundown daemon, and the systemd service:

```bash
curl -sL https://raw.githubusercontent.com/psuede/sundown/main/packaging/install.sh | sudo bash
```

This downloads a pre-built binary from GitHub Releases. No build tools needed.

### Install from source

If you prefer to build from source, or want to make changes:

```bash
git clone https://github.com/psuede/sundown.git
cd sundown
sudo ./packaging/install.sh
```

This requires the [Rust toolchain](https://rustup.rs/).

### What the installer does

1. Installs timekpr-next if not already present
2. Installs the sundown daemon (downloads binary or builds from source)
3. Asks which user account to control
4. Offers to open the firewall port for local network access
5. Starts the service and displays a QR code

Scan the QR code with your device to connect.

### Requirements

- Linux (Ubuntu, Debian, Mint, Pop!_OS, Arch, Manjaro)
- systemd

### Uninstall

```bash
curl -sL https://raw.githubusercontent.com/psuede/sundown/main/packaging/install.sh | sudo bash -s -- --uninstall
```

Or if you have the repo cloned:

```bash
sudo ./packaging/install.sh --uninstall
```

This removes sundown but leaves timekpr-next installed.

## Usage

After installation, sundown runs as a systemd service. Open the URL shown during install on any device on your WiFi.

The web interface has four tabs:

**Today** — current status, quick actions (+15m, +30m, +1h), lock/unlock, custom time adjustments

**Limits** — per-day time limits with a visual day grid, weekly and monthly caps

**Hours** — toggle which hours of the day are allowed, per day or for all days at once

**Settings** — track inactive time, hide tray icon, choose lockout behavior

### Managing the Service

```bash
sudo systemctl status sundown      # check if running
sudo systemctl restart sundown     # restart after config changes
sudo journalctl -u sundown -f      # view logs
```

### Configuration

Config file: `/etc/sundown/config.toml`

```toml
[server]
bind = "0.0.0.0"
port = 48800

[auth]
token_file = "/etc/sundown/token"

[timekpr]
user = "childname"
```

### Rotate Auth Token

If you need to invalidate the current token and generate a new one:

```bash
sudo sundown-daemon --config /etc/sundown/config.toml --rotate-token
sudo systemctl restart sundown
```

## Development

```bash
# Run locally without timekpr-next (mock mode)
cargo run -- --mock --config /tmp/sundown-test/config.toml

# Build release binary
cargo build --release
```

Mock mode simulates a child user in memory — all API operations work without timekpr-next installed.

## Security

- All API endpoints require a 256-bit auth token (Bearer authentication)
- Token file is readable only by root (`chmod 600`)
- No cloud services, no accounts, no telemetry — everything stays on your local network
- Traffic is protected by your WiFi encryption (WPA2/WPA3)
- For remote access outside your home network, consider adding [Tailscale](https://tailscale.com)

## License

[GPL-3.0](LICENSE)
