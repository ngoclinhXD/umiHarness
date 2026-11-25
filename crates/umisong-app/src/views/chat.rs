use crate::app::{ActiveView, HandoffModalState, UmisongApp};
use crate::client::RequestTag;
use crate::theme::{
    Palette, consume_enter_to_send, hover_factor, lerp_color, pill_badge, sakura_cta_button,
};
use crate::widgets::{
    markdown,
    model_picker::{ModelPickerAction, render_unified_model_picker},
    tool_card,
};
use egui::{CornerRadius, Margin, RichText, Stroke};
use umisong_core::{DaemonRequest, PermissionMode, TurnRecord, TurnStatus};

const CONTENT_MAX_WIDTH: f32 = 760.0;

pub fn render_chat_view(app: &mut UmisongApp, ui: &mut egui::Ui, palette: &Palette) {
    let catalog = app.unified_catalog();
    let any_provider_ready = !catalog.is_empty();

    // Minimal Top Bar: [ Plan | Safe | Auto ] segmented control on left, `Hand off to…` on right
    ui.horizontal(|ui| {
        egui::Frame::new()
            .fill(palette.bg_card)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(4))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for mode in PermissionMode::ALL {
                    let selected = app.selected_permission == mode;
                    let (seg_rect, seg_resp) =
                        ui.allocate_exact_size(egui::vec2(54.0, 26.0), egui::Sense::click());
                    let hovered = seg_resp.hovered();
                    if hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let h = hover_factor(
                        ui.ctx(),
                        ui.make_persistent_id(("perm_seg", mode.as_str())),
                        hovered,
                    );
                    let fill = if selected {
                        palette.accent_soft_bg
                    } else {
                        egui::Color32::from_rgba_unmultiplied(
                            palette.bg_elevated.r(),
                            palette.bg_elevated.g(),
                            palette.bg_elevated.b(),
                            (h * 170.0) as u8,
                        )
                    };
                    if fill.a() > 0 {
                        ui.painter()
                            .rect_filled(seg_rect, CornerRadius::same(7), fill);
                    }
                    let text_col = if selected {
                        palette.accent_soft_fg
                    } else {
                        lerp_color(palette.text_secondary, palette.text_primary, h)
                    };
                    let galley = ui.painter().layout_no_wrap(
                        mode.short_label().to_string(),
                        egui::FontId::proportional(12.5),
                        text_col,
                    );
                    ui.painter()
                        .galley(seg_rect.center() - galley.size() * 0.5, galley, text_col);

                    if seg_resp.on_hover_text(mode.label()).clicked() {
                        app.selected_permission = mode;
                    }
                }
            });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let has_turns = app
                .active_session
                .as_ref()
                .is_some_and(|s| !s.turns.is_empty());

            let sense = if has_turns {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let (btn_rect, btn_resp) =
                ui.allocate_exact_size(egui::vec2(104.0, 32.0), sense);
            let hovered = has_turns && btn_resp.hovered();
            if hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let h = hover_factor(ui.ctx(), ui.make_persistent_id("top_handoff_h"), hovered);
            let fill = if has_turns {
                lerp_color(palette.bg_card, palette.bg_elevated, h)
            } else {
                palette.bg_card
            };
            let border = if has_turns {
                lerp_color(palette.border, palette.border_strong, h)
            } else {
                palette.border
            };
            ui.painter().rect(
                btn_rect,
                CornerRadius::same(9),
                fill,
                Stroke::new(1.0_f32, border),
                egui::StrokeKind::Inside,
            );
            let text_col = if has_turns {
                lerp_color(palette.text_secondary, palette.text_primary, h)
            } else {
                egui::Color32::from_rgba_unmultiplied(
                    palette.text_muted.r(),
                    palette.text_muted.g(),
                    palette.text_muted.b(),
                    135,
                )
            };
            let galley = ui.painter().layout_no_wrap(
                "Hand off to…".to_string(),
                egui::FontId::proportional(12.5),
                text_col,
            );
            ui.painter()
                .galley(btn_rect.center() - galley.size() * 0.5, galley, text_col);

            let btn_resp =
                btn_resp.on_hover_text("Transfer session & workspace context to another engine");
            if has_turns && btn_resp.clicked() {
                app.open_handoff_modal(None);
            }
        });
    });

    ui.add_space(12.0);

    // Centered column helper calculation
    let avail_w = ui.available_width();
    let col_w = avail_w.min(CONTENT_MAX_WIDTH);
    let side_pad = ((avail_w - col_w) * 0.5).max(0.0);

    // Onboarding "Sign in to start" Banner (shown when no provider is signed in yet)
    if !any_provider_ready {
        ui.horizontal(|ui| {
            ui.add_space(side_pad);
            egui::Frame::new()
                .fill(palette.bg_card)
                .stroke(Stroke::new(1.0_f32, palette.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::symmetric(18, 14))
                .show(ui, |ui| {
                    ui.set_width((col_w - 36.0).max(280.0));
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Sign in to start")
                                    .size(14.5)
                                    .strong()
                                    .color(palette.text_primary),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(
                                    "Connect Claude Code, Antigravity, or Codex with your own account plan.",
                                )
                                .size(12.0)
                                .color(palette.text_secondary),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if sakura_cta_button(
                                ui,
                                "onboarding_sign_in",
                                "Sign in",
                                egui::vec2(80.0, 32.0),
                                true,
                                palette,
                            )
                            .clicked()
                            {
                                app.active_view = ActiveView::Accounts;
                                app.refresh_all_providers();
                            }
                        });
                    });
                });
        });
        ui.add_space(12.0);
    }

    // Main scrollable conversation canvas (centered column)
    let has_approval = !app.pending_approvals.is_empty();
    let composer_reserve = if has_approval { 268.0 } else { 136.0 };
    let scroll_h = (ui.available_height() - composer_reserve).max(160.0);

    let has_turns = app
        .active_session
        .as_ref()
        .is_some_and(|s| !s.turns.is_empty());

    let mut handoff_from_turn: Option<String> = None;

    let running_turn_id = app.active_running_turn_id();
    let is_running = running_turn_id.is_some();

    egui::ScrollArea::vertical()
        .id_salt("chat_turns_scroll")
        .max_height(scroll_h)
        .auto_shrink([false, false])
        .stick_to_bottom(is_running)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(side_pad);
                ui.vertical(|ui| {
                    ui.set_width(col_w);
                    if !has_turns {
                        render_empty_chat_state(app, ui, col_w, any_provider_ready, palette);
                    } else if let Some(sess) = &app.active_session {
                        for turn in &sess.turns {
                            ui.push_id(&turn.id, |ui| {
                                render_turn_card(turn, ui, palette, &mut handoff_from_turn);
                            });
                            ui.add_space(14.0);
                        }
                    }
                });
            });
        });

    if let Some(tid) = handoff_from_turn {
        app.open_handoff_modal(Some(tid));
    }

    // Safe Mode Approval Banner (docked directly above composer so it's always in view)
    if let Some(appr) = app.pending_approvals.first() {
        ui.add_space(8.0);
        let mut decision = None;
        ui.horizontal(|ui| {
            ui.add_space(side_pad);
            ui.vertical(|ui| {
                ui.set_width(col_w);
                decision = tool_card::render_approval_banner(ui, appr, palette);
            });
        });
        if let Some(approved) = decision {
            let approval_id = appr.approval_id.clone();
            app.pending_approvals.remove(0);
            app.client.send(
                RequestTag::Silent,
                DaemonRequest::RespondApproval {
                    approval_id,
                    approved,
                },
            );
        }
    }

    ui.add_space(10.0);

    // Bottom Docked Centered Composer (with subtle focus ring on border)
    let composer_focus_id = ui.make_persistent_id("chat_composer_focused");
    let was_focused: bool = ui.ctx().data(|d| d.get_temp(composer_focus_id)).unwrap_or(false);
    let focus_t = hover_factor(ui.ctx(), ui.make_persistent_id("chat_composer_focus_t"), was_focused);
    let composer_border = lerp_color(palette.border_strong, palette.accent, focus_t * 0.85);

    ui.horizontal(|ui| {
        ui.add_space(side_pad);
        egui::Frame::new()
            .fill(palette.bg_card)
            .stroke(Stroke::new(1.0_f32, composer_border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::symmetric(16, 12))
            .show(ui, |ui| {
                // Explicitly vertical inside outer `ui.horizontal` so TextEdit sits ABOVE action bar!
                ui.vertical(|ui| {
                    ui.set_width((col_w - 32.0).max(280.0));
                    let edit_id = ui.make_persistent_id("chat_composer_input");
                    let had_focus = ui.ctx().memory(|m| m.has_focus(edit_id));
                    // Enter sends (without Shift); Shift+Enter falls through to insert '\n'.
                    let enter_to_send = had_focus && consume_enter_to_send(ui);

                    let edit = egui::TextEdit::multiline(&mut app.chat_input)
                        .id(edit_id)
                        .hint_text("Ask anything about this workspace")
                        .desired_rows(3)
                        .desired_width(ui.available_width())
                        .frame(false);
                    let resp = ui.add(edit);
                    // Read focus BEFORE acquiring `ctx.data_mut` write lock to avoid RwLock deadlock!
                    let is_focused = resp.has_focus();
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(composer_focus_id, is_focused));

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        // Left: Unified Model + Thinking Level Pill (or "Sign in to pick a model")
                        if let Some(act) = render_unified_model_picker(
                            ui,
                            "composer_unified_model_picker",
                            &catalog,
                            &app.selected_canonical_model,
                            &app.per_model_thinking,
                            palette,
                        ) {
                            match act {
                                ModelPickerAction::Selected {
                                    canonical_id,
                                    provider,
                                    provider_model_id,
                                    thinking_level,
                                } => {
                                    app.select_unified_model(
                                        canonical_id,
                                        provider,
                                        provider_model_id,
                                        thinking_level,
                                    );
                                }
                                ModelPickerAction::OpenAccounts => {
                                    app.active_view = ActiveView::Accounts;
                                    app.refresh_all_providers();
                                }
                            }
                        }

                        // Right: Send ⌘↵ / Stop
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(tid) = running_turn_id {
                                let (stop_rect, stop_resp) = ui.allocate_exact_size(
                                    egui::vec2(84.0, 32.0),
                                    egui::Sense::click(),
                                );
                                let hovered = stop_resp.hovered();
                                let pressed = stop_resp.is_pointer_button_down_on();
                                if hovered {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let h = hover_factor(
                                    ui.ctx(),
                                    ui.make_persistent_id("chat_stop_cta"),
                                    hovered,
                                );
                                let base_bg = lerp_color(palette.bg_code, palette.danger, 0.14);
                                let hover_bg = lerp_color(palette.bg_code, palette.danger, 0.24);
                                let fill = if pressed {
                                    lerp_color(palette.bg_code, palette.danger, 0.10)
                                } else {
                                    lerp_color(base_bg, hover_bg, h)
                                };
                                let draw_rect = if pressed {
                                    stop_rect.shrink(1.0)
                                } else {
                                    stop_rect
                                };
                                ui.painter().rect(
                                    draw_rect,
                                    CornerRadius::same(9),
                                    fill,
                                    Stroke::new(
                                        1.0_f32,
                                        lerp_color(palette.border_strong, palette.danger, 0.65 + h * 0.35),
                                    ),
                                    egui::StrokeKind::Inside,
                                );
                                let galley = ui.painter().layout_no_wrap(
                                    "■  Stop".to_string(),
                                    egui::FontId::proportional(12.5),
                                    palette.danger,
                                );
                                ui.painter().galley(
                                    draw_rect.center() - galley.size() * 0.5,
                                    galley,
                                    palette.danger,
                                );
                                if stop_resp.on_hover_text("Stop current generation").clicked() {
                                    // Optimistically mark turn cancelled in UI immediately
                                    if let Some(sess) = app.active_session.as_mut() {
                                        if let Some(t) = sess.turns.iter_mut().find(|t| t.id == tid)
                                        {
                                            t.status = TurnStatus::Cancelled;
                                        }
                                    }
                                    app.pending_approvals.clear();
                                    app.client.send(
                                        RequestTag::Silent,
                                        DaemonRequest::CancelTurn { turn_id: tid },
                                    );
                                }
                            } else {
                                let provider_ready =
                                    any_provider_ready && app.active_workspace_id.is_some();
                                let can_send = provider_ready && !app.chat_input.trim().is_empty();

                                if sakura_cta_button(
                                    ui,
                                    "chat_send_cta",
                                    "Send ↵",
                                    egui::vec2(82.0, 32.0),
                                    provider_ready,
                                    palette,
                                )
                                .on_hover_text("Enter to send · Shift+Enter for new line")
                                .clicked()
                                    || (enter_to_send && can_send && !is_running)
                                {
                                    if can_send {
                                        app.send_chat_prompt();
                                    }
                                }
                            }
                        });
                    });
                });
            });
    });
}

