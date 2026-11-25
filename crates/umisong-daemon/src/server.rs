use crate::pty_auth::PtyAuthManager;
use crate::store::Store;
use crate::{DaemonConfig, git_ops, handoff};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use umisong_core::{
    AgentEvent, ApprovalDecision, CompareColumnState, CompareIsolationMode, CompareRun,
    DaemonNotification, DaemonRequest, DaemonResponse, PermissionMode, ProviderId,
    ProviderInstallState, ProviderSettings, ProviderStatus, RpcEnvelope, ServerMessage,
    ThinkingLevel, ToolCallRecord, TurnExitReason, TurnRecord, TurnStatus,
};
use umisong_providers::{ProviderProbeCache, ProviderRegistry, TurnExecutionRequest};
use uuid::Uuid;

pub struct DaemonState {
    pub config: DaemonConfig,
    pub store: Store,
    pub registry: ProviderRegistry,
    pub pty_auth: PtyAuthManager,
    pub events_tx: broadcast::Sender<DaemonNotification>,
    pub active_turns: Mutex<HashMap<String, CancellationToken>>,
    pub active_compares: Mutex<HashMap<String, CancellationToken>>,
    pub approval_senders: Mutex<HashMap<String, mpsc::UnboundedSender<ApprovalDecision>>>,
    pub model_discovery_inflight: Mutex<HashSet<ProviderId>>,
    pub install_inflight: Mutex<HashSet<ProviderId>>,
    pub install_states: Mutex<HashMap<ProviderId, ProviderInstallState>>,
}

impl DaemonState {
    pub fn new(config: DaemonConfig) -> Result<Arc<Self>> {
        Self::new_with_registry(config, ProviderRegistry::new())
    }

    pub fn new_with_registry(config: DaemonConfig, registry: ProviderRegistry) -> Result<Arc<Self>> {
        let store = Store::open(&config.db_path)?;
        let (events_tx, _) = broadcast::channel(1024);
        Ok(Arc::new(Self {
            config,
            store,
            registry,
            pty_auth: PtyAuthManager::new(),
            events_tx,
            active_turns: Mutex::new(HashMap::new()),
            active_compares: Mutex::new(HashMap::new()),
            approval_senders: Mutex::new(HashMap::new()),
            model_discovery_inflight: Mutex::new(HashSet::new()),
            install_inflight: Mutex::new(HashSet::new()),
            install_states: Mutex::new(HashMap::new()),
        }))
    }

    pub async fn probe_provider(&self, provider: ProviderId) -> Result<ProviderStatus> {
        let (settings, discovered_models) = self.store.get_provider_settings(provider)?;
        let (detected_plan, detected_account) = self.store.get_provider_cached_plan(provider)?;
        let cache = ProviderProbeCache {
            discovered_models,
            detected_plan,
            detected_account,
        };
        let adapter = self.registry.adapter(provider);
        Ok(adapter.probe_status(&settings, &cache).await)
    }

    /// Run slow live model discovery off the request path and broadcast the refreshed status.
    pub fn spawn_background_live_discovery(self: &Arc<Self>, provider: ProviderId) {
        if !self.model_discovery_inflight.lock().unwrap().insert(provider) {
            return;
        }
        let state = Arc::clone(self);
        tokio::spawn(async move {
            let updated = async {
                let (settings, old_models) = state.store.get_provider_settings(provider).ok()?;
                let (old_plan, old_account) = state.store.get_provider_cached_plan(provider).ok()?;
                let info = state
                    .registry
                    .adapter(provider)
                    .discover_live_info(&settings)
                    .await?;
                let models = if info.models.is_empty() {
                    old_models.clone()
                } else {
                    info.models
                };
                let plan = info.plan_hint.or_else(|| old_plan.clone());
                let account = info.account_hint.or_else(|| old_account.clone());
                if models == old_models && plan == old_plan && account == old_account {
                    return None;
                }
                state
                    .store
                    .save_live_discovery(provider, &models, plan.as_deref(), account.as_deref())
                    .ok()?;
                state.probe_provider(provider).await.ok()
            }
            .await;
            if let Some(st) = updated {
                let _ = state
                    .events_tx
                    .send(DaemonNotification::ProviderStatusUpdated(st));
            }
            state
                .model_discovery_inflight
                .lock()
                .unwrap()
                .remove(&provider);
        });
    }

    pub async fn probe_all_providers(&self) -> Result<Vec<ProviderStatus>> {
        let (c, a, x) = tokio::join!(
            self.probe_provider(ProviderId::ClaudeCode),
            self.probe_provider(ProviderId::Antigravity),
            self.probe_provider(ProviderId::Codex),
        );
        Ok(vec![c?, a?, x?])
    }

    pub fn set_and_broadcast_install_state(
        &self,
        provider: ProviderId,
        state: ProviderInstallState,
    ) {
        self.install_states
            .lock()
            .unwrap()
            .insert(provider, state.clone());
        let _ = self
            .events_tx
            .send(DaemonNotification::ProviderInstallUpdated { provider, state });
    }

    pub fn spawn_background_install(self: &Arc<Self>, provider: ProviderId) {
        if !self.install_inflight.lock().unwrap().insert(provider) {
            if let Some(existing) = self.install_states.lock().unwrap().get(&provider).cloned() {
                let _ = self
                    .events_tx
                    .send(DaemonNotification::ProviderInstallUpdated {
                        provider,
                        state: existing,
                    });
            }
            return;
        }

        self.set_and_broadcast_install_state(
            provider,
            ProviderInstallState::Installing {
                method: "auto".into(),
                detail: format!("Installing {} CLI…", provider.display_name()),
            },
        );

        let state = Arc::clone(self);
        tokio::spawn(async move {
            let adapter = state.registry.adapter(provider);
            let progress_state = Arc::clone(&state);
            let res = adapter
                .install_cli(&move |progress| {
                    progress_state.set_and_broadcast_install_state(provider, progress);
                })
                .await;

            match res {
                Ok(_) => {
                    state.set_and_broadcast_install_state(
                        provider,
                        ProviderInstallState::Installed,
                    );
                    if let Ok(st) = state.probe_provider(provider).await {
                        if st.installed && st.auth.is_ready() {
                            state.spawn_background_live_discovery(provider);
                        }
                        let _ = state
                            .events_tx
                            .send(DaemonNotification::ProviderStatusUpdated(st));
                    }
                }
                Err(err) => {
                    state.set_and_broadcast_install_state(
                        provider,
                        ProviderInstallState::Failed {
                            message: err.to_string(),
                        },
                    );
                }
            }

            state.install_inflight.lock().unwrap().remove(&provider);
        });
    }
}

