#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SUITE_HOME="$HOME/.local/share/free-ai-suite"
CONFIG_HOME="$HOME/.config/free-ai-suite"
LOG_HOME="$HOME/.local/state/free-ai-suite"
APP_HOME="$SUITE_HOME/app"
mkdir -p "$CONFIG_HOME" "$LOG_HOME" "$HOME/.local/share/applications"

say() { printf '\n[Free AI Suite — Rust] %s\n' "$*"; }
fail() { say "ERROR: $*" >&2; exit 1; }

[[ "$(uname -s)" == "Linux" ]] || fail "This installer is for Linux/Ubuntu."
command -v apt-get >/dev/null 2>&1 || fail "This installer expects Ubuntu or an apt-based Linux distribution."

packages=()
command -v git >/dev/null 2>&1 || packages+=(git)
command -v docker >/dev/null 2>&1 || packages+=(docker.io)
command -v openssl >/dev/null 2>&1 || packages+=(openssl)
command -v curl >/dev/null 2>&1 || packages+=(curl)
command -v cc >/dev/null 2>&1 || packages+=(build-essential)
[[ -f /etc/ssl/certs/ca-certificates.crt ]] || packages+=(ca-certificates)
command -v xdg-open >/dev/null 2>&1 || packages+=(xdg-utils)
if ((${#packages[@]})); then
  say "Installing required system packages: ${packages[*]}"
  sudo apt-get update
  sudo apt-get install -y "${packages[@]}"
fi

if ! docker compose version >/dev/null 2>&1; then
  say "Installing Docker Compose v2."
  sudo apt-get install -y docker-compose-v2 || fail "Could not install Docker Compose v2. Install the Docker Compose plugin, then rerun this installer."
fi

# Install the stable Rust toolchain only if it is unavailable or too old.
RUSTC_BIN="$(command -v rustc 2>/dev/null || true)"
CARGO_BIN="$(command -v cargo 2>/dev/null || true)"
if [[ -x "$HOME/.cargo/bin/rustc" && -x "$HOME/.cargo/bin/cargo" ]]; then
  RUSTC_BIN="$HOME/.cargo/bin/rustc"
  CARGO_BIN="$HOME/.cargo/bin/cargo"
fi
RUST_OK=0
if [[ -n "$RUSTC_BIN" && -n "$CARGO_BIN" ]]; then
  RUST_VER="$("$RUSTC_BIN" --version | awk '{print $2}')"
  if [[ "$(printf '%s\n' 1.75.0 "$RUST_VER" | sort -V | head -n1)" == "1.75.0" ]]; then RUST_OK=1; fi
fi
if [[ "$RUST_OK" != 1 ]]; then
  say "Installing the current stable Rust toolchain."
  command -v curl >/dev/null 2>&1 || fail "curl is needed to install Rust."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  export PATH="$HOME/.cargo/bin:$PATH"
  [[ -x "$HOME/.cargo/bin/cargo" ]] || fail "Rust installation did not produce cargo."
fi
export PATH="$HOME/.cargo/bin:$PATH"

say "Enabling Docker."
sudo systemctl enable --now docker

# Copy app sources and assets. User settings and the history database live outside this directory.
mkdir -p "$APP_HOME"
cp -a "$ROOT/app/." "$APP_HOME/"

# Reuse the existing FreeLLMAPI checkout when possible, or fetch it on a fresh setup.
if [[ -f "$HOME/freellmapi/docker-compose.yml" || -f "$HOME/freellmapi/compose.yml" || -f "$HOME/freellmapi/compose.yaml" ]]; then
  API_DIR="$HOME/freellmapi"
elif [[ -f "$SUITE_HOME/freellmapi/docker-compose.yml" || -f "$SUITE_HOME/freellmapi/compose.yml" || -f "$SUITE_HOME/freellmapi/compose.yaml" ]]; then
  API_DIR="$SUITE_HOME/freellmapi"
else
  say "Downloading FreeLLMAPI into $SUITE_HOME/freellmapi"
  git clone https://github.com/tashfeenahmed/freellmapi.git "$SUITE_HOME/freellmapi"
  API_DIR="$SUITE_HOME/freellmapi"
fi
[[ -f "$API_DIR/docker-compose.yml" || -f "$API_DIR/compose.yml" || -f "$API_DIR/compose.yaml" ]] || fail "Could not find a Compose file in $API_DIR."

# Preserve the existing encryption key and environment file.
if [[ ! -f "$API_DIR/.env" ]]; then
  ENCRYPTION_KEY="$(openssl rand -hex 32)"
  printf 'ENCRYPTION_KEY=%s\nPORT=3001\n' "$ENCRYPTION_KEY" > "$API_DIR/.env"
  unset ENCRYPTION_KEY
else
  if ! grep -qE '^ENCRYPTION_KEY=.+$' "$API_DIR/.env"; then
    ENCRYPTION_KEY="$(openssl rand -hex 32)"
    if grep -q '^ENCRYPTION_KEY=' "$API_DIR/.env"; then
      sed -i "s/^ENCRYPTION_KEY=.*/ENCRYPTION_KEY=${ENCRYPTION_KEY}/" "$API_DIR/.env"
    else
      printf 'ENCRYPTION_KEY=%s\n' "$ENCRYPTION_KEY" >> "$API_DIR/.env"
    fi
    unset ENCRYPTION_KEY
  fi
  grep -qE '^PORT=.+$' "$API_DIR/.env" || printf 'PORT=3001\n' >> "$API_DIR/.env"
fi
chmod 600 "$API_DIR/.env"
API_PORT="$(sed -n 's/^PORT=\([0-9][0-9]*\)$/\1/p' "$API_DIR/.env" | tail -n1)"
API_PORT="${API_PORT:-3001}"
if [[ ! "$API_PORT" =~ ^[0-9]+$ ]] || (( API_PORT < 1 || API_PORT > 65535 )); then fail "PORT in $API_DIR/.env is invalid."; fi
printf '%s\n' "$API_DIR" > "$CONFIG_HOME/freellmapi-dir"
printf '%s\n' "$API_PORT" > "$CONFIG_HOME/freellmapi-port"
chmod 600 "$CONFIG_HOME/freellmapi-dir" "$CONFIG_HOME/freellmapi-port"

say "Building the Free AI Suite Rust application (first build downloads Rust crates)."
(cd "$APP_HOME" && cargo build --release)
[[ -x "$APP_HOME/target/release/free-ai-studio" ]] || fail "Rust build did not create the application binary."

say "Starting FreeLLMAPI; existing provider configuration and database are preserved."
(cd "$API_DIR" && sudo docker compose up -d)
say "Waiting for FreeLLMAPI to respond..."
ready=0
for _ in $(seq 1 75); do
  if curl -fsS "http://127.0.0.1:${API_PORT}/api/ping" >/dev/null 2>&1; then ready=1; break; fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  say "FreeLLMAPI has not answered. Inspect logs with:"
  printf '  cd %q && sudo docker compose logs --tail=100 freellmapi\n' "$API_DIR"
  fail "FreeLLMAPI is not responding; the Rust application is built but startup needs troubleshooting."
fi

cat > "$SUITE_HOME/start-router-root.sh" <<'ROOT_HELPER_EOF'
#!/usr/bin/env bash
set -Eeuo pipefail
API_DIR="${1:?FreeLLMAPI directory is required}"
[[ -d "$API_DIR" ]] || { echo "FreeLLMAPI directory not found: $API_DIR" >&2; exit 1; }
systemctl start docker
cd "$API_DIR"
docker compose up -d
ROOT_HELPER_EOF
chmod 755 "$SUITE_HOME/start-router-root.sh"

cat > "$SUITE_HOME/launch.sh" <<'LAUNCH_SCRIPT_EOF'
#!/usr/bin/env bash
set -Eeuo pipefail
SUITE_HOME="$HOME/.local/share/free-ai-suite"
CONFIG_HOME="$HOME/.config/free-ai-suite"
LOG_HOME="$HOME/.local/state/free-ai-suite"
APP_HOME="$SUITE_HOME/app"
API_DIR="$(cat "$CONFIG_HOME/freellmapi-dir" 2>/dev/null || true)"
API_PORT="$(cat "$CONFIG_HOME/freellmapi-port" 2>/dev/null || printf 3001)"
if [[ -z "$API_DIR" || ! -d "$API_DIR" ]]; then
  command -v notify-send >/dev/null 2>&1 && notify-send "Free AI Suite" "FreeLLMAPI folder not found. Rerun the installer."
  exit 1
fi
mkdir -p "$LOG_HOME"
API_URL="http://127.0.0.1:${API_PORT}"
if ! curl -fsS "$API_URL/api/ping" >/dev/null 2>&1; then
  if command -v pkexec >/dev/null 2>&1; then
    pkexec /bin/bash "$SUITE_HOME/start-router-root.sh" "$API_DIR" || {
      command -v notify-send >/dev/null 2>&1 && notify-send "Free AI Suite" "Could not start FreeLLMAPI."
      exit 1
    }
  elif command -v x-terminal-emulator >/dev/null 2>&1; then
    x-terminal-emulator -e bash -lc "cd $(printf %q "$API_DIR") && sudo docker compose up -d; echo; read -r -p 'Press Enter to close this window...' _" >/dev/null 2>&1 &
  else
    command -v notify-send >/dev/null 2>&1 && notify-send "Free AI Suite" "FreeLLMAPI is stopped. Run docker compose up -d from its folder."
    exit 1
  fi
  for _ in $(seq 1 60); do curl -fsS "$API_URL/api/ping" >/dev/null 2>&1 && break; sleep 1; done
fi
if ! curl -fsS "$API_URL/api/ping" >/dev/null 2>&1; then
  command -v notify-send >/dev/null 2>&1 && notify-send "Free AI Suite" "FreeLLMAPI is not responding. Check Docker logs."
  exit 1
fi
STUDIO_URL="http://127.0.0.1:8765"
studio_is_rust() {
  curl -fsS "$STUDIO_URL/api/health" 2>/dev/null | grep -Eq '"runtime"[[:space:]]*:[[:space:]]*"rust"'
}
if ! studio_is_rust; then
  # Stop the previous Python backend from the earlier edition if it is still occupying the port.
  if [[ -f "$APP_HOME/server.py" ]]; then pkill -f "$APP_HOME/server.py" >/dev/null 2>&1 || true; sleep 1; fi
  FREE_AI_STUDIO_BASE_URL="$API_URL/v1" FREE_AI_STUDIO_PORT=8765 nohup "$APP_HOME/target/release/free-ai-studio" >> "$LOG_HOME/studio.log" 2>&1 </dev/null &
  for _ in $(seq 1 30); do studio_is_rust && break; sleep 1; done
fi
if ! studio_is_rust; then
  command -v notify-send >/dev/null 2>&1 && notify-send "Free AI Suite" "Rust Free AI Studio failed to start. See $LOG_HOME/studio.log"
  exit 1
fi
xdg-open "$STUDIO_URL" >/dev/null 2>&1 || true
LAUNCH_SCRIPT_EOF
chmod 755 "$SUITE_HOME/launch.sh"

cat > "$HOME/.local/share/applications/free-ai-suite.desktop" <<DESKTOP_FILE_EOF
[Desktop Entry]
Type=Application
Name=Free AI Suite (Rust)
Comment=One-click local AI coding workspace with Rust and FreeLLMAPI
Exec=$SUITE_HOME/launch.sh
Icon=applications-development
Terminal=false
Categories=Development;Utility;
StartupNotify=true
DESKTOP_FILE_EOF
chmod 644 "$HOME/.local/share/applications/free-ai-suite.desktop"
mkdir -p "$HOME/.local/bin"
cat > "$HOME/.local/bin/free-ai-suite" <<COMMAND_FILE_EOF
#!/usr/bin/env bash
exec "$SUITE_HOME/launch.sh" "\$@"
COMMAND_FILE_EOF
chmod 755 "$HOME/.local/bin/free-ai-suite"

say "Installation finished. Starting Free AI Suite..."
"$SUITE_HOME/launch.sh"
say "Done. Use 'Free AI Suite (Rust)' from the Applications menu next time."
say "First use: click Provider setup, configure provider credentials in FreeLLMAPI, then save its unified key in Router settings."