fn render_empty_chat_state(
    app: &mut UmisongApp,
    ui: &mut egui::Ui,
    col_w: f32,
    any_provider_ready: bool,
    palette: &Palette,
) {
    ui.add_space(if any_provider_ready { 64.0 } else { 36.0 });

    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new("What are we building?")
                .size(28.0)
                .strong()
                .color(palette.text_primary),
        );
    });

    ui.add_space(28.0);

    let starters = [
        (
            "Map the repo",
            "Architecture overview, plus the sharp edges to watch.",
            "Map this repository's architecture and highlight the sharp edges to watch.",
        ),
        (
            "Plan a refactor",
            "A short plan with tests to verify each step.",
            "Write a short, concrete refactoring plan with tests to verify each step.",
        ),
        (
            "Audit edge cases",
            "Error handling review across the core modules.",
            "Audit error handling and edge cases across the core modules.",
        ),
    ];

    let gap = 12.0;
    let card_w = ((col_w - gap * 2.0) / 3.0).max(180.0);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (title, subtitle, prompt) in starters {
            let rect_id = ui.make_persistent_id(("starter_rect", title));
            let prev_rect: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(rect_id));
            let is_hovered = prev_rect.is_some_and(|r| ui.rect_contains_pointer(r));
            let is_pressed = is_hovered && ui.input(|i| i.pointer.primary_down());
            let h = hover_factor(ui.ctx(), ui.make_persistent_id(("starter_hover", title)), is_hovered);

            let card_fill = if is_pressed {
                palette.bg_code
            } else {
                lerp_color(palette.bg_card, palette.bg_elevated, h * 0.65)
            };
            let card_border = lerp_color(
                palette.border,
                lerp_color(palette.border_strong, palette.accent, 0.28),
                h,
            );

            let frame_out = egui::Frame::new()
                .fill(card_fill)
                .stroke(Stroke::new(1.0_f32, card_border))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(card_w - 32.0);
                        ui.set_min_height(66.0);
                        ui.label(
                            RichText::new(title)
                                .size(13.5)
                                .strong()
                                .color(lerp_color(palette.text_primary, palette.accent, h * 0.2)),
                        );
                        ui.add_space(4.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(subtitle)
                                    .size(12.0)
                                    .color(lerp_color(
                                        palette.text_secondary,
                                        palette.text_primary,
                                        h * 0.35,
                                    )),
                            )
                            .wrap(),
                        );
                    });
                });

            ui.ctx()
                .data_mut(|d| d.insert_temp(rect_id, frame_out.response.rect));
            let resp = frame_out.response.interact(egui::Sense::click());

            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                app.chat_input = prompt.to_string();
            }
        }
    });
}

