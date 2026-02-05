# LocalGPT

A Local focused AI assistant with persistent memory and continuous operation capabilities (or reshaped OpenClaw in Rust).

## Features

- **Local-only operation** - Runs entirely on your machine
- **Persistent memory** - Markdown-based knowledge store with FTS search
- **Multiple LLM providers** - OpenAI, Anthropic, and Ollama support
- **Heartbeat runner** - Autonomous task execution
- **HTTP API** - REST endpoints for programmatic access
- **Small footprint** - ~7MB binary, minimal dependencies

## Installation

```bash
# Build from source
cargo build --release

# Install globally
cargo install --path .
```

## Quick Start

```bash
# Initialize configuration
localgpt config init

# Start interactive chat
localgpt chat

# Ask a single question
localgpt ask "What is the meaning of life?"

# Run daemon with HTTP server
localgpt daemon start
```

## Configuration

Configuration is stored at `~/.localgpt/config.toml`:

```toml
[agent]
default_model = "claude-cli/opus"
context_window = 128000
reserve_tokens = 8000

[providers.openai]
api_key = "${OPENAI_API_KEY}"

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"

[heartbeat]
enabled = true
interval = "30m"
active_hours = { start = "09:00", end = "22:00" }

[memory]
workspace = "~/.localgpt/workspace"

[server]
enabled = true
port = 31327
bind = "127.0.0.1"
```

## Memory System

LocalGPT uses plain markdown files as the source of truth:

```
~/.localgpt/workspace/
├── MEMORY.md            # Curated long-term knowledge
├── HEARTBEAT.md         # Pending tasks/reminders
└── memory/
    ├── 2024-01-15.md    # Daily append-only logs
    └── ...
```

Memory files are indexed with SQLite FTS5 for fast keyword search.

## CLI Commands

```bash
# Chat
localgpt chat                     # Interactive chat
localgpt chat --session <id>      # Resume session
localgpt ask "question"           # Single question

# Daemon
localgpt daemon start             # Start daemon
localgpt daemon stop              # Stop daemon
localgpt daemon status            # Show status
localgpt daemon heartbeat         # Run heartbeat once

# Memory
localgpt memory search "query"    # Search memory
localgpt memory reindex           # Reindex files
localgpt memory stats             # Show statistics

# Config
localgpt config show              # Show config
localgpt config get <key>         # Get value
localgpt config set <key> <value> # Set value
localgpt config init              # Create default config
```

## HTTP API

When the daemon is running, the following endpoints are available:

- `GET /health` - Health check
- `GET /api/status` - Server status
- `POST /api/chat` - Chat with the assistant
- `GET /api/memory/search?q=<query>` - Search memory
- `GET /api/memory/stats` - Memory statistics

## License

Apache-2.0
