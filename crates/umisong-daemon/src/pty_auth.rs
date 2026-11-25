use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use umisong_core::{DaemonNotification, ProviderId};
use umisong_providers::CommandSpec;
use uuid::Uuid;

struct ActivePtySession {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>,
}

#[derive(Clone, Default)]
pub struct PtyAuthManager {
    sessions: Arc<Mutex<HashMap<String, Arc<ActivePtySession>>>>,
}

impl PtyAuthManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_session(
        &self,
        provider: ProviderId,
        spec: CommandSpec,
        cols: u16,
        rows: u16,
        events_tx: broadcast::Sender<DaemonNotification>,
        on_exit: impl FnOnce(ProviderId) + Send + 'static,
    ) -> Result<String> {
        let pty_system = native_pty_system();
        let size = PtySize {
            rows: rows.max(12),
            cols: cols.max(60),
            pixel_width: 0,
            pixel_height: 0,
        };
        let pair = pty_system
            .openpty(size)
            .context("Failed to allocate PTY for interactive login")?;

        let mut cmd = CommandBuilder::new(&spec.program);
        for arg in &spec.args {
            cmd.arg(arg);
        }
        cmd.env("TERM", "xterm-256color");
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        if let Some(cwd) = &spec.cwd {
            cmd.cwd(cwd);
        }

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("Failed to spawn login command `{}`", spec.display_command()))?;
        drop(pair.slave);

        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let session_id = Uuid::new_v4().to_string();
        let handle = Arc::new(ActivePtySession {
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            killer: Mutex::new(Some(killer)),
        });

        self.sessions
            .lock()
            .unwrap()
            .insert(session_id.clone(), Arc::clone(&handle));

        let sessions_ref = Arc::clone(&self.sessions);
        let sid_clone = session_id.clone();

        std::thread::spawn(move || {
            let mut parser = vt100::Parser::new(size.rows, size.cols, 200);
            let mut buf = [0u8; 4096];
            let mut seen_urls: HashSet<String> = HashSet::new();
            let mut ordered_urls: Vec<String> = Vec::new();
            let mut raw_accum = String::new();

            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        parser.process(chunk);
                        raw_accum.push_str(&String::from_utf8_lossy(chunk));
                        if raw_accum.len() > 32_768 {
                            let trim_at = raw_accum.len() - 16_384;
                            raw_accum = raw_accum.split_off(trim_at);
                        }
                        for url in extract_urls(&raw_accum) {
                            if seen_urls.insert(url.clone()) {
                                ordered_urls.push(url);
                            }
                        }
                        let screen_lines = extract_screen_lines(parser.screen());
                        let _ = events_tx.send(DaemonNotification::AuthTerminalOutput {
                            auth_session_id: sid_clone.clone(),
                            provider,
                            screen_lines,
                            detected_urls: ordered_urls.clone(),
                            exited: false,
                            exit_code: None,
                        });
                    }
                    Err(_) => break,
                }
            }

            let exit_code = child.wait().ok().map(|s| s.exit_code() as i32);
            let screen_lines = extract_screen_lines(parser.screen());
            let _ = events_tx.send(DaemonNotification::AuthTerminalOutput {
                auth_session_id: sid_clone.clone(),
                provider,
                screen_lines,
                detected_urls: ordered_urls,
                exited: true,
                exit_code,
            });
            sessions_ref.lock().unwrap().remove(&sid_clone);
            on_exit(provider);
        });

        Ok(session_id)
    }

    pub fn send_input(&self, auth_session_id: &str, data: &str) -> Result<()> {
        let sess = {
            let guard = self.sessions.lock().unwrap();
            guard.get(auth_session_id).cloned()
        };
        if let Some(s) = sess {
            let mut w = s.writer.lock().unwrap();
            w.write_all(data.as_bytes())?;
            w.flush()?;
        }
        Ok(())
    }

    pub fn resize(&self, auth_session_id: &str, cols: u16, rows: u16) -> Result<()> {
        let sess = {
            let guard = self.sessions.lock().unwrap();
            guard.get(auth_session_id).cloned()
        };
        if let Some(s) = sess {
            let m = s.master.lock().unwrap();
            let _ = m.resize(PtySize {
                rows: rows.max(10),
                cols: cols.max(40),
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        Ok(())
    }

    pub fn stop(&self, auth_session_id: &str) {
        let sess = self.sessions.lock().unwrap().remove(auth_session_id);
        if let Some(s) = sess {
            if let Some(mut killer) = s.killer.lock().unwrap().take() {
                let _ = killer.kill();
            }
        }
    }
}

fn extract_screen_lines(screen: &vt100::Screen) -> Vec<String> {
    let (rows, cols) = screen.size();
    let mut lines = Vec::with_capacity(rows as usize);
    for r in 0..rows {
        let row = screen.contents_between(r, 0, r, cols);
        lines.push(row.trim_end().to_string());
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// Extract clean `https://` or `http://localhost` OAuth URLs from terminal output
/// (stripping ANSI escape sequences first).
pub fn extract_urls(raw: &str) -> Vec<String> {
    let stripped = strip_ansi(raw);
    let mut out = Vec::new();
    for token in stripped.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';')
        });
        if cleaned.starts_with("https://") || cleaned.starts_with("http://localhost") || cleaned.starts_with("http://127.0.0.1") {
            let url = cleaned.trim_end_matches('.');
            if url.len() > 10 && !out.iter().any(|u| u == url) {
                out.push(url.to_string());
            }
        }
    }
    out
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for nc in chars.by_ref() {
                    if ('@'..='~').contains(&nc) {
                        break;
                    }
                }
            } else if matches!(chars.peek(), Some(']')) {
                chars.next();
                for nc in chars.by_ref() {
                    if nc == '\x07' {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_oauth_urls_from_ansi_output() {
        let sample = "\x1b[36mOpen this link:\x1b[0m https://auth.example.com/oauth/authorize?code=12345.\nFallback: (http://localhost:8976/callback)";
        let urls = extract_urls(sample);
        assert_eq!(
            urls,
            vec![
                "https://auth.example.com/oauth/authorize?code=12345",
                "http://localhost:8976/callback"
            ]
        );
    }

    #[tokio::test]
    async fn runs_interactive_pty_session() {
        let mgr = PtyAuthManager::new();
        let (tx, mut rx) = broadcast::channel(64);
        let spec = CommandSpec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "echo 'Visit https://login.test/verify'; read line; echo GOT:$line".into(),
            ],
            env: HashMap::new(),
            cwd: None,
        };
        let sid = mgr
            .start_session(ProviderId::ClaudeCode, spec, 80, 24, tx, |_| {})
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        mgr.send_input(&sid, "ok-code\n").unwrap();

        let mut saw_url = false;
        let mut saw_got = false;
        let mut saw_exit = false;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(4);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Ok(DaemonNotification::AuthTerminalOutput {
                screen_lines,
                detected_urls,
                exited,
                ..
            })) = tokio::time::timeout(std::time::Duration::from_millis(400), rx.recv()).await
            {
                if detected_urls.iter().any(|u| u == "https://login.test/verify") {
                    saw_url = true;
                }
                if screen_lines.iter().any(|l| l.contains("GOT:ok-code")) {
                    saw_got = true;
                }
                if exited {
                    saw_exit = true;
                    break;
                }
            }
        }
        assert!(saw_url, "expected OAuth URL detection");
        assert!(saw_got, "expected stdin round-trip");
        assert!(saw_exit, "expected clean PTY exit");
    }
}
