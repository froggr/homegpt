use anyhow::Result;
use clap::Args;
use futures::StreamExt;
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::Value;
use std::io::{self, Write};
use std::sync::Arc;

use localgpt::agent::{
    get_last_session_id_for_agent, list_sessions_for_agent, search_sessions_for_agent, Agent,
    AgentConfig,
};
use localgpt::config::Config;
use localgpt::memory::{FastEmbedProvider, MemoryManager};

/// Extract a snippet from content, centered around the query match
fn extract_snippet(content: &str, query: &str, max_len: usize) -> String {
    // Normalize content: collapse whitespace and newlines
    let normalized: String = content.split_whitespace().collect::<Vec<_>>().join(" ");

    // Try to find query (case-insensitive)
    let lower_content = normalized.to_lowercase();
    let lower_query = query.to_lowercase();

    if let Some(pos) = lower_content.find(&lower_query) {
        // Center the snippet around the match
        let half_len = max_len / 2;
        let start = pos.saturating_sub(half_len);
        let end = (pos + query.len() + half_len).min(normalized.len());

        // Adjust to word boundaries
        let mut snippet_start = start;
        let mut snippet_end = end;

        // Find word boundary at start
        if start > 0 {
            if let Some(space_pos) = normalized[start..].find(' ') {
                snippet_start = start + space_pos + 1;
            }
        }

        // Find word boundary at end
        if end < normalized.len() {
            if let Some(space_pos) = normalized[..end].rfind(' ') {
                snippet_end = space_pos;
            }
        }

        let snippet = &normalized[snippet_start..snippet_end];
        let prefix = if snippet_start > 0 { "..." } else { "" };
        let suffix = if snippet_end < normalized.len() {
            "..."
        } else {
            ""
        };

        format!("{}{}{}", prefix, snippet, suffix)
    } else {
        // No match found, show beginning of content
        let truncated: String = normalized.chars().take(max_len).collect();
        if normalized.len() > max_len {
            // Find last word boundary
            if let Some(space_pos) = truncated.rfind(' ') {
                format!("{}...", &truncated[..space_pos])
            } else {
                format!("{}...", truncated)
            }
        } else {
            truncated
        }
    }
}

/// Extract relevant detail from tool arguments for display
fn extract_tool_detail(tool_name: &str, arguments: &str) -> Option<String> {
    let args: Value = serde_json::from_str(arguments).ok()?;

    match tool_name {
        "edit_file" | "write_file" | "read_file" => args
            .get("path")
            .or_else(|| args.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "bash" => {
            args.get("command").and_then(|v| v.as_str()).map(|s| {
                // Truncate long commands
                if s.len() > 60 {
                    format!("{}...", &s[..57])
                } else {
                    s.to_string()
                }
            })
        }
        "memory_search" => args
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| format!("\"{}\"", s)),
        "web_fetch" => args
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        _ => None,
    }
}

#[derive(Args)]
pub struct ChatArgs {
    /// Model to use (overrides config)
    #[arg(short, long)]
    pub model: Option<String>,

    /// Session ID to resume
    #[arg(short, long)]
    pub session: Option<String>,

    /// Resume the most recent session
    #[arg(long)]
    pub resume: bool,
}

