use crate::app::UmisongApp;
use crate::client::RequestTag;
use crate::theme::{Palette, hover_factor, lerp_color, paint_status_dot, sakura_cta_button};
use egui::{CornerRadius, Margin, RichText, Stroke};
use umisong_core::{
    AuthLoginMode, AuthState, DaemonRequest, ProviderId, ProviderInstallState,
};

const ACCOUNTS_MAX_WIDTH: f32 = 720.0;

pub fn render_accounts_view(app: &mut UmisongApp, ui: &mut egui::Ui, palette: &Palette) {
    // Top Header Row
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new("Accounts")
                    .size(22.0)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Sign in to any provider you use. Each one signs in through its official CLI and uses the models in your plan.",
                    )
                    .size(12.5)
                    .color(palette.text_secondary),
                )
                .wrap(),
            );
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            let refresh_btn = egui::Button::new(
                RichText::new("Refresh all")
                    .size(12.5)
                    .color(palette.text_primary),
            )
            .fill(palette.bg_card)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(9));
            if ui.add_sized([88.0, 32.0], refresh_btn).clicked() {
                app.refresh_all_providers();
            }

            let theme_lbl = if app.settings.dark_theme {
                "Light theme"
            } else {
                "Dark theme"
            };
            let theme_btn = egui::Button::new(
                RichText::new(theme_lbl)
                    .size(12.5)
                    .color(palette.text_primary),
            )
            .fill(palette.bg_card)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(9));
            if ui.add_sized([92.0, 32.0], theme_btn).clicked() {
                app.settings.dark_theme = !app.settings.dark_theme;
                app.save_app_settings();
            }

            if ui
                .checkbox(&mut app.settings.auto_install_clis, "Auto-install CLIs")
                .on_hover_text(
                    "Automatically install missing provider CLIs when Umisong Harness opens",
                )
                .changed()
            {
                app.save_app_settings();
                if app.settings.auto_install_clis {
                    app.install_missing_providers();
                }
            }
        });
    });

    ui.add_space(18.0);

    let card_w = ui.available_width().min(ACCOUNTS_MAX_WIDTH);

    egui::ScrollArea::vertical()
        .id_salt("accounts_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(card_w);
                for pid in ProviderId::ALL {
                    render_provider_card(app, ui, pid, card_w, palette);
                    ui.add_space(12.0);
                }

                // Embedded Interactive PTY Auth Drawer (when active)
                if let Some(auth_ui) = app.auth_terminal.clone() {
                    ui.add_space(4.0);
                    render_auth_terminal_drawer(app, ui, &auth_ui, card_w, palette);
                }
            });
        });
}

