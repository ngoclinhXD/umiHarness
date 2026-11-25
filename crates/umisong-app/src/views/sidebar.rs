use crate::app::{ActiveView, UmisongApp};
use crate::client::RequestTag;
use crate::theme::{Palette, hover_factor, lerp_color, paint_status_dot, sakura_cta_button};
use egui::{CornerRadius, Margin, Order, Pos2, Rect, RichText, Stroke, Vec2};
use umisong_core::{DaemonRequest, SessionSummary};

#[derive(Debug, Clone, Default)]
struct SessionMenuState {
    open_session_id: Option<String>,
    anchor_pos: Pos2,
    menu_rect: Option<Rect>,
}

pub fn force_open_session_menu_for_preview(ctx: &egui::Context, session_id: String) {
    let menu_state_id = egui::Id::new("sidebar_session_dropdown_state");
    ctx.data_mut(|d| {
        d.insert_temp(
            menu_state_id,
            SessionMenuState {
                open_session_id: Some(session_id),
                anchor_pos: Pos2::new(260.0, 412.0),
                menu_rect: None,
            },
        );
    });
}

enum SessionRowAction {
    Open(String, bool),
    StartRename(String, String),
    TogglePin(String, bool),
    ToggleUnread(String, bool),
    ToggleArchive(String, bool),
    RequestDelete(String, String),
}

