use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dirs::home_dir;
use walkdir::WalkDir;

use crate::error::{Result, XurlError};
use crate::model::{ActiveSession, ProviderKind, ResolvedThread};

pub mod amp;
pub mod claude;
#[cfg(feature = "sqlite")]
pub mod codex;
pub mod gemini;
#[cfg(feature = "sqlite")]
pub mod opencode;
pub mod pi;

pub trait Provider {
    fn resolve(&self, session_id: &str) -> Result<ResolvedThread>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRoots {
    pub amp_root: PathBuf,
    pub codex_root: PathBuf,
    pub claude_root: PathBuf,
    pub gemini_root: PathBuf,
    pub pi_root: PathBuf,
    pub opencode_root: PathBuf,
}

impl ProviderRoots {
    pub fn from_env_or_home() -> Result<Self> {
        let home = home_dir().ok_or(XurlError::HomeDirectoryNotFound)?;

        // Precedence:
        // 1) XDG_DATA_HOME/amp
        // 2) ~/.local/share/amp
        let amp_root = env::var_os("XDG_DATA_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .map(|path| path.join("amp"))
            .unwrap_or_else(|| home.join(".local/share/amp"));

        // Precedence:
        // 1) CODEX_HOME (official Codex home env)
        // 2) ~/.codex (Codex default)
        let codex_root = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));

        // Precedence:
        // 1) CLAUDE_CONFIG_DIR (official Claude Code config/data root env)
        // 2) ~/.claude (Claude default)
        let claude_root = env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude"));

        // Precedence:
        // 1) GEMINI_CLI_HOME/.gemini (official Gemini CLI home env)
        // 2) ~/.gemini (Gemini default)
        let gemini_root = env::var_os("GEMINI_CLI_HOME")
            .map(PathBuf::from)
            .map(|path| path.join(".gemini"))
            .unwrap_or_else(|| home.join(".gemini"));

