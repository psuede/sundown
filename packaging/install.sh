#!/usr/bin/env bash
#
# Sundown installer
# Installs timekpr-next (if needed) and the sundown daemon.
#
# Usage:
#   curl -sL https://raw.githubusercontent.com/<org>/sundown/main/packaging/install.sh | sudo bash
#   sudo ./install.sh
#   sudo ./install.sh --uninstall
#
set -euo pipefail

SUNDOWN_BIN="/usr/local/bin/sundown-daemon"
SUNDOWN_SERVICE="/etc/systemd/system/sundown.service"
SUNDOWN_CONFIG_DIR="/etc/sundown"
SUNDOWN_CONFIG="$SUNDOWN_CONFIG_DIR/config.toml"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

info()  { echo -e "${GREEN}[sundown]${NC} $*"; }
warn()  { echo -e "${YELLOW}[sundown]${NC} $*"; }
error() { echo -e "${RED}[sundown]${NC} $*" >&2; }
bold()  { echo -e "${BOLD}$*${NC}"; }

# ─── Root check ───────────────────────────────────────────────────────────────

if [[ $EUID -ne 0 ]]; then
    error "This script must be run as root (use sudo)"
    exit 1
fi

# ─── Uninstall ────────────────────────────────────────────────────────────────

if [[ "${1:-}" == "--uninstall" ]]; then
    info "Uninstalling sundown..."

    if systemctl is-active --quiet sundown.service 2>/dev/null; then
        systemctl stop sundown.service
        info "stopped sundown.service"
    fi

    if systemctl is-enabled --quiet sundown.service 2>/dev/null; then
        systemctl disable sundown.service
        info "disabled sundown.service"
    fi

    rm -f "$SUNDOWN_SERVICE"
    rm -f "$SUNDOWN_BIN"
    systemctl daemon-reload

    info "removed binary and service file"

    if [[ -d "$SUNDOWN_CONFIG_DIR" ]]; then
        warn "config directory $SUNDOWN_CONFIG_DIR was NOT removed (contains your token)"
        warn "remove it manually with: sudo rm -rf $SUNDOWN_CONFIG_DIR"
    fi

    warn "timekpr-next was NOT uninstalled (remove it manually if desired)"
    info "uninstall complete"
    exit 0
fi

# ─── Detect distro ───────────────────────────────────────────────────────────

detect_distro() {
    if [[ ! -f /etc/os-release ]]; then
        error "cannot detect distribution (no /etc/os-release)"
        exit 1
    fi

    source /etc/os-release

    case "${ID:-}" in
        ubuntu|debian|linuxmint|pop|elementary|zorin|neon)
            DISTRO_FAMILY="debian"
            ;;
        arch|manjaro|endeavouros|garuda)
            DISTRO_FAMILY="arch"
            ;;
        *)
            # Check ID_LIKE for derivatives
            case "${ID_LIKE:-}" in
                *ubuntu*|*debian*)
                    DISTRO_FAMILY="debian"
                    ;;
                *arch*)
                    DISTRO_FAMILY="arch"
                    ;;
                *)
                    error "unsupported distribution: ${PRETTY_NAME:-$ID}"
                    error "sundown currently supports Ubuntu/Debian and Arch-based distros"
                    exit 1
                    ;;
            esac
            ;;
    esac

    info "detected distro family: $DISTRO_FAMILY ($PRETTY_NAME)"
}

# ─── Install timekpr-next ────────────────────────────────────────────────────

install_timekpr_debian() {
    if dpkg -l timekpr-next 2>/dev/null | grep -q '^ii'; then
        info "timekpr-next is already installed"
        return
    fi

    info "installing timekpr-next..."

    # timekpr-next is in the standard Ubuntu/Mint repos — no PPA needed
    # Use --allow-releaseinfo-change and ignore third-party repo errors
    apt-get update -qq --allow-releaseinfo-change || warn "some repos failed to update (non-fatal)"
    if ! DEBIAN_FRONTEND=noninteractive apt-get install -y timekpr-next; then
        # Fall back to PPA if not in repos
        warn "not found in default repos, trying PPA..."
        apt-get install -y -qq software-properties-common
        add-apt-repository -y ppa:mjasnik/ppa
        apt-get update -qq || true
        DEBIAN_FRONTEND=noninteractive apt-get install -y timekpr-next
    fi

    info "timekpr-next installed"
}

