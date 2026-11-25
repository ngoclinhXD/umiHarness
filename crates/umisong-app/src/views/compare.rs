use crate::app::{ActiveView, CompareColumnTab, UmisongApp};
use crate::client::RequestTag;
use crate::theme::{
    Palette, consume_enter_to_send, hover_factor, lerp_color, paint_status_dot, pill_badge,
    sakura_cta_button,
};
use crate::views::diff_viewer;
use crate::widgets::{markdown, tool_card};
use egui::{CornerRadius, Margin, Order, Pos2, Rect, RichText, Stroke, Vec2};
use umisong_core::{
    CompareColumnState, CompareIsolationMode, CompareTarget, DaemonRequest, ProviderId,
    ThinkingLevel, TurnStatus, UnifiedModelEntry,
};

#[derive(Debug, Clone, Default)]
struct FighterPickerState {
    open_provider: Option<ProviderId>,
    anchor_rect: Option<Rect>,
    hovered_model: Option<String>,
    hovered_row_y: f32,
    menu_rect: Option<Rect>,
    submenu_rect: Option<Rect>,
}

pub fn render_compare_view(app: &mut UmisongApp, ui: &mut egui::Ui, palette: &Palette) {
    // Cohesive Top Header Row
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new("Versus")
                    .size(22.0)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.add_space(3.0);
            ui.label(
                RichText::new(
                    "Run 2–3 models head-to-head on the same prompt and adopt the winner.",
                )
                .size(12.5)
                .color(palette.text_secondary),
            );
        });

        // Segmented Isolation Mode Pill (`Read-only` | `Git worktrees`)
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            let is_git = app.active_workspace().is_some_and(|w| w.is_git_repo);
            egui::Frame::new()
                .fill(palette.bg_card)
                .stroke(Stroke::new(1.0_f32, palette.border))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(4))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (mode, short_lbl) in [
                        (CompareIsolationMode::ReadOnly, "Read-only"),
                        (CompareIsolationMode::GitWorktree, "Git worktrees"),
                    ] {
                        let enabled = mode == CompareIsolationMode::ReadOnly || is_git;
                        let selected = app.compare_isolation == mode;
                        let sense = if enabled {
                            egui::Sense::click()
                        } else {
                            egui::Sense::hover()
                        };
                        let (seg_rect, seg_resp) =
                            ui.allocate_exact_size(egui::vec2(96.0, 26.0), sense);
                        let hovered = enabled && seg_resp.hovered();
                        if hovered {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let h = hover_factor(
                            ui.ctx(),
                            ui.make_persistent_id(("cmp_iso_seg", short_lbl)),
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
                        let text_col = if !enabled {
                            palette.text_muted
                        } else if selected {
                            palette.accent_soft_fg
                        } else {
                            lerp_color(palette.text_secondary, palette.text_primary, h)
                        };
                        let galley = ui.painter().layout_no_wrap(
                            short_lbl.to_string(),
                            egui::FontId::proportional(12.0),
                            text_col,
                        );
                        ui.painter().galley(
                            seg_rect.center() - galley.size() * 0.5,
                            galley,
                            text_col,
                        );

                        let seg_resp = if enabled {
                            seg_resp
                        } else {
                            seg_resp.on_hover_text(
                                "Open a git repository to run isolated Git worktrees",
                            )
                        };
                        if enabled && seg_resp.clicked() {
                            app.compare_isolation = mode;
                        }
                    }
                });
        });
    });

    ui.add_space(12.0);

    // Unified Versus Composer Card (`#12161D`, `14px` corner radius, subtle focus border lift)
    let catalog = app.unified_catalog();
    let ready_count = ProviderId::ALL
        .into_iter()
        .filter(|pid| app.providers.get(pid).is_some_and(|s| s.auth.is_ready()))
        .count();

    let focus_id = ui.make_persistent_id("versus_composer_focused");
    let was_focused: bool = ui.ctx().data(|d| d.get_temp(focus_id)).unwrap_or(false);
    let focus_t = hover_factor(ui.ctx(), ui.make_persistent_id("versus_focus_t"), was_focused);
    let composer_border = lerp_color(palette.border_strong, palette.accent, focus_t * 0.85);

    let picker_state_id = egui::Id::new("versus_fighter_picker_state");
    let mut picker_state: FighterPickerState = ui
        .ctx()
        .data(|d| d.get_temp(picker_state_id))
        .unwrap_or_default();
    let mut clicked_picker_trigger = false;

    egui::Frame::new()
        .fill(palette.bg_card)
        .stroke(Stroke::new(1.0_f32, composer_border))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Prompt input (Enter to run · Shift+Enter for new line)
            let edit_id = ui.make_persistent_id("versus_composer_input");
            let had_focus = ui.ctx().memory(|m| m.has_focus(edit_id));
            let enter_to_run = had_focus && consume_enter_to_send(ui);

            let edit = egui::TextEdit::multiline(&mut app.compare_prompt)
                .id(edit_id)
                .hint_text("Enter a coding prompt to benchmark across your signed-in providers…")
                .desired_rows(2)
                .desired_width(ui.available_width())
                .frame(false);
            let prompt_resp = ui.add(edit);
            let is_focused = prompt_resp.has_focus();
            ui.ctx().data_mut(|d| d.insert_temp(focus_id, is_focused));

            ui.add_space(8.0);
            let (sep_r, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter().hline(
                sep_r.x_range(),
                sep_r.center().y,
                Stroke::new(1.0_f32, palette.border),
            );
            ui.add_space(8.0);

            // Bottom Row inside Composer: Provider Fighters on Left, Run Versus on Right
            let mut open_accounts = false;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                for pid in ProviderId::ALL {
                    let is_ready = app.providers.get(&pid).is_some_and(|s| s.auth.is_ready());
                    let p_col = palette.provider_color(pid);

                    if !is_ready {
                        let lbl = format!("{} · Sign in", pid.short_label());
                        let g = ui.painter().layout_no_wrap(
                            lbl.clone(),
                            egui::FontId::proportional(11.5),
                            palette.text_muted,
                        );
                        let (chip_rect, chip_resp) = ui.allocate_exact_size(
                            Vec2::new(g.size().x + 32.0, 28.0),
                            egui::Sense::click(),
                        );
                        let hov = chip_resp.hovered();
                        if hov {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let h = hover_factor(
                            ui.ctx(),
                            ui.make_persistent_id(("cmp_signin_chip", pid.as_str())),
                            hov,
                        );
                        ui.painter().rect(
                            chip_rect,
                            CornerRadius::same(255),
                            lerp_color(palette.bg_card, palette.bg_elevated, h),
                            Stroke::new(
                                1.0_f32,
                                lerp_color(palette.border, palette.border_strong, h),
                            ),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().circle_stroke(
                            Pos2::new(chip_rect.left() + 13.0, chip_rect.center().y),
                            3.5,
                            Stroke::new(1.3_f32, p_col),
                        );
                        let text_col = lerp_color(palette.text_muted, palette.text_primary, h);
                        let tg = ui.painter().layout_no_wrap(
                            lbl,
                            egui::FontId::proportional(11.5),
                            text_col,
                        );
                        ui.painter().galley(
                            Pos2::new(
                                chip_rect.left() + 23.0,
                                chip_rect.center().y - tg.size().y * 0.5,
                            ),
                            tg,
                            text_col,
                        );
                        if chip_resp
                            .on_hover_text(format!(
                                "Sign in to {} in Accounts to include it in Versus",
                                pid.display_name()
                            ))
                            .clicked()
                        {
                            open_accounts = true;
                        }
                        continue;
                    }

                    let enabled = *app.compare_enabled.entry(pid).or_insert(true);
                    let prov_models: Vec<&UnifiedModelEntry> = catalog
                        .iter()
                        .flat_map(|g| g.models.iter())
                        .filter(|m| m.primary_provider == pid && m.available)
                        .collect();

                    let model_val = app.compare_models.entry(pid).or_default();
                    if model_val.is_empty() {
                        if let Some(first_ok) = prov_models.first() {
                            *model_val = first_ok.provider_model_id.clone();
                        }
                    }

                    let active_entry = prov_models
                        .iter()
                        .copied()
                        .find(|m| m.provider_model_id == *model_val)
                        .or_else(|| prov_models.first().copied());

                    let lvl_entry = app.compare_thinking_levels.entry(pid).or_insert_with(|| {
                        active_entry
                            .map(|e| e.default_thinking_level)
                            .unwrap_or(ThinkingLevel::High)
                    });
                    let clamped_cur = active_entry
                        .map(|m| m.clamp_thinking_level(*lvl_entry))
                        .unwrap_or(*lvl_entry);

                    let is_picker_open = picker_state.open_provider == Some(pid);
                    render_fighter_pill(
                        ui,
                        pid,
                        p_col,
                        enabled,
                        active_entry,
                        clamped_cur,
                        is_picker_open,
                        palette,
                        &mut app.compare_enabled,
                        &mut picker_state,
                        &mut clicked_picker_trigger,
                    );
                }

                // Right-aligned Run Versus / Cancel All CTA
                let selected_targets: Vec<CompareTarget> = ProviderId::ALL
                    .into_iter()
                    .filter(|pid| {
                        app.providers.get(pid).is_some_and(|s| s.auth.is_ready())
                            && *app.compare_enabled.get(pid).unwrap_or(&false)
                    })
                    .map(|pid| CompareTarget {
                        provider: pid,
                        model: app.compare_models.get(&pid).cloned().unwrap_or_default(),
                        thinking_level: app
                            .compare_thinking_levels
                            .get(&pid)
                            .copied()
                            .unwrap_or(ThinkingLevel::High),
                    })
                    .collect();

                let any_running = app
                    .active_compare_run()
                    .is_some_and(|r| r.columns.iter().any(|c| c.status == TurnStatus::Running));

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if any_running {
                        if let Some(cid) = app.active_compare_id.clone() {
                            let (stop_rect, stop_resp) = ui.allocate_exact_size(
                                egui::vec2(98.0, 32.0),
                                egui::Sense::click(),
                            );
                            let hovered = stop_resp.hovered();
                            let pressed = stop_resp.is_pointer_button_down_on();
                            if hovered {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            let h = hover_factor(
                                ui.ctx(),
                                ui.make_persistent_id("versus_cancel_cta"),
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
                                    lerp_color(
                                        palette.border_strong,
                                        palette.danger,
                                        0.65 + h * 0.35,
                                    ),
                                ),
                                egui::StrokeKind::Inside,
                            );
                            let galley = ui.painter().layout_no_wrap(
                                "■  Cancel all".to_string(),
                                egui::FontId::proportional(12.0),
                                palette.danger,
                            );
                            ui.painter().galley(
                                draw_rect.center() - galley.size() * 0.5,
                                galley,
                                palette.danger,
                            );
                            if stop_resp.clicked() {
                                app.client.send(
                                    RequestTag::Silent,
                                    DaemonRequest::CancelCompare { compare_id: cid },
                                );
                            }
                        }
                    } else {
                        let provider_ready =
                            !selected_targets.is_empty() && app.active_workspace_id.is_some();
                        let can_run = provider_ready && !app.compare_prompt.trim().is_empty();

                        if sakura_cta_button(
                            ui,
                            "versus_run_cta",
                            "Run Versus ↵",
                            egui::vec2(118.0, 32.0),
                            provider_ready,
                            palette,
                        )
                        .on_hover_text("Enter to run · Shift+Enter for new line")
                        .clicked()
                            || (enter_to_run && can_run)
                        {
                            if can_run {
                                if let Some(ws_id) = app.active_workspace_id.clone() {
                                    let prompt = app.compare_prompt.trim().to_string();
                                    app.client.send(
                                        RequestTag::StartCompare,
                                        DaemonRequest::StartCompare {
                                            workspace_id: ws_id,
                                            session_id: app
                                                .active_session
                                                .as_ref()
                                                .map(|s| s.id.clone()),
                                            prompt,
                                            targets: selected_targets,
                                            isolation_mode: app.compare_isolation,
                                        },
                                    );
                                }
                            }
                        }
                    }
                });
            });

            if open_accounts {
                app.active_view = ActiveView::Accounts;
                app.refresh_all_providers();
            }
        });

    // Render custom fighter model · thinking popover if open
    render_fighter_model_popover(
        ui.ctx(),
        &catalog,
        &mut app.compare_models,
        &mut app.compare_thinking_levels,
        &mut picker_state,
        clicked_picker_trigger,
        palette,
    );
    ui.ctx()
        .data_mut(|d| d.insert_temp(picker_state_id, picker_state));

    ui.add_space(12.0);

    // Past Versus Runs Selector Strip (`RECENT RUNS`)
    if !app.compare_runs.is_empty() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("RECENT RUNS")
                    .size(10.5)
                    .strong()
                    .color(palette.text_muted),
            );
            ui.add_space(6.0);
            egui::ScrollArea::horizontal()
                .id_salt("compare_history_strip")
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for run in &app.compare_runs {
                            let selected =
                                app.active_compare_id.as_deref() == Some(run.id.as_str());
                            let line = run.prompt.lines().next().unwrap_or("Versus");
                            let preview = if line.chars().count() > 38 {
                                let head: String = line.chars().take(37).collect();
                                format!("{}…", head.trim_end())
                            } else {
                                line.to_string()
                            };
                            let font = egui::FontId::proportional(11.5);
                            let g = ui.painter().layout_no_wrap(
                                preview.clone(),
                                font.clone(),
                                palette.text_primary,
                            );
                            let (pill_r, pill_resp) = ui.allocate_exact_size(
                                Vec2::new(g.size().x + 22.0, 25.0),
                                egui::Sense::click(),
                            );
                            let hov = pill_resp.hovered();
                            if hov {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            let h = hover_factor(
                                ui.ctx(),
                                ui.make_persistent_id(("cmp_hist_pill", &run.id)),
                                hov,
                            );
                            let bg = if selected {
                                palette.bg_elevated
                            } else {
                                lerp_color(palette.bg_card, palette.bg_elevated, h * 0.65)
                            };
                            let border = if selected {
                                palette.border_strong
                            } else {
                                lerp_color(palette.border, palette.border_strong, h)
                            };
                            ui.painter().rect(
                                pill_r,
                                CornerRadius::same(8),
                                bg,
                                Stroke::new(1.0_f32, border),
                                egui::StrokeKind::Inside,
                            );
                            let text_col = if selected {
                                palette.text_primary
                            } else {
                                lerp_color(palette.text_secondary, palette.text_primary, h)
                            };
                            let tg = ui.painter().layout_no_wrap(preview, font, text_col);
                            ui.painter().galley(
                                pill_r.center() - tg.size() * 0.5,
                                tg,
                                text_col,
                            );
                            if pill_resp.clicked() {
                                app.active_compare_id = Some(run.id.clone());
                            }
                        }
                    });
                });
        });
        ui.add_space(10.0);
    }

    // Active Versus Columns or Intentional Empty State
    let Some(run) = app.active_compare_run().cloned() else {
        ui.add_space(48.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Head-to-head comparison")
                    .size(18.0)
                    .strong()
                    .color(palette.text_primary),
            );
            ui.add_space(6.0);
            let sub = if ready_count >= 2 {
                "Send one prompt above to watch your signed-in engines stream side-by-side."
            } else if ready_count == 1 {
                "1 provider is signed in. Sign in to another provider in Accounts to compare engines side-by-side."
            } else {
                "Sign in to at least two providers in Accounts to benchmark them head-to-head."
            };
            ui.label(RichText::new(sub).size(12.5).color(palette.text_secondary));
            if ready_count < 2 {
                ui.add_space(12.0);
                let acct_btn = egui::Button::new(
                    RichText::new("Open Accounts")
                        .size(12.5)
                        .strong()
                        .color(palette.text_primary),
                )
                .fill(palette.bg_card)
                .stroke(Stroke::new(1.0_f32, palette.border_strong))
                .corner_radius(CornerRadius::same(8));
                if ui.add_sized([124.0, 32.0], acct_btn).clicked() {
                    app.active_view = ActiveView::Accounts;
                    app.refresh_all_providers();
                }
            }
        });
        return;
    };

    let col_count = run.columns.len().max(1);
    let avail = ui.available_size();
    let gap = 12.0;
    let col_w = ((avail.x - ((col_count - 1) as f32 * gap)) / col_count as f32).max(260.0);
    let col_h = (avail.y - 6.0).max(260.0);

    let mut adopt_req: Option<(String, ProviderId, bool)> = None;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for col in &run.columns {
            ui.push_id(("versus_col", &run.id, col.provider.as_str()), |ui| {
                render_compare_column(
                    app,
                    &run.id,
                    run.isolation_mode,
                    col,
                    col_w,
                    col_h,
                    ui,
                    palette,
                    &mut adopt_req,
                );
            });
        }
    });

    if let Some((compare_id, provider, apply_patch)) = adopt_req {
        app.client.send(
            RequestTag::AdoptCompare,
            DaemonRequest::AdoptCompareColumn {
                compare_id,
                provider,
                apply_patch,
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_fighter_pill(
    ui: &mut egui::Ui,
    pid: ProviderId,
    p_col: egui::Color32,
    enabled: bool,
    active_entry: Option<&UnifiedModelEntry>,
    clamped_cur: ThinkingLevel,
    is_picker_open: bool,
    palette: &Palette,
    compare_enabled: &mut std::collections::HashMap<ProviderId, bool>,
    picker_state: &mut FighterPickerState,
    clicked_picker_trigger: &mut bool,
) {
    let model_name = active_entry
        .map(|m| m.display_name.as_str())
        .unwrap_or("Plan default");
    let level_lbl = clamped_cur.label();
    let chevron = if is_picker_open { "▴" } else { "▾" };

    let font_prov = egui::FontId::proportional(11.5);
    let font_model = egui::FontId::proportional(11.5);
    let prov_g = ui
        .painter()
        .layout_no_wrap(pid.short_label().to_string(), font_prov.clone(), p_col);

    let model_str = format!("{model_name} · {level_lbl} {chevron}");
    let model_g = ui.painter().layout_no_wrap(
        model_str.clone(),
        font_model.clone(),
        palette.text_primary,
    );

    let toggle_w = 24.0 + prov_g.size().x + if enabled { 6.0 } else { 10.0 };
    let model_w = if enabled { model_g.size().x + 18.0 } else { 0.0 };
    let total_w = toggle_w + model_w;

    let (pill_rect, _) =
        ui.allocate_exact_size(Vec2::new(total_w, 28.0), egui::Sense::hover());

    let toggle_rect = Rect::from_min_size(pill_rect.min, Vec2::new(toggle_w, 28.0));
    let toggle_resp = ui.interact(
        toggle_rect,
        ui.make_persistent_id(("cmp_fighter_toggle", pid.as_str())),
        egui::Sense::click(),
    );
    if toggle_resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let model_rect = Rect::from_min_size(
        Pos2::new(pill_rect.left() + toggle_w, pill_rect.top()),
        Vec2::new(model_w, 28.0),
    );
    let model_resp = if enabled {
        let r = ui.interact(
            model_rect,
            ui.make_persistent_id(("cmp_fighter_model", pid.as_str())),
            egui::Sense::click(),
        );
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        Some(r)
    } else {
        None
    };

    let any_hov = toggle_resp.hovered()
        || model_resp.as_ref().is_some_and(|r| r.hovered())
        || is_picker_open;
    let h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id(("cmp_fighter_h", pid.as_str())),
        any_hov,
    );

    let pill_fill = if enabled {
        egui::Color32::from_rgba_unmultiplied(
            p_col.r(),
            p_col.g(),
            p_col.b(),
            (22.0 + h * 18.0) as u8,
        )
    } else {
        lerp_color(palette.bg_card, palette.bg_elevated, h)
    };
    let pill_border = if enabled {
        egui::Color32::from_rgba_unmultiplied(
            p_col.r(),
            p_col.g(),
            p_col.b(),
            (80.0 + h * 55.0) as u8,
        )
    } else {
        lerp_color(palette.border, palette.border_strong, h)
    };

    ui.painter().rect(
        pill_rect,
        CornerRadius::same(255),
        pill_fill,
        Stroke::new(1.0_f32, pill_border),
        egui::StrokeKind::Inside,
    );

    // Status dot + provider short name
    let dot_center = Pos2::new(pill_rect.left() + 13.0, pill_rect.center().y);
    if enabled {
        ui.painter().circle_filled(dot_center, 3.6, p_col);
    } else {
        ui.painter()
            .circle_stroke(dot_center, 3.5, Stroke::new(1.3_f32, palette.text_muted));
    }

    let prov_col = if enabled { p_col } else { palette.text_muted };
    let pg = ui
        .painter()
        .layout_no_wrap(pid.short_label().to_string(), font_prov, prov_col);
    ui.painter().galley(
        Pos2::new(
            pill_rect.left() + 22.0,
            pill_rect.center().y - pg.size().y * 0.5,
        ),
        pg,
        prov_col,
    );

    if enabled {
        // Subtle vertical divider between toggle and model picker trigger
        let div_x = pill_rect.left() + toggle_w;
        ui.painter().vline(
            div_x,
            (pill_rect.top() + 6.0)..=(pill_rect.bottom() - 6.0),
            Stroke::new(
                1.0_f32,
                egui::Color32::from_rgba_unmultiplied(p_col.r(), p_col.g(), p_col.b(), 55),
            ),
        );

        let mg = ui
            .painter()
            .layout_no_wrap(model_str, font_model, palette.text_primary);
        ui.painter().galley(
            Pos2::new(div_x + 8.0, pill_rect.center().y - mg.size().y * 0.5),
            mg,
            palette.text_primary,
        );
    }

    if toggle_resp
        .on_hover_text("Click to enable or disable this provider in Versus")
        .clicked()
    {
        compare_enabled.insert(pid, !enabled);
        if is_picker_open {
            picker_state.open_provider = None;
        }
    }

    if let Some(mr) = model_resp {
        if mr
            .on_hover_text("Choose model & thinking effort for this fighter")
            .clicked()
        {
            *clicked_picker_trigger = true;
            if is_picker_open {
                picker_state.open_provider = None;
                picker_state.menu_rect = None;
                picker_state.submenu_rect = None;
            } else {
                picker_state.open_provider = Some(pid);
                picker_state.anchor_rect = Some(pill_rect);
                picker_state.hovered_model = active_entry.map(|e| e.provider_model_id.clone());
            }
        }
    }
}

fn render_fighter_model_popover(
    ctx: &egui::Context,
    catalog: &[umisong_core::UnifiedModelGroup],
    compare_models: &mut std::collections::HashMap<ProviderId, String>,
    compare_thinking_levels: &mut std::collections::HashMap<ProviderId, ThinkingLevel>,
    state: &mut FighterPickerState,
    clicked_trigger: bool,
    palette: &Palette,
) {
    let Some(pid) = state.open_provider else {
        return;
    };
    let Some(anchor) = state.anchor_rect else {
        state.open_provider = None;
        return;
    };

    let prov_models: Vec<&UnifiedModelEntry> = catalog
        .iter()
        .flat_map(|g| g.models.iter())
        .filter(|m| m.primary_provider == pid && m.available)
        .collect();
    if prov_models.is_empty() {
        state.open_provider = None;
        return;
    }

    let p_col = palette.provider_color(pid);
    let cur_model_id = compare_models.get(&pid).cloned().unwrap_or_default();
    let cur_level = compare_thinking_levels
        .get(&pid)
        .copied()
        .unwrap_or(ThinkingLevel::High);

    let mut close_menu = false;
    let menu_w = 270.0_f32;
    let menu_pos = Pos2::new(anchor.left(), anchor.bottom() + 6.0);

    let area_out = egui::Area::new(egui::Id::new("versus_fighter_model_popover"))
        .order(Order::Foreground)
        .fixed_pos(menu_pos)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(palette.bg_card)
                .stroke(Stroke::new(1.0_f32, palette.border_strong))
                .corner_radius(CornerRadius::same(14))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 10],
                    blur: 28,
                    spread: 0,
                    color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
                })
                .inner_margin(Margin::symmetric(8, 8))
                .show(ui, |ui| {
                    ui.set_width(menu_w);
                    ui.spacing_mut().item_spacing.y = 2.0;

                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("{} MODELS", pid.display_name().to_uppercase()))
                                .size(10.0)
                                .strong()
                                .color(palette.text_muted),
                        );
                    });
                    ui.add_space(4.0);

                    for m in &prov_models {
                        let is_selected = m.provider_model_id == cur_model_id;
                        let is_hovered_model = state.hovered_model.as_deref()
                            == Some(m.provider_model_id.as_str());
                        let eff_lvl = m.clamp_thinking_level(cur_level);

                        let (row_rect, row_resp) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 32.0),
                            egui::Sense::click(),
                        );
                        let hovered = row_resp.hovered();
                        if hovered {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            state.hovered_model = Some(m.provider_model_id.clone());
                            state.hovered_row_y = row_rect.top();
                        } else if is_selected && state.hovered_model.is_none() {
                            state.hovered_model = Some(m.provider_model_id.clone());
                            state.hovered_row_y = row_rect.top();
                        }

                        let h = hover_factor(
                            ui.ctx(),
                            ui.make_persistent_id(("vs_m_row", &m.provider_model_id)),
                            hovered || is_hovered_model,
                        );
                        if h > 0.01 || is_selected {
                            let bg = if is_selected && h < 0.2 {
                                egui::Color32::from_rgba_unmultiplied(
                                    palette.bg_elevated.r(),
                                    palette.bg_elevated.g(),
                                    palette.bg_elevated.b(),
                                    160,
                                )
                            } else {
                                egui::Color32::from_rgba_unmultiplied(
                                    palette.bg_elevated.r(),
                                    palette.bg_elevated.g(),
                                    palette.bg_elevated.b(),
                                    (h * 255.0) as u8,
                                )
                            };
                            ui.painter().rect_filled(row_rect, CornerRadius::same(8), bg);
                        }

                        // Left indicator dot + model display name
                        if is_selected {
                            ui.painter().circle_filled(
                                Pos2::new(row_rect.left() + 12.0, row_rect.center().y),
                                3.4,
                                p_col,
                            );
                        }
                        let name_g = ui.painter().layout_no_wrap(
                            m.display_name.clone(),
                            egui::FontId::proportional(13.0),
                            if is_selected {
                                palette.text_primary
                            } else {
                                lerp_color(palette.text_secondary, palette.text_primary, h)
                            },
                        );
                        ui.painter().galley(
                            Pos2::new(
                                row_rect.left() + 24.0,
                                row_rect.center().y - name_g.size().y * 0.5,
                            ),
                            name_g,
                            palette.text_primary,
                        );

                        // Right thinking level + chevron
                        let right_str = format!("{}   ▸", eff_lvl.label());
                        let right_g = ui.painter().layout_no_wrap(
                            right_str,
                            egui::FontId::proportional(12.0),
                            palette.text_muted,
                        );
                        ui.painter().galley(
                            Pos2::new(
                                row_rect.right() - right_g.size().x - 10.0,
                                row_rect.center().y - right_g.size().y * 0.5,
                            ),
                            right_g,
                            palette.text_muted,
                        );

                        if row_resp.clicked() {
                            compare_models.insert(pid, m.provider_model_id.clone());
                            compare_thinking_levels.insert(pid, eff_lvl);
                            close_menu = true;
                        }
                    }
                });
        });

    let main_rect = area_out.response.rect;
    state.menu_rect = Some(main_rect);

    // Cascading THINKING submenu for hovered model
    let mut sub_rect_out = None;
    if let Some(hov_id) = state.hovered_model.clone() {
        if let Some(m) = prov_models.iter().copied().find(|m| m.provider_model_id == hov_id) {
            let eff_lvl = m.clamp_thinking_level(cur_level);
            let sub_pos = Pos2::new(
                main_rect.right() + 6.0,
                state.hovered_row_y.max(main_rect.top()),
            );
            let sub_out = egui::Area::new(egui::Id::new("versus_fighter_thinking_submenu"))
                .order(Order::Foreground)
                .fixed_pos(sub_pos)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(palette.bg_card)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(CornerRadius::same(14))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 10],
                            blur: 24,
                            spread: 0,
                            color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
                        })
                        .inner_margin(Margin::symmetric(8, 8))
                        .show(ui, |ui| {
                            ui.set_width(148.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.horizontal(|ui| {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("THINKING")
                                        .size(10.0)
                                        .strong()
                                        .color(palette.text_muted),
                                );
                            });
                            ui.add_space(4.0);

                            for &lvl in &m.supported_thinking_levels {
                                let is_cur =
                                    m.provider_model_id == cur_model_id && eff_lvl == lvl;
                                let (r_rect, r_resp) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 30.0),
                                    egui::Sense::click(),
                                );
                                let hov = r_resp.hovered();
                                if hov {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let h = hover_factor(
                                    ui.ctx(),
                                    ui.make_persistent_id(("vs_think_lvl", &m.provider_model_id, lvl.label())),
                                    hov,
                                );
                                let fill = if is_cur {
                                    palette.accent_soft_bg
                                } else {
                                    egui::Color32::from_rgba_unmultiplied(
                                        palette.bg_elevated.r(),
                                        palette.bg_elevated.g(),
                                        palette.bg_elevated.b(),
                                        (h * 255.0) as u8,
                                    )
                                };
                                if fill.a() > 0 {
                                    ui.painter()
                                        .rect_filled(r_rect, CornerRadius::same(8), fill);
                                }
                                let fg = if is_cur {
                                    palette.accent_soft_fg
                                } else {
                                    lerp_color(palette.text_secondary, palette.text_primary, h)
                                };
                                let lg = ui.painter().layout_no_wrap(
                                    lvl.label().to_string(),
                                    egui::FontId::proportional(12.5),
                                    fg,
                                );
                                ui.painter().galley(
                                    Pos2::new(
                                        r_rect.left() + 10.0,
                                        r_rect.center().y - lg.size().y * 0.5,
                                    ),
                                    lg,
                                    fg,
                                );
                                if is_cur {
                                    let cg = ui.painter().layout_no_wrap(
                                        "✓".to_string(),
                                        egui::FontId::proportional(12.0),
                                        palette.accent,
                                    );
                                    ui.painter().galley(
                                        Pos2::new(
                                            r_rect.right() - cg.size().x - 10.0,
                                            r_rect.center().y - cg.size().y * 0.5,
                                        ),
                                        cg,
                                        palette.accent,
                                    );
                                }
                                if r_resp.clicked() {
                                    compare_models.insert(pid, m.provider_model_id.clone());
                                    compare_thinking_levels.insert(pid, lvl);
                                    close_menu = true;
                                }
                            }
                        });
                });
            sub_rect_out = Some(sub_out.response.rect);
        }
    }
    state.submenu_rect = sub_rect_out;

    let (esc, any_click, click_pos) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            i.pointer.any_pressed(),
            i.pointer.interact_pos(),
        )
    });
    if esc {
        close_menu = true;
    } else if any_click && !clicked_trigger {
        if let Some(pos) = click_pos {
            let in_main = state.menu_rect.is_some_and(|r| r.contains(pos));
            let in_sub = state.submenu_rect.is_some_and(|r| r.contains(pos));
            if !in_main && !in_sub {
                close_menu = true;
            }
        }
    }

    if close_menu {
        state.open_provider = None;
        state.menu_rect = None;
        state.submenu_rect = None;
    }
}

