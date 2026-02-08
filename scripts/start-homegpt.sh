#!/bin/bash
# HomeGPT Master Startup
#
# Starts everything needed for the full HomeGPT + homeschool experience.
# Designed for the Mac Studio (barf) where all services run.
#
# Usage:
#   ./scripts/start-homegpt.sh            # Start all services
#   ./scripts/start-homegpt.sh stop        # Stop all services
#   ./scripts/start-homegpt.sh status      # Check service status
#   ./scripts/start-homegpt.sh logs <svc>  # Tail a service log
#
# Environment (override defaults):
#   HOMESCHOOL_DIR    Path to homeschool app (default: ~/projects/homeschool)
#   HOMEGPT_BIN       Path to homegpt binary (default: auto-detect)
#   SKIP_HOMESCHOOL   Set to 1 to skip starting the homeschool app

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PID_FILE="$PROJECT_DIR/.service-pids"
LOG_DIR="$HOME/.homegpt/logs"

HOMESCHOOL_DIR="${HOMESCHOOL_DIR:-$HOME/projects/homeschool}"
HOMEGPT_BIN="${HOMEGPT_BIN:-}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

mkdir -p "$LOG_DIR"

log()  { echo -e "${GREEN}[HomeGPT]${NC} $1"; }
warn() { echo -e "${YELLOW}[HomeGPT]${NC} $1"; }
err()  { echo -e "${RED}[HomeGPT]${NC} $1"; }
info() { echo -e "${BLUE}[HomeGPT]${NC} $1"; }

# Find the homegpt binary
find_binary() {
    if [ -n "$HOMEGPT_BIN" ]; then
        echo "$HOMEGPT_BIN"
    elif [ -f "$PROJECT_DIR/target/release/homegpt" ]; then
        echo "$PROJECT_DIR/target/release/homegpt"
    elif command -v homegpt > /dev/null 2>&1; then
        command -v homegpt
    else
        echo ""
    fi
}

# Find python (prefer python3.11 for mlx-whisper on Apple Silicon)
find_python() {
    for py in python3.11 python3 python; do
        local p
        # Check common macOS paths first
        for prefix in /opt/homebrew/bin /usr/local/bin ""; do
            if [ -n "$prefix" ]; then
                p="$prefix/$py"
            else
                p="$py"
            fi
            if command -v "$p" > /dev/null 2>&1; then
                echo "$p"
                return
            fi
        done
    done
    echo ""
}

start_service() {
    local name="$1"
    local cmd="$2"
    local log_file="$LOG_DIR/$name.log"

    log "Starting $name..."
    eval "$cmd" >> "$log_file" 2>&1 &
    local pid=$!
    echo "$name=$pid" >> "$PID_FILE"
    log "  $name started (PID: $pid)"
}

stop_services() {
    if [ ! -f "$PID_FILE" ]; then
        warn "No PID file found — nothing to stop"
        return
    fi

    while IFS='=' read -r name pid; do
        [ -z "$name" ] && continue
        if kill -0 "$pid" 2>/dev/null; then
            log "Stopping $name (PID: $pid)..."
            kill "$pid" 2>/dev/null || true
        else
            warn "$name already stopped"
        fi
    done < "$PID_FILE"

    # Also clean up by name in case PIDs were lost
    pkill -f "pocket-tts serve" 2>/dev/null || true
    pkill -f "whisper-server.py" 2>/dev/null || true
    pkill -f "voice-bridge.py" 2>/dev/null || true
    pkill -f "calendar-bridge.ts" 2>/dev/null || true
    pkill -f "discord-bot.ts" 2>/dev/null || true
    docker stop searxng 2>/dev/null || true

    rm -f "$PID_FILE"
    log "All services stopped"
}

check_status() {
    echo ""
    info "Service status:"

    if [ ! -f "$PID_FILE" ]; then
        warn "  No services running (no PID file)"
        echo ""
        return
    fi

    while IFS='=' read -r name pid; do
        [ -z "$name" ] && continue
        if kill -0 "$pid" 2>/dev/null; then
            echo -e "  ${GREEN}●${NC} $name (PID: $pid)"
        else
            echo -e "  ${RED}●${NC} $name (PID: $pid — DEAD)"
        fi
    done < "$PID_FILE"

    # Check LM Studio separately (not managed by us)
    if curl -s http://localhost:1234/v1/models > /dev/null 2>&1; then
        echo -e "  ${GREEN}●${NC} lm-studio (external, port 1234)"
    else
        echo -e "  ${RED}●${NC} lm-studio (not running — start LM Studio app)"
    fi

    echo ""
}

show_logs() {
    local svc="${1:-}"
    if [ -z "$svc" ]; then
        err "Usage: $0 logs <service>"
        err "Services: homegpt, whisper, tts, voice-bridge, calendar, discord-bot, homeschool"
        exit 1
    fi
    local log_file="$LOG_DIR/$svc.log"
    if [ ! -f "$log_file" ]; then
        err "No log file for '$svc'"
        exit 1
    fi
    tail -f "$log_file"
}