fn render_provider_card(
    app: &mut UmisongApp,
    ui: &mut egui::Ui,
    pid: ProviderId,
    card_w: f32,
    palette: &Palette,
) {
    let p_col = palette.provider_color(pid);
    let status = app.providers.get(&pid).cloned();
    let draft = app.provider_drafts.entry(pid).or_default().clone();
    let is_signed_in = status
        .as_ref()
        .is_some_and(|s| matches!(s.auth, AuthState::Authenticated { .. }));
    let is_installed = status.as_ref().is_some_and(|s| s.installed);
    let install_state = app
        .provider_install_states
        .get(&pid)
        .cloned()
        .unwrap_or_default();

    let card_rect_id = ui.make_persistent_id(("acct_card_rect", pid.as_str()));
    let card_hovered = ui
        .ctx()
        .data(|d| d.get_temp::<egui::Rect>(card_rect_id))
        .is_some_and(|r| ui.rect_contains_pointer(r));
    let card_h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id(("acct_card_hover", pid.as_str())),
        card_hovered,
    );

    let frame_out = egui::Frame::new()
        .fill(palette.bg_card)
        .stroke(Stroke::new(
            1.0_f32,
            lerp_color(palette.border, palette.border_strong, card_h),
        ))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_width((card_w - 36.0).max(280.0));

            // Top Summary Row
            ui.horizontal(|ui| {
                // Left: Status ring/dot + Title + Plan description + CLI version
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        paint_status_dot(ui, p_col, is_signed_in);
                        ui.label(
                            RichText::new(pid.display_name())
                                .size(14.5)
                                .strong()
                                .color(palette.text_primary),
                        );
                    });

                    ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 3.0;
                            let plan_label = status.as_ref().and_then(|s| s.active_plan_label());
                            if let (Some(st), Some(plan_label)) = (status.as_ref(), plan_label) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 5.0;
                                    ui.label(
                                        RichText::new(&plan_label)
                                            .size(12.5)
                                            .strong()
                                            .color(palette.text_primary),
                                    );
                                    if st.has_manual_plan_override() {
                                        ui.label(
                                            RichText::new("·  Manual")
                                                .size(11.5)
                                                .color(palette.text_muted),
                                        )
                                        .on_hover_text("Plan tier overridden in Advanced settings");
                                    }
                                    if let Some(acct) = st.account_hint() {
                                        ui.label(
                                            RichText::new(format!("·  {acct}"))
                                                .size(12.0)
                                                .color(palette.text_muted),
                                        );
                                    }
                                });
                            } else {
                                ui.label(
                                    RichText::new(pid.plan_description())
                                        .size(12.0)
                                        .color(palette.text_secondary),
                                );
                            }

                            ui.horizontal(|ui| {
                                if let Some(st) = &status {
                                    if st.installed {
                                        let ver_str = st
                                            .version
                                            .as_deref()
                                            .map(|v| format!("{}  ·  {v}", pid.cli_binary_name()))
                                            .unwrap_or_else(|| pid.cli_binary_name().to_string());
                                        ui.label(
                                            RichText::new(ver_str)
                                                .monospace()
                                                .size(11.0)
                                                .color(palette.text_muted),
                                        );
                                    } else {
                                        match &install_state {
                                            ProviderInstallState::Installing { detail, .. } => {
                                                ui.add(egui::Spinner::new().size(11.0));
                                                ui.label(
                                                    RichText::new(detail)
                                                        .size(11.5)
                                                        .color(palette.accent),
                                                );
                                            }
                                            ProviderInstallState::Failed { message } => {
                                                let preview: String =
                                                    message.chars().take(64).collect();
                                                let suffix = if message.chars().count() > 64 {
                                                    "…"
                                                } else {
                                                    ""
                                                };
                                                ui.label(
                                                    RichText::new(format!(
                                                        "Install failed: {preview}{suffix}"
                                                    ))
                                                    .size(11.0)
                                                    .color(palette.danger),
                                                )
                                                .on_hover_text(message);
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("Copy install cmd")
                                                                .size(11.0)
                                                                .color(palette.accent),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .on_hover_text(pid.install_hint())
                                                    .clicked()
                                                {
                                                    ui.ctx().copy_text(
                                                        pid.install_hint().to_string(),
                                                    );
                                                    app.toast = Some((
                                                        format!(
                                                            "Copied install command for {}",
                                                            pid.display_name()
                                                        ),
                                                        false,
                                                    ));
                                                }
                                            }
                                            _ => {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{}  ·  not installed",
                                                        pid.cli_binary_name()
                                                    ))
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(palette.danger),
                                                );
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("Copy install cmd")
                                                                .size(11.0)
                                                                .color(palette.accent),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .on_hover_text(pid.install_hint())
                                                    .clicked()
                                                {
                                                    ui.ctx().copy_text(
                                                        pid.install_hint().to_string(),
                                                    );
                                                    app.toast = Some((
                                                        format!(
                                                            "Copied install command for {}",
                                                            pid.display_name()
                                                        ),
                                                        false,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    ui.label(
                                        RichText::new("checking…")
                                            .monospace()
                                            .size(11.0)
                                            .color(palette.text_muted),
                                    );
                                }
                            });
                        });
                    });
                });

                // Right: Status Badge + Primary Action Button(s)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    if is_signed_in {
                        let sign_out_btn = egui::Button::new(
                            RichText::new("Sign out")
                                .size(12.5)
                                .color(palette.text_primary),
                        )
                        .fill(palette.bg_elevated)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(CornerRadius::same(8));
                        if ui.add_sized([76.0, 32.0], sign_out_btn).clicked() {
                            app.client.send(
                                RequestTag::RefreshProviders,
                                DaemonRequest::LogoutProvider { provider: pid },
                            );
                        }
                    } else if !is_installed {
                        match &install_state {
                            ProviderInstallState::Installing { .. } => {
                                sakura_cta_button(
                                    ui,
                                    ("acct_installing", pid.as_str()),
                                    "Installing…",
                                    egui::vec2(94.0, 32.0),
                                    false,
                                    palette,
                                );
                            }
                            ProviderInstallState::Failed { .. } => {
                                if sakura_cta_button(
                                    ui,
                                    ("acct_retry_install", pid.as_str()),
                                    "Retry install",
                                    egui::vec2(96.0, 32.0),
                                    true,
                                    palette,
                                )
                                .on_hover_text(pid.install_hint())
                                .clicked()
                                {
                                    app.install_provider_cli(pid);
                                }
                            }
                            _ => {
                                if sakura_cta_button(
                                    ui,
                                    ("acct_install_cli", pid.as_str()),
                                    "Install CLI",
                                    egui::vec2(90.0, 32.0),
                                    true,
                                    palette,
                                )
                                .on_hover_text(pid.install_hint())
                                .clicked()
                                {
                                    app.install_provider_cli(pid);
                                }
                            }
                        }
                    } else {
                        if sakura_cta_button(
                            ui,
                            ("acct_sign_in", pid.as_str()),
                            "Sign in",
                            egui::vec2(74.0, 32.0),
                            is_installed,
                            palette,
                        )
                        .on_disabled_hover_text(pid.install_hint())
                        .clicked()
                        {
                            app.start_auth_login(pid, AuthLoginMode::BrowserOAuth);
                        }

                        if pid == ProviderId::Codex {
                            let dev_btn = egui::Button::new(
                                RichText::new("Use device code")
                                    .size(12.0)
                                    .color(if is_installed {
                                        palette.text_primary
                                    } else {
                                        palette.text_muted
                                    }),
                            )
                            .fill(palette.bg_elevated)
                            .stroke(Stroke::new(1.0_f32, palette.border_strong))
                            .corner_radius(CornerRadius::same(8));
                            if ui
                                .add_enabled(
                                    is_installed,
                                    dev_btn.min_size(egui::vec2(114.0, 32.0)),
                                )
                                .on_disabled_hover_text(pid.install_hint())
                                .clicked()
                            {
                                app.start_auth_login(pid, AuthLoginMode::DeviceCode);
                            }
                        }
                    }

                    // Compact Status Pill ("Signed in" / "Signed out" / "Installing…" / "Install failed" / "Not installed")
                    let (pill_text, pill_bg, pill_fg) = match status.as_ref().map(|s| &s.auth) {
                        Some(AuthState::Authenticated { .. }) => (
                            "Signed in",
                            palette.badge_signed_in_bg,
                            palette.badge_signed_in_fg,
                        ),
                        Some(AuthState::NotInstalled) => match &install_state {
                            ProviderInstallState::Installing { .. } => (
                                "Installing…",
                                palette.accent_soft_bg,
                                palette.accent_soft_fg,
                            ),
                            ProviderInstallState::Failed { .. } => (
                                "Install failed",
                                egui::Color32::from_rgba_unmultiplied(
                                    palette.danger.r(),
                                    palette.danger.g(),
                                    palette.danger.b(),
                                    28,
                                ),
                                palette.danger,
                            ),
                            _ => (
                                "Not installed",
                                egui::Color32::from_rgba_unmultiplied(
                                    palette.danger.r(),
                                    palette.danger.g(),
                                    palette.danger.b(),
                                    28,
                                ),
                                palette.danger,
                            ),
                        },
                        Some(AuthState::LoggedOut) => (
                            "Signed out",
                            palette.badge_signed_out_bg,
                            palette.badge_signed_out_fg,
                        ),
                        Some(AuthState::Unknown { .. }) => (
                            "Check auth",
                            palette.bg_elevated,
                            palette.text_muted,
                        ),
                        None => ("Checking", palette.bg_elevated, palette.text_muted),
                    };
                    status_pill(ui, pill_text, pill_bg, pill_fg);
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Collapsible "Advanced ▾ / ▴" disclosure
            let adv_open = app.accounts_advanced_open.entry(pid).or_insert(false);
            let arrow = if *adv_open { "▴" } else { "▾" };
            let toggle = ui.add(
                egui::Button::new(
                    RichText::new(format!("Advanced  {arrow}"))
                        .size(12.0)
                        .color(palette.text_secondary),
                )
                .frame(false),
            );
            if toggle.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if toggle.clicked() {
                *adv_open = !*adv_open;
            }
            let expanded = *adv_open;

            if expanded {
                ui.add_space(12.0);
                let mut next_draft = draft.clone();
                let inner_w = ui.available_width();

                // CLI path (full width)
                ui.label(
                    RichText::new("CLI path")
                        .size(11.5)
                        .color(palette.text_secondary),
                );
                ui.add_space(4.0);
                ui.add_sized(
                    [inner_w, 32.0],
                    egui::TextEdit::singleline(&mut next_draft.binary_override)
                        .hint_text(format!("/path/to/{}", pid.cli_binary_name()))
                        .margin(Margin::symmetric(10, 7)),
                );

                if pid != ProviderId::Antigravity {
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("API key override")
                            .size(11.5)
                            .color(palette.text_secondary),
                    );
                    ui.add_space(4.0);
                    ui.add_sized(
                        [inner_w, 32.0],
                        egui::TextEdit::singleline(&mut next_draft.api_key_override)
                            .password(true)
                            .hint_text("Blank uses your subscription login")
                            .margin(Margin::symmetric(10, 7)),
                    );
                }

                ui.add_space(12.0);

                // Bottom row: "Having trouble? Re-authenticate" link + "Save" button
                ui.horizontal(|ui| {
                    if is_installed {
                        let reauth = ui.add(
                            egui::Button::new(
                                RichText::new("Having trouble? Re-authenticate")
                                    .size(12.0)
                                    .color(palette.accent),
                            )
                            .frame(false),
                        );
                        if reauth.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if reauth.clicked() {
                            app.start_auth_login(pid, AuthLoginMode::BrowserOAuth);
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let save_btn = egui::Button::new(
                            RichText::new("Save")
                                .size(12.5)
                                .strong()
                                .color(palette.text_primary),
                        )
                        .fill(palette.bg_elevated)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(CornerRadius::same(8));
                        if ui.add_sized([68.0, 32.0], save_btn).clicked() {
                            let api_key_override = if pid == ProviderId::Antigravity {
                                None
                            } else {
                                (!next_draft.api_key_override.trim().is_empty())
                                    .then(|| next_draft.api_key_override.trim().to_string())
                            };
                            app.client.send(
                                RequestTag::SaveProviderConfig,
                                DaemonRequest::UpdateProviderConfig {
                                    provider: pid,
                                    binary_override: (!next_draft.binary_override.trim().is_empty())
                                        .then(|| next_draft.binary_override.trim().to_string()),
                                    default_model: next_draft.default_model.clone(),
                                    custom_models: next_draft.custom_models.clone(),
                                    api_key_override,
                                    plan_tier_override: None,
                                },
                            );
                        }
                    });
                });

                app.provider_drafts.insert(pid, next_draft);
            }
        });

    ui.ctx()
        .data_mut(|d| d.insert_temp(card_rect_id, frame_out.response.rect));
}