pub fn render_sidebar(app: &mut UmisongApp, ctx: &egui::Context, palette: &Palette) {
    egui::SidePanel::left("umisong_sidebar")
        .resizable(true)
        .default_width(254.0)
        .min_width(228.0)
        .max_width(340.0)
        .frame(
            egui::Frame::new()
                .fill(palette.bg_sidebar)
                .stroke(Stroke::new(1.0_f32, palette.border))
                .inner_margin(Margin::symmetric(14, 12)),
        )
        .show(ctx, |ui| {
            // Clearance for native macOS traffic lights under fullsize_content_view
            #[cfg(target_os = "macos")]
            ui.add_space(24.0);

            // Brand Logo (`assets/logos/logowhite.png` in dark mode, `logocolored.png` in light mode)
            ui.horizontal(|ui| {
                if let Some(tex) = app.logo_textures.for_theme(app.settings.dark_theme) {
                    let desired_w = (ui.available_width() - 8.0).clamp(140.0, 192.0);
                    let aspect = tex.size_vec2().y / tex.size_vec2().x.max(1.0);
                    let size = egui::vec2(desired_w, desired_w * aspect);
                    ui.add(egui::Image::new((tex.id(), size)));
                } else {
                    ui.label(
                        RichText::new("UMIKA | Harness")
                            .size(16.0)
                            .strong()
                            .color(palette.text_primary),
                    );
                }
            });

            ui.add_space(14.0);

            // Sleek Workspace Picker Card (with smooth hover border lift)
            let ws_rect_id = ui.make_persistent_id("sidebar_ws_card_rect");
            let ws_hovered = ui
                .ctx()
                .data(|d| d.get_temp::<egui::Rect>(ws_rect_id))
                .is_some_and(|r| ui.rect_contains_pointer(r));
            let ws_h = hover_factor(ui.ctx(), ui.make_persistent_id("sidebar_ws_hover"), ws_hovered);
            if ws_hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            let ws_card = egui::Frame::new()
                .fill(lerp_color(palette.bg_card, palette.bg_elevated, ws_h * 0.45))
                .stroke(Stroke::new(
                    1.0_f32,
                    lerp_color(palette.border, palette.border_strong, ws_h),
                ))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("Workspace")
                            .size(11.0)
                            .color(palette.text_muted),
                    );
                    ui.add_space(2.0);

                    let active_label = app
                        .active_workspace()
                        .map(|w| w.name.clone())
                        .unwrap_or_else(|| "Select a folder".to_string());

                    let mut pick_folder = false;
                    let mut selected_ws_id = app.active_workspace_id.clone();

                    ui.push_id("workspace_menu_btn", |ui| {
                        // Make menu trigger seamless across all states (including active/open)
                        let s = ui.style_mut();
                        for w in [
                            &mut s.visuals.widgets.inactive,
                            &mut s.visuals.widgets.hovered,
                            &mut s.visuals.widgets.active,
                            &mut s.visuals.widgets.open,
                        ] {
                            w.weak_bg_fill = egui::Color32::TRANSPARENT;
                            w.bg_fill = egui::Color32::TRANSPARENT;
                            w.bg_stroke = Stroke::NONE;
                            w.expansion = 0.0;
                        }
                        s.spacing.button_padding = egui::vec2(0.0, 2.0);

                        ui.menu_button(
                            RichText::new(format!("{active_label}   ▾"))
                                .size(13.0)
                                .strong()
                                .color(palette.text_primary),
                            |ui| {
                                ui.set_min_width(210.0);
                                if ui
                                    .button(
                                        RichText::new("Open folder…")
                                            .strong()
                                            .color(palette.accent),
                                    )
                                    .clicked()
                                {
                                    pick_folder = true;
                                    ui.close_menu();
                                }
                                if !app.workspaces.is_empty() {
                                    ui.separator();
                                    for ws in &app.workspaces {
                                        let is_cur = app.active_workspace_id.as_deref()
                                            == Some(ws.id.as_str());
                                        let lbl = match &ws.git_branch {
                                            Some(b) => format!("{}  ({b})", ws.name),
                                            None => ws.name.clone(),
                                        };
                                        if ui.selectable_label(is_cur, lbl).clicked() {
                                            selected_ws_id = Some(ws.id.clone());
                                            ui.close_menu();
                                        }
                                    }
                                }
                            },
                        );
                    });

                    if pick_folder {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            let path = folder.to_string_lossy().into_owned();
                            app.client.send(
                                RequestTag::OpenWorkspace,
                                DaemonRequest::OpenWorkspace { path },
                            );
                        }
                    } else if selected_ws_id != app.active_workspace_id {
                        if let Some(ws_id) = selected_ws_id {
                            app.select_workspace(ws_id);
                        }
                    }
                });

            ui.ctx()
                .data_mut(|d| d.insert_temp(ws_rect_id, ws_card.response.rect));

            ui.add_space(12.0);

            // Primary Sakura Pink CTA Button: "+ New chat" (hover brighten + tactile press)
            if sakura_cta_button(
                ui,
                "sidebar_new_chat_cta",
                "+ New chat",
                egui::vec2(ui.available_width(), 36.0),
                true,
                palette,
            )
            .clicked()
            {
                app.create_new_session();
            }

            ui.add_space(12.0);

            // Vertical Navigation Stack with smooth hover fill & right-aligned shortcut badges
            for (view, label, shortcut) in [
                (ActiveView::Chat, "Chat", "⌘1"),
                (ActiveView::Compare, "Vs", "⌘2"),
                (ActiveView::Diffs, "Changes", "⌘3"),
                (ActiveView::Accounts, "Accounts", "⌘4"),
            ] {
                let selected = app.active_view == view;
                let row_size = egui::vec2(ui.available_width(), 32.0);
                let (row_rect, resp) = ui.allocate_exact_size(row_size, egui::Sense::click());
                let hovered = resp.hovered();
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let h = hover_factor(ui.ctx(), ui.make_persistent_id(("nav_hover", label)), hovered);

                let bg = if selected {
                    palette.bg_elevated
                } else {
                    egui::Color32::from_rgba_unmultiplied(
                        palette.bg_elevated.r(),
                        palette.bg_elevated.g(),
                        palette.bg_elevated.b(),
                        (h * 150.0) as u8,
                    )
                };
                if bg.a() > 0 {
                    ui.painter()
                        .rect_filled(row_rect, CornerRadius::same(8), bg);
                }

                let inner = row_rect.shrink2(egui::vec2(10.0, 0.0));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(label)
                                .size(13.0)
                                .strong()
                                .color(if selected {
                                    palette.text_primary
                                } else {
                                    lerp_color(palette.text_secondary, palette.text_primary, h)
                                }),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(shortcut)
                                    .size(11.5)
                                    .color(lerp_color(palette.text_muted, palette.text_secondary, h)),
                            );
                        });
                    });
                });

                if resp.clicked() {
                    app.active_view = view;
                    if view == ActiveView::Diffs {
                        app.refresh_git_diff();
                    } else if view == ActiveView::Compare {
                        app.load_compare_runs();
                    } else if view == ActiveView::Accounts {
                        app.refresh_all_providers();
                    }
                }
            }

            ui.add_space(14.0);

            // Sessions Section
            ui.label(
                RichText::new("SESSIONS")
                    .size(10.5)
                    .strong()
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);

            let sessions_snapshot = app.sessions.clone();
            if sessions_snapshot.len() > 3 {
                ui.add(
                    egui::TextEdit::singleline(&mut app.session_search)
                        .hint_text("Filter sessions…")
                        .margin(Margin::symmetric(10, 6))
                        .desired_width(ui.available_width()),
                );
                ui.add_space(6.0);
            }

            let filter = app.session_search.trim().to_ascii_lowercase();
            let menu_state_id = egui::Id::new("sidebar_session_dropdown_state");
            let mut menu_state: SessionMenuState = ui
                .ctx()
                .data(|d| d.get_temp(menu_state_id))
                .unwrap_or_default();

            let mut row_action: Option<SessionRowAction> = None;
            let mut commit_rename: Option<(String, String)> = None;
            let mut cancel_rename = false;
            let mut clicked_menu_trigger = false;

            let active_list: Vec<&SessionSummary> = sessions_snapshot
                .iter()
                .filter(|s| {
                    !s.archived
                        && (filter.is_empty() || s.title.to_ascii_lowercase().contains(&filter))
                })
                .collect();
            let archived_list: Vec<&SessionSummary> = sessions_snapshot
                .iter()
                .filter(|s| {
                    s.archived
                        && (filter.is_empty() || s.title.to_ascii_lowercase().contains(&filter))
                })
                .collect();

            let list_height = (ui.available_height() - 12.0).max(80.0);

            egui::ScrollArea::vertical()
                .id_salt("session_history_scroll")
                .min_scrolled_height(list_height)
                .max_height(list_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if active_list.is_empty() && archived_list.is_empty() {
                        ui.label(
                            RichText::new("Nothing here yet. Start a chat and it'll show up.")
                                .size(12.0)
                                .color(palette.text_muted),
                        );
                    }

                    for s in &active_list {
                        render_session_row(
                            app,
                            ui,
                            s,
                            palette,
                            &mut menu_state,
                            &mut clicked_menu_trigger,
                            &mut row_action,
                            &mut commit_rename,
                            &mut cancel_rename,
                        );
                    }

                    if !archived_list.is_empty() {
                        ui.add_space(10.0);
                        let (arch_rect, arch_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 24.0),
                            egui::Sense::click(),
                        );
                        if arch_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let arrow = if app.show_archived_sessions { "▾" } else { "▸" };
                        let lbl = format!("{arrow}  Archived ({})", archived_list.len());
                        let g = ui.painter().layout_no_wrap(
                            lbl,
                            egui::FontId::proportional(11.0),
                            palette.text_muted,
                        );
                        ui.painter().galley(
                            Pos2::new(arch_rect.left() + 4.0, arch_rect.center().y - g.size().y * 0.5),
                            g,
                            palette.text_muted,
                        );
                        if arch_resp.clicked() {
                            app.show_archived_sessions = !app.show_archived_sessions;
                        }

                        if app.show_archived_sessions {
                            ui.add_space(2.0);
                            for s in &archived_list {
                                render_session_row(
                                    app,
                                    ui,
                                    s,
                                    palette,
                                    &mut menu_state,
                                    &mut clicked_menu_trigger,
                                    &mut row_action,
                                    &mut commit_rename,
                                    &mut cancel_rename,
                                );
                            }
                        }
                    }
                });

            // Floating Session Dropdown Menu (Rename · Pin · Mark Unread | Archive · Delete)
            if let Some(open_sid) = menu_state.open_session_id.clone() {
                let target_session = sessions_snapshot.iter().find(|s| s.id == open_sid).cloned();
                if let Some(sess) = target_session {
                    let (esc_pressed, any_clicked, pointer_pos) = ui.ctx().input(|i| {
                        (
                            i.key_pressed(egui::Key::Escape),
                            i.pointer.any_pressed(),
                            i.pointer.interact_pos(),
                        )
                    });
                    let clicked_outside = any_clicked
                        && !clicked_menu_trigger
                        && pointer_pos.is_some_and(|p| {
                            menu_state.menu_rect.is_some_and(|r| !r.contains(p))
                        });

                    if esc_pressed || clicked_outside {
                        menu_state.open_session_id = None;
                        menu_state.menu_rect = None;
                    } else {
                        let mut close_menu = false;
                        let mut new_rect: Option<Rect> = None;

                        egui::Area::new(egui::Id::new("sidebar_session_dropdown_popup"))
                            .order(Order::Foreground)
                            .fixed_pos(menu_state.anchor_pos)
                            .constrain(true)
                            .show(ui.ctx(), |ui| {
                                let frame_out = egui::Frame::new()
                                    .fill(palette.bg_card)
                                    .stroke(Stroke::new(1.0_f32, palette.border_strong))
                                    .corner_radius(CornerRadius::same(14))
                                    .inner_margin(Margin::same(6))
                                    .shadow(egui::epaint::Shadow {
                                        offset: [0, 10],
                                        blur: 28,
                                        spread: 0,
                                        color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 165),
                                    })
                                    .show(ui, |ui| {
                                        ui.set_width(184.0);
                                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);

                                        if session_menu_item(
                                            ui,
                                            ("smenu_rename", &sess.id),
                                            SessionMenuIcon::Rename,
                                            "Rename",
                                            palette.text_primary,
                                            palette,
                                        ) {
                                            row_action = Some(SessionRowAction::StartRename(
                                                sess.id.clone(),
                                                sess.title.clone(),
                                            ));
                                            close_menu = true;
                                        }

                                        if session_menu_item(
                                            ui,
                                            ("smenu_pin", &sess.id),
                                            SessionMenuIcon::Pin,
                                            if sess.pinned { "Unpin" } else { "Pin" },
                                            palette.text_primary,
                                            palette,
                                        ) {
                                            row_action = Some(SessionRowAction::TogglePin(
                                                sess.id.clone(),
                                                !sess.pinned,
                                            ));
                                            close_menu = true;
                                        }

                                        if session_menu_item(
                                            ui,
                                            ("smenu_unread", &sess.id),
                                            SessionMenuIcon::Unread,
                                            if sess.unread {
                                                "Mark Read"
                                            } else {
                                                "Mark Unread"
                                            },
                                            palette.text_primary,
                                            palette,
                                        ) {
                                            row_action = Some(SessionRowAction::ToggleUnread(
                                                sess.id.clone(),
                                                !sess.unread,
                                            ));
                                            close_menu = true;
                                        }

                                        // Clean hairline divider (no Copy / Split)
                                        ui.add_space(4.0);
                                        let (sep_r, _) = ui.allocate_exact_size(
                                            Vec2::new(ui.available_width(), 1.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().hline(
                                            sep_r.x_range(),
                                            sep_r.center().y,
                                            Stroke::new(1.0_f32, palette.border),
                                        );
                                        ui.add_space(4.0);

                                        if session_menu_item(
                                            ui,
                                            ("smenu_archive", &sess.id),
                                            SessionMenuIcon::Archive,
                                            if sess.archived { "Unarchive" } else { "Archive" },
                                            palette.text_primary,
                                            palette,
                                        ) {
                                            row_action = Some(SessionRowAction::ToggleArchive(
                                                sess.id.clone(),
                                                !sess.archived,
                                            ));
                                            close_menu = true;
                                        }

                                        if session_menu_item(
                                            ui,
                                            ("smenu_delete", &sess.id),
                                            SessionMenuIcon::Delete,
                                            "Delete",
                                            palette.danger,
                                            palette,
                                        ) {
                                            row_action = Some(SessionRowAction::RequestDelete(
                                                sess.id.clone(),
                                                sess.title.clone(),
                                            ));
                                            close_menu = true;
                                        }
                                    });
                                new_rect = Some(frame_out.response.rect);
                            });

                        if close_menu {
                            menu_state.open_session_id = None;
                            menu_state.menu_rect = None;
                        } else {
                            menu_state.menu_rect = new_rect;
                        }
                    }
                } else {
                    menu_state.open_session_id = None;
                    menu_state.menu_rect = None;
                }
            }

            ui.ctx().data_mut(|d| d.insert_temp(menu_state_id, menu_state));

            if cancel_rename {
                app.renaming_session = None;
            }
            if let Some((sid, new_title)) = commit_rename {
                app.renaming_session = None;
                let trimmed = new_title.trim().to_string();
                if !trimmed.is_empty() {
                    if let Some(s) = app.sessions.iter_mut().find(|s| s.id == sid) {
                        s.title = trimmed.clone();
                    }
                    if let Some(a) = app.active_session.as_mut() {
                        if a.id == sid {
                            a.title = trimmed.clone();
                        }
                    }
                    app.client.send(
                        RequestTag::UpdateSessionMeta,
                        DaemonRequest::UpdateSessionMeta {
                            session_id: sid,
                            title: Some(trimmed),
                            pinned: None,
                            unread: None,
                            archived: None,
                        },
                    );
                }
            }

            if let Some(act) = row_action {
                match act {
                    SessionRowAction::Open(sid, was_unread) => {
                        app.active_view = ActiveView::Chat;
                        if was_unread {
                            if let Some(s) = app.sessions.iter_mut().find(|s| s.id == sid) {
                                s.unread = false;
                            }
                            app.client.send(
                                RequestTag::UpdateSessionMeta,
                                DaemonRequest::UpdateSessionMeta {
                                    session_id: sid.clone(),
                                    title: None,
                                    pinned: None,
                                    unread: Some(false),
                                    archived: None,
                                },
                            );
                        }
                        app.client.send(
                            RequestTag::GetSession,
                            DaemonRequest::GetSession { session_id: sid },
                        );
                    }
                    SessionRowAction::StartRename(sid, cur_title) => {
                        app.renaming_session = Some((sid, cur_title));
                    }
                    SessionRowAction::TogglePin(sid, pinned) => {
                        if let Some(s) = app.sessions.iter_mut().find(|s| s.id == sid) {
                            s.pinned = pinned;
                        }
                        app.client.send(
                            RequestTag::UpdateSessionMeta,
                            DaemonRequest::UpdateSessionMeta {
                                session_id: sid,
                                title: None,
                                pinned: Some(pinned),
                                unread: None,
                                archived: None,
                            },
                        );
                    }
                    SessionRowAction::ToggleUnread(sid, unread) => {
                        if let Some(s) = app.sessions.iter_mut().find(|s| s.id == sid) {
                            s.unread = unread;
                        }
                        app.client.send(
                            RequestTag::UpdateSessionMeta,
                            DaemonRequest::UpdateSessionMeta {
                                session_id: sid,
                                title: None,
                                pinned: None,
                                unread: Some(unread),
                                archived: None,
                            },
                        );
                    }
                    SessionRowAction::ToggleArchive(sid, archived) => {
                        if let Some(s) = app.sessions.iter_mut().find(|s| s.id == sid) {
                            s.archived = archived;
                        }
                        if archived && app.active_session.as_ref().is_some_and(|a| a.id == sid) {
                            app.active_session = None;
                        }
                        app.client.send(
                            RequestTag::UpdateSessionMeta,
                            DaemonRequest::UpdateSessionMeta {
                                session_id: sid,
                                title: None,
                                pinned: None,
                                unread: None,
                                archived: Some(archived),
                            },
                        );
                    }
                    SessionRowAction::RequestDelete(sid, title) => {
                        app.pending_delete_session = Some((sid, title));
                    }
                }
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn render_session_row(
    app: &mut UmisongApp,
    ui: &mut egui::Ui,
    s: &SessionSummary,
    palette: &Palette,
    menu_state: &mut SessionMenuState,
    clicked_menu_trigger: &mut bool,
    row_action: &mut Option<SessionRowAction>,
    commit_rename: &mut Option<(String, String)>,
    cancel_rename: &mut bool,
) {
    let is_active = app.active_session.as_ref().is_some_and(|a| a.id == s.id);
    let is_menu_open = menu_state.open_session_id.as_deref() == Some(s.id.as_str());
    let is_renaming = app
        .renaming_session
        .as_ref()
        .is_some_and(|(rid, _)| rid == &s.id);
    let p_col = palette.provider_color(s.active_provider);

    let row_size = Vec2::new(ui.available_width(), 32.0);
    let (row_rect, row_resp) = ui.allocate_exact_size(row_size, egui::Sense::click());
    let row_hovered = ui.rect_contains_pointer(row_rect);

    let h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id(("sess_row_h", &s.id)),
        row_hovered || is_menu_open,
    );

    let row_bg = if is_active || is_menu_open {
        palette.bg_elevated
    } else {
        egui::Color32::from_rgba_unmultiplied(
            palette.bg_elevated.r(),
            palette.bg_elevated.g(),
            palette.bg_elevated.b(),
            (h * 195.0) as u8,
        )
    };
    if row_bg.a() > 0 {
        ui.painter()
            .rect_filled(row_rect, CornerRadius::same(8), row_bg);
    }

    // Right-click anywhere on row opens the dropdown menu cleanly
    if row_resp.secondary_clicked() {
        *clicked_menu_trigger = true;
        menu_state.open_session_id = Some(s.id.clone());
        menu_state.anchor_pos = Pos2::new(row_rect.right() + 6.0, row_rect.top());
    }

    let inner = row_rect.shrink2(Vec2::new(8.0, 0.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            paint_status_dot(ui, p_col, true);

            if is_renaming {
                if let Some((_, draft)) = app.renaming_session.as_mut() {
                    let input_id = ui.make_persistent_id(("sess_rename_input", &s.id));
                    let edit = egui::TextEdit::singleline(draft)
                        .id(input_id)
                        .desired_width(ui.available_width())
                        .margin(Margin::symmetric(6, 3));
                    let resp = ui.add(edit);
                    if !resp.has_focus() {
                        resp.request_focus();
                    }
                    let (enter, esc) = ui.ctx().input(|i| {
                        (
                            i.key_pressed(egui::Key::Enter),
                            i.key_pressed(egui::Key::Escape),
                        )
                    });
                    if esc {
                        *cancel_rename = true;
                    } else if enter || resp.lost_focus() {
                        *commit_rename = Some((s.id.clone(), draft.clone()));
                    }
                }
                return;
            }

            if s.pinned {
                let (pin_r, _) =
                    ui.allocate_exact_size(Vec2::new(11.0, 12.0), egui::Sense::hover());
                paint_mini_pin(ui.painter(), pin_r.center(), palette.text_muted);
            }

            let show_menu_btn = row_hovered || is_active || is_menu_open;
            let right_reserve = if show_menu_btn {
                26.0
            } else if s.unread {
                14.0
            } else {
                4.0
            };
            let title_w = (ui.available_width() - right_reserve).max(48.0);

            let (title_rect, title_resp) =
                ui.allocate_exact_size(Vec2::new(title_w, 28.0), egui::Sense::click());
            if title_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(title_rect), |ui| {
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let text_col = if is_active || s.unread {
                        palette.text_primary
                    } else {
                        lerp_color(palette.text_secondary, palette.text_primary, h * 0.6)
                    };
                    let mut rt = RichText::new(&s.title).size(12.5).color(text_col);
                    if s.unread {
                        rt = rt.strong();
                    }
                    ui.add(egui::Label::new(rt).truncate());
                });
            });
            if title_resp.clicked() {
                *row_action = Some(SessionRowAction::Open(s.id.clone(), s.unread));
            }
            if title_resp.secondary_clicked() {
                *clicked_menu_trigger = true;
                menu_state.open_session_id = Some(s.id.clone());
                menu_state.anchor_pos = Pos2::new(row_rect.right() + 6.0, row_rect.top());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if show_menu_btn {
                    let (btn_r, btn_resp) =
                        ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::click());
                    let b_hov = btn_resp.hovered();
                    if b_hov {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if b_hov || is_menu_open {
                        ui.painter().rect_filled(
                            btn_r,
                            CornerRadius::same(6),
                            lerp_color(palette.bg_elevated, palette.border_strong, 0.45),
                        );
                    }
                    paint_vertical_dots(
                        ui.painter(),
                        btn_r.center(),
                        if b_hov || is_menu_open {
                            palette.text_primary
                        } else {
                            palette.text_muted
                        },
                    );
                    if btn_resp.on_hover_text("Session actions").clicked() {
                        *clicked_menu_trigger = true;
                        if is_menu_open {
                            menu_state.open_session_id = None;
                            menu_state.menu_rect = None;
                        } else {
                            menu_state.open_session_id = Some(s.id.clone());
                            menu_state.anchor_pos =
                                Pos2::new(row_rect.right() + 6.0, row_rect.top());
                        }
                    }
                } else if s.unread {
                    let (dot_r, _) =
                        ui.allocate_exact_size(Vec2::new(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(dot_r.center(), 3.2, palette.accent);
                }
            });
        });
    });
}