        // Precedence:
        // 1) PI_CODING_AGENT_DIR (official pi coding agent root env)
        // 2) ~/.pi/agent (pi default)
        let pi_root = env::var_os("PI_CODING_AGENT_DIR")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".pi/agent"));

        // Precedence:
        // 1) XDG_DATA_HOME/opencode
        // 2) ~/.local/share/opencode
        let opencode_root = env::var_os("XDG_DATA_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .map(|path| path.join("opencode"))
            .unwrap_or_else(|| home.join(".local/share/opencode"));

        Ok(Self {
            amp_root,
            codex_root,
            claude_root,
            gemini_root,
            pi_root,
            opencode_root,
        })
    }

    /// Scan all provider root directories, returning sessions modified within `max_age`.
    pub fn list_active_sessions(&self, max_age: Duration) -> Vec<ActiveSession> {
        let providers: &[(ProviderKind, &Path, &str)] = &[
            (ProviderKind::Claude, &self.claude_root, "projects"),
            (ProviderKind::Codex, &self.codex_root, "sessions"),
            (ProviderKind::Amp, &self.amp_root, "threads"),
            (ProviderKind::Gemini, &self.gemini_root, "tmp"),
            (ProviderKind::Pi, &self.pi_root, "sessions"),
            (ProviderKind::Opencode, &self.opencode_root, "sessions"),
        ];

        let now = SystemTime::now();
        let mut sessions = Vec::new();

        for &(provider, root, subdir) in providers {
            let scan_root = root.join(subdir);
            if !scan_root.exists() {
                continue;
            }

            let walker = WalkDir::new(&scan_root)
                .max_depth(4)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file());

            for entry in walker {
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext != "jsonl" && ext != "json" {
                    continue;
                }

                let mtime = entry.metadata().ok().and_then(|m| m.modified().ok());
                let age = mtime.and_then(|mt| now.duration_since(mt).ok());
                if age.is_some_and(|d| d > max_age) {
                    continue;
                }

                let file_len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if file_len < 10 {
                    continue;
                }

                let session_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let mtime_epoch = mtime
                    .and_then(|mt| mt.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let age_secs = age.map(|d| d.as_secs()).unwrap_or(u64::MAX);
                let is_active = age_secs < 60;

                sessions.push(ActiveSession {
                    provider,
                    session_id,
                    path: path.to_path_buf(),
                    mtime_epoch,
                    is_active,
                });
            }
        }

        // Deduplicate by (provider, session_id), keeping the most recent
        let mut best: HashMap<(ProviderKind, String), usize> = HashMap::new();
        let mut deduped: Vec<ActiveSession> = Vec::new();

        for session in sessions {
            let key = (session.provider, session.session_id.clone());
            if let Some(&idx) = best.get(&key) {
                if session.mtime_epoch > deduped[idx].mtime_epoch {
                    deduped[idx] = session;
                }
            } else {
                best.insert(key, deduped.len());
                deduped.push(session);
            }
        }

        // Sort: active first, then by mtime descending
        deduped.sort_by(|a, b| {
            b.is_active
                .cmp(&a.is_active)
                .then(b.mtime_epoch.cmp(&a.mtime_epoch))
        });

        deduped
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use tempfile::tempdir;

    use super::*;

    fn make_roots(base: &std::path::Path) -> ProviderRoots {
        ProviderRoots {
            amp_root: base.join("amp"),
            codex_root: base.join("codex"),
            claude_root: base.join("claude"),
            gemini_root: base.join("gemini"),
            pi_root: base.join("pi"),
            opencode_root: base.join("opencode"),
        }
    }

    #[test]
    fn discovers_session_files() {
        let temp = tempdir().expect("tempdir");
        let roots = make_roots(temp.path());

        // Create a Claude session file
        let projects = roots.claude_root.join("projects").join("proj1");
        fs::create_dir_all(&projects).expect("mkdir");
        let session_file = projects.join("abc123.jsonl");
        fs::write(&session_file, "{\"type\":\"user\"}\n{\"type\":\"assistant\"}\n")
            .expect("write");

        let sessions = roots.list_active_sessions(Duration::from_secs(300));
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider, ProviderKind::Claude);
        assert_eq!(sessions[0].session_id, "abc123");
        assert!(sessions[0].is_active);
    }

    #[test]
    fn skips_tiny_files() {
        let temp = tempdir().expect("tempdir");
        let roots = make_roots(temp.path());

        let sessions_dir = roots.codex_root.join("sessions");
        fs::create_dir_all(&sessions_dir).expect("mkdir");
        fs::write(sessions_dir.join("small.jsonl"), "{}").expect("write");

        let sessions = roots.list_active_sessions(Duration::from_secs(300));
        assert!(sessions.is_empty());
    }

    #[test]
    fn deduplicates_by_session_id() {
        let temp = tempdir().expect("tempdir");
        let roots = make_roots(temp.path());

        // Create two files with same stem in different subdirs
        let dir1 = roots.claude_root.join("projects").join("proj1");
        let dir2 = roots.claude_root.join("projects").join("proj2");
        fs::create_dir_all(&dir1).expect("mkdir");
        fs::create_dir_all(&dir2).expect("mkdir");
        fs::write(
            dir1.join("same-session.jsonl"),
            "{\"type\":\"user\",\"old\":true}\n",
        )
        .expect("write");
        // Small delay to ensure different mtime
        std::thread::sleep(Duration::from_millis(50));
        fs::write(
            dir2.join("same-session.jsonl"),
            "{\"type\":\"user\",\"new\":true}\n",
        )
        .expect("write");

        let sessions = roots.list_active_sessions(Duration::from_secs(300));
        let matching: Vec<_> = sessions
            .iter()
            .filter(|s| s.session_id == "same-session")
            .collect();
        assert_eq!(matching.len(), 1);
    }

    #[test]
    fn empty_roots_returns_empty() {
        let temp = tempdir().expect("tempdir");
        let roots = make_roots(temp.path());
        let sessions = roots.list_active_sessions(Duration::from_secs(300));
        assert!(sessions.is_empty());
    }
}
