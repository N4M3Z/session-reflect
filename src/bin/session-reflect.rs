use serde::Deserialize;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::ExitCode;

/// Combined JSON payload supporting both Stop and PreCompact hook events.
/// Unknown fields are silently ignored by serde.
#[derive(Deserialize)]
struct HookInput {
    /// Stop-specific: true when the hook itself triggered this invocation.
    #[serde(default)]
    stop_hook_active: bool,
    #[serde(default)]
    cwd: String,
    /// Present in Stop hooks; may be absent in PreCompact.
    #[serde(default)]
    transcript_path: String,
    /// PreCompact-specific: "manual" or "auto".
    #[serde(default)]
    trigger: Option<String>,
}

const TOOL_TURN_THRESHOLD: usize = 10;
const USER_MSG_THRESHOLD: usize = 4;

const FALLBACK_REASON: &str =
    "Substantial session with no learnings captured. Create a file in Memory/Learnings/ or Memory/Decisions/ before ending.";

const PRECOMPACT_PREFIX: &str =
    "BEFORE COMPACTING — capture session learnings and decisions now. ";

const MEMORY_PATHS: &[&str] = &["Memory/Learnings/", "Memory/Decisions/"];

fn main() -> ExitCode {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return ExitCode::SUCCESS;
    }

    let input = match serde_json::from_str::<HookInput>(&buf) {
        Ok(i) => i,
        Err(_) => return ExitCode::SUCCESS,
    };

    let is_pre_compact = input.trigger.is_some();

    // Guard: prevent infinite loop (Stop only)
    if !is_pre_compact && input.stop_hook_active {
        return ExitCode::SUCCESS;
    }

    // Guard: only fire inside ~/Data
    let home = std::env::var("HOME").unwrap_or_default();
    let data_prefix = format!("{}/Data", home);
    if !input.cwd.starts_with(&data_prefix) {
        return ExitCode::SUCCESS;
    }

    // For PreCompact: compaction implies substantial session.
    // Always inject the reflection prompt — let the AI decide whether
    // additional capture is needed, even if some memory was already written.
    if is_pre_compact {
        let reason = load_reflection_prompt(&input.cwd)
            .unwrap_or_else(|| FALLBACK_REASON.to_string());

        let output = serde_json::json!({
            "additionalContext": format!("{}{}", PRECOMPACT_PREFIX, reason)
        });
        println!("{}", output);
        return ExitCode::SUCCESS;
    }

    // --- Stop hook path (existing behavior) ---

    let transcript = match fs::read_to_string(&input.transcript_path) {
        Ok(t) => t,
        Err(_) => return ExitCode::SUCCESS,
    };

    let (user_messages, tool_using_turns, has_memory_write) = analyze_transcript(&transcript);

    // Not substantial → allow stop (both thresholds must be met)
    if user_messages < USER_MSG_THRESHOLD || tool_using_turns < TOOL_TURN_THRESHOLD {
        return ExitCode::SUCCESS;
    }

    // Substantial + memory writes → allow stop
    if has_memory_write {
        return ExitCode::SUCCESS;
    }

    // Substantial + no memory writes → block and prompt reflection
    let reason = load_reflection_prompt(&input.cwd).unwrap_or_else(|| FALLBACK_REASON.to_string());

    let output = serde_json::json!({
        "decision": "block",
        "reason": reason
    });

    println!("{}", output);
    ExitCode::SUCCESS
}

/// Analyze transcript for user messages, tool-using turns, and memory writes.
fn analyze_transcript(transcript: &str) -> (usize, usize, bool) {
    let mut user_messages: usize = 0;
    let mut tool_using_turns: usize = 0;
    let mut has_memory_write = false;

    for line in transcript.lines() {
        let entry: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let entry_type = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");

        if entry_type == "human" {
            user_messages += 1;
            continue;
        }

        if entry_type != "assistant" {
            continue;
        }

        let content = match entry
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        {
            Some(arr) => arr,
            None => continue,
        };

        let mut turn_has_tool_use = false;

        for item in content {
            let item_type = match item.get("type").and_then(|t| t.as_str()) {
                Some(t) => t,
                None => continue,
            };

            if item_type != "tool_use" {
                continue;
            }

            turn_has_tool_use = true;

            let tool_name = item.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if tool_name != "Edit" && tool_name != "Write" {
                continue;
            }

            let file_path = item
                .get("input")
                .and_then(|i| i.get("file_path"))
                .and_then(|p| p.as_str())
                .unwrap_or("");

            for memory_path in MEMORY_PATHS {
                if file_path.contains(memory_path) {
                    has_memory_write = true;
                }
            }
        }

        if turn_has_tool_use {
            tool_using_turns += 1;
        }
    }

    (user_messages, tool_using_turns, has_memory_write)
}

/// Load the reflection prompt from the Pattern file, stripping frontmatter and H1.
fn load_reflection_prompt(cwd: &str) -> Option<String> {
    let pattern_path = Path::new(cwd)
        .join("Vaults/Personal/Orchestration/Patterns/Session Reflect.md");

    let content = fs::read_to_string(pattern_path).ok()?;
    let stripped = strip_frontmatter_and_h1(&content);

    if stripped.trim().is_empty() {
        None
    } else {
        Some(stripped.trim().to_string())
    }
}

/// Remove YAML frontmatter (between first --- pair) and the first H1 line.
fn strip_frontmatter_and_h1(content: &str) -> String {
    let mut lines = content.lines();
    let mut result = Vec::new();
    let mut in_frontmatter = false;
    let mut frontmatter_done = false;
    let mut h1_removed = false;

    // Check if first line is frontmatter delimiter
    if let Some(first) = lines.next() {
        if first.trim() == "---" {
            in_frontmatter = true;
        } else {
            // No frontmatter — check if it's an H1
            if first.starts_with("# ") && !h1_removed {
                h1_removed = true;
            } else {
                result.push(first);
            }
            frontmatter_done = true;
        }
    }

    for line in lines {
        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
                frontmatter_done = true;
            }
            continue;
        }

        if frontmatter_done && !h1_removed && line.starts_with("# ") {
            h1_removed = true;
            continue;
        }

        result.push(line);
    }

    result.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_frontmatter_and_h1() {
        let input = "---\ntitle: Test\n---\n# My Title\n\nBody text here.\n";
        let result = strip_frontmatter_and_h1(input);
        assert_eq!(result.trim(), "Body text here.");
    }

    #[test]
    fn test_strip_h1_only() {
        let input = "# My Title\n\nBody text here.\n";
        let result = strip_frontmatter_and_h1(input);
        assert_eq!(result.trim(), "Body text here.");
    }

    #[test]
    fn test_no_frontmatter_no_h1() {
        let input = "Just body text.\n";
        let result = strip_frontmatter_and_h1(input);
        assert_eq!(result.trim(), "Just body text.");
    }
}