pub async fn run(args: ChatArgs, agent_id: &str) -> Result<()> {
    let config = Config::load()?;
    let mut memory = MemoryManager::new_with_agent(&config.memory, agent_id)?;

    // Set up local embedding provider (default - no API key needed)
    match FastEmbedProvider::new(None) {
        Ok(provider) => {
            memory = memory.with_embedding_provider(Arc::new(provider));
        }
        Err(e) => {
            eprintln!("Warning: Failed to create local embedding provider: {}", e);
        }
    }

    let agent_config = AgentConfig {
        model: args.model.unwrap_or(config.agent.default_model.clone()),
        context_window: config.agent.context_window,
        reserve_tokens: config.agent.reserve_tokens,
    };

    let mut agent = Agent::new(agent_config, &config, memory).await?;

    // Determine session to use
    let session_id = if let Some(id) = args.session {
        Some(id)
    } else if args.resume {
        get_last_session_id_for_agent(agent_id)?
    } else {
        None
    };

    // Resume or create session
    if let Some(session_id) = session_id {
        match agent.resume_session(&session_id).await {
            Ok(()) => {
                let status = agent.session_status();
                println!(
                    "Resumed session {} ({} messages)\n",
                    &session_id[..8],
                    status.message_count
                );
            }
            Err(e) => {
                eprintln!("Could not resume session: {}. Starting new session.\n", e);
                agent.new_session().await?;
            }
        }
    } else {
        agent.new_session().await?;
    }

    let embedding_status = if agent.has_embeddings() {
        " | Embeddings: enabled"
    } else {
        ""
    };
    println!(
        "LocalGPT v{} | Agent: {} | Model: {} | Memory: {} chunks{}\n",
        env!("CARGO_PKG_VERSION"),
        agent_id,
        agent.model(),
        agent.memory_chunk_count(),
        embedding_status
    );
    println!("Type /help for commands, /quit to exit\n");

    // Store agent_id for command handling
    let agent_id = agent_id.to_string();

    let mut rl = DefaultEditor::new()?;
    let mut stdout = io::stdout();

    // Track pending file attachments
    let mut pending_attachments: Vec<(String, String)> = Vec::new(); // (filename, content)

    loop {
        let readline = rl.readline("You: ");

        let input = match readline {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                break; // Ctrl+D
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        };

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        // Add to history
        let _ = rl.add_history_entry(input);

        // Handle commands
        if input.starts_with('/') {
            // Special handling for /attach - adds to pending attachments
            if input.starts_with("/attach") {
                let parts: Vec<&str> = input.split_whitespace().collect();
                if parts.len() < 2 {
                    eprintln!("Usage: /attach <file_path>");
                    continue;
                }
                let file_path = parts[1..].join(" ");
                let expanded = shellexpand::tilde(&file_path).to_string();

                match std::fs::read_to_string(&expanded) {
                    Ok(content) => {
                        let filename = std::path::Path::new(&expanded)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(&file_path);
                        pending_attachments.push((filename.to_string(), content));
                        println!(
                            "Attached: {} ({} bytes)",
                            filename,
                            pending_attachments.last().unwrap().1.len()
                        );
                        println!("Type your message to send with attachment(s), or /attachments to list.\n");
                    }
                    Err(e) => {
                        eprintln!("Failed to read file: {}", e);
                    }
                }
                continue;
            }

            // /attachments - list pending attachments
            if input == "/attachments" {
                if pending_attachments.is_empty() {
                    println!("\nNo pending attachments.\n");
                } else {
                    println!("\nPending attachments:");
                    for (i, (name, content)) in pending_attachments.iter().enumerate() {
                        println!("  {}. {} ({} bytes)", i + 1, name, content.len());
                    }
                    println!("\nType your message to send, or /clear-attachments to remove.\n");
                }
                continue;
            }

            // /clear-attachments - clear pending attachments
            if input == "/clear-attachments" {
                let count = pending_attachments.len();
                pending_attachments.clear();
                println!("\nCleared {} attachment(s).\n", count);
                continue;
            }

            match handle_command(input, &mut agent, &agent_id).await {
                CommandResult::Continue => continue,
                CommandResult::Quit => break,
                CommandResult::Error(e) => {
                    eprintln!("Error: {}", e);
                    continue;
                }
            }
        }

        // Build message with attachments
        let message = if pending_attachments.is_empty() {
            input.to_string()
        } else {
            let mut msg = input.to_string();
            msg.push_str("\n\n---\n\n**Attached files:**\n");
            for (name, content) in &pending_attachments {
                msg.push_str(&format!("\n### {}\n```\n{}\n```\n", name, content));
            }
            pending_attachments.clear();
            msg
        };

        // Send message to agent with streaming
        print!("\nLocalGPT: ");
        stdout.flush()?;

        match agent.chat_stream(&message).await {
            Ok(mut stream) => {
                let mut full_response = String::new();
                let mut pending_tool_calls = None;

                while let Some(result) = stream.next().await {
                    match result {
                        Ok(chunk) => {
                            print!("{}", chunk.delta);
                            stdout.flush()?;
                            full_response.push_str(&chunk.delta);

                            // Capture tool calls from the final chunk
                            if chunk.done && chunk.tool_calls.is_some() {
                                pending_tool_calls = chunk.tool_calls;
                            }
                        }
                        Err(e) => {
                            eprintln!("\nStream error: {}", e);
                            break;
                        }
                    }
                }

                // Handle tool calls if any
                if let Some(tool_calls) = pending_tool_calls {
                    // Show tool execution with details
                    for tc in &tool_calls {
                        let detail = extract_tool_detail(&tc.name, &tc.arguments);
                        if let Some(d) = detail {
                            println!("\n[{}: {}]", tc.name, d);
                        } else {
                            println!("\n[{}]", tc.name);
                        }
                    }
                    stdout.flush()?;

                    match agent
                        .execute_streaming_tool_calls(&full_response, tool_calls)
                        .await
                    {
                        Ok(follow_up) => {
                            print!("{}", follow_up);
                            stdout.flush()?;
                        }
                        Err(e) => {
                            eprintln!("Tool execution error: {}", e);
                        }
                    }
                } else {
                    // No tool calls - just finish the stream
                    agent.finish_chat_stream(&full_response);
                }

                if let Err(e) = agent.auto_save_session() {
                    eprintln!("Warning: Failed to auto-save session: {}", e);
                }
                println!("\n");
            }
            Err(e) => {
                eprintln!("Error: {}\n", e);
            }
        }
    }

    println!("Goodbye!");
    Ok(())
}

