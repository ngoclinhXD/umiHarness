use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Shell,
    FileRead,
    FileEdit,
    Search,
    Web,
    Other,
}

impl ToolKind {
    pub fn classify(tool_name: &str) -> Self {
        let lower = tool_name.to_ascii_lowercase();
        if lower.contains("bash")
            || lower.contains("shell")
            || lower.contains("exec")
            || lower.contains("command")
            || lower.contains("terminal")
        {
            ToolKind::Shell
        } else if lower.contains("edit")
            || lower.contains("write")
            || lower.contains("patch")
            || lower.contains("replace")
            || lower.contains("create")
        {
            ToolKind::FileEdit
        } else if lower.contains("read") || lower.contains("view") || lower.contains("cat") {
            ToolKind::FileRead
        } else if lower.contains("grep")
            || lower.contains("glob")
            || lower.contains("find")
            || lower.contains("search")
            || lower.contains("list")
        {
            ToolKind::Search
        } else if lower.contains("web") || lower.contains("fetch") || lower.contains("http") {
            ToolKind::Web
        } else {
            ToolKind::Other
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            ToolKind::Shell => "$",
            ToolKind::FileRead => "R",
            ToolKind::FileEdit => "E",
            ToolKind::Search => "?",
            ToolKind::Web => "W",
            ToolKind::Other => "T",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnExitReason {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    SessionInit {
        provider_session_id: Option<String>,
        resolved_model: Option<String>,
    },
    ThoughtDelta {
        text: String,
    },
    TextDelta {
        text: String,
    },
    ToolCallStarted {
        call_id: String,
        name: String,
        kind: ToolKind,
        input_summary: String,
        raw_input: Option<String>,
    },
    ToolCallUpdated {
        call_id: String,
        status: String,
        output_chunk: Option<String>,
    },
    ToolCallCompleted {
        call_id: String,
        success: bool,
        output: String,
        duration_ms: Option<u64>,
    },
    ApprovalRequested {
        approval_id: String,
        tool_name: String,
        summary: String,
        detail: String,
    },
    UsageUpdate {
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cost_usd: Option<f64>,
    },
    TurnCompleted {
        final_text: Option<String>,
        exit_reason: TurnExitReason,
    },
    Error {
        message: String,
        recoverable: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub approval_id: String,
    pub approved: bool,
}
