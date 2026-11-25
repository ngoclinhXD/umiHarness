use crate::discovery::{enriched_path, find_binary, probe_version};
use crate::{
    CommandSpec, ProviderAdapter, ProviderProbeCache, TurnExecutionRequest, merge_model_options, summarize_json_input,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use umisong_core::{
    AgentEvent, ApprovalDecision, AuthLoginMode, AuthState, ModelOption, PermissionMode, PlanTier,
    ProviderId, ProviderSettings, ProviderStatus, ToolKind, TurnExitReason,
};

pub struct CodexAdapter;

impl CodexAdapter {
    fn default_models() -> Vec<ModelOption> {
        vec![
            ModelOption::plan_default("Uses active ChatGPT Plus / Pro / Team Codex plan model"),
            ModelOption {
                id: "gpt-5.3-codex".into(),
                label: "GPT-5.3 Codex".into(),
                tier_hint: "Primary coding model".into(),
                is_plan_default: false,
                discovered_live: false,
            },
            ModelOption {
                id: "gpt-5-codex".into(),
                label: "GPT-5 Codex".into(),
                tier_hint: "Standard coding tier".into(),
                is_plan_default: false,
                discovered_live: false,
            },
            ModelOption {
                id: "gpt-5-codex-mini".into(),
                label: "GPT-5 Codex Mini".into(),
                tier_hint: "Fast lightweight coding".into(),
                is_plan_default: false,
                discovered_live: false,
            },
        ]
    }
}

#[async_trait]
impl ProviderAdapter for CodexAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    async fn probe_status(
        &self,
        settings: &ProviderSettings,
        cache: &ProviderProbeCache,
    ) -> ProviderStatus {
        let models = merge_model_options(
            Self::default_models(),
            &cache.discovered_models,
            &settings.custom_models,
        );
        let Some(bin) = find_binary("codex", settings.binary_override.as_deref()) else {
            return ProviderStatus {
                provider: ProviderId::Codex,
                installed: false,
                binary_path: None,
                version: None,
                auth: AuthState::NotInstalled,
                effective_plan_tier: PlanTier::AutoDetect,
                models,
                active_default_model: settings.default_model.clone(),
                settings: settings.clone(),
                notes: Some(format!("Install via: {}", ProviderId::Codex.install_hint())),
            };
        };

        let version = probe_version(&bin).await;
        let mut cmd = Command::new(&bin);
        cmd.args(["login", "status"])
            .env("PATH", enriched_path())
            .stdin(Stdio::null());
        if let Some(key) = settings
            .api_key_override
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            cmd.env("OPENAI_API_KEY", key.trim());
        }

        let auth = match tokio::time::timeout(std::time::Duration::from_secs(6), cmd.output()).await
        {
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                parse_codex_login_status(&stdout, &stderr, out.status.success())
            }
            Ok(Err(e)) => AuthState::Unknown {
                detail: format!("Failed to invoke `codex login status`: {e}"),
            },
            Err(_) => AuthState::Unknown {
                detail: "Timed out checking `codex login status`".to_string(),
            },
        };

        let auth = match auth {
            AuthState::Authenticated {
                method,
                account_hint,
                plan_hint,
            } if method == "chatgpt" => {
                match probe_codex_account_rpc(&bin, settings.api_key_override.as_deref()).await {
                    Some((_, email, plan)) => AuthState::Authenticated {
                        method,
                        account_hint: email.or(account_hint),
                        plan_hint: plan.or(plan_hint),
                    },
                    None => AuthState::Authenticated {
                        method,
                        account_hint,
                        plan_hint,
                    },
                }
            }
            other => other,
        };

        let effective_plan_tier = match settings.plan_tier_override {
            Some(t) if t != PlanTier::AutoDetect => t,
            _ => PlanTier::infer_from_auth(ProviderId::Codex, &auth),
        };

        ProviderStatus {
            provider: ProviderId::Codex,
            installed: true,
            binary_path: Some(bin.to_string_lossy().into_owned()),
            version,
            auth,
            effective_plan_tier,
            models,
            active_default_model: settings.default_model.clone(),
            settings: settings.clone(),
            notes: None,
        }
    }

    fn login_command(
        &self,
        settings: &ProviderSettings,
        mode: AuthLoginMode,
    ) -> Result<CommandSpec> {
        let bin = find_binary("codex", settings.binary_override.as_deref())
            .context("OpenAI Codex CLI (`codex`) not found. Install it via `npm install -g @openai/codex`.")?;
        let mut args = vec!["login".to_string()];
        match mode {
            AuthLoginMode::BrowserOAuth => {}
            AuthLoginMode::DeviceCode => args.push("--device-auth".to_string()),
            AuthLoginMode::ApiKey => args.push("--with-api-key".to_string()),
        }
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), enriched_path());
        Ok(CommandSpec {
            program: bin.to_string_lossy().into_owned(),
            args,
            env,
            cwd: None,
        })
    }

    async fn logout(&self, settings: &ProviderSettings) -> Result<()> {
        let bin = find_binary("codex", settings.binary_override.as_deref())
            .context("OpenAI Codex CLI (`codex`) not found")?;
        let out = Command::new(bin)
            .arg("logout")
            .env("PATH", enriched_path())
            .output()
            .await?;
        if !out.status.success() {
            bail!(
                "codex logout failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    }

    async fn run_turn(
        &self,
        req: TurnExecutionRequest,
        tx: mpsc::UnboundedSender<AgentEvent>,
        approval_rx: mpsc::UnboundedReceiver<ApprovalDecision>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let bin = find_binary("codex", req.settings.binary_override.as_deref()).context(
            "OpenAI Codex CLI (`codex`) is not installed. Configure or install it in the Accounts tab.",
        )?;

        // Prefer `codex app-server` JSON-RPC when in AskSafe mode (for interactive tool approvals),
        // falling back cleanly to `codex exec --json` if app-server is unsupported.
        if matches!(req.permission_mode, PermissionMode::AskSafe) {
            if let Ok(()) = run_codex_app_server(&bin, &req, &tx, approval_rx, cancel.clone()).await
            {
                return Ok(());
            }
        }

        run_codex_exec(&bin, &req, &tx, cancel).await
    }
}

async fn run_codex_app_server(
    bin: &Path,
    req: &TurnExecutionRequest,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    mut approval_rx: mpsc::UnboundedReceiver<ApprovalDecision>,
    cancel: CancellationToken,
) -> Result<()> {
    let mut cmd = Command::new(bin);
    cmd.arg("app-server")
        .current_dir(&req.workspace_path)
        .env("PATH", enriched_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(key) = req
        .settings
        .api_key_override
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        cmd.env("OPENAI_API_KEY", key.trim());
    }

    let mut child = cmd.spawn().context("Failed to spawn `codex app-server`")?;
    let mut stdin = child.stdin.take().context("Missing stdin")?;
    let stdout = child.stdout.take().context("Missing stdout")?;

    // 1. Initialize handshake
    let init_msg = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "clientInfo": { "name": "umisong-harness", "version": "0.1.0" }
        }
    });
    stdin
        .write_all(format!("{init_msg}\n").as_bytes())
        .await?;

    // 2. Start or resume thread
    let mut thread_params = json!({
        "cwd": req.workspace_path.to_string_lossy(),
    });
    if !req.model.trim().is_empty() {
        thread_params["model"] = json!(req.model.trim());
    }
    let start_req = if let Some(tid) = req
        .provider_session_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "thread/resume",
            "params": { "threadId": tid }
        })
    } else {
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "thread/start",
            "params": thread_params
        })
    };
    stdin
        .write_all(format!("{start_req}\n").as_bytes())
        .await?;
    stdin.flush().await?;

    let mut lines = BufReader::new(stdout).lines();
    let mut thread_id: Option<String> = req.provider_session_id.clone();
    let mut turn_started = false;
    let mut pending_rpc_approvals: HashMap<String, Value> = HashMap::new();

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                if let Some(tid) = thread_id.as_deref() {
                    let interrupt = json!({
                        "jsonrpc": "2.0",
                        "id": 99,
                        "method": "turn/interrupt",
                        "params": { "threadId": tid }
                    });
                    let _ = stdin.write_all(format!("{interrupt}\n").as_bytes()).await;
                    let _ = stdin.flush().await;
                }
                let _ = child.kill().await;
                let _ = tx.send(AgentEvent::TurnCompleted {
                    final_text: None,
                    exit_reason: TurnExitReason::Cancelled,
                });
                return Ok(());
            }
            Some(decision) = approval_rx.recv() => {
                if let Some(rpc_id) = pending_rpc_approvals.remove(&decision.approval_id) {
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": rpc_id,
                        "result": {
                            "decision": if decision.approved { "approved" } else { "denied" }
                        }
                    });
                    let _ = stdin.write_all(format!("{resp}\n").as_bytes()).await;
                    let _ = stdin.flush().await;
                }
            }
            line_res = lines.next_line() => {
                let Ok(Some(line)) = line_res else {
                    if !turn_started {
                        bail!("codex app-server closed before starting turn");
                    }
                    break;
                };

                let parsed = parse_codex_app_server_line(&line);
                if let Some((approval_key, rpc_id)) = parsed.pending_approval_rpc_id {
                    pending_rpc_approvals.insert(approval_key, rpc_id);
                }

                if let Some(tid) = parsed.thread_id {
                    thread_id = Some(tid.clone());
                    if !turn_started {
                        turn_started = true;
                        let mut turn_params = json!({
                            "threadId": tid,
                            "input": [{ "type": "text", "text": req.prompt }]
                        });
                        if let Some(effort) = req.thinking_level.as_codex_effort() {
                            turn_params["effort"] = json!(effort);
                        }
                        let turn_req = json!({
                            "jsonrpc": "2.0",
                            "id": 3,
                            "method": "turn/start",
                            "params": turn_params
                        });
                        let _ = stdin.write_all(format!("{turn_req}\n").as_bytes()).await;
                        let _ = stdin.flush().await;
                    }
                }

                let mut done = false;
                for ev in parsed.events {
                    if matches!(ev, AgentEvent::TurnCompleted { .. }) {
                        done = true;
                    }
                    let _ = tx.send(ev);
                }
                if done {
                    let _ = child.kill().await;
                    return Ok(());
                }
            }
        }
    }

    let _ = child.kill().await;
    Ok(())
}

