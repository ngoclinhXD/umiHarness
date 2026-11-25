use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use umisong_core::{
    AppSettings, CompareColumnState, CompareIsolationMode, CompareRun, PermissionMode, PlanTier,
    ProviderId, ProviderSettings, SessionDetail, SessionSummary, ToolCallRecord, ToolKind,
    TurnRecord, TurnStatus, Workspace, parse_unified_diff,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite database at {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS provider_settings (
                provider TEXT PRIMARY KEY,
                binary_override TEXT,
                default_model TEXT NOT NULL DEFAULT '',
                custom_models_json TEXT NOT NULL DEFAULT '[]',
                api_key_override TEXT,
                discovered_models_json TEXT NOT NULL DEFAULT '[]',
                plan_tier_override TEXT,
                detected_plan TEXT,
                detected_account TEXT
            );

            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL UNIQUE,
                last_opened_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                active_provider TEXT NOT NULL,
                active_model TEXT NOT NULL DEFAULT '',
                permission_mode TEXT NOT NULL DEFAULT 'ask_safe',
                provider_session_ids_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS turns (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                seq INTEGER NOT NULL,
                provider TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                resolved_model TEXT,
                prompt TEXT NOT NULL,
                thought_text TEXT NOT NULL DEFAULT '',
                response_text TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'running',
                error_message TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL,
                duration_ms INTEGER,
                handoff_from_provider TEXT,
                handoff_from_turn_id TEXT,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS tool_calls (
                id TEXT PRIMARY KEY,
                turn_id TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
                call_id TEXT NOT NULL,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                input_summary TEXT NOT NULL,
                output_text TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'running',
                success INTEGER,
                duration_ms INTEGER
            );

            CREATE TABLE IF NOT EXISTS compare_runs (
                id TEXT PRIMARY KEY,
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
                session_id TEXT,
                prompt TEXT NOT NULL,
                isolation_mode TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS compare_columns (
                compare_id TEXT NOT NULL REFERENCES compare_runs(id) ON DELETE CASCADE,
                provider TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                resolved_model TEXT,
                status TEXT NOT NULL DEFAULT 'running',
                thought_text TEXT NOT NULL DEFAULT '',
                response_text TEXT NOT NULL DEFAULT '',
                tool_calls_json TEXT NOT NULL DEFAULT '[]',
                error_message TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL,
                duration_ms INTEGER,
                worktree_path TEXT,
                diff_patch TEXT,
                adopted INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (compare_id, provider)
            );
            "#,
        )?;
        let _ = conn.execute(
            "ALTER TABLE provider_settings ADD COLUMN plan_tier_override TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE provider_settings ADD COLUMN detected_plan TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE provider_settings ADD COLUMN detected_account TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE sessions ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE sessions ADD COLUMN unread INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE sessions ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // Any turn still marked `running` when opening the database is an orphan from a prior process.
        let _ = conn.execute(
            "UPDATE turns SET status = 'cancelled' WHERE status = 'running'",
            [],
        );
        let _ = conn.execute(
            "UPDATE compare_columns SET status = 'cancelled' WHERE status = 'running'",
            [],
        );
        Ok(())
    }

    pub fn get_app_settings(&self) -> Result<AppSettings> {
        let conn = self.conn.lock().unwrap();
        let raw: Option<String> = conn
            .query_row(
                "SELECT value_json FROM app_settings WHERE key = 'app'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match raw {
            Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
            None => Ok(AppSettings::default()),
        }
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let encoded = serde_json::to_string(settings)?;
        conn.execute(
            "INSERT INTO app_settings (key, value_json) VALUES ('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
            params![encoded],
        )?;
        Ok(())
    }

    pub fn get_provider_settings(&self, provider: ProviderId) -> Result<(ProviderSettings, Vec<String>)> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT binary_override, default_model, custom_models_json, api_key_override, discovered_models_json, plan_tier_override
                 FROM provider_settings WHERE provider = ?1",
                params![provider.as_str()],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;

        match row {
            Some((
                binary_override,
                default_model,
                custom_json,
                api_key_override,
                discovered_json,
                plan_tier_str,
            )) => {
                let custom_models: Vec<String> = serde_json::from_str(&custom_json).unwrap_or_default();
                let discovered: Vec<String> = serde_json::from_str(&discovered_json).unwrap_or_default();
                let plan_tier_override = plan_tier_str
                    .as_deref()
                    .map(PlanTier::parse)
                    .filter(|t| *t != PlanTier::AutoDetect);
                Ok((
                    ProviderSettings {
                        provider,
                        binary_override,
                        default_model,
                        custom_models,
                        api_key_override,
                        plan_tier_override,
                    },
                    discovered,
                ))
            }
            None => Ok((ProviderSettings::default_for(provider), Vec::new())),
        }
    }

    pub fn save_provider_settings(&self, settings: &ProviderSettings) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let custom_json = serde_json::to_string(&settings.custom_models)?;
        let tier_str = settings
            .plan_tier_override
            .filter(|t| *t != PlanTier::AutoDetect)
            .map(PlanTier::as_str);
        conn.execute(
            "INSERT INTO provider_settings (provider, binary_override, default_model, custom_models_json, api_key_override, plan_tier_override)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(provider) DO UPDATE SET
               binary_override = excluded.binary_override,
               default_model = excluded.default_model,
               custom_models_json = excluded.custom_models_json,
               api_key_override = excluded.api_key_override,
               plan_tier_override = excluded.plan_tier_override",
            params![
                settings.provider.as_str(),
                settings.binary_override,
                settings.default_model,
                custom_json,
                settings.api_key_override,
                tier_str,
            ],
        )?;
        Ok(())
    }

    /// Cached `(detected_plan, detected_account)` from live provider discovery.
    pub fn get_provider_cached_plan(
        &self,
        provider: ProviderId,
    ) -> Result<(Option<String>, Option<String>)> {
        let conn = self.conn.lock().unwrap();
        let row = conn
            .query_row(
                "SELECT detected_plan, detected_account FROM provider_settings WHERE provider = ?1",
                params![provider.as_str()],
                |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
            )
            .optional()?;
        Ok(row.unwrap_or((None, None)))
    }

    /// Replace cached live discovery (models, plan, account), keeping all other settings.
    pub fn save_live_discovery(
        &self,
        provider: ProviderId,
        models: &[String],
        detected_plan: Option<&str>,
        detected_account: Option<&str>,
    ) -> Result<()> {
        let mut unique: Vec<&String> = Vec::new();
        for m in models {
            if !m.trim().is_empty() && !unique.contains(&m) {
                unique.push(m);
            }
        }
        let json = serde_json::to_string(&unique)?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO provider_settings (provider, discovered_models_json, detected_plan, detected_account)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider) DO UPDATE SET
               discovered_models_json = excluded.discovered_models_json,
               detected_plan = excluded.detected_plan,
               detected_account = excluded.detected_account",
            params![provider.as_str(), json, detected_plan, detected_account],
        )?;
        Ok(())
    }

    pub fn clear_live_discovery(&self, provider: ProviderId) -> Result<()> {
        self.save_live_discovery(provider, &[], None, None)
    }

    pub fn record_discovered_model(&self, provider: ProviderId, model_id: &str) -> Result<()> {
        let trimmed = model_id.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let (settings, mut discovered) = self.get_provider_settings(provider)?;
        if !discovered.iter().any(|m| m == trimmed) {
            discovered.push(trimmed.to_string());
            let conn = self.conn.lock().unwrap();
            let disc_json = serde_json::to_string(&discovered)?;
            let custom_json = serde_json::to_string(&settings.custom_models)?;
            conn.execute(
                "INSERT INTO provider_settings (provider, binary_override, default_model, custom_models_json, api_key_override, discovered_models_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(provider) DO UPDATE SET discovered_models_json = excluded.discovered_models_json",
                params![
                    provider.as_str(),
                    settings.binary_override,
                    settings.default_model,
                    custom_json,
                    settings.api_key_override,
                    disc_json,
                ],
            )?;
        }
        Ok(())
    }

    pub fn open_workspace(&self, path: &Path) -> Result<Workspace> {
        let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let path_str = canonical.to_string_lossy().into_owned();
        let name = canonical
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Workspace")
            .to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let id = {
            let conn = self.conn.lock().unwrap();
            let existing: Option<String> = conn
                .query_row(
                    "SELECT id FROM workspaces WHERE path = ?1",
                    params![path_str],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(existing_id) = existing {
                conn.execute(
                    "UPDATE workspaces SET last_opened_at = ?1, name = ?2 WHERE id = ?3",
                    params![now_str, name, existing_id],
                )?;
                existing_id
            } else {
                let new_id = Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO workspaces (id, name, path, last_opened_at) VALUES (?1, ?2, ?3, ?4)",
                    params![new_id, name, path_str, now_str],
                )?;
                new_id
            }
        };

        let is_git = crate::git_ops::is_git_repo(&canonical);
        let branch = if is_git {
            crate::git_ops::current_branch(&canonical)
        } else {
            None
        };

        Ok(Workspace {
            id,
            name,
            path: path_str,
            is_git_repo: is_git,
            git_branch: branch,
            last_opened_at: now,
        })
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>> {
        let rows: Vec<(String, String, String, String)> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, name, path, last_opened_at FROM workspaces ORDER BY last_opened_at DESC",
            )?;
            let mapped = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            mapped.filter_map(Result::ok).collect()
        };

        let mut out = Vec::with_capacity(rows.len());
        for (id, name, path_str, ts) in rows {
            let p = Path::new(&path_str);
            let is_git = p.exists() && crate::git_ops::is_git_repo(p);
            let branch = if is_git {
                crate::git_ops::current_branch(p)
            } else {
                None
            };
            let last_opened_at = DateTime::parse_from_rfc3339(&ts)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            out.push(Workspace {
                id,
                name,
                path: path_str,
                is_git_repo: is_git,
                git_branch: branch,
                last_opened_at,
            });
        }
        Ok(out)
    }

    pub fn get_workspace(&self, workspace_id: &str) -> Result<Option<Workspace>> {
        let row: Option<(String, String, String, String)> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT id, name, path, last_opened_at FROM workspaces WHERE id = ?1",
                params![workspace_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?
        };
        Ok(row.map(|(id, name, path_str, ts)| {
            let p = Path::new(&path_str);
            let is_git = p.exists() && crate::git_ops::is_git_repo(p);
            let branch = if is_git {
                crate::git_ops::current_branch(p)
            } else {
                None
            };
            Workspace {
                id,
                name,
                path: path_str,
                is_git_repo: is_git,
                git_branch: branch,
                last_opened_at: DateTime::parse_from_rfc3339(&ts)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            }
        }))
    }

    pub fn create_session(
        &self,
        workspace_id: &str,
        title: &str,
        provider: ProviderId,
        model: &str,
        permission_mode: PermissionMode,
    ) -> Result<SessionDetail> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let clean_title = if title.trim().is_empty() {
            "New Session".to_string()
        } else {
            title.trim().to_string()
        };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO sessions (id, workspace_id, title, active_provider, active_model, permission_mode, provider_session_ids_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', ?7, ?8)",
            params![
                id,
                workspace_id,
                clean_title,
                provider.as_str(),
                model,
                permission_mode.as_str(),
                now_str,
                now_str,
            ],
        )?;

        Ok(SessionDetail {
            id,
            workspace_id: workspace_id.to_string(),
            title: clean_title,
            active_provider: provider,
            active_model: model.to_string(),
            permission_mode,
            provider_session_ids: HashMap::new(),
            created_at: now,
            updated_at: now,
            turns: Vec::new(),
        })
    }

    pub fn list_sessions(&self, workspace_id: &str) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.workspace_id, s.title, s.active_provider, s.active_model, s.permission_mode, s.updated_at,
                    (SELECT COUNT(*) FROM turns t WHERE t.session_id = s.id) as turn_count,
                    s.pinned, s.unread, s.archived
             FROM sessions s
             WHERE s.workspace_id = ?1
             ORDER BY s.archived ASC, s.pinned DESC, s.updated_at DESC",
        )?;
        let rows = stmt.query_map(params![workspace_id], |r| {
            let provider_str: String = r.get(3)?;
            let perm_str: String = r.get(5)?;
            let updated_str: String = r.get(6)?;
            let turn_count: i64 = r.get(7)?;
            let pinned: i64 = r.get(8)?;
            let unread: i64 = r.get(9)?;
            let archived: i64 = r.get(10)?;
            Ok(SessionSummary {
                id: r.get(0)?,
                workspace_id: r.get(1)?,
                title: r.get(2)?,
                active_provider: ProviderId::parse(&provider_str).unwrap_or(ProviderId::ClaudeCode),
                active_model: r.get(4)?,
                permission_mode: PermissionMode::parse(&perm_str),
                turn_count: turn_count.max(0) as usize,
                updated_at: DateTime::parse_from_rfc3339(&updated_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                pinned: pinned != 0,
                unread: unread != 0,
                archived: archived != 0,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn update_session_meta(
        &self,
        session_id: &str,
        title: Option<&str>,
        pinned: Option<bool>,
        unread: Option<bool>,
        archived: Option<bool>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(t) = title {
            let trimmed = t.trim();
            if !trimmed.is_empty() {
                conn.execute(
                    "UPDATE sessions SET title = ?1 WHERE id = ?2",
                    params![trimmed, session_id],
                )?;
            }
        }
        if let Some(p) = pinned {
            conn.execute(
                "UPDATE sessions SET pinned = ?1 WHERE id = ?2",
                params![if p { 1i64 } else { 0i64 }, session_id],
            )?;
        }
        if let Some(u) = unread {
            conn.execute(
                "UPDATE sessions SET unread = ?1 WHERE id = ?2",
                params![if u { 1i64 } else { 0i64 }, session_id],
            )?;
        }
        if let Some(a) = archived {
            conn.execute(
                "UPDATE sessions SET archived = ?1 WHERE id = ?2",
                params![if a { 1i64 } else { 0i64 }, session_id],
            )?;
        }
        Ok(())
    }

    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionDetail>> {
        let conn = self.conn.lock().unwrap();
        let header = conn
            .query_row(
                "SELECT id, workspace_id, title, active_provider, active_model, permission_mode, provider_session_ids_json, created_at, updated_at
                 FROM sessions WHERE id = ?1",
                params![session_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, String>(7)?,
                        r.get::<_, String>(8)?,
                    ))
                },
            )
            .optional()?;

        let Some((id, workspace_id, title, prov_s, active_model, perm_s, psids_json, created_s, updated_s)) = header else {
            return Ok(None);
        };

        let mut turn_stmt = conn.prepare(
            "SELECT id, session_id, seq, provider, model, resolved_model, prompt, thought_text, response_text,
                    status, error_message, input_tokens, output_tokens, cache_read_tokens, cost_usd, duration_ms,
                    handoff_from_provider, handoff_from_turn_id, created_at
             FROM turns WHERE session_id = ?1 ORDER BY seq ASC",
        )?;
        let turn_rows = turn_stmt.query_map(params![session_id], |r| {
            let p_str: String = r.get(3)?;
            let st_str: String = r.get(9)?;
            let hf_p: Option<String> = r.get(16)?;
            let c_str: String = r.get(18)?;
            Ok(TurnRecord {
                id: r.get(0)?,
                session_id: r.get(1)?,
                seq: r.get(2)?,
                provider: ProviderId::parse(&p_str).unwrap_or(ProviderId::ClaudeCode),
                model: r.get(4)?,
                resolved_model: r.get(5)?,
                prompt: r.get(6)?,
                thought_text: r.get(7)?,
                response_text: r.get(8)?,
                status: TurnStatus::parse(&st_str),
                error_message: r.get(10)?,
                input_tokens: r.get::<_, i64>(11)?.max(0) as u64,
                output_tokens: r.get::<_, i64>(12)?.max(0) as u64,
                cache_read_tokens: r.get::<_, i64>(13)?.max(0) as u64,
                cost_usd: r.get(14)?,
                duration_ms: r.get::<_, Option<i64>>(15)?.map(|v| v.max(0) as u64),
                handoff_from_provider: hf_p.as_deref().and_then(ProviderId::parse),
                handoff_from_turn_id: r.get(17)?,
                tool_calls: Vec::new(),
                created_at: DateTime::parse_from_rfc3339(&c_str)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        let mut turns: Vec<TurnRecord> = turn_rows.filter_map(Result::ok).collect();

        let mut tc_stmt = conn.prepare(
            "SELECT call_id, name, kind, input_summary, output_text, status, success, duration_ms
             FROM tool_calls WHERE turn_id = ?1",
        )?;
        for turn in &mut turns {
            let calls = tc_stmt.query_map(params![turn.id], |r| {
                let kind_str: String = r.get(2)?;
                let kind = serde_json::from_str::<ToolKind>(&format!("\"{kind_str}\""))
                    .unwrap_or(ToolKind::Other);
                Ok(ToolCallRecord {
                    call_id: r.get(0)?,
                    name: r.get(1)?,
                    kind,
                    input_summary: r.get(3)?,
                    output_text: r.get(4)?,
                    status: r.get(5)?,
                    success: r.get::<_, Option<i64>>(6)?.map(|v| v != 0),
                    duration_ms: r.get::<_, Option<i64>>(7)?.map(|v| v.max(0) as u64),
                })
            })?;
            turn.tool_calls = calls.filter_map(Result::ok).collect();
        }

        let raw_map: HashMap<String, String> = serde_json::from_str(&psids_json).unwrap_or_default();
        let mut provider_session_ids = HashMap::new();
        for (k, v) in raw_map {
            if let Some(pid) = ProviderId::parse(&k) {
                provider_session_ids.insert(pid, v);
            }
        }

        Ok(Some(SessionDetail {
            id,
            workspace_id,
            title,
            active_provider: ProviderId::parse(&prov_s).unwrap_or(ProviderId::ClaudeCode),
            active_model,
            permission_mode: PermissionMode::parse(&perm_s),
            provider_session_ids,
            created_at: DateTime::parse_from_rfc3339(&created_s)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_s)
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            turns,
        }))
    }

    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
        Ok(())
    }

    /// Force-marks a turn as `cancelled` in SQLite and returns its `(session_id, provider)` if found.
    pub fn cancel_turn_in_db(&self, turn_id: &str) -> Result<Option<(String, ProviderId)>> {
        let conn = self.conn.lock().unwrap();
        let info: Option<(String, String)> = conn
            .query_row(
                "SELECT session_id, provider FROM turns WHERE id = ?1",
                params![turn_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if info.is_some() {
            conn.execute(
                "UPDATE turns SET status = 'cancelled' WHERE id = ?1 AND status = 'running'",
                params![turn_id],
            )?;
        }
        Ok(info.map(|(sid, p)| {
            (
                sid,
                ProviderId::parse(&p).unwrap_or(ProviderId::ClaudeCode),
            )
        }))
    }

    pub fn update_session_config(
        &self,
        session_id: &str,
        provider: ProviderId,
        model: &str,
        permission_mode: PermissionMode,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sessions SET active_provider = ?1, active_model = ?2, permission_mode = ?3, updated_at = ?4
             WHERE id = ?5",
            params![
                provider.as_str(),
                model,
                permission_mode.as_str(),
                Utc::now().to_rfc3339(),
                session_id,
            ],
        )?;
        Ok(())
    }

    pub fn update_session_title_if_default(&self, session_id: &str, prompt: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let current: Option<String> = conn
            .query_row(
                "SELECT title FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        if matches!(current.as_deref(), Some("New Session") | Some("")) {
            let first_line = prompt.lines().next().unwrap_or("Session").trim();
            let title: String = first_line.chars().take(54).collect();
            if !title.is_empty() {
                conn.execute(
                    "UPDATE sessions SET title = ?1 WHERE id = ?2",
                    params![title, session_id],
                )?;
            }
        }
        Ok(())
    }

    pub fn save_provider_session_id(
        &self,
        session_id: &str,
        provider: ProviderId,
        provider_session_id: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let raw: Option<String> = conn
            .query_row(
                "SELECT provider_session_ids_json FROM sessions WHERE id = ?1",
                params![session_id],
                |r| r.get(0),
            )
            .optional()?;
        let mut map: HashMap<String, String> = raw
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        map.insert(provider.as_str().to_string(), provider_session_id.to_string());
        let encoded = serde_json::to_string(&map)?;
        conn.execute(
            "UPDATE sessions SET provider_session_ids_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![encoded, Utc::now().to_rfc3339(), session_id],
        )?;
        Ok(())
    }

    pub fn create_turn(
        &self,
        session_id: &str,
        provider: ProviderId,
        model: &str,
        prompt: &str,
        handoff_from_provider: Option<ProviderId>,
        handoff_from_turn_id: Option<&str>,
    ) -> Result<TurnRecord> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let conn = self.conn.lock().unwrap();
        let next_seq: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM turns WHERE session_id = ?1",
            params![session_id],
            |r| r.get(0),
        )?;

        conn.execute(
            "INSERT INTO turns (id, session_id, seq, provider, model, prompt, status, handoff_from_provider, handoff_from_turn_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7, ?8, ?9)",
            params![
                id,
                session_id,
                next_seq,
                provider.as_str(),
                model,
                prompt,
                handoff_from_provider.map(ProviderId::as_str),
                handoff_from_turn_id,
                now_str,
            ],
        )?;
        conn.execute(
            "UPDATE sessions SET updated_at = ?1, active_provider = ?2, active_model = ?3 WHERE id = ?4",
            params![now_str, provider.as_str(), model, session_id],
        )?;

        Ok(TurnRecord {
            id,
            session_id: session_id.to_string(),
            seq: next_seq,
            provider,
            model: model.to_string(),
            resolved_model: None,
            prompt: prompt.to_string(),
            thought_text: String::new(),
            response_text: String::new(),
            status: TurnStatus::Running,
            error_message: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: None,
            duration_ms: None,
            handoff_from_provider,
            handoff_from_turn_id: handoff_from_turn_id.map(ToString::to_string),
            tool_calls: Vec::new(),
            created_at: now,
        })
    }

    pub fn finish_turn(&self, turn: &TurnRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE turns SET
               resolved_model = ?1,
               thought_text = ?2,
               response_text = ?3,
               status = ?4,
               error_message = ?5,
               input_tokens = ?6,
               output_tokens = ?7,
               cache_read_tokens = ?8,
               cost_usd = ?9,
               duration_ms = ?10
             WHERE id = ?11",
            params![
                turn.resolved_model,
                turn.thought_text,
                turn.response_text,
                turn.status.as_str(),
                turn.error_message,
                turn.input_tokens as i64,
                turn.output_tokens as i64,
                turn.cache_read_tokens as i64,
                turn.cost_usd,
                turn.duration_ms.map(|d| d as i64),
                turn.id,
            ],
        )?;

        conn.execute("DELETE FROM tool_calls WHERE turn_id = ?1", params![turn.id])?;
        for tc in &turn.tool_calls {
            let kind_str = serde_json::to_string(&tc.kind)
                .unwrap_or_else(|_| "\"other\"".into())
                .trim_matches('"')
                .to_string();
            conn.execute(
                "INSERT INTO tool_calls (id, turn_id, call_id, name, kind, input_summary, output_text, status, success, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    Uuid::new_v4().to_string(),
                    turn.id,
                    tc.call_id,
                    tc.name,
                    kind_str,
                    tc.input_summary,
                    tc.output_text,
                    tc.status,
                    tc.success.map(|b| if b { 1i64 } else { 0i64 }),
                    tc.duration_ms.map(|d| d as i64),
                ],
            )?;
        }

        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), turn.session_id],
        )?;
        Ok(())
    }

    pub fn insert_compare_run(&self, run: &CompareRun) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO compare_runs (id, workspace_id, session_id, prompt, isolation_mode, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run.id,
                run.workspace_id,
                run.session_id,
                run.prompt,
                run.isolation_mode.as_str(),
                run.created_at.to_rfc3339(),
            ],
        )?;
        for col in &run.columns {
            conn.execute(
                "INSERT INTO compare_columns (compare_id, provider, model, status, worktree_path)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    run.id,
                    col.provider.as_str(),
                    col.model,
                    col.status.as_str(),
                    col.worktree_path,
                ],
            )?;
        }
        Ok(())
    }

    pub fn update_compare_column(&self, compare_id: &str, col: &CompareColumnState) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let tc_json = serde_json::to_string(&col.tool_calls)?;
        conn.execute(
            "UPDATE compare_columns SET
               resolved_model = ?1,
               status = ?2,
               thought_text = ?3,
               response_text = ?4,
               tool_calls_json = ?5,
               error_message = ?6,
               input_tokens = ?7,
               output_tokens = ?8,
               cost_usd = ?9,
               duration_ms = ?10,
               worktree_path = ?11,
               diff_patch = ?12,
               adopted = ?13
             WHERE compare_id = ?14 AND provider = ?15",
            params![
                col.resolved_model,
                col.status.as_str(),
                col.thought_text,
                col.response_text,
                tc_json,
                col.error_message,
                col.input_tokens as i64,
                col.output_tokens as i64,
                col.cost_usd,
                col.duration_ms.map(|d| d as i64),
                col.worktree_path,
                col.diff_patch,
                if col.adopted { 1i64 } else { 0i64 },
                compare_id,
                col.provider.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn list_compare_runs(&self, workspace_id: &str) -> Result<Vec<CompareRun>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, workspace_id, session_id, prompt, isolation_mode, created_at
             FROM compare_runs WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT 30",
        )?;
        let headers: Vec<(String, String, Option<String>, String, String, String)> = stmt
            .query_map(params![workspace_id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .filter_map(Result::ok)
            .collect();

        let mut col_stmt = conn.prepare(
            "SELECT provider, model, resolved_model, status, thought_text, response_text, tool_calls_json,
                    error_message, input_tokens, output_tokens, cost_usd, duration_ms, worktree_path, diff_patch, adopted
             FROM compare_columns WHERE compare_id = ?1",
        )?;

        let mut runs = Vec::with_capacity(headers.len());
        for (id, ws_id, session_id, prompt, iso_s, created_s) in headers {
            let cols: Vec<CompareColumnState> = col_stmt
                .query_map(params![id], |r| {
                    let prov_s: String = r.get(0)?;
                    let st_s: String = r.get(3)?;
                    let tc_json: String = r.get(6)?;
                    let diff_patch: Option<String> = r.get(13)?;
                    let diffs = diff_patch
                        .as_deref()
                        .map(parse_unified_diff)
                        .unwrap_or_default();
                    Ok(CompareColumnState {
                        provider: ProviderId::parse(&prov_s).unwrap_or(ProviderId::ClaudeCode),
                        model: r.get(1)?,
                        resolved_model: r.get(2)?,
                        status: TurnStatus::parse(&st_s),
                        thought_text: r.get(4)?,
                        response_text: r.get(5)?,
                        tool_calls: serde_json::from_str(&tc_json).unwrap_or_default(),
                        error_message: r.get(7)?,
                        input_tokens: r.get::<_, i64>(8)?.max(0) as u64,
                        output_tokens: r.get::<_, i64>(9)?.max(0) as u64,
                        cost_usd: r.get(10)?,
                        duration_ms: r.get::<_, Option<i64>>(11)?.map(|v| v.max(0) as u64),
                        worktree_path: r.get(12)?,
                        diff_patch,
                        diffs,
                        adopted: r.get::<_, i64>(14)? != 0,
                    })
                })?
                .filter_map(Result::ok)
                .collect();

            runs.push(CompareRun {
                id,
                workspace_id: ws_id,
                session_id,
                prompt,
                isolation_mode: CompareIsolationMode::parse(&iso_s),
                columns: cols,
                created_at: DateTime::parse_from_rfc3339(&created_s)
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            });
        }
        Ok(runs)
    }

    pub fn get_compare_run(&self, compare_id: &str) -> Result<Option<CompareRun>> {
        let ws_id: Option<String> = {
            let conn = self.conn.lock().unwrap();
            conn.query_row(
                "SELECT workspace_id FROM compare_runs WHERE id = ?1",
                params![compare_id],
                |r| r.get(0),
            )
            .optional()?
        };
        let Some(ws_id) = ws_id else {
            return Ok(None);
        };
        let runs = self.list_compare_runs(&ws_id)?;
        Ok(runs.into_iter().find(|r| r.id == compare_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_discovery_persists_and_clears_keeping_plan_override() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("disc.db")).unwrap();
        let mut settings = ProviderSettings::default_for(ProviderId::Antigravity);
        settings.plan_tier_override = Some(PlanTier::GoogleAiUltra);
        store.save_provider_settings(&settings).unwrap();

        store
            .save_live_discovery(
                ProviderId::Antigravity,
                &["a-1".into(), "a-1".into(), "b-2".into()],
                Some("Google AI Pro"),
                Some("me@x.com"),
            )
            .unwrap();
        let (s, d) = store.get_provider_settings(ProviderId::Antigravity).unwrap();
        assert_eq!(d, vec!["a-1".to_string(), "b-2".to_string()]);
        assert_eq!(s.plan_tier_override, Some(PlanTier::GoogleAiUltra));
        assert_eq!(
            store.get_provider_cached_plan(ProviderId::Antigravity).unwrap(),
            (Some("Google AI Pro".into()), Some("me@x.com".into()))
        );

        store.clear_live_discovery(ProviderId::Antigravity).unwrap();
        let (s, d) = store.get_provider_settings(ProviderId::Antigravity).unwrap();
        assert!(d.is_empty());
        assert_eq!(s.plan_tier_override, Some(PlanTier::GoogleAiUltra));
        assert_eq!(
            store.get_provider_cached_plan(ProviderId::Antigravity).unwrap(),
            (None, None)
        );
    }

    #[test]
    fn sqlite_round_trip_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("test.db")).unwrap();

        let ws = store.open_workspace(dir.path()).unwrap();
        assert_eq!(store.list_workspaces().unwrap().len(), 1);

        let sess = store
            .create_session(
                &ws.id,
                "New Session",
                ProviderId::ClaudeCode,
                "sonnet",
                PermissionMode::AskSafe,
            )
            .unwrap();
        store
            .update_session_title_if_default(&sess.id, "Implement OAuth login")
            .unwrap();

        let mut turn = store
            .create_turn(
                &sess.id,
                ProviderId::ClaudeCode,
                "sonnet",
                "Implement OAuth login",
                None,
                None,
            )
            .unwrap();
        turn.response_text = "Done!".into();
        turn.status = TurnStatus::Completed;
        turn.tool_calls.push(ToolCallRecord {
            call_id: "c1".into(),
            name: "Bash".into(),
            kind: ToolKind::Shell,
            input_summary: "cargo test".into(),
            output_text: "passed".into(),
            status: "completed".into(),
            success: Some(true),
            duration_ms: Some(120),
        });
        store.finish_turn(&turn).unwrap();

        let loaded = store.get_session(&sess.id).unwrap().unwrap();
        assert_eq!(loaded.title, "Implement OAuth login");
        assert_eq!(loaded.turns.len(), 1);
        assert_eq!(loaded.turns[0].tool_calls.len(), 1);
        assert_eq!(loaded.turns[0].response_text, "Done!");
    }
}
