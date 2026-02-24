use std::path::Path;
use std::process::Command;

use crate::model::ProviderKind;

/// Information about a discovered agent process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProcess {
    pub pid: u32,
    pub provider: ProviderKind,
    pub command: String,
}

/// Discover the PID(s) of running agent processes for a given provider.
///
/// Uses a combination of provider-specific heuristics and `pgrep`:
/// - **Claude**: checks `pgrep -f "claude"` (the Claude Code CLI)
/// - **Codex**: checks `pgrep -f "codex"`
/// - **Amp**: checks `pgrep -f "amp"`
/// - **Gemini**: checks `pgrep -f "gemini"`
/// - **Pi**: checks `pgrep -f "pi"`
/// - **Opencode**: checks `pgrep -f "opencode"`
///
/// Returns all matching PIDs (not just the first), allowing callers
/// to correlate with session files.
pub fn discover_agent_pids(provider: ProviderKind) -> Vec<AgentProcess> {
    let binary_hint = provider_binary_hint(provider);

    let pids = pgrep_by_name(binary_hint);
    let mut results = Vec::new();

    for pid in pids {
        let command = read_process_command(pid).unwrap_or_else(|| binary_hint.to_string());
        results.push(AgentProcess {
            pid,
            provider,
            command,
        });
    }

    results
}

/// Discover a single PID for a provider (convenience wrapper).
///
/// Returns the first matching PID, or `None` if no process found.
pub fn discover_agent_pid(provider: ProviderKind) -> Option<u32> {
    let binary_hint = provider_binary_hint(provider);
    pgrep_by_name(binary_hint).into_iter().next()
}

/// Try to find a PID for a specific session by checking provider-specific
/// lock/marker files.
///
/// Currently supports:
/// - **Claude**: reads `<claude_root>/projects/<project_hash>/.active_session`
///   which may contain a PID or session reference.
pub fn discover_pid_for_session(
    provider: ProviderKind,
    _session_id: &str,
    provider_root: &Path,
) -> Option<u32> {
    match provider {
        ProviderKind::Claude => discover_claude_session_pid(provider_root),
        _ => {
            // Fall back to generic pgrep
            discover_agent_pid(provider)
        }
    }
}

/// Claude-specific: look for `.active_session` markers in project dirs.
fn discover_claude_session_pid(claude_root: &Path) -> Option<u32> {
    // Claude Code stores active session info in:
    //   ~/.claude/projects/<hash>/.active_session
    // or a lock file with PID. Try to find it.
    let projects_dir = claude_root.join("projects");
    if !projects_dir.exists() {
        return discover_agent_pid(ProviderKind::Claude);
    }

    // Check each project dir for .lock or .active_session files
    let entries = std::fs::read_dir(&projects_dir).ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let lock_path = entry.path().join(".lock");
        if lock_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&lock_path) {
                let trimmed = content.trim();
                if let Ok(pid) = trimmed.parse::<u32>() {
                    // Verify this PID is still alive
                    if process_alive(pid) {
                        return Some(pid);
                    }
                }
            }
        }
    }

    // Fall back to pgrep
    discover_agent_pid(ProviderKind::Claude)
}

fn provider_binary_hint(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Claude => "claude",
        ProviderKind::Codex => "codex",
        ProviderKind::Amp => "amp",
        ProviderKind::Gemini => "gemini",
        ProviderKind::Pi => "pi",
        ProviderKind::Opencode => "opencode",
    }
}

/// Run `pgrep -f <pattern>` and return all matching PIDs.
fn pgrep_by_name(pattern: &str) -> Vec<u32> {
    let output = Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .ok();

    let Some(output) = output else {
        return Vec::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

/// Read the command line of a process (macOS/Linux).
fn read_process_command(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;

    let cmd = String::from_utf8_lossy(&output.stdout);
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Check if a process is still alive via `kill -0`.
fn process_alive(pid: u32) -> bool {
    // kill(pid, 0) checks if the process exists without sending a signal
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_binary_hints_are_correct() {
        assert_eq!(provider_binary_hint(ProviderKind::Claude), "claude");
        assert_eq!(provider_binary_hint(ProviderKind::Codex), "codex");
        assert_eq!(provider_binary_hint(ProviderKind::Amp), "amp");
        assert_eq!(provider_binary_hint(ProviderKind::Gemini), "gemini");
        assert_eq!(provider_binary_hint(ProviderKind::Pi), "pi");
        assert_eq!(provider_binary_hint(ProviderKind::Opencode), "opencode");
    }

    #[test]
    fn process_alive_returns_true_for_self() {
        let pid = std::process::id();
        assert!(process_alive(pid));
    }

    #[test]
    fn process_alive_returns_false_for_invalid_pid() {
        // PID 99999999 almost certainly doesn't exist
        assert!(!process_alive(99_999_999));
    }

    #[test]
    fn read_process_command_works_for_self() {
        let pid = std::process::id();
        let cmd = read_process_command(pid);
        assert!(cmd.is_some());
    }

    #[test]
    fn discover_agent_pids_returns_vec() {
        // This is a smoke test — may return empty if no agents running
        let pids = discover_agent_pids(ProviderKind::Claude);
        // Just verify it doesn't panic
        let _ = pids;
    }

    #[test]
    fn discover_pid_for_session_falls_back_to_pgrep() {
        use tempfile::tempdir;

        let temp = tempdir().expect("tempdir");
        // Empty provider root — should fall back to pgrep
        let _pid = discover_pid_for_session(ProviderKind::Codex, "session-1", temp.path());
        // Just verify it doesn't panic
    }
}
