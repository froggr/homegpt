//! Background worker that owns the Agent
//!
//! The worker runs in a separate thread with its own tokio runtime.
//! It receives commands from the UI and sends back status updates.

use std::pin::pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use anyhow::Result;
use futures::StreamExt;

use crate::agent::{
    list_sessions_for_agent, Agent, AgentConfig, StreamEvent, ToolCall, DEFAULT_AGENT_ID,
};
use crate::config::Config;
use crate::memory::MemoryManager;

use super::state::{UiMessage, WorkerMessage};

/// Handle to the background worker
pub struct WorkerHandle {
    /// Send commands to the worker
    pub tx: Sender<UiMessage>,
    /// Receive updates from the worker
    pub rx: Receiver<WorkerMessage>,
    /// Thread handle
    _thread: JoinHandle<()>,
}

impl WorkerHandle {
    /// Start the background worker
    pub fn start(agent_id: Option<String>) -> Result<Self> {
        let (ui_tx, ui_rx) = mpsc::channel::<UiMessage>();
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();

        let agent_id = agent_id.unwrap_or_else(|| DEFAULT_AGENT_ID.to_string());

        let thread = thread::spawn(move || {
            // Create tokio runtime for this thread
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            rt.block_on(async {
                if let Err(e) = worker_loop(agent_id, ui_rx, worker_tx).await {
                    eprintln!("Worker error: {}", e);
                }
            });
        });

        Ok(Self {
            tx: ui_tx,
            rx: worker_rx,
            _thread: thread,
        })
    }

    /// Send a message to the worker
    pub fn send(&self, msg: UiMessage) -> Result<()> {
        self.tx.send(msg)?;
        Ok(())
    }

    /// Try to receive a message from the worker (non-blocking)
    pub fn try_recv(&self) -> Option<WorkerMessage> {
        self.rx.try_recv().ok()
    }
}

async fn worker_loop(
    agent_id: String,
    rx: Receiver<UiMessage>,
    tx: Sender<WorkerMessage>,
) -> Result<()> {
    // Initialize agent
    let config = Config::load()?;
    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), &agent_id)?;

    let agent_config = AgentConfig {
        model: config.agent.default_model.clone(),
        context_window: config.agent.context_window,
        reserve_tokens: config.agent.reserve_tokens,
    };

    let mut agent = Agent::new(agent_config, &config, memory).await?;
    agent.new_session().await?;

    // Send ready message
    let _ = tx.send(WorkerMessage::Ready {
        model: agent.model().to_string(),
        memory_chunks: agent.memory_chunk_count(),
        has_embeddings: agent.has_embeddings(),
    });

    // Send initial session list
    if let Ok(sessions) = list_sessions_for_agent(&agent_id) {
        let _ = tx.send(WorkerMessage::Sessions(sessions));
    }

    // Send initial status
    let _ = tx.send(WorkerMessage::Status(agent.session_status()));

    // Track tools requiring approval
    let approval_tools: Vec<String> = agent.approval_required_tools().to_vec();

    // Main loop
    while let Ok(msg) = rx.recv() {
        let mut should_auto_save = false;

        match msg {
            UiMessage::Chat(message) => {
                // Stream response with tool support
                match agent.chat_stream_with_tools(&message).await {
                    Ok(stream) => {
                        let mut stream = pin!(stream);
                        let mut pending_tools: Vec<ToolCall> = Vec::new();

                        while let Some(result) = stream.next().await {
                            match result {
                                Ok(event) => match event {
                                    StreamEvent::Content(text) => {
                                        let _ = tx.send(WorkerMessage::ContentChunk(text));
                                    }
                                    StreamEvent::ToolCallStart { name, id } => {
                                        // Check if this tool requires approval
                                        if approval_tools.contains(&name) {
                                            // Collect for approval
                                            pending_tools.push(ToolCall {
                                                id,
                                                name,
                                                arguments: String::new(),
                                            });
                                        } else {
                                            let _ =
                                                tx.send(WorkerMessage::ToolCallStart { name, id });
                                        }
                                    }
                                    StreamEvent::ToolCallEnd { name, id, output } => {
                                        let _ = tx.send(WorkerMessage::ToolCallEnd {
                                            name,
                                            id,
                                            output,
                                        });
                                    }
                                    StreamEvent::Done => {
                                        if !pending_tools.is_empty() {
                                            let _ = tx.send(WorkerMessage::ToolsPendingApproval(
                                                pending_tools.clone(),
                                            ));
                                            pending_tools.clear();
                                        } else {
                                            let _ = tx.send(WorkerMessage::Done);
                                        }
                                        should_auto_save = true;
                                    }
                                },
                                Err(e) => {
                                    let _ = tx.send(WorkerMessage::Error(e.to_string()));
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(WorkerMessage::Error(e.to_string()));
                    }
                }
            }
            UiMessage::NewSession => match agent.new_session().await {
                Ok(()) => {
                    let status = agent.session_status();
                    let _ = tx.send(WorkerMessage::SessionChanged {
                        id: status.id.clone(),
                        message_count: status.message_count,
                    });
                    let _ = tx.send(WorkerMessage::Status(status));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::Error(e.to_string()));
                }
            },
            UiMessage::ResumeSession(session_id) => match agent.resume_session(&session_id).await {
                Ok(()) => {
                    let status = agent.session_status();
                    let _ = tx.send(WorkerMessage::SessionChanged {
                        id: status.id.clone(),
                        message_count: status.message_count,
                    });
                    let _ = tx.send(WorkerMessage::Status(status));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::Error(e.to_string()));
                }
            },
            UiMessage::ApproveTools(_tools) => {
                // Tool approval is handled in chat loop
                // For now, just send done
                let _ = tx.send(WorkerMessage::Done);
            }
            UiMessage::DenyTools => {
                let _ = tx.send(WorkerMessage::Done);
            }
            UiMessage::RefreshSessions => {
                if let Ok(sessions) = list_sessions_for_agent(&agent_id) {
                    let _ = tx.send(WorkerMessage::Sessions(sessions));
                }
            }
            UiMessage::RefreshStatus => {
                let _ = tx.send(WorkerMessage::Status(agent.session_status()));
            }
        }

        // Auto-save session after chat completes
        if should_auto_save {
            if let Err(e) = agent.auto_save_session() {
                eprintln!("Warning: Failed to auto-save session: {}", e);
            }
        }
    }

    Ok(())
}