async fn run_codex_exec(
    bin: &Path,
    req: &TurnExecutionRequest,
    tx: &mpsc::UnboundedSender<AgentEvent>,
    cancel: CancellationToken,
) -> Result<()> {
    let mut cmd = Command::new(bin);
    cmd.current_dir(&req.workspace_path)
        .env("PATH", enriched_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(key) = req
        .settings
        .api_key_override
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        cmd.env("OPENAI_API_KEY", key.trim());
    }

    let sandbox = match req.permission_mode {
        PermissionMode::PlanReadOnly => "read-only",
        PermissionMode::AskSafe => "workspace-write",
        PermissionMode::FullAuto => "danger-full-access",
    };

    cmd.arg("exec");
    if let Some(sess_id) = req
        .provider_session_id
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        // `codex exec resume [OPTIONS] [SESSION_ID] [PROMPT]` does NOT accept `--sandbox`;
        // override sandbox via `-c sandbox_mode="..."` per `codex exec resume --help`.
        cmd.arg("resume");
        cmd.args(["--json", "--skip-git-repo-check"]);
        if !req.model.trim().is_empty() {
            cmd.args(["--model", req.model.trim()]);
        }
        if let Some(effort) = req.thinking_level.as_codex_effort() {
            cmd.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
        }
        cmd.args(["-c", &format!("sandbox_mode=\"{sandbox}\"")]);
        cmd.arg(sess_id);
        cmd.arg(&req.prompt);
    } else {
        cmd.args(["--json", "--skip-git-repo-check"]);
        if !req.model.trim().is_empty() {
            cmd.args(["--model", req.model.trim()]);
        }
        if let Some(effort) = req.thinking_level.as_codex_effort() {
            cmd.args(["-c", &format!("model_reasoning_effort=\"{effort}\"")]);
        }
        cmd.args(["--sandbox", sandbox]);
        cmd.arg(&req.prompt);
    }

    let mut child = cmd.spawn().context("Failed to spawn `codex exec`")?;
    let stdout = child.stdout.take().context("Missing stdout on codex")?;
    let stderr = child.stderr.take().context("Missing stderr on codex")?;

    let stderr_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        let mut buf = String::new();
        while let Ok(Some(line)) = reader.next_line().await {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&line);
        }
        buf
    });

    let mut lines = BufReader::new(stdout).lines();
    let mut emitted_complete = false;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = tx.send(AgentEvent::TurnCompleted {
                    final_text: None,
                    exit_reason: TurnExitReason::Cancelled,
                });
                return Ok(());
            }
            line_res = lines.next_line() => {
                match line_res {
                    Ok(Some(line)) => {
                        for ev in parse_codex_exec_line(&line) {
                            if matches!(ev, AgentEvent::TurnCompleted { .. }) {
                                emitted_complete = true;
                            }
                            let _ = tx.send(ev);
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx.send(AgentEvent::Error {
                            message: format!("Error reading codex output: {e}"),
                            recoverable: false,
                        });
                        break;
                    }
                }
            }
        }
    }

    let status = child.wait().await?;
    let stderr_text = stderr_handle.await.unwrap_or_default();

    if !status.success() && !emitted_complete {
        let msg = if stderr_text.trim().is_empty() {
            format!("`codex exec` exited with status {status}")
        } else {
            stderr_text.trim().to_string()
        };
        let _ = tx.send(AgentEvent::Error {
            message: msg,
            recoverable: false,
        });
        let _ = tx.send(AgentEvent::TurnCompleted {
            final_text: None,
            exit_reason: TurnExitReason::Failed,
        });
    } else if !emitted_complete {
        let _ = tx.send(AgentEvent::TurnCompleted {
            final_text: None,
            exit_reason: TurnExitReason::Completed,
        });
    }

    Ok(())
}