fn render_turn_card(
    turn: &TurnRecord,
    ui: &mut egui::Ui,
    palette: &Palette,
    _handoff_from_turn: &mut Option<String>,
) {
    // 1. User Prompt Bubble — right-aligned compact pill/bubble (`#1E2635`), no "You" header
    let max_bubble_w = (ui.available_width() * 0.68).max(220.0);
    let prompt_font = egui::FontId::proportional(13.5);
    let mut prompt_job = egui::text::LayoutJob::simple(
        turn.prompt.clone(),
        prompt_font,
        palette.text_primary,
        max_bubble_w,
    );
    prompt_job.break_on_newline = true;
    let prompt_galley = ui.fonts(|f| f.layout_job(prompt_job));
    let bubble_text_w = prompt_galley
        .rows
        .iter()
        .map(|r| r.rect.width())
        .fold(0.0_f32, f32::max)
        .clamp(24.0, max_bubble_w);

    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
        egui::Frame::new()
            .fill(palette.bg_elevated)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(16))
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.set_width(bubble_text_w + 4.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&turn.prompt)
                                .size(13.5)
                                .color(palette.text_primary),
                        )
                        .wrap()
                        .selectable(true),
                    );
                });
            });

        if let Some(from_p) = turn.handoff_from_provider {
            ui.add_space(6.0);
            pill_badge(
                ui,
                &format!(
                    "Handoff: {} → {}",
                    from_p.short_label(),
                    turn.provider.short_label()
                ),
                palette.accent,
            );
        }
    });

    ui.add_space(16.0);

    // 2. Assistant Response — frameless directly on the `#0E1116` canvas
    let running = turn.status == TurnStatus::Running;

    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;

        // Minimal inline Thinking line: `Thought for 12s ›`
        if !turn.thought_text.is_empty() || (running && turn.response_text.is_empty() && turn.tool_calls.is_empty()) {
            tool_card::render_thought_line(
                ui,
                &format!("thought_{}", turn.id),
                &turn.thought_text,
                turn.duration_ms,
                running && turn.response_text.is_empty(),
                palette,
            );
        }

        // Minimal inline grouped Tool-calls line: `✓ Edited calculator.py · Ran 2 commands ›`
        if !turn.tool_calls.is_empty() {
            tool_card::render_tool_calls_summary(ui, &turn.id, &turn.tool_calls, palette);
        }

        if !turn.thought_text.is_empty() || !turn.tool_calls.is_empty() {
            ui.add_space(6.0);
        }

        if let Some(err) = &turn.error_message {
            egui::Frame::new()
                .fill(egui::Color32::from_rgba_unmultiplied(
                    palette.danger.r(),
                    palette.danger.g(),
                    palette.danger.b(),
                    26,
                ))
                .stroke(Stroke::new(1.0_f32, palette.danger))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(err).color(palette.danger).size(12.5));
                });
            ui.add_space(6.0);
        }

        if !turn.response_text.is_empty() {
            ui.spacing_mut().item_spacing.y = 6.0;
            markdown::render_markdown(ui, &turn.response_text, palette);
            if running {
                ui.label(
                    RichText::new("▍")
                        .size(13.0)
                        .color(palette.text_muted),
                );
            }
        }
    });

    ui.add_space(12.0);
}