fn status_pill(ui: &mut egui::Ui, label: &str, bg: egui::Color32, fg: egui::Color32) {
    let pill = egui::Button::new(RichText::new(label).size(11.5).strong().color(fg))
        .fill(bg)
        .stroke(Stroke::new(
            1.0_f32,
            egui::Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 60),
        ))
        .corner_radius(CornerRadius::same(255))
        .sense(egui::Sense::hover());

    ui.add(pill.min_size(egui::vec2(0.0, 24.0)));
}

fn render_auth_terminal_drawer(
    app: &mut UmisongApp,
    ui: &mut egui::Ui,
    auth: &crate::app::AuthTerminalUi,
    card_w: f32,
    palette: &Palette,
) {
    let accent = palette.provider_color(auth.provider);
    let inner_w = (card_w - 36.0).max(280.0);
    egui::Frame::new()
        .fill(palette.bg_card)
        .stroke(Stroke::new(1.5_f32, accent))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        RichText::new(format!(
                            "Interactive Login — {}",
                            auth.provider.display_name()
                        ))
                        .size(14.0)
                        .strong()
                        .color(accent),
                    );
                    ui.label(
                        RichText::new(&auth.command)
                            .monospace()
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let close_btn = egui::Button::new(
                        RichText::new("Close")
                            .size(12.0)
                            .color(palette.text_primary),
                    )
                    .fill(palette.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, palette.border_strong))
                    .corner_radius(CornerRadius::same(8));
                    let close_resp = ui.add_sized([64.0, 28.0], close_btn);
                    if close_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if close_resp.clicked() {
                        app.client.send(
                            RequestTag::Silent,
                            DaemonRequest::StopAuthTerminal {
                                auth_session_id: auth.auth_session_id.clone(),
                            },
                        );
                        app.auth_terminal = None;
                        app.refresh_all_providers();
                    }
                    if !auth.exited {
                        let ctrl_c_btn = egui::Button::new(
                            RichText::new("Ctrl+C")
                                .size(12.0)
                                .color(palette.text_primary),
                        )
                        .fill(palette.bg_elevated)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(CornerRadius::same(8));
                        let ctrl_c_resp = ui.add_sized([64.0, 28.0], ctrl_c_btn);
                        if ctrl_c_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if ctrl_c_resp.clicked() {
                            app.client.send(
                                RequestTag::Silent,
                                DaemonRequest::SendAuthInput {
                                    auth_session_id: auth.auth_session_id.clone(),
                                    data: "\x03".to_string(),
                                },
                            );
                        }
                    }
                });
            });

            if !auth.detected_urls.is_empty() {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new("Login URL:")
                            .size(12.0)
                            .strong()
                            .color(palette.success),
                    );
                    for (idx, url) in auth.detected_urls.iter().enumerate() {
                        if sakura_cta_button(
                            ui,
                            ("auth_open_url", idx),
                            "Open in Browser",
                            egui::vec2(126.0, 30.0),
                            true,
                            palette,
                        )
                        .clicked()
                        {
                            let _ = open::that(url);
                        }
                        let copy_btn = egui::Button::new(
                            RichText::new("Copy URL")
                                .size(12.0)
                                .color(palette.text_primary),
                        )
                        .fill(palette.bg_elevated)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(CornerRadius::same(8));
                        let copy_resp = ui.add_sized([82.0, 30.0], copy_btn);
                        if copy_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if copy_resp.clicked() {
                            ui.ctx().copy_text(url.clone());
                            app.toast = Some(("Copied login URL".to_string(), false));
                        }
                    }
                });
            }

            ui.add_space(10.0);
            egui::Frame::new()
                .fill(palette.bg_code)
                .stroke(Stroke::new(1.0_f32, palette.border))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    ui.set_width((inner_w - 24.0).max(200.0));
                    egui::ScrollArea::vertical()
                        .id_salt("auth_pty_scroll")
                        .max_height(220.0)
                        .auto_shrink([false, true])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            let content = if auth.screen_lines.is_empty() {
                                "Starting interactive authentication flow…".to_string()
                            } else {
                                auth.screen_lines.join("\n")
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(content)
                                        .monospace()
                                        .size(12.0)
                                        .color(palette.text_primary),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        });
                });

            ui.add_space(10.0);
            if auth.exited {
                ui.label(
                    RichText::new(format!(
                        "Login finished (exit code {:?}). Status refreshed automatically.",
                        auth.exit_code
                    ))
                    .size(12.0)
                    .color(palette.success),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let send_w = 68.0;
                    let input_w = (inner_w - send_w - 8.0).max(120.0);
                    let resp = ui.add_sized(
                        [input_w, 32.0],
                        egui::TextEdit::singleline(&mut app.auth_input_line)
                            .hint_text("Type input or code for CLI prompt and press Enter…")
                            .margin(Margin::symmetric(10, 7)),
                    );
                    let send_btn = egui::Button::new(
                        RichText::new("Send")
                            .size(12.5)
                            .strong()
                            .color(palette.text_primary),
                    )
                    .fill(palette.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, palette.border_strong))
                    .corner_radius(CornerRadius::same(8));
                    let send_resp = ui.add_sized([send_w, 32.0], send_btn);
                    if send_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let send_clicked = send_resp.clicked()
                        || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if send_clicked {
                        let line = format!("{}\n", app.auth_input_line);
                        app.auth_input_line.clear();
                        app.client.send(
                            RequestTag::Silent,
                            DaemonRequest::SendAuthInput {
                                auth_session_id: auth.auth_session_id.clone(),
                                data: line,
                            },
                        );
                    }
                });
            }
        });
}