/// Map a ChatGPT `planType` (from `account/read`) to a human-readable plan name.
pub fn normalize_codex_plan(plan_type: &str) -> Option<String> {
    let key = plan_type.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    let name = match key.as_str() {
        "" | "unknown" => return None,
        "pro_500" | "pro500" => "ChatGPT Pro 500".to_string(),
        "pro_200" | "pro200" => "ChatGPT Pro 200".to_string(),
        "pro_100" | "pro100" | "prolite" | "pro_lite" => "ChatGPT Pro 100".to_string(),
        "pro" => "ChatGPT Pro".to_string(),
        "plus" => "ChatGPT Plus".to_string(),
        "go" => "ChatGPT Go".to_string(),
        "free" => "ChatGPT Free".to_string(),
        "business" | "team" => "ChatGPT Business".to_string(),
        "enterprise" => "ChatGPT Enterprise".to_string(),
        "rosalind" | "academic" => "Rosalind Academic Plan".to_string(),
        "edu" => "ChatGPT Edu".to_string(),
        other => {
            let title: Vec<String> = other
                .split('_')
                .filter(|w| !w.is_empty())
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                })
                .collect();
            format!("ChatGPT {}", title.join(" "))
        }
    };
    Some(name)
}

/// Parse a `codex app-server` `account/read` response into `(method, email, plan)`.
pub fn parse_codex_account_read_response(
    val: &Value,
) -> Option<(String, Option<String>, Option<String>)> {
    let account = val.get("result")?.get("account")?;
    match account.get("type").and_then(Value::as_str)? {
        "chatgpt" => {
            let email = account
                .get("email")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let plan = account
                .get("planType")
                .and_then(Value::as_str)
                .and_then(normalize_codex_plan)
                .or_else(|| Some("ChatGPT Subscription".to_string()));
            Some(("chatgpt".to_string(), email, plan))
        }
        "apiKey" | "api_key" => Some((
            "api_key".to_string(),
            None,
            Some("OpenAI API".to_string()),
        )),
        _ => None,
    }
}