#[derive(Clone, Copy)]
enum SessionMenuIcon {
    Rename,
    Pin,
    Unread,
    Archive,
    Delete,
}

fn session_menu_item(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    icon: SessionMenuIcon,
    label: &str,
    fg: egui::Color32,
    palette: &Palette,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), egui::Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let h = hover_factor(ui.ctx(), ui.make_persistent_id(id_salt), hovered);
    if h > 0.01 {
        let bg = egui::Color32::from_rgba_unmultiplied(
            palette.bg_elevated.r(),
            palette.bg_elevated.g(),
            palette.bg_elevated.b(),
            (h * 255.0) as u8,
        );
        ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    }

    let inner = rect.shrink2(Vec2::new(10.0, 0.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (icon_rect, _) =
                ui.allocate_exact_size(Vec2::new(15.0, 15.0), egui::Sense::hover());
            let icon_col = if matches!(icon, SessionMenuIcon::Delete) {
                fg
            } else {
                lerp_color(palette.text_secondary, fg, h)
            };
            paint_session_menu_icon(ui.painter(), icon_rect.center(), icon, icon_col);
            ui.label(RichText::new(label).size(13.0).color(fg));
        });
    });

    resp.clicked()
}

fn paint_vertical_dots(painter: &egui::Painter, c: Pos2, color: egui::Color32) {
    for dy in [-4.5_f32, 0.0, 4.5] {
        painter.circle_filled(Pos2::new(c.x, c.y + dy), 1.35, color);
    }
}