case "${1:-start}" in
    stop)
        stop_services
        ;;
    status)
        check_status
        ;;
    logs)
        show_logs "${2:-}"
        ;;
    start)
        # Clean up old PID file
        rm -f "$PID_FILE"

        log "Starting all HomeGPT services..."
        echo ""

        # ── 1. HomeGPT daemon ──────────────────────────────────────
        BINARY=$(find_binary)
        if [ -n "$BINARY" ]; then
            start_service "homegpt" "$BINARY daemon start --foreground"
            sleep 1
        else
            err "homegpt binary not found!"
            err "  Build with: ./scripts/build.sh"
            err "  Or set HOMEGPT_BIN=/path/to/homegpt"
        fi

        # ── 2. Voice services ─────────────────────────────────────
        # Pocket TTS
        if command -v pocket-tts > /dev/null 2>&1; then
            start_service "tts" "pocket-tts serve"
        else
            warn "pocket-tts not found — TTS unavailable"
        fi

        # Whisper STT
        PYTHON=$(find_python)
        WHISPER_SCRIPT="$SCRIPT_DIR/whisper-server.py"
        if [ -n "$PYTHON" ] && [ -f "$WHISPER_SCRIPT" ]; then
            start_service "whisper" "$PYTHON $WHISPER_SCRIPT"
        elif [ -f "$HOMESCHOOL_DIR/scripts/whisper-server.py" ]; then
            # Fall back to homeschool's copy
            start_service "whisper" "$PYTHON $HOMESCHOOL_DIR/scripts/whisper-server.py"
        else
            warn "Whisper STT not available (no python or script)"
        fi

        # Voice bridge
        if [ -n "$PYTHON" ] && [ -f "$SCRIPT_DIR/voice-bridge.py" ]; then
            start_service "voice-bridge" "$PYTHON $SCRIPT_DIR/voice-bridge.py"
        fi

        # ── 3. Integration services ──────────────────────────────
        # Calendar bridge
        if command -v npx > /dev/null 2>&1 && [ -f "$SCRIPT_DIR/calendar-bridge.ts" ]; then
            start_service "calendar" "npx tsx $SCRIPT_DIR/calendar-bridge.ts"
        else
            warn "Calendar bridge unavailable (need npx + tsx)"
        fi

        # Discord bot
        if command -v npx > /dev/null 2>&1 && [ -f "$SCRIPT_DIR/discord-bot.ts" ]; then
            if [ -n "${DISCORD_BOT_TOKEN:-}" ]; then
                start_service "discord-bot" "npx tsx $SCRIPT_DIR/discord-bot.ts"
            else
                warn "DISCORD_BOT_TOKEN not set — Discord bot skipped"
            fi
        fi

        # ── 4. Search ────────────────────────────────────────────
        if command -v docker > /dev/null 2>&1; then
            if docker ps --format '{{.Names}}' 2>/dev/null | grep -q searxng; then
                log "SearXNG already running"
            else
                log "Starting SearXNG..."
                docker start searxng 2>/dev/null || warn "SearXNG container not found (run docker pull searxng/searxng first)"
            fi
        fi

        # ── 5. Homeschool app ────────────────────────────────────
        if [ "${SKIP_HOMESCHOOL:-}" = "1" ]; then
            info "Skipping homeschool app (SKIP_HOMESCHOOL=1)"
        elif [ -d "$HOMESCHOOL_DIR" ]; then
            if [ -f "$HOMESCHOOL_DIR/package.json" ]; then
                log "Starting homeschool app (port 443, HTTPS)..."
                # Port 443 requires root. If we're not root, use sudo.
                if [ "$(id -u)" -eq 0 ]; then
                    start_service "homeschool" "sh -c 'cd $HOMESCHOOL_DIR && npm run dev'"
                else
                    start_service "homeschool" "sudo sh -c 'cd $HOMESCHOOL_DIR && npm run dev'"
                fi
            else
                warn "Homeschool dir exists but no package.json — run: cd $HOMESCHOOL_DIR && npm install"
            fi
        else
            warn "Homeschool app not found at $HOMESCHOOL_DIR"
            warn "  Set HOMESCHOOL_DIR to the correct path, or SKIP_HOMESCHOOL=1 to skip"
        fi

        # ── Summary ──────────────────────────────────────────────
        echo ""
        log "Services started:"
        echo ""
        echo "  HomeGPT API:       http://localhost:31327"
        echo "  Homeschool app:    https://barf.skunk-dominant.ts.net"
        echo "  Calendar bridge:   http://localhost:31340"
        echo "  Voice bridge:      http://localhost:31341"
        echo "  Discord bot:       http://localhost:31342 (internal)"
        echo "  Whisper STT:       http://localhost:8001"
        echo "  Pocket TTS:        http://localhost:8000"
        echo "  SearXNG:           http://localhost:8080"
        echo ""
        log "Logs: $LOG_DIR/"
        log "  $0 logs <service>    Tail a service log"
        log "  $0 status            Check what's running"
        log "  $0 stop              Stop everything"
        echo ""
        ;;
    *)
        echo "Usage: $0 {start|stop|status|logs <service>}"
        exit 1
        ;;
esac
