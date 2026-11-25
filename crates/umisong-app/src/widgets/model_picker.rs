use crate::theme::{Palette, hover_factor, lerp_color};
use egui::{Align2, CornerRadius, Margin, Order, Pos2, Rect, RichText, Stroke, Vec2};
use std::collections::HashMap;
use umisong_core::{
    ProviderId, ThinkingLevel, UnifiedModelEntry, UnifiedModelGroup, find_unified_entry,
};

#[derive(Debug, Clone)]
pub enum ModelPickerAction {
    Selected {
        canonical_id: String,
        provider: ProviderId,
        provider_model_id: String,
        thinking_level: ThinkingLevel,
    },
    OpenAccounts,
}

#[derive(Debug, Clone, Default)]
struct PickerPopupState {
    open: bool,
    hovered_model: Option<String>,
    hovered_row_y: f32,
    menu_rect: Option<Rect>,
    submenu_rect: Option<Rect>,
}

/// Programmatically opens a model picker popover + cascading thinking submenu
/// (used by `UMISONG_PREVIEW=model_menu` for live screenshot review).
pub fn force_open_picker_for_preview(
    ctx: &egui::Context,
    id_salt: &str,
    hovered_canonical_id: String,
) {
    let state_id = egui::Id::new(("umisong_model_picker_state", id_salt));
    ctx.data_mut(|d| {
        let mut st: PickerPopupState = d.get_temp(state_id).unwrap_or_default();
        st.open = true;
        if st.hovered_model.is_none() {
            st.hovered_model = Some(hovered_canonical_id);
        }
        d.insert_temp(state_id, st);
    });
}

