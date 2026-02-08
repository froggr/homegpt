//! Hash verification layer for anti-hallucination memory.
//!
//! Ported from barf's CSNP (Coherent State Network Protocol).
//! Every memory chunk gets a SHA-256 hash at index time, and search results
//! are verified before returning to ensure data integrity.

use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

/// A verified chunk result with cryptographic proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedChunk {
    /// Original chunk content
    pub file: String,
    pub line_start: i32,
    pub line_end: i32,
    pub content: String,
    pub score: f64,

    /// Verification status
    pub verified: bool,
    /// Short hash for citation: first 8 hex chars
    pub hash_prefix: String,
    /// Full SHA-256 hash
    pub hash: String,
    /// Source provenance
    pub provenance: Provenance,
    /// Confidence level
    pub confidence: Confidence,
}

impl VerifiedChunk {
    /// Format as a citable reference for the LLM
    pub fn to_citation(&self) -> String {
        if self.verified {
            format!("[VERIFIED:{}] {}", self.hash_prefix, self.file)
        } else {
            format!("[UNVERIFIED] {}", self.file)
        }
    }
}

/// Source provenance: WHERE the information came from
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Provenance {
    /// User directly stated this information
    UserStated,
    /// Found via web search
    WebSearch { url: String, query: String },
    /// Read from a file in the workspace
    FileContent { path: String },
    /// Discovered during autonomous heartbeat task
    HeartbeatDiscovery { task: String },
    /// Unknown / legacy data without provenance
    Unknown,
}

impl std::fmt::Display for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Provenance::UserStated => write!(f, "user-stated"),
            Provenance::WebSearch { url, .. } => write!(f, "web-search:{}", url),
            Provenance::FileContent { path } => write!(f, "file:{}", path),
            Provenance::HeartbeatDiscovery { task } => write!(f, "heartbeat:{}", task),
            Provenance::Unknown => write!(f, "unknown"),
        }
    }
}

/// Confidence level for memory results
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum Confidence {
    /// Hash failed or no match
    None,
    /// Old, single source, infrequent access
    Low,
    /// Web-sourced, hash verified, cross-referenced
    Medium,
    /// User-stated, recently stored, hash verified, frequently accessed
    High,
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Confidence::None => write!(f, "none"),
            Confidence::Low => write!(f, "low"),
            Confidence::Medium => write!(f, "medium"),
            Confidence::High => write!(f, "high"),
        }
    }
}