pub async fn serve(config: DaemonConfig) -> Result<()> {
    if let Some(parent) = config.socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Remove stale socket file if no live daemon responds on it.
    if config.socket_path.exists() {
        match UnixStream::connect(&config.socket_path).await {
            Ok(_) => {
                bail!(
                    "Another umisongd instance is already listening on {}",
                    config.socket_path.display()
                );
            }
            Err(_) => {
                let _ = std::fs::remove_file(&config.socket_path);
            }
        }
    }

    let listener = UnixListener::bind(&config.socket_path).with_context(|| {
        format!(
            "Failed to bind Unix socket at {}",
            config.socket_path.display()
        )
    })?;

    let state = DaemonState::new(config)?;
    serve_with_listener(listener, state).await
}

pub async fn serve_with_listener(listener: UnixListener, state: Arc<DaemonState>) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state_clone = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(err) = handle_client(stream, state_clone).await {
                tracing::debug!("Client disconnected: {err}");
            }
        });
    }
}

pub async fn handle_client(stream: UnixStream, state: Arc<DaemonState>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Forward daemon notifications to this client.
    let mut notif_rx = state.events_tx.subscribe();
    let notif_tx = out_tx.clone();
    let notif_task = tokio::spawn(async move {
        loop {
            match notif_rx.recv().await {
                Ok(notif) => {
                    if notif_tx.send(ServerMessage::Notification(notif)).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Dedicated writer task to serialize outgoing messages cleanly.
    let write_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            match serde_json::to_string(&msg) {
                Ok(json) => {
                    if writer.write_all(json.as_bytes()).await.is_err() {
                        break;
                    }
                    if writer.write_all(b"\n").await.is_err() {
                        break;
                    }
                    if writer.flush().await.is_err() {
                        break;
                    }
                }
                Err(err) => {
                    tracing::error!("Failed to serialize ServerMessage: {err}");
                }
            }
        }
    });

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let env: RpcEnvelope = match serde_json::from_str(trimmed) {
            Ok(e) => e,
            Err(e) => {
                let _ = out_tx.send(ServerMessage::Response {
                    id: 0,
                    result: Err(format!("Malformed RPC request: {e}")),
                });
                continue;
            }
        };

        let state_req = Arc::clone(&state);
        let resp_tx = out_tx.clone();
        tokio::spawn(async move {
            let result = dispatch_request(state_req, env.request)
                .await
                .map_err(|e| e.to_string());
            let _ = resp_tx.send(ServerMessage::Response {
                id: env.id,
                result,
            });
        });
    }

    notif_task.abort();
    drop(out_tx);
    let _ = write_task.await;
    Ok(())
}

async fn dispatch_request(
    state: Arc<DaemonState>,
    request: DaemonRequest,
) -> Result<DaemonResponse> {
    match request {
        DaemonRequest::Ping => Ok(DaemonResponse::Pong {
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),

        DaemonRequest::GetSettings => Ok(DaemonResponse::Settings(state.store.get_app_settings()?)),

        DaemonRequest::UpdateSettings { settings } => {
            state.store.save_app_settings(&settings)?;
            for pid in ProviderId::ALL {
                if let Ok(st) = state.probe_provider(pid).await {
                    let _ = state
                        .events_tx
                        .send(DaemonNotification::ProviderStatusUpdated(st));
                }
            }
            Ok(DaemonResponse::Settings(settings))
        }

        DaemonRequest::ListProviders => {
            let list = state.probe_all_providers().await?;
            for st in &list {
                if st.installed && st.auth.is_ready() {
                    state.spawn_background_live_discovery(st.provider);
                }
            }
            let cached_installs: Vec<(ProviderId, ProviderInstallState)> = {
                let guard = state.install_states.lock().unwrap();
                guard
                    .iter()
                    .filter(|(_, s)| {
                        matches!(
                            s,
                            ProviderInstallState::Installing { .. }
                                | ProviderInstallState::Failed { .. }
                        )
                    })
                    .map(|(p, s)| (*p, s.clone()))
                    .collect()
            };
            for (provider, install_state) in cached_installs {
                let _ = state
                    .events_tx
                    .send(DaemonNotification::ProviderInstallUpdated {
                        provider,
                        state: install_state,
                    });
            }
            Ok(DaemonResponse::Providers(list))
        }

        DaemonRequest::RefreshProviderStatus { provider } => {
            let st = state.probe_provider(provider).await?;
            if st.installed && st.auth.is_ready() {
                state.spawn_background_live_discovery(provider);
            }
            let _ = state
                .events_tx
                .send(DaemonNotification::ProviderStatusUpdated(st.clone()));
            Ok(DaemonResponse::Provider(st))
        }

        DaemonRequest::InstallProvider { provider } => {
            let st = state.probe_provider(provider).await?;
            if st.installed {
                state.set_and_broadcast_install_state(provider, ProviderInstallState::Installed);
                let _ = state
                    .events_tx
                    .send(DaemonNotification::ProviderStatusUpdated(st));
                return Ok(DaemonResponse::InstallStarted {
                    providers: Vec::new(),
                });
            }
            state.spawn_background_install(provider);
            Ok(DaemonResponse::InstallStarted {
                providers: vec![provider],
            })
        }

        DaemonRequest::InstallMissingProviders => {
            let list = state.probe_all_providers().await?;
            let mut started = Vec::new();
            for st in list {
                if !st.installed && st.settings.binary_override.is_none() {
                    state.spawn_background_install(st.provider);
                    started.push(st.provider);
                }
            }
            Ok(DaemonResponse::InstallStarted { providers: started })
        }

        DaemonRequest::UpdateProviderConfig {
            provider,
            binary_override,
            default_model,
            custom_models,
            api_key_override,
            plan_tier_override,
        } => {
            let new_settings = ProviderSettings {
                provider,
                binary_override: binary_override.filter(|s| !s.trim().is_empty()),
                default_model: default_model.trim().to_string(),
                custom_models,
                api_key_override: api_key_override.filter(|s| !s.trim().is_empty()),
                plan_tier_override,
            };
            state.store.save_provider_settings(&new_settings)?;
            let st = state.probe_provider(provider).await?;
            let _ = state
                .events_tx
                .send(DaemonNotification::ProviderStatusUpdated(st.clone()));
            Ok(DaemonResponse::Provider(st))
        }

        DaemonRequest::StartAuthTerminal {
            provider,
            mode,
            cols,
            rows,
        } => {
            let (settings, _) = state.store.get_provider_settings(provider)?;
            let adapter = state.registry.adapter(provider);
            let spec = adapter.login_command(&settings, mode)?;
            let display_cmd = spec.display_command();

            let state_exit = Arc::clone(&state);
            let handle = tokio::runtime::Handle::current();
            let auth_session_id = state.pty_auth.start_session(
                provider,
                spec,
                cols,
                rows,
                state.events_tx.clone(),
                move |pid| {
                    handle.spawn(async move {
                        if let Ok(st) = state_exit.probe_provider(pid).await {
                            if st.auth.is_ready() {
                                state_exit.spawn_background_live_discovery(pid);
                            }
                            let _ = state_exit
                                .events_tx
                                .send(DaemonNotification::ProviderStatusUpdated(st));
                        }
                    });
                },
            )?;
            Ok(DaemonResponse::AuthTerminalStarted {
                auth_session_id,
                command: display_cmd,
            })
        }

        DaemonRequest::SendAuthInput {
            auth_session_id,
            data,
        } => {
            state.pty_auth.send_input(&auth_session_id, &data)?;
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::ResizeAuthTerminal {
            auth_session_id,
            cols,
            rows,
        } => {
            state.pty_auth.resize(&auth_session_id, cols, rows)?;
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::StopAuthTerminal { auth_session_id } => {
            state.pty_auth.stop(&auth_session_id);
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::LogoutProvider { provider } => {
            let (settings, _) = state.store.get_provider_settings(provider)?;
            let adapter = state.registry.adapter(provider);
            adapter.logout(&settings).await?;
            let _ = state.store.clear_live_discovery(provider);
            let st = state.probe_provider(provider).await?;
            let _ = state
                .events_tx
                .send(DaemonNotification::ProviderStatusUpdated(st.clone()));
            Ok(DaemonResponse::Provider(st))
        }

        DaemonRequest::ListWorkspaces => Ok(DaemonResponse::Workspaces(
            state.store.list_workspaces()?,
        )),

        DaemonRequest::OpenWorkspace { path } => {
            let p = PathBuf::from(&path);
            if !p.exists() || !p.is_dir() {
                bail!("Workspace directory does not exist: {path}");
            }
            let ws = state.store.open_workspace(&p)?;
            Ok(DaemonResponse::Workspace(ws))
        }

        DaemonRequest::ListSessions { workspace_id } => Ok(DaemonResponse::Sessions(
            state.store.list_sessions(&workspace_id)?,
        )),

        DaemonRequest::CreateSession {
            workspace_id,
            title,
            provider,
            model,
            permission_mode,
        } => {
            let sess = state.store.create_session(
                &workspace_id,
                &title,
                provider,
                &model,
                permission_mode,
            )?;
            Ok(DaemonResponse::Session(sess))
        }

        DaemonRequest::GetSession { session_id } => {
            let sess = state
                .store
                .get_session(&session_id)?
                .context("Session not found")?;
            Ok(DaemonResponse::Session(sess))
        }

        DaemonRequest::DeleteSession { session_id } => {
            state.store.delete_session(&session_id)?;
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::UpdateSessionMeta {
            session_id,
            title,
            pinned,
            unread,
            archived,
        } => {
            state.store.update_session_meta(
                &session_id,
                title.as_deref(),
                pinned,
                unread,
                archived,
            )?;
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::UpdateSessionConfig {
            session_id,
            provider,
            model,
            permission_mode,
        } => {
            state
                .store
                .update_session_config(&session_id, provider, &model, permission_mode)?;
            let sess = state
                .store
                .get_session(&session_id)?
                .context("Session not found")?;
            Ok(DaemonResponse::Session(sess))
        }

        DaemonRequest::SendTurn {
            session_id,
            provider,
            model,
            thinking_level,
            permission_mode,
            prompt,
        } => {
            let turn_id = launch_turn(
                state,
                session_id.clone(),
                provider,
                model,
                thinking_level,
                permission_mode,
                prompt,
                None,
                None,
            )
            .await?;
            Ok(DaemonResponse::TurnStarted {
                session_id,
                turn_id,
            })
        }

        DaemonRequest::CancelTurn { turn_id } => {
            if let Some(tok) = state.active_turns.lock().unwrap().remove(&turn_id) {
                tok.cancel();
            }
            if let Ok(Some((session_id, provider))) = state.store.cancel_turn_in_db(&turn_id) {
                let _ = state.events_tx.send(DaemonNotification::TurnEvent {
                    session_id,
                    turn_id,
                    provider,
                    event: AgentEvent::TurnCompleted {
                        final_text: None,
                        exit_reason: TurnExitReason::Cancelled,
                    },
                });
            }
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::RespondApproval {
            approval_id,
            approved,
        } => {
            let sender = state.approval_senders.lock().unwrap().remove(&approval_id);
            if let Some(tx) = sender {
                let _ = tx.send(ApprovalDecision {
                    approval_id,
                    approved,
                });
            }
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::HandoffSession {
            source_session_id,
            up_to_turn_id,
            target_provider,
            target_model,
            thinking_level,
            instruction,
            create_new_session,
        } => {
            let source = state
                .store
                .get_session(&source_session_id)?
                .context("Source session not found")?;
            let ws = state
                .store
                .get_workspace(&source.workspace_id)?
                .context("Workspace not found")?;

            let (handoff_prompt, from_provider, from_turn_id) = handoff::build_handoff_prompt(
                &source,
                Path::new(&ws.path),
                up_to_turn_id.as_deref(),
                target_provider,
                &instruction,
            );

            let target_session_id = if create_new_session {
                let title = format!(
                    "{} -> {}",
                    source.title,
                    target_provider.short_label()
                );
                let new_sess = state.store.create_session(
                    &source.workspace_id,
                    &title,
                    target_provider,
                    &target_model,
                    source.permission_mode,
                )?;
                new_sess.id
            } else {
                source.id.clone()
            };

            let turn_id = launch_turn(
                state,
                target_session_id.clone(),
                target_provider,
                target_model,
                thinking_level,
                source.permission_mode,
                handoff_prompt,
                from_provider,
                from_turn_id,
            )
            .await?;

            Ok(DaemonResponse::HandoffStarted {
                session_id: target_session_id,
                turn_id,
            })
        }

        DaemonRequest::ListCompareRuns { workspace_id } => Ok(DaemonResponse::CompareRuns(
            state.store.list_compare_runs(&workspace_id)?,
        )),

        DaemonRequest::StartCompare {
            workspace_id,
            session_id,
            prompt,
            targets,
            isolation_mode,
        } => {
            if targets.is_empty() {
                bail!("Select at least one provider for Parallel Compare");
            }
            let ws = state
                .store
                .get_workspace(&workspace_id)?
                .context("Workspace not found")?;
            let ws_path = PathBuf::from(&ws.path);

            let effective_isolation =
                if matches!(isolation_mode, CompareIsolationMode::GitWorktree)
                    && !git_ops::is_git_repo(&ws_path)
                {
                    CompareIsolationMode::ReadOnly
                } else {
                    isolation_mode
                };

            let compare_id = Uuid::new_v4().to_string();
            let mut columns = Vec::with_capacity(targets.len());
            let mut worktree_paths: HashMap<ProviderId, PathBuf> = HashMap::new();

            for target in &targets {
                let wt = if matches!(effective_isolation, CompareIsolationMode::GitWorktree) {
                    let created = git_ops::create_compare_worktree(
                        &ws_path,
                        &state.config.worktrees_dir,
                        &compare_id,
                        target.provider,
                    )?;
                    worktree_paths.insert(target.provider, created.clone());
                    Some(created.to_string_lossy().into_owned())
                } else {
                    None
                };

                columns.push(CompareColumnState {
                    provider: target.provider,
                    model: target.model.clone(),
                    resolved_model: None,
                    status: TurnStatus::Running,
                    thought_text: String::new(),
                    response_text: String::new(),
                    tool_calls: Vec::new(),
                    error_message: None,
                    input_tokens: 0,
                    output_tokens: 0,
                    cost_usd: None,
                    duration_ms: None,
                    worktree_path: wt,
                    diff_patch: None,
                    diffs: Vec::new(),
                    adopted: false,
                });
            }

            let run = CompareRun {
                id: compare_id.clone(),
                workspace_id: workspace_id.clone(),
                session_id,
                prompt: prompt.clone(),
                isolation_mode: effective_isolation,
                columns: columns.clone(),
                created_at: Utc::now(),
            };
            state.store.insert_compare_run(&run)?;

            let cancel = CancellationToken::new();
            state
                .active_compares
                .lock()
                .unwrap()
                .insert(compare_id.clone(), cancel.clone());

            let target_levels: HashMap<ProviderId, ThinkingLevel> = targets
                .iter()
                .map(|t| (t.provider, t.thinking_level))
                .collect();

            for col in columns {
                let st = Arc::clone(&state);
                let cmp_id = compare_id.clone();
                let prompt_c = prompt.clone();
                let run_dir = worktree_paths
                    .get(&col.provider)
                    .cloned()
                    .unwrap_or_else(|| ws_path.clone());
                let cancel_col = cancel.clone();
                let thinking_level = target_levels
                    .get(&col.provider)
                    .copied()
                    .unwrap_or_default();
                let perm = match effective_isolation {
                    CompareIsolationMode::ReadOnly => PermissionMode::PlanReadOnly,
                    CompareIsolationMode::GitWorktree => PermissionMode::FullAuto,
                };

                tokio::spawn(async move {
                    run_compare_column(
                        st,
                        cmp_id,
                        col,
                        run_dir,
                        prompt_c,
                        thinking_level,
                        perm,
                        effective_isolation,
                        cancel_col,
                    )
                    .await;
                });
            }

            Ok(DaemonResponse::CompareStarted(run))
        }

        DaemonRequest::CancelCompare { compare_id } => {
            if let Some(tok) = state.active_compares.lock().unwrap().get(&compare_id) {
                tok.cancel();
            }
            Ok(DaemonResponse::Ack)
        }

        DaemonRequest::AdoptCompareColumn {
            compare_id,
            provider,
            apply_patch,
        } => {
            let run = state
                .store
                .get_compare_run(&compare_id)?
                .context("Compare run not found")?;
            let ws = state
                .store
                .get_workspace(&run.workspace_id)?
                .context("Workspace not found")?;
            let ws_path = PathBuf::from(&ws.path);

            let mut target_col = run
                .columns
                .iter()
                .find(|c| c.provider == provider)
                .cloned()
                .context("Compare column not found")?;

            let mut patch_applied = false;
            if apply_patch {
                if let Some(patch) = target_col.diff_patch.as_deref() {
                    if !patch.trim().is_empty() {
                        git_ops::apply_patch_to_workspace(&ws_path, patch)?;
                        patch_applied = true;
                        let _ = state.events_tx.send(DaemonNotification::WorkspaceDiffChanged {
                            workspace_path: ws.path.clone(),
                        });
                    }
                }
            }

            // Mark adopted and cleanup temporary compare worktrees.
            target_col.adopted = true;
            state.store.update_compare_column(&compare_id, &target_col)?;
            for col in &run.columns {
                if let Some(wt) = &col.worktree_path {
                    git_ops::remove_worktree(&ws_path, Path::new(wt));
                }
            }

            // Promote the winning prompt + response into the linked (or new) session.
            let target_session_id = if let Some(sid) = run.session_id.clone() {
                sid
            } else {
                let title: String = run.prompt.lines().next().unwrap_or("Adopted Compare").chars().take(50).collect();
                let created = state.store.create_session(
                    &run.workspace_id,
                    &title,
                    provider,
                    &target_col.model,
                    PermissionMode::AskSafe,
                )?;
                created.id
            };

            let mut turn = state.store.create_turn(
                &target_session_id,
                provider,
                &target_col.model,
                &run.prompt,
                None,
                None,
            )?;
            turn.resolved_model = target_col.resolved_model.clone();
            turn.thought_text = target_col.thought_text.clone();
            turn.response_text = target_col.response_text.clone();
            turn.tool_calls = target_col.tool_calls.clone();
            turn.input_tokens = target_col.input_tokens;
            turn.output_tokens = target_col.output_tokens;
            turn.cost_usd = target_col.cost_usd;
            turn.duration_ms = target_col.duration_ms;
            turn.status = TurnStatus::Completed;
            state.store.finish_turn(&turn)?;

            let msg = if patch_applied {
                format!(
                    "Adopted {} response and applied its worktree patch to workspace.",
                    provider.display_name()
                )
            } else {
                format!(
                    "Adopted {} response into active session.",
                    provider.display_name()
                )
            };

            Ok(DaemonResponse::CompareAdopted {
                session_id: target_session_id,
                patch_applied,
                message: msg,
            })
        }

        DaemonRequest::GetGitDiff {
            workspace_path,
            scope,
        } => Ok(DaemonResponse::GitDiff(git_ops::get_git_diff(
            Path::new(&workspace_path),
            scope,
        ))),

        DaemonRequest::RevertFile {
            workspace_path,
            file_path,
        } => {
            git_ops::revert_file(Path::new(&workspace_path), &file_path)?;
            let _ = state.events_tx.send(DaemonNotification::WorkspaceDiffChanged {
                workspace_path,
            });
            Ok(DaemonResponse::Ack)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn launch_turn(
    state: Arc<DaemonState>,
    session_id: String,
    provider: ProviderId,
    model: String,
    thinking_level: ThinkingLevel,
    permission_mode: PermissionMode,
    prompt: String,
    handoff_from_provider: Option<ProviderId>,
    handoff_from_turn_id: Option<String>,
) -> Result<String> {
    let session = state
        .store
        .get_session(&session_id)?
        .context("Session not found")?;
    let ws = state
        .store
        .get_workspace(&session.workspace_id)?
        .context("Workspace not found")?;
    let (prov_settings, _) = state.store.get_provider_settings(provider)?;
    let adapter = state.registry.adapter(provider);

    state
        .store
        .update_session_config(&session_id, provider, &model, permission_mode)?;
    state
        .store
        .update_session_title_if_default(&session_id, &prompt)?;

    let turn = state.store.create_turn(
        &session_id,
        provider,
        &model,
        &prompt,
        handoff_from_provider,
        handoff_from_turn_id.as_deref(),
    )?;
    let turn_id = turn.id.clone();

    let cancel = CancellationToken::new();
    state
        .active_turns
        .lock()
        .unwrap()
        .insert(turn_id.clone(), cancel.clone());

    let provider_session_id = if handoff_from_turn_id.is_some() {
        None
    } else {
        session.provider_session_ids.get(&provider).cloned()
    };

    let exec_req = TurnExecutionRequest {
        workspace_path: PathBuf::from(&ws.path),
        prompt,
        model,
        thinking_level,
        permission_mode,
        provider_session_id,
        settings: prov_settings,
    };

    let state_bg = Arc::clone(&state);
    let ws_path_str = ws.path.clone();
    tokio::spawn(async move {
        let start = Instant::now();
        let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (appr_tx, appr_rx) = mpsc::unbounded_channel::<ApprovalDecision>();
        let cancel_loop = cancel.clone();

        let run_handle = tokio::spawn(async move {
            adapter.run_turn(exec_req, ev_tx, appr_rx, cancel).await
        });

        let mut turn_acc: TurnRecord = turn;
        let mut deferred_completed: Option<AgentEvent> = None;

        loop {
            let maybe_ev = tokio::select! {
                _ = cancel_loop.cancelled() => {
                    turn_acc.status = TurnStatus::Cancelled;
                    deferred_completed = Some(AgentEvent::TurnCompleted {
                        final_text: None,
                        exit_reason: TurnExitReason::Cancelled,
                    });
                    run_handle.abort();
                    break;
                }
                ev = ev_rx.recv() => ev,
            };
            let Some(ev) = maybe_ev else {
                break;
            };
            if let AgentEvent::ApprovalRequested {
                ref approval_id, ..
            } = ev
            {
                state_bg
                    .approval_senders
                    .lock()
                    .unwrap()
                    .insert(approval_id.clone(), appr_tx.clone());
            }

            apply_event_to_turn(&state_bg.store, &mut turn_acc, &ev);

            // Hold TurnCompleted until AFTER `finish_turn` commits to SQLite so a GUI
            // `GetSession` triggered by TurnCompleted can NEVER read stale `running` state.
            if matches!(ev, AgentEvent::TurnCompleted { .. }) {
                deferred_completed = Some(ev);
                continue;
            }

            let _ = state_bg.events_tx.send(DaemonNotification::TurnEvent {
                session_id: turn_acc.session_id.clone(),
                turn_id: turn_acc.id.clone(),
                provider,
                event: ev,
            });
        }

        if let Ok(Err(err)) = run_handle.await {
            let msg = err.to_string();
            turn_acc.status = TurnStatus::Failed;
            turn_acc.error_message = Some(msg.clone());
            let _ = state_bg.events_tx.send(DaemonNotification::TurnEvent {
                session_id: turn_acc.session_id.clone(),
                turn_id: turn_acc.id.clone(),
                provider,
                event: AgentEvent::Error {
                    message: msg,
                    recoverable: false,
                },
            });
            deferred_completed = Some(AgentEvent::TurnCompleted {
                final_text: None,
                exit_reason: TurnExitReason::Failed,
            });
        }

        if turn_acc.status == TurnStatus::Running {
            turn_acc.status = TurnStatus::Completed;
        }
        turn_acc.duration_ms = Some(start.elapsed().as_millis() as u64);
        let _ = state_bg.store.finish_turn(&turn_acc);

        let final_ev = deferred_completed.unwrap_or(AgentEvent::TurnCompleted {
            final_text: (!turn_acc.response_text.is_empty()).then(|| turn_acc.response_text.clone()),
            exit_reason: match turn_acc.status {
                TurnStatus::Cancelled => TurnExitReason::Cancelled,
                TurnStatus::Failed => TurnExitReason::Failed,
                _ => TurnExitReason::Completed,
            },
        });
        let _ = state_bg.events_tx.send(DaemonNotification::TurnEvent {
            session_id: turn_acc.session_id.clone(),
            turn_id: turn_acc.id.clone(),
            provider,
            event: final_ev,
        });

        state_bg
            .active_turns
            .lock()
            .unwrap()
            .remove(&turn_acc.id);
        let _ = state_bg
            .events_tx
            .send(DaemonNotification::WorkspaceDiffChanged {
                workspace_path: ws_path_str,
            });
    });

    Ok(turn_id)
}

#[allow(clippy::too_many_arguments)]
async fn run_compare_column(
    state: Arc<DaemonState>,
    compare_id: String,
    mut col: CompareColumnState,
    run_dir: PathBuf,
    prompt: String,
    thinking_level: ThinkingLevel,
    permission_mode: PermissionMode,
    isolation_mode: CompareIsolationMode,
    cancel: CancellationToken,
) {
    let start = Instant::now();
    let Ok((prov_settings, _)) = state.store.get_provider_settings(col.provider) else {
        return;
    };

    let adapter = state.registry.adapter(col.provider);

    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let (_appr_tx, appr_rx) = mpsc::unbounded_channel::<ApprovalDecision>();

    let req = TurnExecutionRequest {
        workspace_path: run_dir.clone(),
        prompt,
        model: col.model.clone(),
        thinking_level,
        permission_mode,
        provider_session_id: None,
        settings: prov_settings,
    };

    let run_handle = tokio::spawn(async move {
        adapter.run_turn(req, ev_tx, appr_rx, cancel).await
    });

    while let Some(ev) = ev_rx.recv().await {
        apply_event_to_compare_column(&state.store, &mut col, &ev);
        let _ = state.events_tx.send(DaemonNotification::CompareEvent {
            compare_id: compare_id.clone(),
            provider: col.provider,
            event: ev,
        });
    }

    if let Ok(Err(err)) = run_handle.await {
        let msg = err.to_string();
        col.status = TurnStatus::Failed;
        col.error_message = Some(msg.clone());
        let _ = state.events_tx.send(DaemonNotification::CompareEvent {
            compare_id: compare_id.clone(),
            provider: col.provider,
            event: AgentEvent::Error {
                message: msg,
                recoverable: false,
            },
        });
        let _ = state.events_tx.send(DaemonNotification::CompareEvent {
            compare_id: compare_id.clone(),
            provider: col.provider,
            event: AgentEvent::TurnCompleted {
                final_text: None,
                exit_reason: TurnExitReason::Failed,
            },
        });
    }

    if col.status == TurnStatus::Running {
        col.status = TurnStatus::Completed;
    }
    col.duration_ms = Some(start.elapsed().as_millis() as u64);

    if matches!(isolation_mode, CompareIsolationMode::GitWorktree) {
        let (patch, diffs) = git_ops::capture_worktree_patch(&run_dir);
        if !patch.trim().is_empty() {
            col.diff_patch = Some(patch);
            col.diffs = diffs;
        }
    }

    let _ = state.store.update_compare_column(&compare_id, &col);
    let _ = state
        .events_tx
        .send(DaemonNotification::CompareColumnFinished {
            compare_id,
            provider: col.provider,
            diff_patch: col.diff_patch.clone(),
            diffs: col.diffs.clone(),
        });
}

fn apply_event_to_turn(store: &Store, turn: &mut TurnRecord, ev: &AgentEvent) {
    match ev {
        AgentEvent::SessionInit {
            provider_session_id,
            resolved_model,
        } => {
            if let Some(psid) = provider_session_id {
                let _ = store.save_provider_session_id(&turn.session_id, turn.provider, psid);
            }
            if let Some(m) = resolved_model {
                turn.resolved_model = Some(m.clone());
                let _ = store.record_discovered_model(turn.provider, m);
            }
        }
        AgentEvent::ThoughtDelta { text } => turn.thought_text.push_str(text),
        AgentEvent::TextDelta { text } => turn.response_text.push_str(text),
        AgentEvent::ToolCallStarted {
            call_id,
            name,
            kind,
            input_summary,
            ..
        } => {
            if let Some(existing) = turn.tool_calls.iter_mut().find(|c| c.call_id == *call_id) {
                existing.input_summary = input_summary.clone();
            } else {
                turn.tool_calls.push(ToolCallRecord {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    kind: *kind,
                    input_summary: input_summary.clone(),
                    output_text: String::new(),
                    status: "running".to_string(),
                    success: None,
                    duration_ms: None,
                });
            }
        }
        AgentEvent::ToolCallUpdated {
            call_id,
            status,
            output_chunk,
        } => {
            if let Some(existing) = turn.tool_calls.iter_mut().find(|c| c.call_id == *call_id) {
                existing.status = status.clone();
                if let Some(chunk) = output_chunk {
                    existing.output_text.push_str(chunk);
                }
            }
        }
        AgentEvent::ToolCallCompleted {
            call_id,
            success,
            output,
            duration_ms,
        } => {
            if let Some(existing) = turn.tool_calls.iter_mut().find(|c| c.call_id == *call_id) {
                existing.success = Some(*success);
                existing.status = if *success { "completed" } else { "failed" }.to_string();
                if !output.is_empty() {
                    existing.output_text = output.clone();
                }
                existing.duration_ms = *duration_ms;
            }
        }
        AgentEvent::UsageUpdate {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cost_usd,
        } => {
            turn.input_tokens = (*input_tokens).max(turn.input_tokens);
            turn.output_tokens = (*output_tokens).max(turn.output_tokens);
            turn.cache_read_tokens = (*cache_read_tokens).max(turn.cache_read_tokens);
            if cost_usd.is_some() {
                turn.cost_usd = *cost_usd;
            }
        }
        AgentEvent::Error { message, .. } => {
            turn.error_message = Some(message.clone());
        }
        AgentEvent::TurnCompleted {
            final_text,
            exit_reason,
        } => {
            if turn.response_text.is_empty() {
                if let Some(ft) = final_text {
                    turn.response_text = ft.clone();
                }
            }
            turn.status = match exit_reason {
                TurnExitReason::Completed => TurnStatus::Completed,
                TurnExitReason::Cancelled => TurnStatus::Cancelled,
                TurnExitReason::Failed => TurnStatus::Failed,
            };
        }
        AgentEvent::ApprovalRequested { .. } => {}
    }
}

fn apply_event_to_compare_column(store: &Store, col: &mut CompareColumnState, ev: &AgentEvent) {
    match ev {
        AgentEvent::SessionInit {
            resolved_model: Some(m),
            ..
        } => {
            col.resolved_model = Some(m.clone());
            let _ = store.record_discovered_model(col.provider, m);
        }
        AgentEvent::ThoughtDelta { text } => col.thought_text.push_str(text),
        AgentEvent::TextDelta { text } => col.response_text.push_str(text),
        AgentEvent::ToolCallStarted {
            call_id,
            name,
            kind,
            input_summary,
            ..
        } => {
            col.tool_calls.push(ToolCallRecord {
                call_id: call_id.clone(),
                name: name.clone(),
                kind: *kind,
                input_summary: input_summary.clone(),
                output_text: String::new(),
                status: "running".to_string(),
                success: None,
                duration_ms: None,
            });
        }
        AgentEvent::ToolCallCompleted {
            call_id,
            success,
            output,
            duration_ms,
        } => {
            if let Some(existing) = col.tool_calls.iter_mut().find(|c| c.call_id == *call_id) {
                existing.success = Some(*success);
                existing.status = if *success { "completed" } else { "failed" }.to_string();
                existing.output_text = output.clone();
                existing.duration_ms = *duration_ms;
            }
        }
        AgentEvent::UsageUpdate {
            input_tokens,
            output_tokens,
            cost_usd,
            ..
        } => {
            col.input_tokens = (*input_tokens).max(col.input_tokens);
            col.output_tokens = (*output_tokens).max(col.output_tokens);
            if cost_usd.is_some() {
                col.cost_usd = *cost_usd;
            }
        }
        AgentEvent::Error { message, .. } => {
            col.error_message = Some(message.clone());
        }
        AgentEvent::TurnCompleted {
            final_text,
            exit_reason,
        } => {
            if col.response_text.is_empty() {
                if let Some(ft) = final_text {
                    col.response_text = ft.clone();
                }
            }
            col.status = match exit_reason {
                TurnExitReason::Completed => TurnStatus::Completed,
                TurnExitReason::Cancelled => TurnStatus::Cancelled,
                TurnExitReason::Failed => TurnStatus::Failed,
            };
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use umisong_core::{AuthLoginMode, AuthState, CompareTarget, PlanTier, ToolKind};
    use umisong_providers::{CommandSpec, ProviderAdapter};

    struct TestMockAdapter(ProviderId);

    #[async_trait]
    impl ProviderAdapter for TestMockAdapter {
        fn id(&self) -> ProviderId {
            self.0
        }

        async fn probe_status(
            &self,
            settings: &ProviderSettings,
            _cache: &umisong_providers::ProviderProbeCache,
        ) -> ProviderStatus {
            ProviderStatus {
                provider: self.0,
                installed: true,
                binary_path: Some("/usr/bin/mock".into()),
                version: Some("test-1.0".into()),
                auth: AuthState::Authenticated {
                    method: "test".into(),
                    account_hint: None,
                    plan_hint: Some("Test".into()),
                },
                effective_plan_tier: PlanTier::AutoDetect,
                models: Vec::new(),
                active_default_model: settings.default_model.clone(),
                settings: settings.clone(),
                notes: None,
            }
        }

        fn login_command(
            &self,
            _settings: &ProviderSettings,
            _mode: AuthLoginMode,
        ) -> Result<CommandSpec> {
            Ok(CommandSpec {
                program: "/bin/echo".into(),
                args: vec!["ok".into()],
                env: HashMap::new(),
                cwd: None,
            })
        }

        async fn logout(&self, _settings: &ProviderSettings) -> Result<()> {
            Ok(())
        }

        async fn run_turn(
            &self,
            req: TurnExecutionRequest,
            tx: mpsc::UnboundedSender<AgentEvent>,
            _approval_rx: mpsc::UnboundedReceiver<ApprovalDecision>,
            _cancel: CancellationToken,
        ) -> Result<()> {
            let _ = tx.send(AgentEvent::ThoughtDelta {
                text: "analyzing...".into(),
            });
            let _ = tx.send(AgentEvent::ToolCallStarted {
                call_id: "tc-1".into(),
                name: "read_file".into(),
                kind: ToolKind::FileRead,
                input_summary: "main.rs".into(),
                raw_input: None,
            });
            let _ = tx.send(AgentEvent::ToolCallCompleted {
                call_id: "tc-1".into(),
                success: true,
                output: "fn main() {}".into(),
                duration_ms: Some(10),
            });
            let reply = format!("Reply from {} for: {}", self.0.display_name(), req.prompt);
            let _ = tx.send(AgentEvent::TextDelta {
                text: reply.clone(),
            });
            let _ = tx.send(AgentEvent::TurnCompleted {
                final_text: Some(reply),
                exit_reason: TurnExitReason::Completed,
            });
            Ok(())
        }
    }

    fn test_registry() -> ProviderRegistry {
        let mut map: HashMap<ProviderId, Arc<dyn ProviderAdapter>> = HashMap::new();
        for pid in ProviderId::ALL {
            map.insert(pid, Arc::new(TestMockAdapter(pid)));
        }
        ProviderRegistry::with_adapters(map)
    }

    async fn rpc_call(
        writer: &mut tokio::net::unix::OwnedWriteHalf,
        lines: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
        id: u64,
        request: DaemonRequest,
    ) -> (DaemonResponse, Vec<DaemonNotification>) {
        let env = RpcEnvelope { id, request };
        let payload = format!("{}\n", serde_json::to_string(&env).unwrap());
        writer.write_all(payload.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        let mut notifs = Vec::new();
        while let Some(line) = lines.next_line().await.unwrap() {
            let msg: ServerMessage = serde_json::from_str(&line).unwrap();
            match msg {
                ServerMessage::Response { id: rid, result } if rid == id => {
                    return (result.unwrap(), notifs);
                }
                ServerMessage::Notification(n) => notifs.push(n),
                _ => {}
            }
        }
        panic!("Socket closed before response for {id}");
    }

    #[tokio::test]
    async fn end_to_end_turn_handoff_and_compare_over_unix_socket() {
        let tmp = tempfile::tempdir().unwrap();
        let ws_dir = tmp.path().join("repo");
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(ws_dir.join("main.rs"), "fn main() {}\n").unwrap();

        let config = DaemonConfig {
            socket_path: tmp.path().join("umisongd.sock"),
            db_path: tmp.path().join("umisong.db"),
            worktrees_dir: tmp.path().join("worktrees"),
        };
        let state = DaemonState::new_with_registry(config, test_registry()).unwrap();
        let (client_stream, server_stream) = UnixStream::pair().unwrap();
        let state_srv = Arc::clone(&state);
        let daemon_handle = tokio::spawn(async move {
            let _ = handle_client(server_stream, state_srv).await;
        });

        let (r, mut w) = client_stream.into_split();
        let mut lines = BufReader::new(r).lines();

        // 1. Open workspace & create session
        let (resp, _) = rpc_call(
            &mut w,
            &mut lines,
            2,
            DaemonRequest::OpenWorkspace {
                path: ws_dir.to_string_lossy().into_owned(),
            },
        )
        .await;
        let ws = match resp {
            DaemonResponse::Workspace(w) => w,
            other => panic!("expected Workspace, got {other:?}"),
        };

        let (resp, _) = rpc_call(
            &mut w,
            &mut lines,
            3,
            DaemonRequest::CreateSession {
                workspace_id: ws.id.clone(),
                title: "New Session".into(),
                provider: ProviderId::ClaudeCode,
                model: String::new(),
                permission_mode: PermissionMode::AskSafe,
            },
        )
        .await;
        let sess = match resp {
            DaemonResponse::Session(s) => s,
            other => panic!("expected Session, got {other:?}"),
        };

        // 3. Send a turn to Claude Code and stream until TurnCompleted
        let _ = rpc_call(
            &mut w,
            &mut lines,
            4,
            DaemonRequest::SendTurn {
                session_id: sess.id.clone(),
                provider: ProviderId::ClaudeCode,
                model: String::new(),
                thinking_level: ThinkingLevel::High,
                permission_mode: PermissionMode::AskSafe,
                prompt: "Inspect main.rs".into(),
            },
        )
        .await;

        let mut saw_thought = false;
        let mut saw_tool = false;
        let mut saw_text = false;
        while let Some(line) = lines.next_line().await.unwrap() {
            if let ServerMessage::Notification(DaemonNotification::TurnEvent { event, .. }) =
                serde_json::from_str(&line).unwrap()
            {
                match event {
                    AgentEvent::ThoughtDelta { .. } => saw_thought = true,
                    AgentEvent::ToolCallCompleted { .. } => saw_tool = true,
                    AgentEvent::TextDelta { .. } => saw_text = true,
                    AgentEvent::TurnCompleted { .. } => break,
                    _ => {}
                }
            }
        }
        assert!(saw_thought && saw_tool && saw_text);

        // 4. Hand off the session from Claude Code -> Codex
        let _ = rpc_call(
            &mut w,
            &mut lines,
            5,
            DaemonRequest::HandoffSession {
                source_session_id: sess.id.clone(),
                up_to_turn_id: None,
                target_provider: ProviderId::Codex,
                target_model: String::new(),
                thinking_level: ThinkingLevel::Medium,
                instruction: "Write unit tests next".into(),
                create_new_session: false,
            },
        )
        .await;

        while let Some(line) = lines.next_line().await.unwrap() {
            if let ServerMessage::Notification(DaemonNotification::TurnEvent {
                provider,
                event: AgentEvent::TurnCompleted { .. },
                ..
            }) = serde_json::from_str(&line).unwrap()
            {
                if provider == ProviderId::Codex {
                    break;
                }
            }
        }

        // Verify both turns persisted with handoff provenance
        let (resp, _) = rpc_call(
            &mut w,
            &mut lines,
            6,
            DaemonRequest::GetSession {
                session_id: sess.id.clone(),
            },
        )
        .await;
        let loaded = match resp {
            DaemonResponse::Session(s) => s,
            other => panic!("expected Session, got {other:?}"),
        };
        assert_eq!(loaded.turns.len(), 2);
        assert_eq!(loaded.turns[1].provider, ProviderId::Codex);
        assert_eq!(loaded.turns[1].handoff_from_provider, Some(ProviderId::ClaudeCode));

        // 5. Run 3-way Parallel Compare across Claude Code, Antigravity, and Codex
        let (resp, _) = rpc_call(
            &mut w,
            &mut lines,
            7,
            DaemonRequest::StartCompare {
                workspace_id: ws.id.clone(),
                session_id: Some(sess.id.clone()),
                prompt: "Suggest error handling strategy".into(),
                targets: vec![
                    CompareTarget {
                        provider: ProviderId::ClaudeCode,
                        model: String::new(),
                        thinking_level: ThinkingLevel::High,
                    },
                    CompareTarget {
                        provider: ProviderId::Antigravity,
                        model: String::new(),
                        thinking_level: ThinkingLevel::High,
                    },
                    CompareTarget {
                        provider: ProviderId::Codex,
                        model: String::new(),
                        thinking_level: ThinkingLevel::Medium,
                    },
                ],
                isolation_mode: CompareIsolationMode::ReadOnly,
            },
        )
        .await;
        let cmp = match resp {
            DaemonResponse::CompareStarted(c) => c,
            other => panic!("expected CompareStarted, got {other:?}"),
        };

        let mut finished = 0usize;
        while finished < 3 {
            let line = lines.next_line().await.unwrap().unwrap();
            if let ServerMessage::Notification(DaemonNotification::CompareColumnFinished {
                compare_id,
                ..
            }) = serde_json::from_str(&line).unwrap()
            {
                if compare_id == cmp.id {
                    finished += 1;
                }
            }
        }

        // 6. Adopt Antigravity's column into the session
        let (resp, _) = rpc_call(
            &mut w,
            &mut lines,
            8,
            DaemonRequest::AdoptCompareColumn {
                compare_id: cmp.id.clone(),
                provider: ProviderId::Antigravity,
                apply_patch: false,
            },
        )
        .await;
        assert!(matches!(resp, DaemonResponse::CompareAdopted { .. }));

        daemon_handle.abort();
    }

    struct InstallableMockAdapter {
        provider: ProviderId,
        installed: Arc<std::sync::atomic::AtomicBool>,
    }

    #[async_trait]
    impl ProviderAdapter for InstallableMockAdapter {
        fn id(&self) -> ProviderId {
            self.provider
        }

        async fn probe_status(
            &self,
            settings: &ProviderSettings,
            _cache: &umisong_providers::ProviderProbeCache,
        ) -> ProviderStatus {
            let is_installed = self.installed.load(std::sync::atomic::Ordering::SeqCst);
            ProviderStatus {
                provider: self.provider,
                installed: is_installed,
                binary_path: is_installed.then(|| "/usr/local/bin/mock".into()),
                version: is_installed.then(|| "1.0.0".into()),
                auth: if is_installed {
                    AuthState::LoggedOut
                } else {
                    AuthState::NotInstalled
                },
                effective_plan_tier: PlanTier::AutoDetect,
                models: Vec::new(),
                active_default_model: settings.default_model.clone(),
                settings: settings.clone(),
                notes: None,
            }
        }

        fn login_command(
            &self,
            _settings: &ProviderSettings,
            _mode: AuthLoginMode,
        ) -> Result<CommandSpec> {
            Ok(CommandSpec {
                program: "/bin/echo".into(),
                args: vec!["ok".into()],
                env: HashMap::new(),
                cwd: None,
            })
        }

        async fn logout(&self, _settings: &ProviderSettings) -> Result<()> {
            Ok(())
        }

        async fn install_cli(
            &self,
            on_progress: &(dyn Fn(ProviderInstallState) + Send + Sync),
        ) -> Result<PathBuf> {
            on_progress(ProviderInstallState::Installing {
                method: "mock".into(),
                detail: "Installing mock CLI…".into(),
            });
            self.installed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(PathBuf::from("/usr/local/bin/mock"))
        }

        async fn run_turn(
            &self,
            _req: TurnExecutionRequest,
            _tx: mpsc::UnboundedSender<AgentEvent>,
            _approval_rx: mpsc::UnboundedReceiver<ApprovalDecision>,
            _cancel: CancellationToken,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn auto_install_missing_provider_updates_status_over_unix_socket() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = DaemonConfig {
            socket_path: tmp.path().join("umisongd.sock"),
            db_path: tmp.path().join("umisong.db"),
            worktrees_dir: tmp.path().join("worktrees"),
        };

        let claude_installed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut adapters: HashMap<ProviderId, Arc<dyn ProviderAdapter>> = HashMap::new();
        adapters.insert(
            ProviderId::ClaudeCode,
            Arc::new(InstallableMockAdapter {
                provider: ProviderId::ClaudeCode,
                installed: Arc::clone(&claude_installed),
            }),
        );
        adapters.insert(
            ProviderId::Antigravity,
            Arc::new(TestMockAdapter(ProviderId::Antigravity)),
        );
        adapters.insert(ProviderId::Codex, Arc::new(TestMockAdapter(ProviderId::Codex)));
        let registry = ProviderRegistry::with_adapters(adapters);

        let state = DaemonState::new_with_registry(cfg, registry).unwrap();
        let (stream, server_stream) = UnixStream::pair().unwrap();
        let daemon_handle = tokio::spawn(async move {
            let _ = handle_client(server_stream, state).await;
        });

        let (r, mut w) = stream.into_split();
        let mut lines = BufReader::new(r).lines();

        let (resp, _) = rpc_call(&mut w, &mut lines, 1, DaemonRequest::InstallMissingProviders).await;
        match resp {
            DaemonResponse::InstallStarted { providers } => {
                assert_eq!(providers, vec![ProviderId::ClaudeCode]);
            }
            other => panic!("expected InstallStarted, got {other:?}"),
        }

        let mut saw_installed_notif = false;
        let mut saw_provider_ready = false;
        for _ in 0..20 {
            let line = tokio::time::timeout(
                std::time::Duration::from_secs(3),
                lines.next_line(),
            )
            .await
            .unwrap()
            .unwrap()
            .unwrap();
            if let ServerMessage::Notification(notif) = serde_json::from_str(&line).unwrap() {
                match notif {
                    DaemonNotification::ProviderInstallUpdated { provider, state } => {
                        if provider == ProviderId::ClaudeCode
                            && matches!(state, ProviderInstallState::Installed)
                        {
                            saw_installed_notif = true;
                        }
                    }
                    DaemonNotification::ProviderStatusUpdated(st) => {
                        if st.provider == ProviderId::ClaudeCode && st.installed {
                            saw_provider_ready = true;
                        }
                    }
                    _ => {}
                }
            }
            if saw_installed_notif && saw_provider_ready {
                break;
            }
        }

        assert!(saw_installed_notif, "expected ProviderInstallUpdated::Installed");
        assert!(saw_provider_ready, "expected ProviderStatusUpdated with installed=true");
        daemon_handle.abort();
    }
}