/// Renders a single unified model + thinking pill (`● Gemini 4 Neon · High ▾`):
/// - Single capsule pill (no duplicate thinking dropdown beside it)
/// - Grouped by family (`CLAUDE`, `GEMINI`, `CODEX`), ranked strongest-first
/// - Right-aligned thinking level + provider tag + chevron for crisp scannability
/// - Hovering any available model opens its per-model `THINKING` submenu right beside it
/// - Clicking a model row directly selects it immediately at its current thinking level
/// - Plan-locked models appear dimmed with an `Unavailable` pill + upgrade tooltip
pub fn render_unified_model_picker(
    ui: &mut egui::Ui,
    id_salt: &str,
    groups: &[UnifiedModelGroup],
    selected_canonical_id: &str,
    per_model_thinking: &HashMap<String, ThinkingLevel>,
    palette: &Palette,
) -> Option<ModelPickerAction> {
    let mut action: Option<ModelPickerAction> = None;

    if groups.is_empty() {
        let label = "Sign in to pick a model";
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(158.0, 28.0), egui::Sense::click());
        let hovered = resp.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let h = hover_factor(
            ui.ctx(),
            ui.make_persistent_id((id_salt, "empty_hover")),
            hovered,
        );
        let fill = lerp_color(palette.bg_card, palette.bg_elevated, h);
        let border = lerp_color(palette.border, palette.border_strong, h);
        ui.painter().rect(
            rect,
            CornerRadius::same(255),
            fill,
            Stroke::new(1.0_f32, border),
            egui::StrokeKind::Inside,
        );
        let text_col = lerp_color(palette.text_secondary, palette.text_primary, h);
        let galley = ui.painter().layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(12.0),
            text_col,
        );
        ui.painter()
            .galley(rect.center() - galley.size() * 0.5, galley, text_col);
        if resp.clicked() {
            return Some(ModelPickerAction::OpenAccounts);
        }
        return None;
    }

    let active_entry = find_unified_entry(groups, selected_canonical_id).or_else(|| {
        groups
            .iter()
            .flat_map(|g| g.models.iter())
            .find(|m| m.available)
    });

    let active_provider_color = active_entry
        .map(|e| palette.provider_color(e.primary_provider))
        .unwrap_or(palette.accent);

    let state_id = egui::Id::new(("umisong_model_picker_state", id_salt));
    let mut state: PickerPopupState = ui
        .ctx()
        .data(|d| d.get_temp(state_id))
        .unwrap_or_default();

    // Build pill text galley to measure exact capsule size
    let (model_text, level_text) = if let Some(entry) = active_entry {
        let cur_lvl = effective_level(entry, per_model_thinking);
        (entry.display_name.clone(), cur_lvl.label().to_string())
    } else {
        ("Select model".to_string(), String::new())
    };

    let mut pill_job = egui::text::LayoutJob::default();
    pill_job.append(
        &model_text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(12.5),
            color: active_provider_color,
            ..Default::default()
        },
    );
    if !level_text.is_empty() {
        pill_job.append(
            "  ·  ",
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(12.0),
                color: palette.text_muted,
                ..Default::default()
            },
        );
        pill_job.append(
            &level_text,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(12.0),
                color: palette.text_secondary,
                ..Default::default()
            },
        );
    }
    pill_job.append(
        if state.open { "  ▴" } else { "  ▾" },
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(10.5),
            color: active_provider_color,
            ..Default::default()
        },
    );

    let galley = ui.painter().layout_job(pill_job);
    let dot_space = 16.0;
    let pad_x = 12.0;
    let pill_w = pad_x + dot_space + galley.size().x + pad_x;
    let pill_h = 28.0;

    let (pill_rect, pill_resp) =
        ui.allocate_exact_size(Vec2::new(pill_w, pill_h), egui::Sense::click());
    let is_hovered = pill_resp.hovered();
    let is_pressed = pill_resp.is_pointer_button_down_on();

    if is_hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id((id_salt, "pill_hover")),
        is_hovered && !is_pressed && !state.open,
    );

    // Single provider color, zero gradients; cleanly darkens on click/open
    let rest_fill = lerp_color(palette.bg_card, active_provider_color, 0.13);
    let hover_fill = lerp_color(palette.bg_card, active_provider_color, 0.21);
    let dark_fill = lerp_color(palette.bg_code, active_provider_color, 0.08);

    let pill_fill = if is_pressed || state.open {
        dark_fill
    } else {
        lerp_color(rest_fill, hover_fill, h)
    };

    let rest_border = lerp_color(palette.border, active_provider_color, 0.38);
    let hover_border = lerp_color(palette.border_strong, active_provider_color, 0.58);
    let active_border = lerp_color(palette.border_strong, active_provider_color, 0.72);
    let pill_border = if is_pressed || state.open {
        active_border
    } else {
        lerp_color(rest_border, hover_border, h)
    };

    ui.painter().rect(
        pill_rect,
        CornerRadius::same(255),
        pill_fill,
        Stroke::new(1.0_f32, pill_border),
        egui::StrokeKind::Inside,
    );

    let dot_center = Pos2::new(pill_rect.left() + pad_x + 4.0, pill_rect.center().y);
    ui.painter()
        .circle_filled(dot_center, 3.8, active_provider_color);

    let text_pos = Pos2::new(
        pill_rect.left() + pad_x + dot_space,
        pill_rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(text_pos, galley, active_provider_color);

    let route_tip = active_entry
        .map(|entry| {
            if entry.alternate_providers.is_empty() {
                format!(
                    "Using {} ({})",
                    entry.primary_provider.display_name(),
                    entry.plan_badge
                )
            } else {
                let alts: Vec<&str> = entry
                    .alternate_providers
                    .iter()
                    .map(|p| p.display_name())
                    .collect();
                format!(
                    "Prioritizing {} account tokens ({}) · also on {}",
                    entry.primary_provider.display_name(),
                    entry.plan_badge,
                    alts.join(", ")
                )
            }
        })
        .unwrap_or_default();

    let clicked_pill = pill_resp.clicked();
    if !route_tip.is_empty() && !state.open {
        pill_resp.on_hover_text(route_tip);
    }

    if clicked_pill {
        state.open = !state.open;
        if state.open {
            state.hovered_model = active_entry.map(|e| e.canonical_id.clone());
        } else {
            state.hovered_model = None;
            state.menu_rect = None;
            state.submenu_rect = None;
        }
    }

    if state.open {
        // Close on Escape or click outside pill + menu + submenu
        let (esc_pressed, any_clicked, pointer_pos) = ui.ctx().input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.pointer.any_pressed(),
                i.pointer.interact_pos(),
            )
        });

        let clicked_outside = any_clicked
            && !clicked_pill
            && pointer_pos.is_some_and(|pos| {
                let in_pill = pill_rect.contains(pos);
                let in_menu = state.menu_rect.is_some_and(|r| r.contains(pos));
                let in_sub = state.submenu_rect.is_some_and(|r| r.contains(pos));
                !in_pill && !in_menu && !in_sub
            });

        if esc_pressed || clicked_outside {
            state.open = false;
            state.hovered_model = None;
            state.menu_rect = None;
            state.submenu_rect = None;
        }
    }

    if state.open {
        let screen_rect = ui.ctx().screen_rect();
        let open_above = pill_rect.top() > 320.0;
        let (anchor_pivot, anchor_pos) = if open_above {
            (Align2::LEFT_BOTTOM, Pos2::new(pill_rect.left(), pill_rect.top() - 8.0))
        } else {
            (Align2::LEFT_TOP, Pos2::new(pill_rect.left(), pill_rect.bottom() + 8.0))
        };

        let mut close_after = false;
        let mut new_menu_rect: Option<Rect> = None;
        let mut hovered_entry_for_sub: Option<(&UnifiedModelEntry, f32)> = None;

        let area_resp = egui::Area::new(egui::Id::new(("model_picker_popup", id_salt)))
            .order(Order::Foreground)
            .pivot(anchor_pivot)
            .fixed_pos(anchor_pos)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                let frame_out = egui::Frame::new()
                    .fill(palette.bg_card)
                    .stroke(Stroke::new(1.0_f32, palette.border_strong))
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(Margin::same(8))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 28,
                        spread: 0,
                        color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 155),
                    })
                    .show(ui, |ui| {
                        ui.set_width(328.0);
                        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);

                        for (g_idx, group) in groups.iter().enumerate() {
                            if g_idx > 0 {
                                ui.add_space(5.0);
                                draw_hairline(ui, palette.border);
                                ui.add_space(6.0);
                            }

                            // Family Group Header (CLAUDE / GEMINI / CODEX)
                            ui.horizontal(|ui| {
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new(group.family.display_name().to_uppercase())
                                        .size(10.5)
                                        .strong()
                                        .color(palette.text_muted),
                                );
                            });
                            ui.add_space(3.0);

                            for entry in &group.models {
                                let is_selected = entry.canonical_id == selected_canonical_id;
                                let cur_level = effective_level(entry, per_model_thinking);

                                if !entry.available {
                                    render_unavailable_row(ui, entry, palette);
                                    continue;
                                }

                                let is_sub_active = state
                                    .hovered_model
                                    .as_deref()
                                    == Some(entry.canonical_id.as_str());

                                let (row_rect, row_resp) = render_available_model_row(
                                    ui,
                                    (id_salt, &entry.canonical_id),
                                    entry,
                                    cur_level,
                                    is_selected,
                                    is_sub_active,
                                    palette,
                                );

                                if row_resp.hovered() {
                                    state.hovered_model = Some(entry.canonical_id.clone());
                                    state.hovered_row_y = row_rect.top();
                                }

                                if state.hovered_model.as_deref()
                                    == Some(entry.canonical_id.as_str())
                                {
                                    hovered_entry_for_sub = Some((entry, row_rect.top()));
                                }

                                if row_resp.clicked() {
                                    action = Some(ModelPickerAction::Selected {
                                        canonical_id: entry.canonical_id.clone(),
                                        provider: entry.primary_provider,
                                        provider_model_id: entry.provider_model_id.clone(),
                                        thinking_level: cur_level,
                                    });
                                    close_after = true;
                                }
                            }
                        }

                        ui.add_space(5.0);
                        draw_hairline(ui, palette.border);
                        ui.add_space(4.0);

                        if render_menu_footer_link(
                            ui,
                            (id_salt, "footer_accounts"),
                            "Manage accounts & plan tiers…",
                            palette,
                        ) {
                            action = Some(ModelPickerAction::OpenAccounts);
                            close_after = true;
                        }
                    });
                new_menu_rect = Some(frame_out.response.rect);
            });

        let menu_rect = new_menu_rect.unwrap_or(area_resp.response.rect);
        state.menu_rect = Some(menu_rect);

        // Cascading THINKING Submenu anchored right beside the hovered model row
        let mut new_sub_rect: Option<Rect> = None;
        if let Some((entry, row_y)) = hovered_entry_for_sub {
            let cur_level = effective_level(entry, per_model_thinking);
            let sub_w = 160.0;
            let place_right = menu_rect.right() + 8.0 + sub_w <= screen_rect.right() - 8.0;
            let sub_x = if place_right {
                menu_rect.right() + 6.0
            } else {
                (menu_rect.left() - sub_w - 6.0).max(8.0)
            };
            let sub_y = (row_y - 36.0).clamp(12.0, screen_rect.bottom() - 196.0);

            let sub_area = egui::Area::new(egui::Id::new(("model_thinking_submenu", id_salt)))
                .order(Order::Foreground)
                .fixed_pos(Pos2::new(sub_x, sub_y))
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    let frame_out = egui::Frame::new()
                        .fill(palette.bg_card)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(CornerRadius::same(14))
                        .inner_margin(Margin::same(8))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 10],
                            blur: 24,
                            spread: 0,
                            color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 145),
                        })
                        .show(ui, |ui| {
                            ui.set_width(sub_w);
                            ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);

                            ui.horizontal(|ui| {
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new("THINKING")
                                        .size(10.5)
                                        .strong()
                                        .color(palette.text_muted),
                                );
                            });
                            ui.add_space(4.0);
                            draw_hairline(ui, palette.border);
                            ui.add_space(4.0);

                            for &lvl in &entry.supported_thinking_levels {
                                let is_cur = cur_level == lvl;
                                if render_thinking_row(
                                    ui,
                                    (id_salt, &entry.canonical_id, lvl.as_str()),
                                    lvl.label(),
                                    is_cur,
                                    palette,
                                ) {
                                    action = Some(ModelPickerAction::Selected {
                                        canonical_id: entry.canonical_id.clone(),
                                        provider: entry.primary_provider,
                                        provider_model_id: entry.provider_model_id.clone(),
                                        thinking_level: lvl,
                                    });
                                    close_after = true;
                                }
                            }
                        });
                    new_sub_rect = Some(frame_out.response.rect);
                });
            state.submenu_rect = Some(new_sub_rect.unwrap_or(sub_area.response.rect));
        } else {
            state.submenu_rect = None;
        }

        if close_after {
            state.open = false;
            state.hovered_model = None;
            state.menu_rect = None;
            state.submenu_rect = None;
        }
    }

    ui.ctx().data_mut(|d| d.insert_temp(state_id, state));

    action
}

