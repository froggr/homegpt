# HomeGPT

A local home assistant built in Rust. Runs on a Mac Studio, manages family life, tutors the kids, monitors the business, and never makes things up.

Forked from [LocalGPT](https://github.com/localgpt-app/localgpt) and rebuilt for home use.

## What It Does

HomeGPT is the brain behind our household. It connects to LM Studio (or Ollama) running locally, keeps a verified markdown memory of everything about the family, and runs autonomous tasks in the background.

- **Verified memory** — every fact is SHA-256 hashed. When HomeGPT recalls something, it proves it with `[VERIFIED:abc12345]` tags. If it doesn't know, it says so instead of making things up.
- **Autonomous heartbeat** — checks the calendar, monitors the business, and sends Discord alerts without being asked.
- **Voice tutoring** — kids talk to it like a tutor. It listens (Whisper), thinks (LM Studio), and speaks back (Pocket TTS).
- **Discord bot** — family can message HomeGPT from their phones. It posts morning briefings, business alerts, and school summaries.
- **Homeschool integration** — plugs into our [homeschool app](https://github.com/froggr/homeschool) as the `homegpt` LLM provider, powering both the general chat and the `/tutor` page.

## Architecture

```
Phone/Tablet/Laptop (any device on Tailnet)
    |
    v
Homeschool App (:443)           Discord
    |       |                      |
    v       v                      v
LM Studio (:1234)          Discord Bot (:31342)
    ^                              |
    |                              v
HomeGPT Daemon (:31327) <---------+
    |
    +-- Verified Memory (SQLite + markdown)
    +-- Heartbeat (autonomous tasks)
    +-- Calendar Bridge (:31340)
    +-- Voice Bridge (:31341)
    +-- ErgoTools Monitor
```

Everything runs on one machine (Mac Studio M4 Pro, 128GB). All data stays local.

## Quick Start

```bash
# Build HomeGPT + install script dependencies
./scripts/build.sh

# Optional: install the binary to ~/.cargo/bin
./scripts/build.sh install

# Initialize config + workspace
./target/release/homegpt config init

# Edit config to point at your LLM
vim ~/.homegpt/config.toml

# Start everything (HomeGPT + voice + calendar + discord + homeschool app)
./scripts/start-homegpt.sh

# Or just the daemon (no sidecars)
./target/release/homegpt daemon start
```

## Configuration

Edit `~/.homegpt/config.toml`:

```toml
[agent]
# LM Studio (OpenAI-compatible API)
default_model = "openai/qwen/qwen3-next-80b"

[providers.openai]
api_key = "not-needed"
base_url = "http://localhost:1234/v1"

# Or use Ollama instead:
# [agent]
# default_model = "ollama/qwen3:32b"
# [providers.ollama]
# endpoint = "http://localhost:11434"

[heartbeat]
enabled = true
interval = "15m"
active_hours = { start = "08:00", end = "22:00" }

[memory]
workspace = "~/.homegpt/workspace"
embedding_provider = "local"
embedding_model = "all-MiniLM-L6-v2"

[server]
enabled = true
port = 31327
bind = "0.0.0.0"   # accessible across Tailnet
```

## Memory Workspace

On first run, HomeGPT creates an organized workspace:

```
~/.homegpt/workspace/
    MEMORY.md                    # Core family facts (names, birthdays, preferences)
    HEARTBEAT.md                 # Recurring autonomous tasks
    SOUL.md                      # Assistant personality
    memory/
        family/
            members.md           # Family member details
            routines.md          # Daily routines, school schedule
        home/
            maintenance.md       # HVAC, warranties, contractors
        school/
            curriculum.md        # AO years, TGTB math, per-child
            progress.md          # What each kid is working on
            tutor-notes.md       # Auto-logged tutoring sessions
        food/
            meal-plans.md        # Weekly plans
            shopping-lists.md    # Active lists
        calendar/
            upcoming.md          # Auto-synced from Google Calendar
        finance/
            budget.md            # Monthly tracking
        business/
            ergotools-status.md  # Auto-updated from PocketBase
        YYYY-MM-DD.md            # Daily session logs
    skills/
        tutor/SKILL.md           # Tutoring persona
        shopping/SKILL.md        # Shopping list management
        maintenance/SKILL.md     # Home maintenance tracking
```

Edit these files directly. The assistant loads `MEMORY.md`, `SOUL.md`, and recent daily logs into every conversation. Everything else is searchable via verified memory.

## Anti-Hallucination System

This is the core differentiator. Every memory chunk gets a SHA-256 hash when indexed. When the assistant searches memory:

1. Each result is verified against its stored hash
2. Verified results show `[VERIFIED:abc12345]` with the hash prefix
3. Provenance is tracked: where did this fact come from? (you said it, a file, a web search, a heartbeat discovery)
4. Confidence is scored: High (user-stated + verified + frequently accessed), Medium, Low, None

The system prompt enforces this:
- Always search memory before claiming stored facts
- Only cite `[VERIFIED]` information
- If nothing found: "I don't have that in my verified memory"
- Never fabricate stored information

The `memory_store` tool lets the assistant save verified facts to `memory/facts/` with YAML frontmatter tracking source, category, and confidence.

## Heartbeat (Autonomous Tasks)

The heartbeat runs every 15 minutes (configurable). It reads `HEARTBEAT.md` and executes pending tasks.

### How It Works

1. Daemon wakes up on interval
2. Checks active hours (skips overnight)
3. Acquires workspace lock (skips if user is actively chatting)
4. Reads `HEARTBEAT.md`
5. If nothing to do: responds `HEARTBEAT_OK` and sleeps
6. If tasks found: executes them, marks `[x]`, logs results

### Example HEARTBEAT.md

```markdown
## Calendar Sync (every hour)
- [ ] Fetch today's events and update memory/calendar/upcoming.md

## ErgoTools Check (every 2 hours)
- [ ] Run scripts/ergotools-heartbeat.ts
- [ ] If pending reviews > 5 or flagged content, alert via Discord

## School Summary (daily, 8pm)
- [ ] Summarize today's tutoring sessions from tutor-notes.md
- [ ] Post summary to Discord #school channel

## Home Maintenance (weekly, Sunday)
- [ ] Check memory/home/maintenance.md for upcoming service dates
- [ ] Remind if anything is due this week
```

Tasks are marked `[x]` when done, never deleted. The assistant checks timestamps and only repeats truly pending work.

### CLI

```bash
homegpt daemon heartbeat    # Run one heartbeat cycle manually
homegpt daemon status       # Show last heartbeat result
```

## Discord Bot

Two-way Discord integration. Family can message HomeGPT from anywhere, and it can send proactive alerts.

### Setup

1. Create a bot at [Discord Developer Portal](https://discord.com/developers/applications)
2. Enable Message Content Intent
3. Invite to your server with Send Messages + Read Messages permissions
4. Set environment variables:

```bash
export DISCORD_BOT_TOKEN=your_token
export DISCORD_GUILD_ID=your_server_id
export DISCORD_OWNER_IDS=user1_id,user2_id    # you + partner
export HOMEGPT_URL=http://localhost:31327
```

### What You Can Do

**Talk to it:**
- `@HomeGPT what's for dinner tonight?`
- DM the bot for private questions

**Slash commands:**
- `/status` — system health + business alerts
- `/calendar` — today's events
- `/shopping` — current shopping list
- `/ask <question>` — ask HomeGPT anything
- `/briefing` — morning briefing (calendar + business + school)

**What it sends you:**
- Morning briefing to `#general`
- ErgoTools alerts to `#ergotools` (flagged content DMs the owners directly)
- School session summaries to `#school`
- Calendar reminders to `#calendar`

### Channel Setup

The bot auto-detects channels by name: `#general`, `#ergotools`, `#school`, `#home`, `#calendar`. Or override with env vars (`DISCORD_CHANNEL_GENERAL`, etc.).

### Internal API

The heartbeat and other scripts send messages through the bot's internal API on `127.0.0.1:31342`:

```
POST /send/channel    { "channel": "ergotools", "content": "...", "embed": {...} }
POST /send/dm         { "userId": "123", "content": "..." }
POST /send/owners     { "content": "Flagged review needs attention" }
GET  /health
```

## Voice Tutoring

Kids talk to HomeGPT like a tutor through the homeschool app's `/tutor` page.

### How It Works

```
Kid speaks → Mic → Hark (voice detection) → Whisper STT → text
    → HomeGPT /api/chat (with tutor context) → response text
    → Pocket TTS → audio → Speaker
    → Auto-listen again (conversational mode)
```

### The Tutor Persona

The tutor system prompt is injected via the `/api/chat` `context` field:

- Friendly older sibling vibe, not cheesy or lame
- Guides to answers, doesn't give them directly
- Keeps responses to 1-3 sentences (critical for voice)
- No markdown, no emoji — clean speech output
- Ends with a question to keep the conversation going
- Knows the child's name, AO year, and curriculum

### Voice Services

| Service | Port | What |
|---------|------|------|
| Whisper STT | 8001 | Speech-to-text (mlx-whisper on Apple Silicon) |
| Pocket TTS | 8000 | Text-to-speech (8 voices: cosette, marius, alba, etc.) |
| Voice Bridge | 31341 | Middleware connecting STT + TTS to HomeGPT |

### Session Logging

Tutoring sessions are auto-logged to `memory/school/tutor-notes.md` with the child's name, subject, what they struggled with, and what they got right.

## ErgoTools Business Monitoring

Monitors the [ErgoTools](https://ergonomicshelp.com) PocketBase instance for things that need attention.

### What It Checks

- Pending product reviews (alerts if > 5)
- Flagged reviews (DMs the owner immediately)
- Products awaiting moderation
- Expired announcements still active
- New product submissions

### How It Runs

Triggered by the heartbeat every 2 hours. Writes status to `memory/business/ergotools-status.md` and sends Discord alerts to `#ergotools`. Flagged content sends a DM to the owners.

```bash
# Environment
export POCKETBASE_URL=https://app.ergonomicshelp.com
export DISCORD_BOT_URL=http://127.0.0.1:31342
```

## Calendar Integration

Google Calendar bridge service on port 31340. Handles OAuth and exposes simple REST endpoints.

```bash
# Environment (reuse from homeschool app)
export GOOGLE_CLIENT_ID=...
export GOOGLE_CLIENT_SECRET=...
export GOOGLE_REFRESH_TOKEN=...
export CALENDAR_IDS=primary,school@group.calendar.google.com
```

### Endpoints

```
GET  /events/today      # Today's events
GET  /events/week       # This week's events
POST /events/create     # Create an event
POST /events/move       # Reschedule an event
GET  /health            # Status + calendar list
```

The heartbeat syncs events to `memory/calendar/upcoming.md` every hour. The Discord bot's `/calendar` command reads from here.

## HTTP API

When the daemon is running on port 31327:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check |
| `/api/status` | GET | Version, model, memory stats, active sessions |
| `/api/chat` | POST | Chat (accepts `message`, `session_id`, `model`, `context`) |
| `/api/chat/stream` | POST | Streaming chat via SSE (with tool calls) |
| `/api/ws` | GET | WebSocket chat |
| `/api/memory/search?q=...` | GET | Search verified memory |
| `/api/memory/stats` | GET | Memory index statistics |
| `/api/memory/reindex` | POST | Reindex workspace files |
| `/api/sessions` | GET/POST | List or create sessions |
| `/api/config` | GET | Current config (safe subset) |
| `/api/heartbeat/status` | GET | Last heartbeat result |

### The `context` Field

`POST /api/chat` accepts an optional `context` string. This gets appended to the system prompt as additional instructions for that session. The homeschool app uses this to inject the tutor persona without replacing HomeGPT's built-in system prompt (safety, tools, memory verification).

```json
{
  "message": "What's two thirds plus one third?",
  "context": "You are a friendly tutor helping Emma (AO Year 2) with math...",
  "session_id": "optional-session-id"
}
```

## CLI Commands

```bash
# Chat
homegpt chat                     # Interactive chat
homegpt chat --session <id>      # Resume session
homegpt ask "question"           # Single question

# Daemon
homegpt daemon start             # Start daemon (API + heartbeat)
homegpt daemon stop              # Stop daemon
homegpt daemon status            # Show status
homegpt daemon heartbeat         # Run one heartbeat cycle

# Memory
homegpt memory search "query"    # Search memory
homegpt memory reindex           # Reindex workspace files
homegpt memory stats             # Show index statistics

# Config
homegpt config init              # Create default config + workspace
homegpt config show              # Show current config
```

### Interactive Chat Commands

Inside `homegpt chat`:

- `/help` — available commands
- `/new` — fresh session (reloads memory)
- `/skills` — list available skills
- `/compact` — compress session history
- `/memory <query>` — search memory
- `/status` — session info (tokens, messages, compactions)
- `/save` — save session to disk
- `/quit` — exit

## Services & Ports

| Port | Service | Description |
|------|---------|-------------|
| 31327 | HomeGPT | Main API + web UI + heartbeat |
| 31340 | Calendar Bridge | Google Calendar REST wrapper |
| 31341 | Voice Bridge | STT + TTS middleware |
| 31342 | Discord Bot | Two-way Discord (internal API) |
| 8000 | Pocket TTS | Text-to-speech |
| 8001 | Whisper STT | Speech-to-text |
| 1234 | LM Studio | LLM inference |
| 443 | Homeschool App | SvelteKit frontend (HTTPS via Tailscale) |

## Starting Everything

The master script starts all services including the homeschool app:

```bash
# Start all services
./scripts/start-homegpt.sh

# Check what's running
./scripts/start-homegpt.sh status

# Tail a specific service log
./scripts/start-homegpt.sh logs homegpt
./scripts/start-homegpt.sh logs homeschool

# Stop everything
./scripts/start-homegpt.sh stop
```

Or start just the core:

```bash
# Just the daemon (API + heartbeat, no voice/discord/calendar)
./target/release/homegpt daemon start
```

### Start on Boot (macOS)

```bash
# Install the launchd service (runs as root for port 443)
sudo cp scripts/com.homegpt.plist /Library/LaunchDaemons/
sudo launchctl load /Library/LaunchDaemons/com.homegpt.plist

# Control it
sudo launchctl start com.homegpt
sudo launchctl stop com.homegpt

# Uninstall
sudo launchctl unload /Library/LaunchDaemons/com.homegpt.plist
sudo rm /Library/LaunchDaemons/com.homegpt.plist
```

Edit `scripts/com.homegpt.plist` to set your Discord token and other env vars before installing.

### Environment Overrides

The start script accepts these overrides:

```bash
HOMESCHOOL_DIR=~/projects/homeschool   # Path to homeschool app
HOMEGPT_BIN=/path/to/homegpt           # Custom binary path
SKIP_HOMESCHOOL=1                      # Don't start the web app
```

## Environment Variables

```bash
# LLM (pick one)
LMSTUDIO_HOST=http://localhost:1234      # LM Studio
OLLAMA_HOST=http://localhost:11434       # Ollama

# Google Calendar
GOOGLE_CLIENT_ID=...
GOOGLE_CLIENT_SECRET=...
GOOGLE_REFRESH_TOKEN=...
CALENDAR_IDS=primary,school@group.calendar.google.com

# Discord
DISCORD_BOT_TOKEN=...
DISCORD_GUILD_ID=...
DISCORD_OWNER_IDS=user1,user2

# Business monitoring
POCKETBASE_URL=https://app.ergonomicshelp.com

# Workspace (optional overrides)
HOMEGPT_WORKSPACE=~/.homegpt/workspace
HOMEGPT_PROFILE=home                     # Uses ~/.homegpt/workspace-home
```

## Built With

Rust, Tokio, Axum, SQLite (FTS5 + sqlite-vec), fastembed, sha2

Sidecar services: Node.js (Discord, Calendar, ErgoTools), Python (Voice Bridge, Whisper)

Forked from [LocalGPT](https://github.com/localgpt-app/localgpt). Licensed under [Apache-2.0](LICENSE).
