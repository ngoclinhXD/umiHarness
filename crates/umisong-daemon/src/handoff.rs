use crate::git_ops;
use std::path::Path;
use umisong_core::{ProviderId, SessionDetail, TurnRecord};

/// Builds a concise, structured handoff prompt so a different provider can pick up
/// an in-flight coding session with full context, workspace state, and provenance.
pub fn build_handoff_prompt(
    session: &SessionDetail,
    workspace_path: &Path,
    up_to_turn_id: Option<&str>,
    target_provider: ProviderId,
    user_instruction: &str,
) -> (String, Option<ProviderId>, Option<String>) {
    let mut included_turns: Vec<&TurnRecord> = Vec::new();
    for turn in &session.turns {
        included_turns.push(turn);
        if let Some(stop_id) = up_to_turn_id {
            if turn.id == stop_id {
                break;
            }
        }
    }

    let last_turn = included_turns.last().copied();
    let from_provider = last_turn.map(|t| t.provider);
    let from_turn_id = last_turn.map(|t| t.id.clone());

    let mut buf = String::new();
    buf.push_str(&format!(
        "# Context Handoff to {}\n\n",
        target_provider.display_name()
    ));
    buf.push_str(&format!(
        "You are continuing session **{}** in workspace `{}`.\n",
        session.title,
        workspace_path.display()
    ));

    if let Some(branch) = git_ops::current_branch(workspace_path) {
        buf.push_str(&format!("Current git branch: `{branch}`.\n"));
    }

    let stat = git_ops::diff_stat(workspace_path);
    if !stat.trim().is_empty() {
        buf.push_str("\n## Current Workspace Changes (`git diff --stat`)\n```text\n");
        buf.push_str(stat.trim());
        buf.push_str("\n```\n");
    }

    if !included_turns.is_empty() {
        buf.push_str("\n## Prior Conversation Summary\n");
        // Keep the most recent turns within a bounded character budget (~12,000 chars)
        let recent_start = included_turns.len().saturating_sub(6);
        if recent_start > 0 {
            buf.push_str(&format!(
                "_(Omitted {recent_start} earlier turns for brevity.)_\n\n"
            ));
        }

        for turn in &included_turns[recent_start..] {
            buf.push_str(&format!(
                "### Turn {} ({}{})\n",
                turn.seq,
                turn.provider.display_name(),
                turn.resolved_model
                    .as_deref()
                    .or((!turn.model.is_empty()).then_some(turn.model.as_str()))
                    .map(|m| format!(" · `{m}`"))
                    .unwrap_or_default()
            ));
            buf.push_str("**User:**\n");
            buf.push_str(&truncate_block(&turn.prompt, 1200));
            buf.push('\n');

            if !turn.tool_calls.is_empty() {
                buf.push_str("\n**Key tool actions:**\n");
                for tc in turn.tool_calls.iter().take(6) {
                    let ok = match tc.success {
                        Some(true) => "ok",
                        Some(false) => "failed",
                        None => "running",
                    };
                    buf.push_str(&format!("- `{}` ({ok}): {}\n", tc.name, tc.input_summary));
                }
            }

            if !turn.response_text.trim().is_empty() {
                buf.push_str("\n**Assistant:**\n");
                buf.push_str(&truncate_block(&turn.response_text, 2200));
                buf.push('\n');
            }
            buf.push('\n');
        }
    }

    buf.push_str("## Next Step / Handoff Instruction\n");
    if user_instruction.trim().is_empty() {
        buf.push_str(
            "Review the context and current workspace state above, verify what has been done so far, and continue from where the previous agent left off.",
        );
    } else {
        buf.push_str(user_instruction.trim());
    }

    (buf, from_provider, from_turn_id)
}

fn truncate_block(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}\n… [truncated]")
    } else {
        head
    }
}