fn draw_hairline(ui: &mut egui::Ui, color: egui::Color32) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0_f32, color));
}

fn render_available_model_row(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    entry: &UnifiedModelEntry,
    cur_level: ThinkingLevel,
    is_selected: bool,
    is_sub_active: bool,
    palette: &Palette,
) -> (Rect, egui::Response) {
    let p_col = palette.provider_color(entry.primary_provider);
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), egui::Sense::click());

    let hovered = resp.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id(("model_row_h", id_salt)),
        hovered || is_sub_active,
    );
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
            ui.spacing_mut().item_spacing.x = 6.0;

            // Fixed 12px check/dot column so all model names line up cleanly
            let (mark_rect, _) =
                ui.allocate_exact_size(Vec2::new(12.0, 14.0), egui::Sense::hover());
            if is_selected {
                let g = ui.painter().layout_no_wrap(
                    "✓".to_string(),
                    egui::FontId::proportional(11.5),
                    p_col,
                );
                ui.painter()
                    .galley(mark_rect.center() - g.size() * 0.5, g, p_col);
            } else {
                paint_status_dot_at(ui.painter(), mark_rect.center(), p_col, false);
            }

            ui.label(
                RichText::new(&entry.display_name)
                    .size(12.5)
                    .strong()
                    .color(if is_selected { p_col } else { palette.text_primary }),
            );

            // Right-aligned metadata: Thinking level + Provider tag + Chevron
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new("▸").size(10.5).color(if hovered || is_sub_active {
                        palette.text_primary
                    } else {
                        palette.text_muted
                    }),
                );
                ui.label(
                    RichText::new(entry.primary_provider.short_label())
                        .size(10.5)
                        .color(palette.text_muted),
                );
                ui.label(
                    RichText::new(cur_level.label())
                        .size(11.5)
                        .color(palette.text_secondary),
                );
            });
        });
    });

    let hover_tip = if entry.alternate_providers.is_empty() {
        format!(
            "Routes via {} ({})",
            entry.primary_provider.display_name(),
            entry.plan_badge
        )
    } else {
        let alts: Vec<&str> = entry
            .alternate_providers
            .iter()
            .map(|p| p.display_name())
            .collect();
        format!(
            "Prioritizes {} account tokens ({}) · also on {}",
            entry.primary_provider.display_name(),
            entry.plan_badge,
            alts.join(", ")
        )
    };

    let resp = resp.on_hover_text(hover_tip);
    (rect, resp)
}