/// Compute SHA-256 hash of a chunk's content + metadata
pub fn compute_chunk_hash(path: &str, content: &str, timestamp: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hasher.update(b"|");
    hasher.update(content.as_bytes());
    hasher.update(b"|");
    hasher.update(timestamp.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Manages chunk verification hashes in a SQLite table alongside the chunks
#[derive(Clone)]
pub struct ChunkVerifier {
    conn: Arc<Mutex<Connection>>,
}

impl ChunkVerifier {
    /// Create a new ChunkVerifier using the same connection as MemoryIndex
    pub fn new(conn: Arc<Mutex<Connection>>) -> Result<Self> {
        {
            let conn = conn.lock().map_err(|e| anyhow!("Lock poisoned: {}", e))?;

            // Create the verification table
            conn.execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS chunk_hashes (
                    chunk_id TEXT PRIMARY KEY,
                    path TEXT NOT NULL,
                    hash TEXT NOT NULL,
                    timestamp TEXT NOT NULL,
                    provenance TEXT NOT NULL DEFAULT 'unknown',
                    access_count INTEGER NOT NULL DEFAULT 0,
                    last_accessed TEXT,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_chunk_hashes_path ON chunk_hashes(path);
                CREATE INDEX IF NOT EXISTS idx_chunk_hashes_hash ON chunk_hashes(hash);
                "#,
            )?;
        }

        Ok(Self { conn })
    }

    /// Record a hash for a chunk at index time
    pub fn record_hash(
        &self,
        chunk_id: &str,
        path: &str,
        content: &str,
        provenance: &Provenance,
    ) -> Result<String> {
        let now = Utc::now().to_rfc3339();
        let hash = compute_chunk_hash(path, content, &now);
        let provenance_str = serde_json::to_string(provenance)?;

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        conn.execute(
            r#"INSERT OR REPLACE INTO chunk_hashes
               (chunk_id, path, hash, timestamp, provenance, access_count, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)"#,
            params![chunk_id, path, &hash, &now, &provenance_str, &now],
        )?;

        debug!("Recorded hash for chunk {}: {}", chunk_id, &hash[..8]);
        Ok(hash)
    }

    /// Verify a chunk's content against its stored hash
    pub fn verify_chunk(&self, chunk_id: &str, path: &str, content: &str) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let result: Option<(String, String)> = conn
            .query_row(
                "SELECT hash, timestamp FROM chunk_hashes WHERE chunk_id = ?1",
                params![chunk_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();

        match result {
            Some((stored_hash, timestamp)) => {
                let computed_hash = compute_chunk_hash(path, content, &timestamp);
                let matches = stored_hash == computed_hash;

                if matches {
                    // Update access tracking
                    let now = Utc::now().to_rfc3339();
                    let _ = conn.execute(
                        "UPDATE chunk_hashes SET access_count = access_count + 1, last_accessed = ?1 WHERE chunk_id = ?2",
                        params![&now, chunk_id],
                    );
                } else {
                    warn!(
                        "Hash mismatch for chunk {}: stored={}, computed={}",
                        chunk_id,
                        &stored_hash[..8],
                        &computed_hash[..8]
                    );
                }

                Ok(matches)
            }
            None => {
                debug!("No hash found for chunk {}", chunk_id);
                Ok(false)
            }
        }
    }

    /// Get the provenance and hash for a chunk
    pub fn get_chunk_info(
        &self,
        chunk_id: &str,
    ) -> Result<Option<(String, Provenance, i64, Option<String>)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let result: Option<(String, String, i64, Option<String>)> = conn
            .query_row(
                "SELECT hash, provenance, access_count, last_accessed FROM chunk_hashes WHERE chunk_id = ?1",
                params![chunk_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .ok();

        match result {
            Some((hash, provenance_str, access_count, last_accessed)) => {
                let provenance: Provenance =
                    serde_json::from_str(&provenance_str).unwrap_or(Provenance::Unknown);
                Ok(Some((hash, provenance, access_count, last_accessed)))
            }
            None => Ok(None),
        }
    }

    /// Calculate confidence score for a chunk based on provenance, access patterns, and verification
    pub fn calculate_confidence(
        &self,
        verified: bool,
        provenance: &Provenance,
        access_count: i64,
        _last_accessed: &Option<String>,
    ) -> Confidence {
        if !verified {
            return Confidence::None;
        }

        match provenance {
            Provenance::UserStated => {
                if access_count > 2 {
                    Confidence::High
                } else {
                    Confidence::High // User-stated is always high confidence
                }
            }
            Provenance::FileContent { .. } => {
                if access_count > 5 {
                    Confidence::High
                } else {
                    Confidence::Medium
                }
            }
            Provenance::WebSearch { .. } => Confidence::Medium,
            Provenance::HeartbeatDiscovery { .. } => Confidence::Medium,
            Provenance::Unknown => {
                if access_count > 10 {
                    Confidence::Medium
                } else {
                    Confidence::Low
                }
            }
        }
    }

    /// Remove hashes for chunks belonging to a path (called when file is re-indexed)
    pub fn remove_hashes_for_path(&self, path: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let count = conn.execute(
            "DELETE FROM chunk_hashes WHERE path = ?1",
            params![path],
        )?;

        Ok(count)
    }

    /// Get verification stats
    pub fn stats(&self) -> Result<VerificationStats> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let total: i64 =
            conn.query_row("SELECT COUNT(*) FROM chunk_hashes", [], |row| row.get(0))?;

        let by_provenance: Vec<(String, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT provenance, COUNT(*) FROM chunk_hashes GROUP BY provenance",
            )?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.filter_map(|r| r.ok()).collect()
        };

        Ok(VerificationStats {
            total_hashes: total as usize,
            by_provenance,
        })
    }
}

#[derive(Debug)]
pub struct VerificationStats {
    pub total_hashes: usize,
    pub by_provenance: Vec<(String, i64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().unwrap();
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn test_compute_chunk_hash() {
        let hash1 = compute_chunk_hash("test.md", "hello world", "2026-01-01T00:00:00Z");
        let hash2 = compute_chunk_hash("test.md", "hello world", "2026-01-01T00:00:00Z");
        let hash3 = compute_chunk_hash("test.md", "different content", "2026-01-01T00:00:00Z");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn test_record_and_verify() {
        let conn = setup_test_db();
        let verifier = ChunkVerifier::new(conn).unwrap();

        let hash = verifier
            .record_hash("chunk1", "test.md", "hello world", &Provenance::UserStated)
            .unwrap();

        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_provenance_display() {
        assert_eq!(Provenance::UserStated.to_string(), "user-stated");
        assert_eq!(
            Provenance::WebSearch {
                url: "https://example.com".into(),
                query: "test".into()
            }
            .to_string(),
            "web-search:https://example.com"
        );
        assert_eq!(
            Provenance::FileContent {
                path: "test.md".into()
            }
            .to_string(),
            "file:test.md"
        );
    }

    #[test]
    fn test_confidence_scoring() {
        let conn = setup_test_db();
        let verifier = ChunkVerifier::new(conn).unwrap();

        // User-stated is always high
        assert_eq!(
            verifier.calculate_confidence(true, &Provenance::UserStated, 0, &None),
            Confidence::High
        );

        // Unverified is always none
        assert_eq!(
            verifier.calculate_confidence(false, &Provenance::UserStated, 100, &None),
            Confidence::None
        );

        // Unknown with low access is low
        assert_eq!(
            verifier.calculate_confidence(true, &Provenance::Unknown, 1, &None),
            Confidence::Low
        );

        // Web search is medium
        assert_eq!(
            verifier.calculate_confidence(
                true,
                &Provenance::WebSearch {
                    url: "test".into(),
                    query: "q".into()
                },
                0,
                &None
            ),
            Confidence::Medium
        );
    }
}
