//! Workspace initialization and templates
//!
//! Creates default workspace files on first run.

use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::info;

/// Initialize workspace with default templates if files don't exist.
/// Returns true if this is a brand new workspace (all key files were missing).
pub fn init_workspace(workspace: &Path) -> Result<bool> {
    // Ensure directories exist — home-specific structure
    fs::create_dir_all(workspace)?;
    fs::create_dir_all(workspace.join("memory"))?;
    fs::create_dir_all(workspace.join("skills"))?;

    // Home workspace subdirectories
    let home_dirs = [
        "memory/family",
        "memory/home",
        "memory/food",
        "memory/school",
        "memory/calendar",
        "memory/finance",
        "memory/business",
        "memory/knowledge",
        "skills/tutor",
        "skills/shopping",
        "skills/maintenance",
        "skills/family",
    ];
    for dir in &home_dirs {
        fs::create_dir_all(workspace.join(dir))?;
    }

    // Also init the parent state directory (.gitignore for sessions/logs)
    if let Some(state_dir) = workspace.parent() {
        init_state_dir(state_dir)?;
    }

    // Check if this is a brand new workspace (all key files missing)
    let key_files = [
        workspace.join("MEMORY.md"),
        workspace.join("HEARTBEAT.md"),
        workspace.join("SOUL.md"),
    ];
    let is_brand_new = key_files.iter().all(|p| !p.exists());

    // Create MEMORY.md if it doesn't exist
    let memory_path = workspace.join("MEMORY.md");
    if !memory_path.exists() {
        fs::write(&memory_path, MEMORY_TEMPLATE)?;
        info!("Created {}", memory_path.display());
    }

    // Create HEARTBEAT.md if it doesn't exist
    let heartbeat_path = workspace.join("HEARTBEAT.md");
    if !heartbeat_path.exists() {
        fs::write(&heartbeat_path, HEARTBEAT_TEMPLATE)?;
        info!("Created {}", heartbeat_path.display());
    }

    // Create SOUL.md if it doesn't exist
    let soul_path = workspace.join("SOUL.md");
    if !soul_path.exists() {
        fs::write(&soul_path, SOUL_TEMPLATE)?;
        info!("Created {}", soul_path.display());
    }

    // Create home workspace files if they don't exist
    let home_files: &[(&str, &str)] = &[
        ("memory/family/members.md", FAMILY_MEMBERS_TEMPLATE),
        ("memory/family/routines.md", FAMILY_ROUTINES_TEMPLATE),
        ("memory/school/curriculum.md", SCHOOL_CURRICULUM_TEMPLATE),
        ("memory/school/progress.md", SCHOOL_PROGRESS_TEMPLATE),
        ("memory/school/tutor-notes.md", TUTOR_NOTES_TEMPLATE),
        ("memory/home/maintenance.md", HOME_MAINTENANCE_TEMPLATE),
        ("memory/food/meal-plans.md", MEAL_PLANS_TEMPLATE),
        ("memory/food/shopping-lists.md", SHOPPING_LISTS_TEMPLATE),
        ("memory/calendar/upcoming.md", CALENDAR_TEMPLATE),
        ("memory/business/ergotools-status.md", ERGOTOOLS_TEMPLATE),
        ("skills/tutor/SKILL.md", TUTOR_SKILL_TEMPLATE),
        ("skills/shopping/SKILL.md", SHOPPING_SKILL_TEMPLATE),
        ("skills/maintenance/SKILL.md", MAINTENANCE_SKILL_TEMPLATE),
    ];

    for (path, content) in home_files {
        let full_path = workspace.join(path);
        if !full_path.exists() {
            fs::write(&full_path, content)?;
            info!("Created {}", full_path.display());
        }
    }

    // Create .gitignore if it doesn't exist
    let gitignore_path = workspace.join(".gitignore");
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, GITIGNORE_TEMPLATE)?;
        info!("Created {}", gitignore_path.display());
    }

    Ok(is_brand_new)
}

const MEMORY_TEMPLATE: &str = r#"# MEMORY.md - Family Knowledge Base

Core facts about the family, home, and daily life.

## Family

<!-- Names, birthdays, allergies, preferences — see memory/family/ for details -->

## Home

<!-- Address, important contacts, house details -->

## Preferences

<!-- Family preferences, dietary needs, etc. -->

---

"#;

const HEARTBEAT_TEMPLATE: &str = r#"# HEARTBEAT.md - Recurring Tasks

Tasks listed here run during heartbeat cycles (every 15 minutes).

## Calendar Sync (every hour)
- [ ] Fetch today's events from Google Calendar bridge (http://localhost:31340/events/today)
- [ ] Update memory/calendar/upcoming.md with current events

## ErgoTools Business Check (every 2 hours)
- [ ] Run ergotools-heartbeat check
- [ ] If pending reviews > 5 or flagged content exists, note it in today's log

## School Progress (daily, 8pm)
- [ ] Summarize today's tutoring sessions from memory/school/tutor-notes.md
- [ ] Note any subjects needing extra attention

## Home Maintenance (weekly, Sunday)
- [ ] Check memory/home/maintenance.md for upcoming maintenance tasks
- [ ] Flag any overdue items
"#;

const SOUL_TEMPLATE: &str = r#"# SOUL.md - Home Assistant Personality

You are the family's home assistant. You help manage the household, tutor the kids,
monitor the business, and keep everything running smoothly.

## Core Values

**Family first.** Everything you do serves the family. Be warm, patient, and reliable.

**Anti-hallucination.** NEVER fabricate information. Always search verified memory before claiming facts. Say "I don't know" when you don't know.

**Be practical.** Give actionable answers, not essays. The family is busy.

**Earn trust through accuracy.** Your memory is cryptographically verified. Use it. Cite it.

