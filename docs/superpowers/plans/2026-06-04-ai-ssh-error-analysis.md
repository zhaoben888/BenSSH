# AI SSH Error Analysis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an AI SSH login mode that captures remote Linux errors inside the TUI and shows local/DeepSeek-compatible advice in the right panel.

**Architecture:** Keep existing Windows Terminal SSH unchanged. Add focused modules for AI analysis and managed SSH PTY sessions, then wire a new `AppMode::AiSsh` into the Ratatui event loop. Start with testable local analysis/config behavior, then integrate UI and networking.

**Tech Stack:** Rust 2021, `ssh2`, `ratatui`, `crossterm`, `tokio`, `reqwest`, `serde`, `serde_json`.

---

## File Structure

- Create `src/ai.rs`: model configuration, local rule analysis, prompt construction, OpenAI-compatible API call.
- Create `src/ai_ssh.rs`: direct SSH session with PTY shell, input sending, output polling.
- Modify `src/main.rs`: add AI SSH state, rendering, keyboard handling, analysis trigger.
- Modify `src/sftp.rs`: expose shared SSH authentication so SFTP and AI SSH do not duplicate credential handling.
- Modify `Cargo.toml`: add `reqwest`.
- Modify `README.md`: document `i` AI SSH shortcut and DeepSeek/OpenAI-compatible variables.

## Task 1: AI Local Analysis And Config

**Files:**
- Create: `src/ai.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write failing unit tests in `src/ai.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_analysis_detects_permission_denied() {
        let result = analyze_locally("cat /etc/shadow: Permission denied");
        assert!(result.is_some());
        let text = result.unwrap().to_panel_text();
        assert!(text.contains("权限"));
        assert!(text.contains("sudo"));
    }

    #[test]
    fn local_analysis_ignores_normal_output() {
        assert!(analyze_locally("hello\nworld").is_none());
    }

    #[test]
    fn ai_config_prefers_deepseek_variables() {
        let vars = [
            ("DEEPSEEK_API_KEY", "deepseek-key"),
            ("DEEPSEEK_BASE_URL", "https://api.deepseek.com"),
            ("DEEPSEEK_MODEL", "deepseek-chat"),
            ("OPENAI_API_KEY", "openai-key"),
            ("OPENAI_BASE_URL", "https://api.openai.com/v1"),
            ("OPENAI_MODEL", "gpt-4.1-mini"),
        ];
        let config = AiConfig::from_pairs(vars);
        assert_eq!(config.api_key.as_deref(), Some("deepseek-key"));
        assert_eq!(config.base_url, "https://api.deepseek.com");
        assert_eq!(config.model, "deepseek-chat");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test ai::tests`

Expected: FAIL because `src/ai.rs`, `AiConfig`, `analyze_locally`, and `AiAnalysis` do not exist.

- [ ] **Step 3: Implement minimal `src/ai.rs`**

Add `AiAnalysis`, `AiConfig::from_env`, `AiConfig::from_pairs`, `analyze_locally`, `looks_like_error`, and `to_panel_text`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test ai::tests`

Expected: PASS.

## Task 2: OpenAI-Compatible API Client

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/ai.rs`

- [ ] **Step 1: Write failing tests for endpoint and prompt building**

```rust
#[test]
fn chat_endpoint_normalizes_base_url() {
    let config = AiConfig {
        api_key: Some("k".into()),
        base_url: "https://api.deepseek.com/".into(),
        model: "deepseek-chat".into(),
    };
    assert_eq!(config.chat_endpoint(), "https://api.deepseek.com/chat/completions");
}

#[test]
fn prompt_does_not_include_secret_fields() {
    let prompt = build_prompt("prod", "root", "Permission denied");
    assert!(prompt.contains("prod"));
    assert!(prompt.contains("root"));
    assert!(prompt.contains("Permission denied"));
    assert!(!prompt.to_lowercase().contains("password"));
    assert!(!prompt.contains("keyPath"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test ai::tests`

Expected: FAIL because `chat_endpoint` and `build_prompt` are missing.

- [ ] **Step 3: Add API helpers and dependency**

Add `reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }` to `Cargo.toml`.

Add async `analyze_with_model(config, node_name, user, context)` that calls `POST {base}/chat/completions` with bearer auth and parses `choices[0].message.content`.

- [ ] **Step 4: Run tests**

Run: `cargo test ai::tests`

Expected: PASS.

## Task 3: Shared SSH Authentication And AI SSH Session

**Files:**
- Modify: `src/sftp.rs`
- Create: `src/ai_ssh.rs`

- [ ] **Step 1: Make authentication reusable**

Change `fn authenticate_session` in `src/sftp.rs` to:

```rust
pub fn authenticate_session(sess: &mut Session, entry: &VpsEntry) -> Result<(), Box<dyn std::error::Error>>
```

- [ ] **Step 2: Create `src/ai_ssh.rs` session wrapper**

Define `AiSshSession` with `connect`, `read_available`, `send_key`, `send_text`, and `close`.

- [ ] **Step 3: Run compile check**

Run: `cargo check`

Expected: PASS or actionable compile errors only in the new session API.

## Task 4: TUI Integration

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add modules and state**

Add `mod ai; mod ai_ssh;`, extend `AppMode` with `AiSsh`, and add output/advice/session state variables.

- [ ] **Step 2: Add rendering**

Render AI SSH as a horizontal split: terminal output left, AI advice right.

- [ ] **Step 3: Add keyboard flow**

In server list, `i` connects to selected node through `ai_ssh::AiSshSession::connect`. In AI SSH mode, normal keys are sent to the session, `Esc`/`b` returns to server list, and `Ctrl+C` sends `\x03`.

- [ ] **Step 4: Add analysis trigger**

After reading remote output, append to buffer, call `ai::looks_like_error`, show local advice immediately, then call model if API key is configured.

- [ ] **Step 5: Run compile check**

Run: `cargo check`

Expected: PASS.

## Task 5: README And Manual Verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README**

Document `i` AI SSH login, DeepSeek variables, OpenAI-compatible fallback, and privacy note.

- [ ] **Step 2: Final verification**

Run: `cargo test`

Run: `cargo check`

Expected: PASS.

Manual check when a reachable server is configured:

- `Enter` still opens Windows Terminal SSH.
- `i` enters AI SSH mode.
- Running `ls /not-exist` shows advice in the right panel.
- Without API key, local advice still appears.