async fn wait_for_rpc_id(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
) -> Option<Value> {
    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(val) = serde_json::from_str::<Value>(line.trim()) {
            if val.get("id").and_then(Value::as_u64) == Some(id) {
                return Some(val);
            }
        }
    }
    None
}

/// Ask `codex app-server` for the signed-in ChatGPT account (email + plan type).
async fn probe_codex_account_rpc(
    bin: &Path,
    api_key_override: Option<&str>,
) -> Option<(String, Option<String>, Option<String>)> {
    let mut cmd = Command::new(bin);
    cmd.arg("app-server")
        .env("PATH", enriched_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(key) = api_key_override.filter(|s| !s.trim().is_empty()) {
        cmd.env("OPENAI_API_KEY", key.trim());
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;

    let result = tokio::time::timeout(std::time::Duration::from_secs(4), async {
        let mut lines = BufReader::new(stdout).lines();
        let init = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "clientInfo": { "name": "umisong-harness", "version": "0.1.0" } }
        });
        stdin.write_all(format!("{init}\n").as_bytes()).await.ok()?;
        stdin.flush().await.ok()?;
        wait_for_rpc_id(&mut lines, 1).await?;
        let initialized = json!({ "jsonrpc": "2.0", "method": "initialized" });
        let read = json!({
            "jsonrpc": "2.0", "id": 2, "method": "account/read",
            "params": { "refreshToken": false }
        });
        stdin
            .write_all(format!("{initialized}\n{read}\n").as_bytes())
            .await
            .ok()?;
        stdin.flush().await.ok()?;
        let resp = wait_for_rpc_id(&mut lines, 2).await?;
        parse_codex_account_read_response(&resp)
    })
    .await
    .ok()
    .flatten();

    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

