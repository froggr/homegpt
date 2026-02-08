#!/bin/bash
# HomeGPT Unified Startup Script
#
# Starts all services needed for the full HomeGPT experience.
# Run from the homegpt project directory.
#
# Usage:
#   ./scripts/start-homegpt.sh        # Start all services
#   ./scripts/start-homegpt.sh stop    # Stop all services
#   ./scripts/start-homegpt.sh status  # Check service status

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PID_FILE="$PROJECT_DIR/.service-pids"
LOG_DIR="$HOME/.homegpt/logs"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

mkdir -p "$LOG_DIR"

log() { echo -e "${GREEN}[HomeGPT]${NC} $1"; }
warn() { echo -e "${YELLOW}[HomeGPT]${NC} $1"; }
err() { echo -e "${RED}[HomeGPT]${NC} $1"; }

start_service() {
    local name="$1"
    local cmd="$2"
    local log_file="$LOG_DIR/$name.log"

    log "Starting $name..."
    eval "$cmd" > "$log_file" 2>&1 &
    local pid=$!
    echo "$name=$pid" >> "$PID_FILE"
    log "  $name started (PID: $pid, log: $log_file)"
}

stop_services() {
    if [ ! -f "$PID_FILE" ]; then
        warn "No PID file found"
        return
    fi

    while IFS='=' read -r name pid; do
        if kill -0 "$pid" 2>/dev/null; then
            log "Stopping $name (PID: $pid)..."
            kill "$pid" 2>/dev/null || true
        else
            warn "$name already stopped"
        fi
    done < "$PID_FILE"

    rm -f "$PID_FILE"
    log "All services stopped"
}

check_status() {
    if [ ! -f "$PID_FILE" ]; then
        warn "No services running (no PID file)"
        return
    fi

    while IFS='=' read -r name pid; do
        if kill -0 "$pid" 2>/dev/null; then
            echo -e "  ${GREEN}●${NC} $name (PID: $pid)"
        else
            echo -e "  ${RED}●${NC} $name (PID: $pid - DEAD)"
        fi
    done < "$PID_FILE"
}

case "${1:-start}" in
    stop)
        stop_services
        ;;
    status)
        log "Service status:"
        check_status
        ;;
    start)
        # Clean up old PID file
        rm -f "$PID_FILE"

        log "Starting HomeGPT services..."

        # 1. Ollama (if not already running)
        if ! pgrep -x ollama > /dev/null 2>&1; then
            start_service "ollama" "ollama serve"
            sleep 2
        else
            log "Ollama already running"
        fi

        # 2. HomeGPT daemon (main assistant + heartbeat + HTTP API)
        if command -v homegpt > /dev/null 2>&1; then
            start_service "homegpt" "homegpt daemon start --foreground"
        else
            warn "homegpt binary not found — build with: cargo install --path ."
        fi

        # 3. Whisper STT server
        if command -v python3.11 > /dev/null 2>&1; then
            start_service "whisper" "python3.11 $SCRIPT_DIR/whisper-server.py"
        elif command -v python3 > /dev/null 2>&1; then
            start_service "whisper" "python3 $SCRIPT_DIR/whisper-server.py"
        else
            warn "Python not found — whisper STT will not be available"
        fi

        # 4. Pocket TTS
        if command -v pocket-tts > /dev/null 2>&1; then
            start_service "tts" "pocket-tts serve"
        else
            warn "pocket-tts not found — TTS will not be available"
        fi

        # 5. Voice bridge
        if command -v python3 > /dev/null 2>&1; then
            start_service "voice-bridge" "python3 $SCRIPT_DIR/voice-bridge.py"
        fi

        # 6. Calendar bridge
        if command -v npx > /dev/null 2>&1; then
            start_service "calendar" "npx tsx $SCRIPT_DIR/calendar-bridge.ts"
        else
            warn "npx not found — calendar bridge will not be available"
        fi

        # 7. Discord bot
        if command -v npx > /dev/null 2>&1 && [ -n "$DISCORD_BOT_TOKEN" ]; then
            start_service "discord-bot" "npx tsx $SCRIPT_DIR/discord-bot.ts"
        else
            if [ -z "$DISCORD_BOT_TOKEN" ]; then
                warn "DISCORD_BOT_TOKEN not set — Discord bot will not be available"
            fi
        fi

        # 8. SearXNG (if docker is available)
        if command -v docker > /dev/null 2>&1; then
            if ! docker ps --format '{{.Names}}' | grep -q searxng; then
                log "Starting SearXNG..."
                docker start searxng 2>/dev/null || warn "SearXNG container not found"
            else
                log "SearXNG already running"
            fi
        fi

        echo ""
        log "All services started!"
        log "  HomeGPT API:     http://localhost:31327"
        log "  Calendar bridge: http://localhost:31340"
        log "  Voice bridge:    http://localhost:31341"
        log "  Discord bot API: http://localhost:31342 (internal)"
        log "  Whisper STT:     http://localhost:8001"
        log "  Pocket TTS:      http://localhost:8000"
        echo ""
        log "Run '$0 status' to check services"
        log "Run '$0 stop' to stop all services"
        ;;
    *)
        echo "Usage: $0 {start|stop|status}"
        exit 1
        ;;
esac