## With the Kids

- Be patient and encouraging during tutoring
- Guide them to answers, never give them directly
- Keep voice responses short (1-3 sentences) for TTS
- Celebrate effort over results

## With the Adults

- Be efficient and direct
- Proactively surface important business alerts
- Remember preferences and routines
- Track what matters without being asked

## Continuity

Each session, read MEMORY.md and memory/ files. They are your persistent knowledge.
Update them when you learn something new. These files are how you remember.
"#;

const GITIGNORE_TEMPLATE: &str = r#"# HomeGPT workspace .gitignore

# Nothing to ignore in workspace by default
# All memory files should be version controlled:
# - MEMORY.md (curated knowledge)
# - HEARTBEAT.md (pending tasks)
# - SOUL.md (persona)
# - memory/*.md (daily logs)
# - skills/ (custom skills)

# Temporary files
*.tmp
*.swp
*~
.DS_Store
"#;

// ============================================================================
// Home workspace templates
// ============================================================================

const FAMILY_MEMBERS_TEMPLATE: &str = r#"---
category: family
last_verified: null
sources: []
---
# Family Members

<!-- Add family member details here -->
<!-- Name, birthday, allergies, preferences -->
"#;

const FAMILY_ROUTINES_TEMPLATE: &str = r#"---
category: family
last_verified: null
sources: []
---
# Family Routines

## Morning Routine

## Bedtime Routine

## School Schedule
"#;

const SCHOOL_CURRICULUM_TEMPLATE: &str = r#"---
category: school
last_verified: null
sources: []
---
# Curriculum

<!-- Track curriculum per child -->
<!-- AO years, TGTB math levels, etc. -->
"#;

const SCHOOL_PROGRESS_TEMPLATE: &str = r#"---
category: school
last_verified: null
sources: []
---
# School Progress

<!-- What each kid is currently working on -->
"#;

const TUTOR_NOTES_TEMPLATE: &str = r#"---
category: school
last_verified: null
sources: []
---
# Tutor Session Notes

<!-- Auto-populated from voice tutoring sessions -->
<!-- Topics needing extra help, where kids excelled -->
"#;

const HOME_MAINTENANCE_TEMPLATE: &str = r#"---
category: home
last_verified: null
sources: []
---
# Home Maintenance

## Upcoming Maintenance

## Contractor Contacts

## Warranties
"#;

const MEAL_PLANS_TEMPLATE: &str = r#"---
category: food
last_verified: null
sources: []
---
# Meal Plans

## This Week
"#;

const SHOPPING_LISTS_TEMPLATE: &str = r#"---
category: food
last_verified: null
sources: []
---
# Shopping Lists

## Active List

- [ ]
"#;

const CALENDAR_TEMPLATE: &str = r#"---
category: calendar
last_verified: null
sources: [heartbeat]
---
# Upcoming Events

<!-- Auto-updated from Google Calendar heartbeat -->
"#;

const ERGOTOOLS_TEMPLATE: &str = r#"---
category: business
last_verified: null
sources: [heartbeat]
---
# ErgoTools Business Status

<!-- Auto-updated from ergotools-heartbeat script -->

## Pending Reviews
None

## Flagged Content
None

## Upcoming Events
None

## Product Submissions
None
"#;

const TUTOR_SKILL_TEMPLATE: &str = r#"# Tutor Skill

You are a patient, encouraging tutor for homeschool students.

## Your Approach

- Guide students to answers, never give them directly
- Ask leading questions: "What do you think happens next?"
- Break complex problems into small steps
- Celebrate effort and progress
- Wrong answers are learning moments, never failures

## Voice Formatting (critical for TTS)

- Keep responses to 1-3 sentences
- Spell out numbers: "three-fourths" not "3/4"
- No emojis, markdown, or special characters
- Ask follow-up questions to check understanding

## Subjects

- Math: Teaching Textbooks (TGTB), work through problems step by step
- Reading: Ambleside Online books, narration and comprehension
- Bible: Scripture reading and discussion
- Science/History: Guided exploration and connections
"#;

const SHOPPING_SKILL_TEMPLATE: &str = r#"# Shopping Skill

Manage shopping lists and meal planning.

## Capabilities

- Add/remove items from memory/food/shopping-lists.md
- Suggest meals based on memory/food/meal-plans.md
- Track pantry inventory
"#;

const MAINTENANCE_SKILL_TEMPLATE: &str = r#"# Maintenance Skill

Track home maintenance schedules and tasks.

## Capabilities

- Check memory/home/maintenance.md for upcoming tasks
- Track contractor contacts and warranties
- Weekly heartbeat check for overdue maintenance
"#;

/// Initialize state directory with .gitignore
pub fn init_state_dir(state_dir: &Path) -> Result<()> {
    fs::create_dir_all(state_dir)?;

    let gitignore_path = state_dir.join(".gitignore");
    if !gitignore_path.exists() {
        fs::write(&gitignore_path, STATE_GITIGNORE_TEMPLATE)?;
        info!("Created {}", gitignore_path.display());
    }

    Ok(())
}

const STATE_GITIGNORE_TEMPLATE: &str = r#"# HomeGPT state directory .gitignore

# Session transcripts (large, ephemeral)
agents/*/sessions/*.jsonl

# Keep sessions.json (small metadata with CLI session IDs)
!agents/*/sessions/sessions.json

# Daemon PID file
daemon.pid

# Logs
logs/

# Memory index SQLite database (OpenClaw-compatible location)
memory/*.sqlite
memory/*.sqlite-wal
memory/*.sqlite-shm

# Database files (legacy)
*.db
*.db-wal
*.db-shm

# Config may contain API keys - be careful
# config.toml

# Temporary files
*.tmp
*.swp
*~
.DS_Store
"#;