/// Parse `codex login status` output into `AuthState`.
pub fn parse_codex_login_status(stdout: &str, stderr: &str, exit_ok: bool) -> AuthState {
    let combined = format!("{stdout}\n{stderr}");
    let lower = combined.to_ascii_lowercase();

    if lower.contains("not logged in") || lower.contains("logged out") || !exit_ok {
        return AuthState::LoggedOut;
    }

    let (method, plan_hint) = if lower.contains("chatgpt") {
        ("chatgpt".to_string(), Some("ChatGPT Subscription".to_string()))
    } else if lower.contains("api key") || lower.contains("api_key") {
        ("api_key".to_string(), Some("OpenAI API".to_string()))
    } else {
        ("codex".to_string(), None)
    };

    // Extract email if printed in status text.
    let account_hint = combined
        .split_whitespace()
        .find(|tok| tok.contains('@') && tok.contains('.'))
        .map(|s| s.trim_matches(|c: char| c == '(' || c == ')' || c == ',' || c == '.').to_string());

    AuthState::Authenticated {
        method,
        account_hint,
        plan_hint,
    }
}

pub struct ParsedAppServerLine {
    pub thread_id: Option<String>,
    pub pending_approval_rpc_id: Option<(String, Value)>,
    pub events: Vec<AgentEvent>,
}