install_timekpr_arch() {
    if pacman -Q timekpr-next &>/dev/null; then
        info "timekpr-next is already installed"
        return
    fi

    info "installing timekpr-next from AUR..."

    # Find an AUR helper
    local aur_helper=""
    for helper in yay paru; do
        if command -v "$helper" &>/dev/null; then
            aur_helper="$helper"
            break
        fi
    done

    if [[ -z "$aur_helper" ]]; then
        error "no AUR helper found (yay or paru required)"
        error "install one first: sudo pacman -S yay"
        exit 1
    fi

    # AUR helpers should not run as root — find the invoking user
    local real_user="${SUDO_USER:-}"
    if [[ -z "$real_user" ]]; then
        error "cannot determine non-root user for AUR install"
        error "run this script with: sudo ./install.sh"
        exit 1
    fi

    sudo -u "$real_user" "$aur_helper" -S --noconfirm timekpr-next
    info "timekpr-next installed"
}

ensure_timekpr_running() {
    if systemctl is-active --quiet timekpr.service 2>/dev/null; then
        info "timekpr.service is running"
    else
        info "starting timekpr.service..."
        systemctl enable --now timekpr.service
        info "timekpr.service started"
    fi
}

# ─── Install sundown ─────────────────────────────────────────────────────────

install_sundown_binary() {
    # Stop running service before replacing the binary
    if systemctl is-active --quiet sundown.service 2>/dev/null; then
        systemctl stop sundown.service
    fi

    # Check if we're running from the source tree
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local repo_root="${script_dir%/packaging}"

    if [[ -f "$repo_root/Cargo.toml" ]] && grep -q "sundown-daemon" "$repo_root/Cargo.toml" 2>/dev/null; then
        # Building from source
        info "building sundown from source..."

        local real_user="${SUDO_USER:-}"
        local cargo_bin=""

        # Find cargo — check user's rustup install first, then system PATH
        if [[ -n "$real_user" ]]; then
            local user_home
            user_home="$(eval echo "~$real_user")"
            if [[ -x "$user_home/.cargo/bin/cargo" ]]; then
                cargo_bin="$user_home/.cargo/bin/cargo"
            fi
        fi

        if [[ -z "$cargo_bin" ]] && command -v cargo &>/dev/null; then
            cargo_bin="cargo"
        fi

        if [[ -z "$cargo_bin" ]]; then
            error "Rust toolchain (cargo) not found"
            error "install it: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
            exit 1
        fi

        # Build as the real user (not root) to avoid permission issues with cargo cache
        if [[ -n "$real_user" ]]; then
            sudo -u "$real_user" bash -c "export PATH=\"\$HOME/.cargo/bin:\$PATH\" && cd '$repo_root' && cargo build --release" 2>&1
        else
            (cd "$repo_root" && "$cargo_bin" build --release) 2>&1
        fi

        cp "$repo_root/target/release/sundown-daemon" "$SUNDOWN_BIN"
        chmod 755 "$SUNDOWN_BIN"
        info "installed binary to $SUNDOWN_BIN (built from source)"
    else
        # Download pre-built binary from GitHub Releases
        local arch
        arch="$(uname -m)"
        case "$arch" in
            x86_64)  arch="x86_64" ;;
            aarch64) arch="aarch64" ;;
            *)
                error "unsupported architecture: $arch"
                exit 1
                ;;
        esac

        local url="https://github.com/psuede/sundown/releases/latest/download/sundown-daemon-${arch}.tar.gz"
        info "downloading sundown from $url ..."

        local tmp
        tmp="$(mktemp -d)"
        if ! curl -fsSL "$url" -o "$tmp/sundown.tar.gz"; then
            error "download failed — no release found at $url"
            error "if you have the source, run this script from the repo: sudo ./packaging/install.sh"
            rm -rf "$tmp"
            exit 1
        fi

        tar -xzf "$tmp/sundown.tar.gz" -C "$tmp"
        cp "$tmp/sundown-daemon" "$SUNDOWN_BIN"
        chmod 755 "$SUNDOWN_BIN"
        rm -rf "$tmp"
        info "installed binary to $SUNDOWN_BIN"
    fi
}

install_sundown_service() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local service_src="$script_dir/systemd/sundown.service"

    if [[ -f "$service_src" ]]; then
        cp "$service_src" "$SUNDOWN_SERVICE"
    else
        # Generate service file inline if not running from repo
        cat > "$SUNDOWN_SERVICE" <<'EOF'