fn paint_status_dot_at(painter: &egui::Painter, center: Pos2, color: egui::Color32, filled: bool) {
    if filled {
        painter.circle_filled(center, 3.2, color);
    } else {
        painter.circle_stroke(
            center,
            3.0,
            Stroke::new(
                1.2_f32,
                egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 150),
            ),
        );
    }
}

fn render_thinking_row(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    label: &str,
    is_selected: bool,
    palette: &Palette,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::click());
    let hovered = resp.hovered();

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id(("think_row", id_salt)),
        hovered,
    );
    let bg = if is_selected {
        lerp_color(palette.accent_soft_bg, palette.bg_elevated, h * 0.22)
    } else {
        egui::Color32::from_rgba_unmultiplied(
            palette.bg_elevated.r(),
            palette.bg_elevated.g(),
            palette.bg_elevated.b(),
            (h * 255.0) as u8,
        )
    };

    if bg.a() > 0 {
        ui.painter().rect_filled(rect, CornerRadius::same(8), bg);
    }

    let inner_rect = rect.shrink2(Vec2::new(10.0, 0.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(label)
                    .size(12.5)
                    .strong()
                    .color(if is_selected {
                        palette.accent_soft_fg
                    } else {
                        palette.text_primary
                    }),
            );
            if is_selected {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("✓").size(12.0).strong().color(palette.accent));
                });
            }
        });
    });

    resp.clicked()
}

