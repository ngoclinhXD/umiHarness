use crate::{
    AgentEvent, AppSettings, AuthLoginMode, CompareIsolationMode, CompareRun, CompareTarget,
    DiffScope, FileDiff, GitDiffSnapshot, PermissionMode, PlanTier, ProviderId, ProviderStatus,
    SessionDetail, SessionSummary, ThinkingLevel, Workspace,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEnvelope {
    pub id: u64,
    pub request: DaemonRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum ServerMessage {
    Response {
        id: u64,
        result: Result<DaemonResponse, String>,
    },
    Notification(DaemonNotification),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum DaemonRequest {
    Ping,
    GetSettings,
    UpdateSettings {
        settings: AppSettings,
    },
    ListProviders,
    RefreshProviderStatus {
        provider: ProviderId,
    },
    UpdateProviderConfig {
        provider: ProviderId,
        binary_override: Option<String>,
        default_model: String,
        custom_models: Vec<String>,
        api_key_override: Option<String>,
        plan_tier_override: Option<PlanTier>,
    },
    StartAuthTerminal {
        provider: ProviderId,
        mode: AuthLoginMode,
        cols: u16,
        rows: u16,
    },
    SendAuthInput {
        auth_session_id: String,
        data: String,
    },
    ResizeAuthTerminal {
        auth_session_id: String,
        cols: u16,
        rows: u16,
    },
    StopAuthTerminal {
        auth_session_id: String,
    },
    LogoutProvider {
        provider: ProviderId,
    },
    ListWorkspaces,
    OpenWorkspace {
        path: String,
    },
    ListSessions {
        workspace_id: String,
    },
    CreateSession {
        workspace_id: String,
        title: String,
        provider: ProviderId,
        model: String,
        permission_mode: PermissionMode,
    },
    GetSession {
        session_id: String,
    },
    DeleteSession {
        session_id: String,
    },
    UpdateSessionMeta {
        session_id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        pinned: Option<bool>,
        #[serde(default)]
        unread: Option<bool>,
        #[serde(default)]
        archived: Option<bool>,
    },
    UpdateSessionConfig {
        session_id: String,
        provider: ProviderId,
        model: String,
        permission_mode: PermissionMode,
    },
    SendTurn {
        session_id: String,
        provider: ProviderId,
        model: String,
        #[serde(default)]
        thinking_level: ThinkingLevel,
        permission_mode: PermissionMode,
        prompt: String,
    },
    CancelTurn {
        turn_id: String,
    },
    RespondApproval {
        approval_id: String,
        approved: bool,
    },
    HandoffSession {
        source_session_id: String,
        up_to_turn_id: Option<String>,
        target_provider: ProviderId,
        target_model: String,
        #[serde(default)]
        thinking_level: ThinkingLevel,
        instruction: String,
        create_new_session: bool,
    },
    ListCompareRuns {
        workspace_id: String,
    },
    StartCompare {
        workspace_id: String,
        session_id: Option<String>,
        prompt: String,
        targets: Vec<CompareTarget>,
        isolation_mode: CompareIsolationMode,
    },
    CancelCompare {
        compare_id: String,
    },
    AdoptCompareColumn {
        compare_id: String,
        provider: ProviderId,
        apply_patch: bool,
    },
    GetGitDiff {
        workspace_path: String,
        scope: DiffScope,
    },
    RevertFile {
        workspace_path: String,
        file_path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum DaemonResponse {
    Pong { version: String },
    Ack,
    Settings(AppSettings),
    Providers(Vec<ProviderStatus>),
    Provider(ProviderStatus),
    AuthTerminalStarted { auth_session_id: String, command: String },
    Workspaces(Vec<Workspace>),
    Workspace(Workspace),
    Sessions(Vec<SessionSummary>),
    Session(SessionDetail),
    TurnStarted { session_id: String, turn_id: String },
    HandoffStarted { session_id: String, turn_id: String },
    CompareRuns(Vec<CompareRun>),
    CompareStarted(CompareRun),
    CompareAdopted {
        session_id: String,
        patch_applied: bool,
        message: String,
    },
    GitDiff(GitDiffSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "notification", content = "data", rename_all = "snake_case")]
pub enum DaemonNotification {
    ProviderStatusUpdated(ProviderStatus),
    TurnEvent {
        session_id: String,
        turn_id: String,
        provider: ProviderId,
        event: AgentEvent,
    },
    CompareEvent {
        compare_id: String,
        provider: ProviderId,
        event: AgentEvent,
    },
    CompareColumnFinished {
        compare_id: String,
        provider: ProviderId,
        diff_patch: Option<String>,
        diffs: Vec<FileDiff>,
    },
    AuthTerminalOutput {
        auth_session_id: String,
        provider: ProviderId,
        screen_lines: Vec<String>,
        detected_urls: Vec<String>,
        exited: bool,
        exit_code: Option<i32>,
    },
    WorkspaceDiffChanged {
        workspace_path: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_round_trip() {
        let env = RpcEnvelope {
            id: 42,
            request: DaemonRequest::StartCompare {
                workspace_id: "ws-1".into(),
                session_id: None,
                prompt: "Compare approaches".into(),
                targets: vec![
                    CompareTarget {
                        provider: ProviderId::ClaudeCode,
                        model: "sonnet".into(),
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
                isolation_mode: CompareIsolationMode::GitWorktree,
            },
        };
        let encoded = serde_json::to_string(&env).unwrap();
        let decoded: RpcEnvelope = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.id, 42);
        match decoded.request {
            DaemonRequest::StartCompare { targets, .. } => assert_eq!(targets.len(), 3),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn server_message_vec_variants_round_trip() {
        let msgs = vec![
            ServerMessage::Response {
                id: 1,
                result: Ok(DaemonResponse::Providers(vec![])),
            },
            ServerMessage::Response {
                id: 2,
                result: Ok(DaemonResponse::Workspaces(vec![])),
            },
            ServerMessage::Response {
                id: 3,
                result: Ok(DaemonResponse::Sessions(vec![])),
            },
            ServerMessage::Response {
                id: 4,
                result: Ok(DaemonResponse::CompareRuns(vec![])),
            },
            ServerMessage::Response {
                id: 5,
                result: Ok(DaemonResponse::Ack),
            },
            ServerMessage::Response {
                id: 6,
                result: Ok(DaemonResponse::Settings(AppSettings::default())),
            },
            ServerMessage::Notification(DaemonNotification::WorkspaceDiffChanged {
                workspace_path: "/tmp".into(),
            }),
        ];

        for msg in msgs {
            let encoded = serde_json::to_string(&msg).expect("ServerMessage must serialize");
            let _: ServerMessage =
                serde_json::from_str(&encoded).expect("ServerMessage must deserialize");
        }
    }
}
