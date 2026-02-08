
# HomeGPT

A local device focused AI assistant built in Rust — persistent memory, autonomous tasks, ~27MB binary. Inspired by and compatible with OpenClaw.

`cargo install homegpt`

## Why HomeGPT?

- **Single binary** — no Node.js, Docker, or Python required
- **Local device focused** — runs entirely on your machine, your memory data stays yours
- **Persistent memory** — markdown-based knowledge store with full-text and semantic search
- **Autonomous heartbeat** — delegate tasks and let it work in the background
- **Multiple interfaces** — CLI, web UI, desktop GUI
- **Multiple LLM providers** — Anthropic (Claude), OpenAI, Ollama
- **OpenClaw compatible** — works with SOUL, MEMORY, HEARTBEAT markdown files and skills format

## Install

```bash
# Full install (includes desktop GUI)
cargo install homegpt

# Headless (no desktop GUI — for servers, Docker, CI)
cargo install homegpt --no-default-features
```

## Quick Start

```bash
# Initialize configuration
homegpt config init

# Start interactive chat
homegpt chat

# Ask a single question
homegpt ask "What is the meaning of life?"

# Run as a daemon with heartbeat, HTTP API and web ui
homegpt daemon start
```

## How It Works

HomeGPT uses plain markdown files as its memory:

```
~/.homegpt/workspace/
├── MEMORY.md            # Long-term knowledge (auto-loaded each session)
├── HEARTBEAT.md         # Autonomous task queue
├── SOUL.md              # Personality and behavioral guidance
└── knowledge/           # Structured knowledge bank (optional)
    ├── finance/
    ├── legal/
    └── tech/
```

Files are indexed with SQLite FTS5 for fast keyword search, and sqlite-vec for semantic search with local embeddings 

## Configuration

Stored at `~/.homegpt/config.toml`:

```toml
[agent]
default_model = "claude-cli/opus"

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"

[heartbeat]
enabled = true
interval = "30m"
active_hours = { start = "09:00", end = "22:00" }

[memory]
workspace = "~/.homegpt/workspace"
```

## CLI Commands

```bash
# Chat
homegpt chat                     # Interactive chat
homegpt chat --session <id>      # Resume session
homegpt ask "question"           # Single question

# Daemon
homegpt daemon start             # Start background daemon
homegpt daemon stop              # Stop daemon
homegpt daemon status            # Show status
homegpt daemon heartbeat         # Run one heartbeat cycle

# Memory
homegpt memory search "query"    # Search memory
homegpt memory reindex           # Reindex files
homegpt memory stats             # Show statistics

# Config
homegpt config init              # Create default config
homegpt config show              # Show current config
```

## HTTP API

When the daemon is running:

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /api/status` | Server status |
| `POST /api/chat` | Chat with the assistant |
| `GET /api/memory/search?q=<query>` | Search memory |
| `GET /api/memory/stats` | Memory statistics |

## Blog

[Why I Built HomeGPT in 4 Nights](https://homegpt.local/blog/why-i-built-homegpt-in-4-nights) — the full story with commit-by-commit breakdown.

## Built With

Rust, Tokio, Axum, SQLite (FTS5 + sqlite-vec), fastembed, eframe

## Contributors

<a href="https://github.com/froggr/homegpt/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=froggr/homegpt" />
</a>

## Stargazers

[![Star History Chart](https://api.star-history.com/svg?repos=froggr/homegpt&type=Date)](https://star-history.com/#froggr/homegpt&Date)

## License

[Apache-2.0](LICENSE)