fn paint_mini_pin(painter: &egui::Painter, c: Pos2, color: egui::Color32) {
    let s = Stroke::new(1.1_f32, color);
    painter.line_segment([Pos2::new(c.x - 2.5, c.y - 3.5), Pos2::new(c.x + 2.5, c.y - 3.5)], s);
    painter.line_segment([Pos2::new(c.x - 1.5, c.y - 3.5), Pos2::new(c.x - 2.2, c.y + 0.5)], s);
    painter.line_segment([Pos2::new(c.x + 1.5, c.y - 3.5), Pos2::new(c.x + 2.2, c.y + 0.5)], s);
    painter.line_segment([Pos2::new(c.x - 3.2, c.y + 0.5), Pos2::new(c.x + 3.2, c.y + 0.5)], s);
    painter.line_segment([Pos2::new(c.x, c.y + 0.5), Pos2::new(c.x, c.y + 4.5)], s);
}

fn paint_session_menu_icon(
    painter: &egui::Painter,
    c: Pos2,
    icon: SessionMenuIcon,
    color: egui::Color32,
) {
    let s = Stroke::new(1.3_f32, color);
    match icon {
        SessionMenuIcon::Rename => {
            // Diagonal pencil
            painter.line_segment([Pos2::new(c.x - 4.5, c.y + 4.5), Pos2::new(c.x + 3.5, c.y - 3.5)], s);
            painter.line_segment([Pos2::new(c.x - 2.0, c.y + 5.0), Pos2::new(c.x + 5.0, c.y - 2.0)], s);
            painter.line_segment([Pos2::new(c.x + 2.0, c.y - 5.0), Pos2::new(c.x + 5.0, c.y - 2.0)], s);
            painter.line_segment([Pos2::new(c.x - 5.0, c.y + 5.0), Pos2::new(c.x - 4.5, c.y + 2.5)], s);
            painter.line_segment([Pos2::new(c.x - 5.0, c.y + 5.0), Pos2::new(c.x - 2.5, c.y + 4.5)], s);
        }
        SessionMenuIcon::Pin => {
            paint_mini_pin(painter, c, color);
        }
        SessionMenuIcon::Unread => {
            // Chat bubble with unread dot at top-right
            let r = Rect::from_center_size(Pos2::new(c.x, c.y + 0.5), Vec2::new(11.0, 9.0));
            painter.rect_stroke(r, CornerRadius::same(2), s, egui::StrokeKind::Inside);
            painter.line_segment([Pos2::new(c.x - 2.5, c.y + 5.0), Pos2::new(c.x - 4.5, c.y + 6.5)], s);
            painter.circle_filled(Pos2::new(c.x + 4.5, c.y - 4.0), 2.1, color);
        }
        SessionMenuIcon::Archive => {
            // Archive tray with downward arrow
            let lid = Rect::from_center_size(Pos2::new(c.x, c.y - 3.8), Vec2::new(12.0, 3.0));
            let body = Rect::from_center_size(Pos2::new(c.x, c.y + 1.5), Vec2::new(10.5, 7.5));
            painter.rect_stroke(lid, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.rect_stroke(body, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.line_segment([Pos2::new(c.x, c.y - 0.5), Pos2::new(c.x, c.y + 3.5)], s);
            painter.line_segment([Pos2::new(c.x - 2.0, c.y + 1.5), Pos2::new(c.x, c.y + 3.5)], s);
            painter.line_segment([Pos2::new(c.x + 2.0, c.y + 1.5), Pos2::new(c.x, c.y + 3.5)], s);
        }
        SessionMenuIcon::Delete => {
            // Clean trash can
            painter.line_segment([Pos2::new(c.x - 5.5, c.y - 3.5), Pos2::new(c.x + 5.5, c.y - 3.5)], s);
            painter.line_segment([Pos2::new(c.x - 2.0, c.y - 5.5), Pos2::new(c.x + 2.0, c.y - 5.5)], s);
            let can = Rect::from_center_size(Pos2::new(c.x, c.y + 1.2), Vec2::new(8.5, 9.0));
            painter.rect_stroke(can, CornerRadius::same(1), s, egui::StrokeKind::Inside);
            painter.line_segment([Pos2::new(c.x - 1.6, c.y - 1.2), Pos2::new(c.x - 1.6, c.y + 3.6)], s);
            painter.line_segment([Pos2::new(c.x + 1.6, c.y - 1.2), Pos2::new(c.x + 1.6, c.y + 3.6)], s);
        }
    }
}