pub fn render_handoff_modal(
    app: &mut UmisongApp,
    ctx: &egui::Context,
    modal: &mut HandoffModalState,
    palette: &Palette,
) -> bool {
    let mut keep_open = true;
    let mut trigger_handoff = false;
    let catalog = app.unified_catalog();

    egui::Window::new("Context Handoff")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([540.0, 320.0])
        .show(ctx, |ui| {
            ui.label(
                RichText::new(
                    "Package conversation history + current workspace git status and continue with another model/engine:",
                )
                .size(12.5)
                .color(palette.text_secondary),
            );
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                ui.label(RichText::new("Target Model:").strong());
                let mut modal_levels = app.per_model_thinking.clone();
                modal_levels.insert(
                    modal.target_canonical_id.clone(),
                    modal.target_thinking_level,
                );
                if let Some(ModelPickerAction::Selected {
                    canonical_id,
                    provider,
                    provider_model_id,
                    thinking_level,
                }) = render_unified_model_picker(
                    ui,
                    "handoff_unified_picker",
                    &catalog,
                    &modal.target_canonical_id,
                    &modal_levels,
                    palette,
                ) {
                    modal.target_canonical_id = canonical_id;
                    modal.target_provider = provider;
                    modal.target_model = provider_model_id;
                    modal.target_thinking_level = thinking_level;
                }
            });

            ui.add_space(8.0);
            ui.checkbox(
                &mut modal.create_new_session,
                "Fork into a new linked session (otherwise continue in current thread)",
            );

            ui.add_space(8.0);
            ui.label(RichText::new("Follow-up instruction (optional):").strong());
            ui.add(
                egui::TextEdit::multiline(&mut modal.instruction)
                    .hint_text("e.g. Review the previous patch for edge cases and add unit tests.")
                    .desired_rows(3)
                    .desired_width(ui.available_width()),
            );

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Run Handoff")
                                    .strong()
                                    .color(palette.accent_fg),
                            )
                            .fill(palette.accent)
                            .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        trigger_handoff = true;
                        keep_open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        keep_open = false;
                    }
                });
            });
        });

    if trigger_handoff {
        if let Some(source_sid) = app.active_session.as_ref().map(|s| s.id.clone()) {
            app.select_unified_model(
                modal.target_canonical_id.clone(),
                modal.target_provider,
                modal.target_model.clone(),
                modal.target_thinking_level,
            );
            app.client.send(
                RequestTag::Handoff,
                DaemonRequest::HandoffSession {
                    source_session_id: source_sid,
                    up_to_turn_id: modal.up_to_turn_id.clone(),
                    target_provider: modal.target_provider,
                    target_model: modal.target_model.clone(),
                    thinking_level: modal.target_thinking_level,
                    instruction: modal.instruction.clone(),
                    create_new_session: modal.create_new_session,
                },
            );
        }
    }

    keep_open
}
