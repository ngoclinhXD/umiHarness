# Umisong Harness

A pure-Rust All-In-One (AIO) native desktop coding harness (`eframe` / `egui`) backed by an async Rust daemon (`umisongd`) that unifies **Claude Code**, **Antigravity**, and **OpenAI Codex** in one workspace.

Sign in with whichever account(s) you already have — Claude Pro/Max/Team/Enterprise, Google Antigravity, and/or ChatGPT Plus/Pro/Team/Enterprise — and automatically use the models included with each account's plan.

## Architecture

- **`crates/umisong-core`**: Shared domain types, normalized streaming `AgentEvent` protocol, unified git diff parser, and JSON-RPC IPC definitions.
- **`crates/umisong-providers`**: Official CLI discovery and adapters for:
  - **Claude Code** (`claude -p --output-format stream-json`, `claude auth status/login/logout`)
  - **Antigravity** (`agy -p --output-format stream-json`, `~/.gemini/antigravity-cli/settings.json` + PTY login)
  - **OpenAI Codex** (`codex app-server` JSON-RPC 2.0 with live approvals + `codex exec --json` fallback, `codex login status/login/logout`)
- **`crates/umisong-daemon`** (`umisongd`): Async Tokio daemon over Unix Domain Socket (`~/.umisong-harness/umisongd.sock`) with SQLite persistence (`~/.umisong-harness/umisong.db`), interactive PTY Auth Manager (`portable-pty` + `vt100`), Context Handoff builder, and Git Diff + Worktree Isolation manager.
- **`crates/umisong-app`** (`umisong`): Pure-Rust native desktop GUI (`egui` / `eframe`) that automatically connects to or starts `umisongd`.

## Features

- **Accounts & Plan Entitlements**: Live CLI discovery, auth & plan-tier detection (`Claude Pro/Max/Team`, `Google AI Pro/Ultra/ADC`, `ChatGPT Plus/Pro/Team`), embedded PTY Login Terminal with one-click OAuth URL opening, custom binary/model overrides, and plan-tier verification.
- **Unified Model Menu & Thinking Levels**:
  - **Logged-In-Only Visibility**: Models from unauthenticated accounts are hidden.
  - **Family-Grouped & Strength-Ranked**: Organized under `---- Claude ------`, `---- Gemini ------`, and `---- Codex -------` sorted by strength descending, with plan-locked higher-tier models clearly marked `(Unavailable)` (e.g., `Gemini 4 Carbon` on Google AI Pro vs unlocked on Google AI Ultra).
  - **Native Provider Token Prioritization**: When multiple accounts offer the same model (e.g., both Claude Code and Antigravity signed in and you pick `Claude Sonnet 5.5`), native account tokens (`Claude Code`) are automatically prioritized.
  - **Per-Model Cascading Thinking Levels**: Hovering/clicking any model opens a submenu listing only that model's supported thinking levels (e.g., `Low / Medium / High / Extra` for `Gemini 4 Neon`, `Low / Medium / High` for `Gemini 4 Argon`, `Medium / High / Max` for `Claude Fable 5.2`) and forwards the selected effort to each CLI.
- **Live Streaming & Approvals**: Stream markdown, collapsible reasoning/thoughts, live tool-call cards, and interactive approval prompts.
- **Context Handoff**: Package conversation history + workspace git status and transfer execution cleanly to another engine (in-thread or forked session).
- **Parallel Compare**: Run 2 or 3 providers side-by-side in **Read-Only** mode or **Isolated Git Worktrees**, compare responses, activity, and diffs, and click **Adopt & Apply Patch** to promote the winner.
- **Workspace Git Diff Viewer**: Inspect `Unstaged`, `Staged`, and `All vs HEAD` diffs with color-coded hunks, line numbers, filtering, and file revert.
- **Persistent Session History**: SQLite-backed workspaces, sessions, turns, tool calls, and compare runs.

## Quick Start

```bash
# Run the native desktop app (auto-starts the daemon in-process if umisongd isn't already running)
cargo run --bin umisong

# Or run the persistent background daemon standalone
cargo run --bin umisongd

# Run the full test suite
cargo test --workspace
```

## Installing Provider CLIs

Install any combination of the official CLIs you want to use:

- **Claude Code**: `npm install -g @anthropic-ai/claude-code`
- **Antigravity**: See https://antigravity.google/docs/cli/reference
- **OpenAI Codex**: `npm install -g @openai/codex`

Then open the **Accounts** tab (`⌘4`) in Umisong Harness and click **Sign In** on any installed provider.
