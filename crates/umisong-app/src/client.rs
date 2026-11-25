use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc as tokio_mpsc;
use umisong_core::{DaemonNotification, DaemonRequest, DaemonResponse, RpcEnvelope, ServerMessage};
use umisong_daemon::DaemonConfig;

#[derive(Debug, Clone)]
pub enum RequestTag {
    Bootstrap,
    RefreshProviders,
    InstallProviders,
    SaveSettings,
    SaveProviderConfig,
    StartAuth,
    OpenWorkspace,
    LoadSessions,
    CreateSession,
    GetSession,
    DeleteSession,
    UpdateSessionMeta,
    SendTurn,
    Handoff,
    LoadCompares,
    StartCompare,
    AdoptCompare,
    GetDiff,
    RevertFile,
    Silent,
}

#[derive(Debug)]
pub enum InboundMessage {
    Connected { embedded: bool },
    ConnectionError(String),
    Response {
        tag: RequestTag,
        result: Result<DaemonResponse, String>,
    },
    Notification(DaemonNotification),
}

pub struct DaemonClient {
    outbound_tx: tokio_mpsc::UnboundedSender<(u64, DaemonRequest)>,
    inbound_rx: std_mpsc::Receiver<InboundMessage>,
    next_id: AtomicU64,
    pending_tags: Arc<Mutex<HashMap<u64, RequestTag>>>,
}

impl DaemonClient {
    pub fn connect(ctx: egui::Context) -> Self {
        let (outbound_tx, mut outbound_rx) = tokio_mpsc::unbounded_channel::<(u64, DaemonRequest)>();
        let (inbound_tx, inbound_rx) = std_mpsc::channel::<InboundMessage>();
        let pending_tags: Arc<Mutex<HashMap<u64, RequestTag>>> = Arc::new(Mutex::new(HashMap::new()));
        let tags_bg = Arc::clone(&pending_tags);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime for daemon client");

            rt.block_on(async move {
                let cfg = DaemonConfig::default();
                let mut embedded = false;

                // Connect to an already-running `umisongd`, or auto-boot it in-process.
                let stream = match UnixStream::connect(&cfg.socket_path).await {
                    Ok(s) => s,
                    Err(_) => {
                        embedded = true;
                        let cfg_clone = cfg.clone();
                        tokio::spawn(async move {
                            if let Err(err) = umisong_daemon::run_daemon(cfg_clone).await {
                                tracing::error!("Embedded daemon exited with error: {err}");
                            }
                        });

                        let mut connected = None;
                        for _ in 0..30 {
                            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                            if let Ok(s) = UnixStream::connect(&cfg.socket_path).await {
                                connected = Some(s);
                                break;
                            }
                        }
                        match connected {
                            Some(s) => s,
                            None => {
                                // Fallback to an anonymous in-process UnixStream pair if named sockets are restricted
                                match (umisong_daemon::server::DaemonState::new(cfg.clone()), UnixStream::pair()) {
                                    (Ok(state), Ok((client_end, server_end))) => {
                                        tokio::spawn(async move {
                                            let _ = umisong_daemon::server::handle_client(server_end, state).await;
                                        });
                                        client_end
                                    }
                                    _ => {
                                        let _ = inbound_tx.send(InboundMessage::ConnectionError(
                                            format!(
                                                "Could not connect to daemon socket at {}",
                                                cfg.socket_path.display()
                                            ),
                                        ));
                                        ctx.request_repaint();
                                        return;
                                    }
                                }
                            }
                        }
                    }
                };

                let _ = inbound_tx.send(InboundMessage::Connected { embedded });
                ctx.request_repaint();

                let (reader, mut writer) = stream.into_split();

                // Outbound writer task
                let write_task = tokio::spawn(async move {
                    while let Some((id, request)) = outbound_rx.recv().await {
                        let env = RpcEnvelope { id, request };
                        if let Ok(json) = serde_json::to_string(&env) {
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
                    }
                });

                // Inbound reader loop (coalesces streaming repaints to ~60fps so UI stays buttery)
                let mut lines = BufReader::new(reader).lines();
                let mut last_repaint = std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_millis(50))
                    .unwrap_or_else(std::time::Instant::now);

                while let Ok(Some(line)) = lines.next_line().await {
                    let msg = match serde_json::from_str::<ServerMessage>(line.trim()) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!("Failed to decode daemon message: {e}");
                            continue;
                        }
                    };
                    match msg {
                        ServerMessage::Response { id, result } => {
                            let tag = tags_bg
                                .lock()
                                .unwrap()
                                .remove(&id)
                                .unwrap_or(RequestTag::Silent);
                            let _ = inbound_tx.send(InboundMessage::Response { tag, result });
                            last_repaint = std::time::Instant::now();
                            ctx.request_repaint();
                        }
                        ServerMessage::Notification(n) => {
                            let urgent = matches!(
                                &n,
                                DaemonNotification::TurnEvent {
                                    event: umisong_core::AgentEvent::TurnCompleted { .. }
                                        | umisong_core::AgentEvent::Error { .. }
                                        | umisong_core::AgentEvent::ApprovalRequested { .. },
                                    ..
                                } | DaemonNotification::CompareColumnFinished { .. }
                                    | DaemonNotification::ProviderStatusUpdated(_)
                                    | DaemonNotification::ProviderInstallUpdated { .. }
                            );
                            let _ = inbound_tx.send(InboundMessage::Notification(n));
                            let now = std::time::Instant::now();
                            if urgent
                                || now.duration_since(last_repaint)
                                    >= std::time::Duration::from_millis(16)
                            {
                                last_repaint = now;
                                ctx.request_repaint();
                            } else {
                                ctx.request_repaint_after(std::time::Duration::from_millis(16));
                            }
                        }
                    }
                }

                write_task.abort();
                let _ = inbound_tx.send(InboundMessage::ConnectionError(
                    "Disconnected from umisongd daemon".to_string(),
                ));
                ctx.request_repaint();
            });
        });

        Self {
            outbound_tx,
            inbound_rx,
            next_id: AtomicU64::new(1),
            pending_tags,
        }
    }

    pub fn send(&self, tag: RequestTag, request: DaemonRequest) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pending_tags.lock().unwrap().insert(id, tag);
        let _ = self.outbound_tx.send((id, request));
    }

    pub fn drain(&self) -> Vec<InboundMessage> {
        let mut msgs = Vec::new();
        while let Ok(m) = self.inbound_rx.try_recv() {
            msgs.push(m);
        }
        msgs
    }

    pub fn disconnected_offline() -> Self {
        let (outbound_tx, _outbound_rx) = tokio_mpsc::unbounded_channel();
        let (_inbound_tx, inbound_rx) = std_mpsc::channel();
        Self {
            outbound_tx,
            inbound_rx,
            next_id: AtomicU64::new(1),
            pending_tags: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}