/// Parse one JSON-RPC line from `codex app-server`.
pub fn parse_codex_app_server_line(line: &str) -> ParsedAppServerLine {
    let mut out = ParsedAppServerLine {
        thread_id: None,
        pending_approval_rpc_id: None,
        events: Vec::new(),
    };
    let Ok(val) = serde_json::from_str::<Value>(line.trim()) else {
        return out;
    };

    // Response to `thread/start` or `thread/resume`
    if let Some(res) = val.get("result") {
        if let Some(thread) = res.get("thread") {
            let tid = thread
                .get("id")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let model = thread
                .get("model")
                .or_else(|| res.get("model"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            if tid.is_some() {
                out.thread_id = tid.clone();
                out.events.push(AgentEvent::SessionInit {
                    provider_session_id: tid,
                    resolved_model: model,
                });
            }
        } else if let Some(tid) = res.get("threadId").and_then(Value::as_str) {
            out.thread_id = Some(tid.to_string());
            out.events.push(AgentEvent::SessionInit {
                provider_session_id: Some(tid.to_string()),
                resolved_model: res
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            });
        }
    }

    let Some(method) = val.get("method").and_then(Value::as_str) else {
        return out;
    };
    let params = val.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "item/agentMessage/delta" => {
            if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                out.events.push(AgentEvent::TextDelta {
                    text: delta.to_string(),
                });
            }
        }
        "item/reasoning/delta" | "item/reasoning/textDelta" => {
            if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                out.events.push(AgentEvent::ThoughtDelta {
                    text: delta.to_string(),
                });
            }
        }
        "item/started" => {
            if let Some(item) = params.get("item") {
                let kind_str = item.get("type").and_then(Value::as_str).unwrap_or("tool");
                if kind_str != "agentMessage" && kind_str != "reasoning" {
                    let call_id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("codex-item")
                        .to_string();
                    out.events.push(AgentEvent::ToolCallStarted {
                        call_id,
                        kind: ToolKind::classify(kind_str),
                        name: kind_str.to_string(),
                        input_summary: summarize_json_input(item),
                        raw_input: Some(item.to_string()),
                    });
                }
            }
        }
        "item/completed" => {
            if let Some(item) = params.get("item") {
                let kind_str = item.get("type").and_then(Value::as_str).unwrap_or("");
                if kind_str != "agentMessage" && kind_str != "reasoning" && !kind_str.is_empty() {
                    let call_id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("codex-item")
                        .to_string();
                    let exit_code = item.get("exitCode").and_then(Value::as_i64).unwrap_or(0);
                    let output = item
                        .get("aggregatedOutput")
                        .or_else(|| item.get("output"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    out.events.push(AgentEvent::ToolCallCompleted {
                        call_id,
                        success: exit_code == 0,
                        output,
                        duration_ms: None,
                    });
                }
            }
        }
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            if let Some(rpc_id) = val.get("id").cloned() {
                let approval_id = format!("codex-appr-{rpc_id}");
                let tool_name = if method.contains("command") {
                    "commandExecution"
                } else {
                    "fileChange"
                };
                out.pending_approval_rpc_id = Some((approval_id.clone(), rpc_id));
                out.events.push(AgentEvent::ApprovalRequested {
                    approval_id,
                    tool_name: tool_name.to_string(),
                    summary: summarize_json_input(&params),
                    detail: serde_json::to_string_pretty(&params).unwrap_or_default(),
                });
            }
        }
        "turn/completed" => {
            if let Some(usage) = params.get("usage") {
                out.events.push(AgentEvent::UsageUpdate {
                    input_tokens: usage
                        .get("input_tokens")
                        .or_else(|| usage.get("inputTokens"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    output_tokens: usage
                        .get("output_tokens")
                        .or_else(|| usage.get("outputTokens"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cache_read_tokens: usage
                        .get("cached_input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cost_usd: None,
                });
            }
            let status_str = params
                .get("turn")
                .and_then(|t| t.get("status"))
                .or_else(|| params.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("completed");
            let reason = match status_str {
                "interrupted" | "cancelled" => TurnExitReason::Cancelled,
                "failed" => TurnExitReason::Failed,
                _ => TurnExitReason::Completed,
            };
            out.events.push(AgentEvent::TurnCompleted {
                final_text: None,
                exit_reason: reason,
            });
        }
        _ => {}
    }

    out
}

/// Parse one JSONL line from `codex exec --json`.
pub fn parse_codex_exec_line(line: &str) -> Vec<AgentEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
        return vec![AgentEvent::TextDelta {
            text: format!("{trimmed}\n"),
        }];
    };

    let mut out = Vec::new();
    let ev_type = val.get("type").and_then(Value::as_str).unwrap_or("");

    match ev_type {
        "thread.started" => {
            out.push(AgentEvent::SessionInit {
                provider_session_id: val
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                resolved_model: val
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            });
        }
        "item.started" => {
            if let Some(item) = val.get("item") {
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                if item_type == "command_execution" || item_type == "file_change" || item_type == "mcp_tool_call" {
                    let call_id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("codex-call")
                        .to_string();
                    out.push(AgentEvent::ToolCallStarted {
                        call_id,
                        kind: ToolKind::classify(item_type),
                        name: item_type.to_string(),
                        input_summary: summarize_json_input(item),
                        raw_input: Some(item.to_string()),
                    });
                }
            }
        }
        "item.completed" => {
            if let Some(item) = val.get("item") {
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                match item_type {
                    "agent_message" => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            out.push(AgentEvent::TextDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                    "reasoning" => {
                        if let Some(text) = item.get("text").and_then(Value::as_str) {
                            out.push(AgentEvent::ThoughtDelta {
                                text: text.to_string(),
                            });
                        }
                    }
                    "command_execution" | "file_change" | "mcp_tool_call" => {
                        let call_id = item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("codex-call")
                            .to_string();
                        let exit_code = item.get("exit_code").and_then(Value::as_i64).unwrap_or(0);
                        let output = item
                            .get("aggregated_output")
                            .or_else(|| item.get("output"))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        out.push(AgentEvent::ToolCallCompleted {
                            call_id,
                            success: exit_code == 0,
                            output,
                            duration_ms: None,
                        });
                    }
                    _ => {}
                }
            }
        }
        "turn.completed" => {
            if let Some(usage) = val.get("usage") {
                out.push(AgentEvent::UsageUpdate {
                    input_tokens: usage
                        .get("input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    output_tokens: usage
                        .get("output_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cache_read_tokens: usage
                        .get("cached_input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    cost_usd: None,
                });
            }
            out.push(AgentEvent::TurnCompleted {
                final_text: None,
                exit_reason: TurnExitReason::Completed,
            });
        }
        "turn.failed" | "error" => {
            let msg = val
                .get("error")
                .and_then(|e| e.get("message").or(Some(e)))
                .or_else(|| val.get("message"))
                .and_then(Value::as_str)
                .unwrap_or(trimmed)
                .to_string();
            out.push(AgentEvent::Error {
                message: msg,
                recoverable: false,
            });
            if ev_type == "turn.failed" {
                out.push(AgentEvent::TurnCompleted {
                    final_text: None,
                    exit_reason: TurnExitReason::Failed,
                });
            }
        }
        _ => {}
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_login_status_variants() {
        let chatgpt = parse_codex_login_status("Logged in using ChatGPT (dev@openai.test)", "", true);
        assert_eq!(
            chatgpt,
            AuthState::Authenticated {
                method: "chatgpt".into(),
                account_hint: Some("dev@openai.test".into()),
                plan_hint: Some("ChatGPT Subscription".into()),
            }
        );

        let api = parse_codex_login_status("Logged in using API key", "", true);
        assert!(matches!(api, AuthState::Authenticated { method, .. } if method == "api_key"));

        let out = parse_codex_login_status("Not logged in", "", false);
        assert_eq!(out, AuthState::LoggedOut);
    }

    #[test]
    fn normalizes_codex_plans_and_account_read() {
        assert_eq!(normalize_codex_plan("plus").as_deref(), Some("ChatGPT Plus"));
        assert_eq!(normalize_codex_plan("prolite").as_deref(), Some("ChatGPT Pro 100"));
        assert_eq!(normalize_codex_plan("pro_200").as_deref(), Some("ChatGPT Pro 200"));
        assert_eq!(normalize_codex_plan("team").as_deref(), Some("ChatGPT Business"));
        assert_eq!(normalize_codex_plan("go").as_deref(), Some("ChatGPT Go"));
        assert_eq!(normalize_codex_plan("unknown"), None);
        assert_eq!(normalize_codex_plan("super_duper").as_deref(), Some("ChatGPT Super Duper"));

        let v: Value = serde_json::from_str(
            r#"{"id":2,"result":{"account":{"type":"chatgpt","email":"a@b.co","planType":"pro"}}}"#,
        )
        .unwrap();
        assert_eq!(
            parse_codex_account_read_response(&v),
            Some(("chatgpt".into(), Some("a@b.co".into()), Some("ChatGPT Pro".into())))
        );
        let api: Value =
            serde_json::from_str(r#"{"id":2,"result":{"account":{"type":"apiKey"}}}"#).unwrap();
        assert_eq!(
            parse_codex_account_read_response(&api),
            Some(("api_key".into(), None, Some("OpenAI API".into())))
        );
        let none: Value = serde_json::from_str(r#"{"id":2,"result":{"account":null}}"#).unwrap();
        assert_eq!(parse_codex_account_read_response(&none), None);
    }

    #[test]
    fn parses_codex_app_server_and_exec() {
        let started = parse_codex_app_server_line(
            r#"{"jsonrpc":"2.0","id":2,"result":{"thread":{"id":"thr_99","model":"gpt-5.3-codex"}}}"#,
        );
        assert_eq!(started.thread_id.as_deref(), Some("thr_99"));
        assert_eq!(started.events.len(), 1);

        let delta = parse_codex_app_server_line(
            r#"{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"delta":"Patch applied."}}"#,
        );
        assert!(matches!(&delta.events[0], AgentEvent::TextDelta { text } if text == "Patch applied."));

        let appr = parse_codex_app_server_line(
            r#"{"jsonrpc":"2.0","id":7,"method":"item/commandExecution/requestApproval","params":{"command":"cargo test"}}"#,
        );
        assert!(appr.pending_approval_rpc_id.is_some());
        assert!(matches!(&appr.events[0], AgentEvent::ApprovalRequested { .. }));

        let exec_lines = [
            r#"{"type":"thread.started","thread_id":"t_1","model":"gpt-5-codex"}"#,
            r#"{"type":"item.completed","item":{"id":"r1","type":"reasoning","text":"Planning edit"}}"#,
            r#"{"type":"item.started","item":{"id":"c1","type":"command_execution","command":"git status"}}"#,
            r#"{"type":"item.completed","item":{"id":"c1","type":"command_execution","exit_code":0,"aggregated_output":"clean"}}"#,
            r#"{"type":"item.completed","item":{"id":"m1","type":"agent_message","text":"All clean!"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":50,"output_tokens":20,"cached_input_tokens":10}}"#,
        ];
        let mut events = Vec::new();
        for l in exec_lines {
            events.extend(parse_codex_exec_line(l));
        }
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ThoughtDelta { text } if text == "Planning edit")));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolCallCompleted { success: true, .. })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TextDelta { text } if text == "All clean!")));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { exit_reason: TurnExitReason::Completed, .. })));
    }
}