[Unit]
Description=Sundown - Remote parental control for timekpr-next
After=network.target timekpr.service dbus.service
Requires=dbus.service

[Service]
Type=simple
ExecStart=/usr/local/bin/sundown-daemon
Restart=on-failure
RestartSec=5
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF
    fi

    systemctl daemon-reload
    info "installed systemd service"
}

# ─── User selection ──────────────────────────────────────────────────────────

select_child_user() {
    # Get human users: UID >= 1000, has a real login shell, not "nobody"
    local users=()
    while IFS=: read -r username _ uid _ _ _ shell; do
        if [[ $uid -ge 1000 ]] && [[ "$shell" != */nologin ]] && [[ "$shell" != */false ]] && [[ "$username" != "nobody" ]]; then
            users+=("$username")
        fi
    done < /etc/passwd

    if [[ ${#users[@]} -eq 0 ]]; then
        error "no regular user accounts found on this system"
        exit 1
    fi

    if [[ ${#users[@]} -eq 1 ]]; then
        CHILD_USER="${users[0]}"
        info "found one user account: $CHILD_USER"
        return
    fi

    # Multiple users — let the parent choose
    echo ""
    bold "  Which account should sundown control?"
    echo ""
    for i in "${!users[@]}"; do
        echo "    $((i + 1))) ${users[$i]}"
    done
    echo ""

    while true; do
        read -rp "  Enter number [1-${#users[@]}]: " choice < /dev/tty
        if [[ "$choice" =~ ^[0-9]+$ ]] && [[ "$choice" -ge 1 ]] && [[ "$choice" -le ${#users[@]} ]]; then
            CHILD_USER="${users[$((choice - 1))]}"
            break
        fi
        warn "invalid choice, try again"
    done

    info "selected user: $CHILD_USER"

    # Check if timekpr is tracking this user
    if [[ ! -f "/var/lib/timekpr/config/timekpr.${CHILD_USER}.conf" ]]; then
        warn ""
        warn "timekpr-next is not yet tracking \"$CHILD_USER\""
        warn "this usually means the user has never logged in to a desktop session"
        warn ""
        warn "to fix this:"
        warn "  1. Log out of this session"
        warn "  2. Log in as \"$CHILD_USER\" (even briefly)"
        warn "  3. Log back in as yourself and re-run this installer"
        warn ""
        read -rp "  Continue anyway? [y/N]: " confirm < /dev/tty
        if [[ "${confirm,,}" != "y" ]]; then
            info "aborting — log in as \"$CHILD_USER\" first, then re-run"
            exit 0
        fi
    fi
}

# ─── First-run setup ─────────────────────────────────────────────────────────

first_run_setup() {
    # Stop existing service if upgrading
    if systemctl is-active --quiet sundown.service 2>/dev/null; then
        systemctl stop sundown.service
    fi

    mkdir -p "$SUNDOWN_CONFIG_DIR"

    # Generate config if it doesn't exist
    if [[ ! -f "$SUNDOWN_CONFIG" ]]; then
        info "generating config..."
        cat > "$SUNDOWN_CONFIG" <<EOF
[server]
bind = "0.0.0.0"
port = 48800

[auth]
token_file = "$SUNDOWN_CONFIG_DIR/token"

[timekpr]
user = "$CHILD_USER"
EOF
        info "config written to $SUNDOWN_CONFIG"
    else
        # Update existing config
        if grep -q '^user = ' "$SUNDOWN_CONFIG"; then
            sed -i "s/^user = .*/user = \"$CHILD_USER\"/" "$SUNDOWN_CONFIG"
            info "updated user to $CHILD_USER in existing config"
        fi
        # Ensure port is current
        if grep -q '^port = ' "$SUNDOWN_CONFIG"; then
            sed -i "s/^port = .*/port = $SUNDOWN_PORT/" "$SUNDOWN_CONFIG"
        fi
    fi

    # Generate auth token if it doesn't exist
    if [[ ! -f "$SUNDOWN_CONFIG_DIR/token" ]]; then
        info "generating auth token..."
        # Generate 32 random bytes, base64url-encode, no padding
        head -c 32 /dev/urandom | base64 -w0 | tr '+/' '-_' | tr -d '=' > "$SUNDOWN_CONFIG_DIR/token"
        chmod 600 "$SUNDOWN_CONFIG_DIR/token"
        info "token saved to $SUNDOWN_CONFIG_DIR/token"
    fi

    # Enable and start the service
    systemctl enable --now sundown.service
    info "sundown.service enabled and started"

    # Wait for it to start
    sleep 1

    # Verify it's running
    if systemctl is-active --quiet sundown.service; then
        info "sundown is running"
    else
        warn "sundown may not have started correctly"
        warn "check logs: sudo journalctl -u sundown -n 20"
    fi
}

# ─── Firewall ─────────────────────────────────────────────────────────────────

SUNDOWN_PORT=48800

configure_firewall() {
    # Detect firewall
    if command -v ufw &>/dev/null && ufw status 2>/dev/null | grep -q "^Status: active"; then
        # Check if rule already exists
        if ufw status 2>/dev/null | grep -q "$SUNDOWN_PORT"; then
            info "firewall already allows port $SUNDOWN_PORT"
            return
        fi

        echo ""
        warn "a firewall (ufw) is active on this system"
        warn "sundown needs port $SUNDOWN_PORT open for your phone to connect"
        echo ""
        read -rp "  Open port $SUNDOWN_PORT in the firewall? [Y/n]: " choice < /dev/tty
        choice="${choice:-y}"

        if [[ "${choice,,}" == "y" ]]; then
            ufw allow "$SUNDOWN_PORT/tcp" > /dev/null 2>&1
            info "firewall rule added: allow $SUNDOWN_PORT/tcp"
        else
            warn "skipped — you will need to open the port manually:"
            warn "  sudo ufw allow $SUNDOWN_PORT/tcp"
            warn "without this, your phone won't be able to connect"
        fi

    elif command -v firewall-cmd &>/dev/null && firewall-cmd --state 2>/dev/null | grep -q "running"; then
        # firewalld (Fedora, some Arch setups)
        if firewall-cmd --list-ports 2>/dev/null | grep -q "$SUNDOWN_PORT"; then
            info "firewall already allows port $SUNDOWN_PORT"
            return
        fi

        echo ""
        warn "a firewall (firewalld) is active on this system"
        warn "sundown needs port $SUNDOWN_PORT open for your phone to connect"
        echo ""
        read -rp "  Open port $SUNDOWN_PORT in the firewall? [Y/n]: " choice < /dev/tty
        choice="${choice:-y}"

        if [[ "${choice,,}" == "y" ]]; then
            firewall-cmd --permanent --add-port="$SUNDOWN_PORT/tcp" > /dev/null 2>&1
            firewall-cmd --reload > /dev/null 2>&1
            info "firewall rule added: allow $SUNDOWN_PORT/tcp"
        else
            warn "skipped — you will need to open the port manually:"
            warn "  sudo firewall-cmd --permanent --add-port=$SUNDOWN_PORT/tcp && sudo firewall-cmd --reload"
        fi

    else
        info "no active firewall detected"
    fi
}

# ─── Main ─────────────────────────────────────────────────────────────────────

main() {
    echo ""
    bold "  ☀ Sundown Installer"
    echo ""

    detect_distro

    echo ""
    bold "Step 1: timekpr-next"
    case "$DISTRO_FAMILY" in
        debian) install_timekpr_debian ;;
        arch)   install_timekpr_arch ;;
    esac
    ensure_timekpr_running

    echo ""
    bold "Step 2: sundown daemon"
    install_sundown_binary
    install_sundown_service

    echo ""
    bold "Step 3: select child account"
    select_child_user

    echo ""
    bold "Step 4: configure & start"
    first_run_setup

    echo ""
    bold "Step 5: firewall"
    configure_firewall

    echo ""
    bold "Step 6: pairing"
    echo ""
    "$SUNDOWN_BIN" --config "$SUNDOWN_CONFIG" --show-pairing 2>/dev/null || true

    echo ""
    bold "  ✓ Installation complete!"
    echo ""
    info "controlling user: $CHILD_USER"
    info "config: $SUNDOWN_CONFIG"
    echo ""
    info "next steps:"
    info "  1. Connect your phone to the same WiFi as this PC"
    info "  2. Scan the QR code above with your phone camera"
    echo ""
    info "manage the service:"
    info "  sudo systemctl status sundown"
    info "  sudo systemctl restart sundown"
    info "  sudo journalctl -u sundown -f"
    echo ""
}

main "$@"