fn render_menu_footer_link(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    label: &str,
    palette: &Palette,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::click());
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
    let inner_rect = rect.shrink2(Vec2::new(10.0, 0.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.label(RichText::new(label).size(12.0).color(palette.accent));
        });
    });
    resp.clicked()
}

fn render_unavailable_row(ui: &mut egui::Ui, entry: &UnifiedModelEntry, palette: &Palette) {
    let reason = entry
        .unavailable_reason
        .as_deref()
        .unwrap_or("Unavailable on your current plan");

    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::hover());

    let inner_rect = rect.shrink2(Vec2::new(10.0, 0.0));
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner_rect), |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            // Match the exact 12px icon + 6px gap column of available rows
            ui.add_space(18.0);
            ui.label(
                RichText::new(&entry.display_name)
                    .size(12.5)
                    .color(egui::Color32::from_rgba_unmultiplied(
                        palette.text_muted.r(),
                        palette.text_muted.g(),
                        palette.text_muted.b(),
                        160,
                    )),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let badge = egui::Button::new(
                    RichText::new("Unavailable")
                        .size(10.5)
                        .color(palette.text_muted),
                )
                .fill(palette.bg_elevated)
                .stroke(Stroke::new(1.0_f32, palette.border))
                .corner_radius(CornerRadius::same(255))
                .sense(egui::Sense::hover());
                ui.add(badge.min_size(Vec2::new(0.0, 21.0)));
            });
        });
    });

    resp.on_hover_text(reason);
}

pub fn effective_level(
    entry: &UnifiedModelEntry,
    per_model_thinking: &HashMap<String, ThinkingLevel>,
) -> ThinkingLevel {
    per_model_thinking
        .get(&entry.canonical_id)
        .copied()
        .map(|lvl| entry.clamp_thinking_level(lvl))
        .unwrap_or(entry.default_thinking_level)
}