#[allow(clippy::too_many_arguments)]
fn render_compare_column(
    app: &mut UmisongApp,
    compare_id: &str,
    isolation: CompareIsolationMode,
    col: &CompareColumnState,
    width: f32,
    height: f32,
    ui: &mut egui::Ui,
    palette: &Palette,
    adopt_req: &mut Option<(String, ProviderId, bool)>,
) {
    let p_col = palette.provider_color(col.provider);
    let running = col.status == TurnStatus::Running;

    let border_col = if col.adopted {
        palette.badge_signed_in_fg
    } else if running {
        egui::Color32::from_rgba_unmultiplied(p_col.r(), p_col.g(), p_col.b(), 150)
    } else {
        palette.border
    };

    egui::Frame::new()
        .fill(palette.bg_card)
        .stroke(Stroke::new(
            if col.adopted { 1.5_f32 } else { 1.0_f32 },
            border_col,
        ))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                let inner_w = (width - 30.0).max(220.0);
                ui.set_width(inner_w);
                ui.set_min_height(height - 26.0);

                // 1. Column Header: Provider dot + Display name + model pill + duration/status
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    paint_status_dot(ui, p_col, true);
                    ui.label(
                        RichText::new(col.provider.display_name())
                            .size(13.5)
                            .strong()
                            .color(palette.text_primary),
                    );

                    if col.adopted {
                        pill_badge(ui, "Winner · Adopted", palette.badge_signed_in_fg);
                    } else if running {
                        pill_badge(ui, "Streaming", p_col);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if let Some(ms) = col.duration_ms {
                            ui.label(
                                RichText::new(format!("{:.1}s", ms as f64 / 1000.0))
                                    .monospace()
                                    .size(11.5)
                                    .color(palette.text_muted),
                            );
                        }
                        if col.output_tokens > 0 {
                            ui.label(
                                RichText::new(format!("{} tok ·", col.output_tokens))
                                    .monospace()
                                    .size(11.0)
                                    .color(palette.text_muted),
                            );
                        }
                    });
                });

                let model_disp = col
                    .resolved_model
                    .as_deref()
                    .or((!col.model.is_empty()).then_some(col.model.as_str()))
                    .unwrap_or("account plan default");
                ui.label(
                    RichText::new(model_disp)
                        .monospace()
                        .size(11.0)
                        .color(palette.text_muted),
                );

                ui.add_space(8.0);

                // 2. Custom Segmented Tabs: `Response` | `Activity (N)` | `Diff (N)`
                let tab_entry = app
                    .compare_col_tabs
                    .entry((compare_id.to_string(), col.provider))
                    .or_insert(CompareColumnTab::Response);
                let mut active_tab = *tab_entry;

                egui::Frame::new()
                    .fill(palette.bg_code)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(3))
                    .show(ui, |ui| {
                        ui.set_width(inner_w - 8.0);
                        let seg_w = ((inner_w - 16.0) / 3.0).max(60.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let tabs = [
                                (CompareColumnTab::Response, "Response".to_string()),
                                (
                                    CompareColumnTab::Activity,
                                    format!("Activity ({})", col.tool_calls.len()),
                                ),
                                (
                                    CompareColumnTab::Diff,
                                    if col.diffs.is_empty() {
                                        "Diff".to_string()
                                    } else {
                                        format!("Diff ({})", col.diffs.len())
                                    },
                                ),
                            ];
                            for (tab_val, tab_lbl) in tabs {
                                let selected = active_tab == tab_val;
                                let (t_rect, t_resp) = ui.allocate_exact_size(
                                    Vec2::new(seg_w, 25.0),
                                    egui::Sense::click(),
                                );
                                let hov = t_resp.hovered();
                                if hov {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                let h = hover_factor(
                                    ui.ctx(),
                                    ui.make_persistent_id((
                                        "cmp_col_tab",
                                        compare_id,
                                        col.provider.as_str(),
                                        &tab_lbl,
                                    )),
                                    hov,
                                );
                                let fill = if selected {
                                    palette.bg_elevated
                                } else {
                                    egui::Color32::from_rgba_unmultiplied(
                                        palette.bg_elevated.r(),
                                        palette.bg_elevated.g(),
                                        palette.bg_elevated.b(),
                                        (h * 165.0) as u8,
                                    )
                                };
                                if fill.a() > 0 {
                                    ui.painter()
                                        .rect_filled(t_rect, CornerRadius::same(6), fill);
                                }
                                let fg = if selected {
                                    palette.text_primary
                                } else {
                                    lerp_color(palette.text_secondary, palette.text_primary, h)
                                };
                                let tg = ui.painter().layout_no_wrap(
                                    tab_lbl,
                                    egui::FontId::proportional(11.5),
                                    fg,
                                );
                                ui.painter().galley(
                                    t_rect.center() - tg.size() * 0.5,
                                    tg,
                                    fg,
                                );
                                if t_resp.clicked() {
                                    active_tab = tab_val;
                                }
                            }
                        });
                    });
                *tab_entry = active_tab;

                ui.add_space(10.0);

                // 3. Column Content Scroll Area
                let body_h = (height - 166.0).max(120.0);
                egui::ScrollArea::vertical()
                    .id_salt(("cmp_col_scroll", compare_id, col.provider.as_str()))
                    .min_scrolled_height(body_h)
                    .max_height(body_h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        match active_tab {
                            CompareColumnTab::Response => {
                                if !col.thought_text.is_empty() {
                                    tool_card::render_thought_line(
                                        ui,
                                        &format!(
                                            "cmp_inline_thought_{}_{}",
                                            compare_id,
                                            col.provider.as_str()
                                        ),
                                        &col.thought_text,
                                        col.duration_ms,
                                        running && col.response_text.is_empty(),
                                        palette,
                                    );
                                }
                                if !col.tool_calls.is_empty() {
                                    tool_card::render_tool_calls_summary(
                                        ui,
                                        &format!(
                                            "cmp_inline_tc_{}_{}",
                                            compare_id,
                                            col.provider.as_str()
                                        ),
                                        &col.tool_calls,
                                        palette,
                                    );
                                }
                                if !col.thought_text.is_empty() || !col.tool_calls.is_empty() {
                                    ui.add_space(6.0);
                                }
                                if let Some(err) = &col.error_message {
                                    ui.label(
                                        RichText::new(err).color(palette.danger).size(12.0),
                                    );
                                }
                                if col.response_text.is_empty() && running {
                                    ui.label(
                                        RichText::new("Streaming response…")
                                            .size(12.5)
                                            .color(palette.text_muted),
                                    );
                                } else {
                                    ui.push_id(
                                        ("cmp_md", compare_id, col.provider.as_str()),
                                        |ui| {
                                            markdown::render_markdown(
                                                ui,
                                                &col.response_text,
                                                palette,
                                            );
                                        },
                                    );
                                }
                            }
                            CompareColumnTab::Activity => {
                                if !col.thought_text.is_empty() {
                                    tool_card::render_thought_line(
                                        ui,
                                        &format!(
                                            "cmp_thought_{}_{}",
                                            compare_id,
                                            col.provider.as_str()
                                        ),
                                        &col.thought_text,
                                        col.duration_ms,
                                        running,
                                        palette,
                                    );
                                    ui.add_space(6.0);
                                }
                                if col.tool_calls.is_empty() {
                                    ui.label(
                                        RichText::new("No tool calls recorded yet.")
                                            .size(12.0)
                                            .color(palette.text_muted),
                                    );
                                } else {
                                    tool_card::render_tool_calls(
                                        ui,
                                        &format!(
                                            "cmp_tc_{}_{}",
                                            compare_id,
                                            col.provider.as_str()
                                        ),
                                        &col.tool_calls,
                                        palette,
                                    );
                                }
                            }
                            CompareColumnTab::Diff => {
                                if col.diffs.is_empty() {
                                    let hint = match isolation {
                                        CompareIsolationMode::ReadOnly => {
                                            "Read-only mode (no file edits)."
                                        }
                                        CompareIsolationMode::GitWorktree => {
                                            "No file modifications detected in this provider's worktree."
                                        }
                                    };
                                    ui.label(
                                        RichText::new(hint)
                                            .size(12.0)
                                            .color(palette.text_muted),
                                    );
                                } else {
                                    for f in &col.diffs {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&f.path)
                                                    .monospace()
                                                    .strong()
                                                    .size(12.0)
                                                    .color(palette.text_primary),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(format!("-{}", f.deletions))
                                                            .monospace()
                                                            .size(11.0)
                                                            .color(palette.danger),
                                                    );
                                                    ui.label(
                                                        RichText::new(format!("+{}", f.additions))
                                                            .monospace()
                                                            .size(11.0)
                                                            .color(palette.badge_signed_in_fg),
                                                    );
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                        diff_viewer::render_file_diff_body(
                                            ui,
                                            f,
                                            palette,
                                            &format!(
                                                "cmp_diff_{}_{}",
                                                compare_id,
                                                col.provider.as_str()
                                            ),
                                        );
                                        ui.add_space(8.0);
                                    }
                                }
                            }
                        }
                    });

                ui.add_space(8.0);
                let (sep_r, _) = ui
                    .allocate_exact_size(Vec2::new(inner_w, 1.0), egui::Sense::hover());
                ui.painter().hline(
                    sep_r.x_range(),
                    sep_r.center().y,
                    Stroke::new(1.0_f32, palette.border),
                );
                ui.add_space(8.0);

                // 4. Bottom Adopt Button (`Adopt & apply patch` or `Adopt & continue`)
                let has_patch = col
                    .diff_patch
                    .as_deref()
                    .is_some_and(|p| !p.trim().is_empty())
                    || !col.diffs.is_empty();
                let btn_label = if col.adopted {
                    "✓  Adopted winner"
                } else if has_patch {
                    "Adopt & apply patch"
                } else {
                    "Adopt & continue"
                };
                let can_adopt = !running && col.status != TurnStatus::Failed && !col.adopted;
                let sense = if can_adopt {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let (btn_rect, btn_resp) =
                    ui.allocate_exact_size(Vec2::new(inner_w, 32.0), sense);
                let hov = can_adopt && btn_resp.hovered();
                let pressed = can_adopt && btn_resp.is_pointer_button_down_on();
                if hov {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let h = hover_factor(
                    ui.ctx(),
                    ui.make_persistent_id(("cmp_adopt_btn", compare_id, col.provider.as_str())),
                    hov,
                );
                let fill = if col.adopted {
                    palette.badge_signed_in_bg
                } else if !can_adopt || pressed {
                    palette.bg_code
                } else {
                    lerp_color(palette.bg_elevated, palette.border_strong, h * 0.45)
                };
                let border = if col.adopted {
                    egui::Color32::from_rgba_unmultiplied(
                        palette.badge_signed_in_fg.r(),
                        palette.badge_signed_in_fg.g(),
                        palette.badge_signed_in_fg.b(),
                        110,
                    )
                } else if !can_adopt {
                    palette.border
                } else {
                    lerp_color(palette.border_strong, p_col, h * 0.65)
                };
                let draw_r = if pressed { btn_rect.shrink(1.0) } else { btn_rect };
                ui.painter().rect(
                    draw_r,
                    CornerRadius::same(8),
                    fill,
                    Stroke::new(1.0_f32, border),
                    egui::StrokeKind::Inside,
                );
                let fg = if col.adopted {
                    palette.badge_signed_in_fg
                } else if !can_adopt {
                    palette.text_muted
                } else {
                    palette.text_primary
                };
                let bg_galley = ui.painter().layout_no_wrap(
                    btn_label.to_string(),
                    egui::FontId::proportional(12.5),
                    fg,
                );
                ui.painter().galley(
                    draw_r.center() - bg_galley.size() * 0.5,
                    bg_galley,
                    fg,
                );
                if can_adopt && btn_resp.clicked() {
                    *adopt_req = Some((compare_id.to_string(), col.provider, has_patch));
                }
            });
        });
}