enum CommandResult {
    Continue,
    Quit,
    Error(String),
}

async fn handle_command(input: &str, agent: &mut Agent, agent_id: &str) -> CommandResult {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let cmd = parts[0];

    match cmd {
        "/quit" | "/exit" | "/q" => CommandResult::Quit,

        "/help" | "/h" | "/?" => {
            println!("\nCommands:");
            println!("  /help, /h, /?     - Show this help");
            println!("  /quit, /exit, /q  - Exit chat");
            println!("  /new              - Start a fresh session (reloads memory context)");
            println!("  /sessions         - List available sessions");
            println!("  /search <query>   - Search across all sessions");
            println!("  /resume <id>      - Resume a specific session");
            println!("  /model [name]     - Show or switch model (e.g., /model gpt-4o)");
            println!("  /models           - List available model prefixes");
            println!("  /context          - Show context window usage");
            println!("  /export [file]    - Export session as markdown");
            println!("  /attach <file>    - Attach file to next message");
            println!("  /attachments      - List pending attachments");
            println!("  /compact          - Compact session history");
            println!("  /clear            - Clear session history (keeps context)");
            println!("  /memory <query>   - Search memory");
            println!("  /reindex          - Rebuild memory index");
            println!("  /save             - Save current session");
            println!("  /status           - Show session status and API token usage");
            println!();
            CommandResult::Continue
        }

        "/sessions" => match list_sessions_for_agent(agent_id) {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("\nNo saved sessions found.\n");
                } else {
                    println!("\nAvailable sessions:");
                    for (i, session) in sessions.iter().take(10).enumerate() {
                        println!(
                            "  {}. {} ({} messages, {})",
                            i + 1,
                            &session.id[..8],
                            session.message_count,
                            session.created_at.format("%Y-%m-%d %H:%M")
                        );
                    }
                    if sessions.len() > 10 {
                        println!("  ... and {} more", sessions.len() - 10);
                    }
                    println!("\nUse /resume <id> to resume a session.\n");
                }
                CommandResult::Continue
            }
            Err(e) => CommandResult::Error(format!("Failed to list sessions: {}", e)),
        },

        "/search" => {
            if parts.len() < 2 {
                return CommandResult::Error("Usage: /search <query>".into());
            }
            let query = parts[1..].join(" ");

            match search_sessions_for_agent(agent_id, &query) {
                Ok(results) => {
                    if results.is_empty() {
                        println!("\nNo sessions found matching '{}'.\n", query);
                    } else {
                        println!("\nSessions matching '{}':", query);
                        for (i, result) in results.iter().take(10).enumerate() {
                            println!(
                                "  {}. {} ({} matches, {})",
                                i + 1,
                                &result.session_id[..8.min(result.session_id.len())],
                                result.match_count,
                                result.created_at.format("%Y-%m-%d")
                            );
                            if !result.message_preview.is_empty() {
                                println!("     \"{}\"", result.message_preview);
                            }
                        }
                        if results.len() > 10 {
                            println!("  ... and {} more", results.len() - 10);
                        }
                        println!("\nUse /resume <id> to resume a session.\n");
                    }
                    CommandResult::Continue
                }
                Err(e) => CommandResult::Error(format!("Search failed: {}", e)),
            }
        }

        "/resume" => {
            if parts.len() < 2 {
                return CommandResult::Error("Usage: /resume <session-id>".into());
            }
            let session_id = parts[1];

            // Find session by prefix match
            match list_sessions_for_agent(agent_id) {
                Ok(sessions) => {
                    let matching: Vec<_> = sessions
                        .iter()
                        .filter(|s| s.id.starts_with(session_id))
                        .collect();

                    match matching.len() {
                        0 => CommandResult::Error(format!(
                            "No session found matching '{}'",
                            session_id
                        )),
                        1 => {
                            let full_id = matching[0].id.clone();
                            match futures::executor::block_on(agent.resume_session(&full_id)) {
                                Ok(()) => {
                                    let status = agent.session_status();
                                    println!(
                                        "\nResumed session {} ({} messages)\n",
                                        &full_id[..8],
                                        status.message_count
                                    );
                                    CommandResult::Continue
                                }
                                Err(e) => CommandResult::Error(format!("Failed to resume: {}", e)),
                            }
                        }
                        _ => CommandResult::Error(format!(
                            "Multiple sessions match '{}'. Please be more specific.",
                            session_id
                        )),
                    }
                }
                Err(e) => CommandResult::Error(format!("Failed to list sessions: {}", e)),
            }
        }

        "/model" => {
            if parts.len() < 2 {
                println!("\nCurrent model: {}\n", agent.model());
                return CommandResult::Continue;
            }
            let model = parts[1];
            match agent.set_model(model) {
                Ok(()) => {
                    println!("\nSwitched to model: {}\n", model);
                    CommandResult::Continue
                }
                Err(e) => CommandResult::Error(format!("Failed to switch model: {}", e)),
            }
        }

        "/compact" => match agent.compact_session().await {
            Ok((before, after)) => {
                println!("\nSession compacted. Token count: {} → {}\n", before, after);
                CommandResult::Continue
            }
            Err(e) => CommandResult::Error(format!("Failed to compact: {}", e)),
        },

        "/clear" => {
            agent.clear_session();
            println!("\nSession cleared.\n");
            CommandResult::Continue
        }

        "/new" => {
            // Save current session to memory before starting new one
            match agent.save_session_to_memory().await {
                Ok(Some(path)) => {
                    println!("\nSession saved to: {}", path.display());
                }
                Ok(None) => {} // No messages to save
                Err(e) => {
                    eprintln!("Warning: Failed to save session to memory: {}", e);
                }
            }

            match agent.new_session().await {
                Ok(()) => {
                    println!("New session started. Memory context reloaded.\n");
                    CommandResult::Continue
                }
                Err(e) => CommandResult::Error(format!("Failed to create new session: {}", e)),
            }
        }

        "/memory" => {
            if parts.len() < 2 {
                return CommandResult::Error("Usage: /memory <query>".into());
            }
            let query = parts[1..].join(" ");
            match agent.search_memory(&query).await {
                Ok(results) => {
                    if results.is_empty() {
                        println!(
                            "\nNo results found for '{}'. Try /reindex to rebuild memory index.\n",
                            query
                        );
                    } else {
                        println!("\nMemory search results for '{}':", query);
                        for (i, result) in results.iter().enumerate() {
                            let snippet = extract_snippet(&result.content, &query, 120);
                            println!(
                                "{}. [{}:{}] {}",
                                i + 1,
                                result.file,
                                result.line_start,
                                snippet
                            );
                        }
                        println!();
                    }
                    CommandResult::Continue
                }
                Err(e) => CommandResult::Error(format!("Memory search failed: {}", e)),
            }
        }

        "/reindex" => match futures::executor::block_on(agent.reindex_memory()) {
            Ok((files, chunks, embedded)) => {
                if embedded > 0 {
                    println!(
                        "\nMemory index rebuilt: {} files, {} chunks, {} embeddings\n",
                        files, chunks, embedded
                    );
                } else {
                    println!(
                        "\nMemory index rebuilt: {} files, {} chunks\n",
                        files, chunks
                    );
                }
                CommandResult::Continue
            }
            Err(e) => CommandResult::Error(format!("Failed to reindex: {}", e)),
        },

        "/save" => match agent.save_session().await {
            Ok(path) => {
                println!("\nSession saved to: {}\n", path.display());
                CommandResult::Continue
            }
            Err(e) => CommandResult::Error(format!("Failed to save session: {}", e)),
        },

        "/status" => {
            let status = agent.session_status();
            println!("\nSession Status:");
            println!("  ID: {}", status.id);
            println!("  Model: {}", agent.model());
            println!("  Messages: {}", status.message_count);
            println!("  Context tokens: ~{}", status.token_count);
            println!("  Compactions: {}", status.compaction_count);

            println!("\nMemory:");
            println!("  Chunks: {}", agent.memory_chunk_count());
            if agent.has_embeddings() {
                println!("  Embeddings: enabled");
            }

            if status.api_input_tokens > 0 || status.api_output_tokens > 0 {
                println!("\nAPI Usage:");
                println!("  Input tokens: {}", status.api_input_tokens);
                println!("  Output tokens: {}", status.api_output_tokens);
                println!(
                    "  Total tokens: {}",
                    status.api_input_tokens + status.api_output_tokens
                );
            }
            println!();
            CommandResult::Continue
        }

        "/models" => {
            println!("\nAvailable model prefixes:");
            println!(
                "  claude-cli/*    - Use Claude CLI (e.g., claude-cli/opus, claude-cli/sonnet)"
            );
            println!("  gpt-*           - OpenAI (requires API key)");
            println!("  claude-*        - Anthropic API (requires API key)");
            println!("  ollama/*        - Ollama local (e.g., ollama/llama3)");
            println!("  <other>         - Defaults to Ollama");
            println!("\nCurrent model: {}", agent.model());
            println!("Use /model <name> to switch.\n");
            CommandResult::Continue
        }

        "/context" => {
            let (used, usable, total) = agent.context_usage();
            let pct = (used as f64 / usable as f64 * 100.0).min(100.0);
            println!("\nContext Window:");
            println!("  Used: {} tokens ({:.1}%)", used, pct);
            println!("  Usable: {} tokens", usable);
            println!("  Total: {} tokens", total);
            println!("  Reserve: {} tokens", total - usable);

            if pct > 80.0 {
                println!("\n⚠ Context nearly full. Consider /compact or /new.");
            }
            println!();
            CommandResult::Continue
        }

        "/export" => {
            let markdown = agent.export_markdown();
            if parts.len() >= 2 {
                let path = parts[1..].join(" ");
                let expanded = shellexpand::tilde(&path).to_string();
                match std::fs::write(&expanded, &markdown) {
                    Ok(()) => {
                        println!("\nSession exported to: {}\n", expanded);
                        CommandResult::Continue
                    }
                    Err(e) => CommandResult::Error(format!("Failed to export: {}", e)),
                }
            } else {
                // Print to stdout
                println!("\n{}", markdown);
                CommandResult::Continue
            }
        }

        _ => CommandResult::Error(format!(
            "Unknown command: {}. Type /help for commands.",
            cmd
        )),
    }
}
