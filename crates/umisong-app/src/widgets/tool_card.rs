use crate::theme::{Palette, hover_factor, lerp_color};
use egui::{CornerRadius, Margin, RichText, Stroke, Vec2};
use umisong_core::{ToolCallRecord, ToolKind};

/// Minimal inline disclosure line (`Thought for 12s ›`) matching the frameless chat design.
pub fn render_thought_line(
    ui: &mut egui::Ui,
    id_salt: &str,
    thought_text: &str,
    duration_ms: Option<u64>,
    running: bool,
    palette: &Palette,
) {
    let trimmed = thought_text.trim();
    if trimmed.is_empty() && !running {
        return;
    }

    let open_id = egui::Id::new(("thought_line_open", id_salt));
    let mut is_open: bool = ui.ctx().data(|d| d.get_temp(open_id)).unwrap_or(false);

    let label = if running {
        "Thinking…".to_string()
    } else if let Some(ms) = duration_ms {
        let secs = ((ms as f64) / 1000.0).round().max(1.0) as u64;
        format!("Thought for {secs}s")
    } else {
        let secs = ((trimmed.len() as f64 / 45.0).round() as u64).clamp(2, 25);
        format!("Thought for {secs}s")
    };
    let chevron = if is_open { "▾" } else { "›" };

    let text_str = format!("{label}  {chevron}");
    let font = egui::FontId::proportional(12.5);
    let approx_w = ui
        .painter()
        .layout_no_wrap(text_str.clone(), font.clone(), palette.text_muted)
        .size()
        .x
        + 8.0;

    let (rect, resp) = ui.allocate_exact_size(Vec2::new(approx_w, 22.0), egui::Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id(("thought_line_h", id_salt)),
        hovered,
    );
    let col = lerp_color(palette.text_muted, palette.text_primary, h);
    let galley = ui.painter().layout_no_wrap(text_str, font, col);
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5),
        galley,
        col,
    );

    if resp.clicked() && !trimmed.is_empty() {
        is_open = !is_open;
    }

    if is_open && !trimmed.is_empty() {
        ui.add_space(4.0);
        let text_resp = ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.add(
                egui::Label::new(
                    RichText::new(trimmed)
                        .size(12.5)
                        .color(palette.text_secondary),
                )
                .wrap()
                .selectable(true),
            );
        });
        let bar_x = text_resp.response.rect.left() + 2.0;
        ui.painter().vline(
            bar_x,
            text_resp.response.rect.y_range(),
            Stroke::new(2.0_f32, palette.border_strong),
        );
        ui.add_space(4.0);
    }

    ui.ctx().data_mut(|d| d.insert_temp(open_id, is_open));
}

/// Forces a turn's tool summary and a specific tool call index open for deterministic preview screenshots.
pub fn force_open_tool_summary_for_preview(
    ctx: &egui::Context,
    prefix: &str,
    call_id: &str,
    call_idx: usize,
) {
    let summary_open_id = egui::Id::new(("tool_summary_open", prefix));
    let item_open_id = egui::Id::new(("tool_card_open", prefix, call_id, call_idx));
    ctx.data_mut(|d| {
        d.insert_temp(summary_open_id, true);
        d.insert_temp(item_open_id, true);
    });
}

/// Renders a single compact inline summary row (`✓ Edited calculator.py · Ran 2 commands ›`)
/// that expands on click to show a tree-indented list of individual tool calls.
pub fn render_tool_calls_summary(
    ui: &mut egui::Ui,
    prefix: &str,
    calls: &[ToolCallRecord],
    palette: &Palette,
) {
    if calls.is_empty() {
        return;
    }

    let open_id = egui::Id::new(("tool_summary_open", prefix));
    let mut is_open: bool = ui.ctx().data(|d| d.get_temp(open_id)).unwrap_or(false);

    let any_running = calls.iter().any(|c| c.success.is_none());
    let any_failed = calls.iter().any(|c| c.success == Some(false));
    let summary_text = summarize_tool_calls(calls);
    let chevron = if is_open { "▾" } else { "›" };

    let (status_glyph, status_col) = if any_running {
        ("●", palette.badge_signed_out_fg)
    } else if any_failed {
        ("✕", palette.danger)
    } else {
        ("✓", palette.badge_signed_in_fg)
    };

    let font = egui::FontId::proportional(12.5);
    let full_label = format!("{summary_text}  {chevron}");
    let text_w = ui
        .painter()
        .layout_no_wrap(full_label.clone(), font.clone(), palette.text_secondary)
        .size()
        .x;

    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(18.0 + text_w + 8.0, 22.0), egui::Sense::click());
    let hovered = resp.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let h = hover_factor(
        ui.ctx(),
        ui.make_persistent_id(("tool_sum_h", prefix)),
        hovered,
    );

    let icon_g = ui.painter().layout_no_wrap(
        status_glyph.to_string(),
        egui::FontId::proportional(12.0),
        status_col,
    );
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - icon_g.size().y * 0.5),
        icon_g,
        status_col,
    );

    let text_col = lerp_color(palette.text_secondary, palette.text_primary, h);
    let text_g = ui.painter().layout_no_wrap(full_label, font, text_col);
    ui.painter().galley(
        egui::pos2(rect.left() + 17.0, rect.center().y - text_g.size().y * 0.5),
        text_g,
        text_col,
    );

    if resp.clicked() {
        is_open = !is_open;
    }

    if is_open {
        ui.add_space(6.0);
        render_tool_calls_tree(ui, prefix, calls, palette);
        ui.add_space(6.0);
    }

    ui.ctx().data_mut(|d| d.insert_temp(open_id, is_open));
}

fn summarize_tool_calls(calls: &[ToolCallRecord]) -> String {
    let mut edited_files: Vec<String> = Vec::new();
    let mut read_files: Vec<String> = Vec::new();
    let mut cmd_count = 0usize;
    let mut search_count = 0usize;
    let mut other_count = 0usize;

    for tc in calls {
        let lower_name = tc.name.to_ascii_lowercase();
        let kind = if matches!(tc.kind, ToolKind::FileEdit)
            || lower_name.contains("write")
            || lower_name.contains("edit")
            || lower_name.contains("replace")
            || lower_name.contains("create")
        {
            ToolKind::FileEdit
        } else if matches!(tc.kind, ToolKind::Shell)
            || lower_name.contains("command")
            || lower_name.contains("bash")
            || lower_name.contains("exec")
        {
            ToolKind::Shell
        } else if matches!(tc.kind, ToolKind::FileRead)
            || lower_name.contains("read")
            || lower_name.contains("view")
            || lower_name.contains("list")
        {
            ToolKind::FileRead
        } else if matches!(tc.kind, ToolKind::Search)
            || lower_name.contains("grep")
            || lower_name.contains("find")
            || lower_name.contains("search")
        {
            ToolKind::Search
        } else {
            tc.kind
        };

        match kind {
            ToolKind::FileEdit => {
                let fname = extract_basename(&tc.input_summary);
                if !fname.is_empty() && !edited_files.contains(&fname) {
                    edited_files.push(fname);
                } else if edited_files.is_empty() {
                    edited_files.push("file".to_string());
                }
            }
            ToolKind::Shell => cmd_count += 1,
            ToolKind::FileRead => {
                let fname = extract_basename(&tc.input_summary);
                if !fname.is_empty() && !read_files.contains(&fname) {
                    read_files.push(fname);
                } else {
                    read_files.push("file".to_string());
                }
            }
            ToolKind::Search => search_count += 1,
            _ => other_count += 1,
        }
    }

    let mut parts: Vec<String> = Vec::new();
    if !edited_files.is_empty() {
        if edited_files.len() == 1 {
            parts.push(format!("Edited {}", edited_files[0]));
        } else {
            parts.push(format!(
                "Edited {} + {} more",
                edited_files[0],
                edited_files.len() - 1
            ));
        }
    }
    if !read_files.is_empty() {
        if read_files.len() == 1 && read_files[0] != "file" {
            parts.push(format!("Read {}", read_files[0]));
        } else {
            parts.push(format!(
                "Read {} {}",
                read_files.len(),
                if read_files.len() == 1 { "file" } else { "files" }
            ));
        }
    }
    if cmd_count > 0 {
        parts.push(format!(
            "Ran {} {}",
            cmd_count,
            if cmd_count == 1 { "command" } else { "commands" }
        ));
    }
    if search_count > 0 {
        parts.push(format!(
            "Ran {} {}",
            search_count,
            if search_count == 1 { "search" } else { "searches" }
        ));
    }
    if parts.is_empty() && other_count > 0 {
        parts.push(format!(
            "Ran {} {}",
            other_count,
            if other_count == 1 { "tool" } else { "tools" }
        ));
    }

    parts.join(" · ")
}

fn extract_basename(summary: &str) -> String {
    let cleaned = summary
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '{' || c == '}');
    // If raw JSON like `"Path":"/Users/foo/bar.py"`, extract the path token
    let token = if let Some(idx) = cleaned.find(":\"") {
        cleaned[idx + 2..]
            .split('"')
            .next()
            .unwrap_or(cleaned)
    } else {
        cleaned.split_whitespace().next().unwrap_or(cleaned)
    };
    let without_lines = token.split(':').next().unwrap_or(token);
    std::path::Path::new(without_lines)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

fn classify_tool_kind(tc: &ToolCallRecord) -> ToolKind {
    let lower_name = tc.name.to_ascii_lowercase();
    if matches!(tc.kind, ToolKind::FileEdit)
        || lower_name.contains("write")
        || lower_name.contains("edit")
        || lower_name.contains("replace")
        || lower_name.contains("create")
    {
        ToolKind::FileEdit
    } else if matches!(tc.kind, ToolKind::Shell)
        || lower_name.contains("command")
        || lower_name.contains("bash")
        || lower_name.contains("exec")
    {
        ToolKind::Shell
    } else if matches!(tc.kind, ToolKind::FileRead)
        || lower_name.contains("read")
        || lower_name.contains("view")
        || lower_name.contains("list")
    {
        ToolKind::FileRead
    } else if matches!(tc.kind, ToolKind::Search)
        || lower_name.contains("grep")
        || lower_name.contains("find")
        || lower_name.contains("search")
    {
        ToolKind::Search
    } else {
        tc.kind
    }
}

fn clean_tool_target(summary: &str, kind: ToolKind) -> String {
    let trimmed = summary.trim();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(obj) = val.as_object() {
            for key in [
                "CommandLine",
                "command",
                "cmd",
                "Path",
                "file_path",
                "path",
                "Query",
                "query",
                "pattern",
            ] {
                if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
                    return s.to_string();
                }
            }
        }
    }
    if matches!(kind, ToolKind::FileEdit | ToolKind::FileRead) {
        if let Some(idx) = trimmed.find(":\"") {
            if let Some(rest) = trimmed[idx + 2..].split('"').next() {
                return rest.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// Tree-indented frameless tool-call list shown when `✓ Edited … · Ran … ▾` is expanded.
fn render_tool_calls_tree(
    ui: &mut egui::Ui,
    prefix: &str,
    calls: &[ToolCallRecord],
    palette: &Palette,
) {
    let outer = ui.horizontal(|ui| {
        ui.add_space(18.0);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;

            for (idx, tc) in calls.iter().enumerate() {
                let kind = classify_tool_kind(tc);
                let verb = match kind {
                    ToolKind::FileEdit => "Edited",
                    ToolKind::Shell => "Ran",
                    ToolKind::FileRead => "Read",
                    ToolKind::Search => "Searched",
                    ToolKind::Web => "Fetched",
                    ToolKind::Other => tc.name.as_str(),
                };
                let target = clean_tool_target(&tc.input_summary, kind);
                // Match the clean mockup: expandable output is for shell / command / failure outputs
                let has_expandable_output = !tc.output_text.trim().is_empty()
                    && (!matches!(kind, ToolKind::FileEdit) || tc.success == Some(false));

                let open_id = egui::Id::new(("tool_card_open", prefix, &tc.call_id, idx));
                let mut is_open: bool =
                    ui.ctx().data(|d| d.get_temp(open_id)).unwrap_or(false);

                let (status_glyph, status_col) = match tc.success {
                    Some(true) => ("✓", palette.badge_signed_in_fg),
                    Some(false) => ("✕", palette.danger),
                    None => ("●", palette.badge_signed_out_fg),
                };

                let sense = if has_expandable_output {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let (row_rect, row_resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), sense);
                let hovered = has_expandable_output && row_resp.hovered();
                if hovered {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let h = hover_factor(
                    ui.ctx(),
                    ui.make_persistent_id(("tool_tree_row_h", prefix, idx)),
                    hovered,
                );

                // Left status glyph (`✓`)
                let icon_g = ui.painter().layout_no_wrap(
                    status_glyph.to_string(),
                    egui::FontId::proportional(11.5),
                    status_col,
                );
                ui.painter().galley(
                    egui::pos2(
                        row_rect.left(),
                        row_rect.center().y - icon_g.size().y * 0.5,
                    ),
                    icon_g,
                    status_col,
                );

                // Strong action verb (`Edited`, `Ran`)
                let verb_col = lerp_color(palette.text_primary, palette.accent, h * 0.15);
                let verb_g = ui.painter().layout_no_wrap(
                    verb.to_string(),
                    egui::FontId::proportional(12.5),
                    verb_col,
                );
                let verb_x = row_rect.left() + 17.0;
                let verb_w = verb_g.size().x;
                ui.painter().galley(
                    egui::pos2(verb_x, row_rect.center().y - verb_g.size().y * 0.5),
                    verb_g,
                    verb_col,
                );

                // Right-aligned duration (`2.0s`) + optional disclosure chevron (`›` / `▾`)
                let chevron_reserve = 14.0;
                if has_expandable_output {
                    let chev = if is_open { "▾" } else { "›" };
                    let chev_g = ui.painter().layout_no_wrap(
                        chev.to_string(),
                        egui::FontId::proportional(12.0),
                        lerp_color(palette.text_muted, palette.text_primary, h),
                    );
                    ui.painter().galley(
                        egui::pos2(
                            row_rect.right() - chev_g.size().x - 2.0,
                            row_rect.center().y - chev_g.size().y * 0.5,
                        ),
                        chev_g,
                        lerp_color(palette.text_muted, palette.text_primary, h),
                    );
                }

                let mut dur_w = 0.0;
                if let Some(ms) = tc.duration_ms {
                    let dur_str = format!("{:.1}s", (ms as f64 / 1000.0).max(0.1));
                    let dur_g = ui.painter().layout_no_wrap(
                        dur_str,
                        egui::FontId::monospace(11.5),
                        palette.text_muted,
                    );
                    dur_w = dur_g.size().x;
                    let dur_right = row_rect.right() - chevron_reserve - 4.0;
                    ui.painter().galley(
                        egui::pos2(
                            dur_right - dur_w,
                            row_rect.center().y - dur_g.size().y * 0.5,
                        ),
                        dur_g,
                        palette.text_muted,
                    );
                }

                // Monospace target path or command string
                let target_x = verb_x + verb_w + 8.0;
                let max_target_w =
                    (row_rect.right() - chevron_reserve - dur_w - 16.0 - target_x).max(40.0);
                let target_rect = egui::Rect::from_min_size(
                    egui::pos2(target_x, row_rect.top()),
                    Vec2::new(max_target_w, row_rect.height()),
                );
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(target_rect), |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(&target)
                                    .monospace()
                                    .size(12.0)
                                    .color(lerp_color(
                                        palette.text_secondary,
                                        palette.text_primary,
                                        h * 0.65,
                                    )),
                            )
                            .truncate(),
                        );
                    });
                });

                if has_expandable_output && row_resp.clicked() {
                    is_open = !is_open;
                }

                if has_expandable_output && is_open {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(17.0);
                        egui::Frame::new()
                            .fill(palette.bg_card)
                            .stroke(Stroke::new(1.0_f32, palette.border))
                            .corner_radius(CornerRadius::same(10))
                            .inner_margin(Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.set_width((ui.available_width() - 4.0).max(160.0));
                                egui::ScrollArea::vertical()
                                    .max_height(150.0)
                                    .id_salt((prefix, "tree_out", idx))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(tc.output_text.trim_end())
                                                    .monospace()
                                                    .size(12.0)
                                                    .color(palette.text_secondary),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    });
                            });
                    });
                    ui.add_space(4.0);
                }

                ui.ctx().data_mut(|d| d.insert_temp(open_id, is_open));
            }
        });
    });

    // Subtle vertical tree guide line on the left (`1px #232A36`)
    let line_x = outer.response.rect.left() + 5.0;
    ui.painter().vline(
        line_x,
        outer.response.rect.y_range(),
        Stroke::new(1.0_f32, palette.border),
    );
}

pub fn render_tool_calls(
    ui: &mut egui::Ui,
    prefix: &str,
    calls: &[ToolCallRecord],
    palette: &Palette,
) {
    if calls.is_empty() {
        return;
    }

    for (idx, tc) in calls.iter().enumerate() {
        let has_output = !tc.output_text.trim().is_empty();
        let open_id = egui::Id::new(("tool_card_open", prefix, &tc.call_id, idx));
        let mut is_open: bool = ui.ctx().data(|d| d.get_temp(open_id)).unwrap_or(false);

        let (status_label, status_bg, status_fg) = match tc.success {
            Some(true) => ("Done", palette.badge_signed_in_bg, palette.badge_signed_in_fg),
            Some(false) => (
                "Failed",
                egui::Color32::from_rgba_unmultiplied(
                    palette.danger.r(),
                    palette.danger.g(),
                    palette.danger.b(),
                    28,
                ),
                palette.danger,
            ),
            None => (
                "Running",
                palette.badge_signed_out_bg,
                palette.badge_signed_out_fg,
            ),
        };

        let kind_tag = match tc.kind {
            ToolKind::Shell => "CMD",
            ToolKind::FileRead => "READ",
            ToolKind::FileEdit => "EDIT",
            ToolKind::Search => "FIND",
            ToolKind::Web => "WEB",
            ToolKind::Other => "TOOL",
        };

        let card_rect_id = egui::Id::new(("tool_card_rect", prefix, &tc.call_id, idx));
        let prev_rect: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(card_rect_id));
        let hovered = has_output && prev_rect.is_some_and(|r| ui.rect_contains_pointer(r));
        let h = hover_factor(
            ui.ctx(),
            ui.make_persistent_id(("tool_card_h", prefix, idx)),
            hovered,
        );

        let frame_out = egui::Frame::new()
            .fill(palette.bg_code)
            .stroke(Stroke::new(
                1.0_f32,
                lerp_color(palette.border, palette.border_strong, h),
            ))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                let sense = if has_output {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let (hdr_rect, hdr_resp) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), sense);

                if has_output && hdr_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }

                if h > 0.01 {
                    let bg = egui::Color32::from_rgba_unmultiplied(
                        palette.bg_elevated.r(),
                        palette.bg_elevated.g(),
                        palette.bg_elevated.b(),
                        (h * 155.0) as u8,
                    );
                    let cr = if is_open {
                        CornerRadius {
                            nw: 10,
                            ne: 10,
                            sw: 0,
                            se: 0,
                        }
                    } else {
                        CornerRadius::same(10)
                    };
                    ui.painter().rect_filled(hdr_rect, cr, bg);
                }

                let inner = hdr_rect.shrink2(Vec2::new(10.0, 0.0));
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;

                        // Fixed-size neutral kind tag (never stretches vertically inside row)
                        let (tag_rect, _) = ui.allocate_exact_size(
                            Vec2::new(38.0, 20.0),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect(
                            tag_rect,
                            CornerRadius::same(6),
                            palette.bg_elevated,
                            Stroke::new(1.0_f32, palette.border),
                            egui::StrokeKind::Inside,
                        );
                        let tag_g = ui.painter().layout_no_wrap(
                            kind_tag.to_string(),
                            egui::FontId::monospace(10.0),
                            palette.text_secondary,
                        );
                        ui.painter().galley(
                            tag_rect.center() - tag_g.size() * 0.5,
                            tag_g,
                            palette.text_secondary,
                        );

                        ui.label(
                            RichText::new(&tc.name)
                                .size(12.5)
                                .strong()
                                .color(palette.text_primary),
                        );

                        // Left-aligned command / path summary (truncated gracefully before right pills)
                        let right_reserve = 144.0;
                        let summary_w = (ui.available_width() - right_reserve).max(40.0);
                        ui.allocate_ui_with_layout(
                            Vec2::new(summary_w, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&tc.input_summary)
                                            .monospace()
                                            .size(11.5)
                                            .color(palette.text_muted),
                                    )
                                    .truncate(),
                                );
                            },
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if has_output {
                                ui.label(
                                    RichText::new(if is_open { "▾" } else { "▸" })
                                        .size(11.0)
                                        .color(palette.text_muted),
                                );
                            }
                            compact_status_pill(ui, status_label, status_bg, status_fg);
                            if let Some(ms) = tc.duration_ms {
                                let dur = if ms >= 1000 {
                                    format!("{:.1}s", ms as f64 / 1000.0)
                                } else {
                                    format!("{ms}ms")
                                };
                                ui.label(
                                    RichText::new(dur)
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.text_muted),
                                );
                            }
                        });
                    });
                });

                if has_output && hdr_resp.clicked() {
                    is_open = !is_open;
                }

                if has_output && is_open {
                    let (sep_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 1.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().hline(
                        sep_rect.x_range(),
                        sep_rect.center().y,
                        Stroke::new(1.0_f32, palette.border),
                    );

                    egui::Frame::new()
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical()
                                .max_height(150.0)
                                .id_salt((prefix, "out", idx))
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(tc.output_text.trim_end())
                                                .monospace()
                                                .size(11.5)
                                                .color(palette.text_secondary),
                                        )
                                        .wrap()
                                        .selectable(true),
                                    );
                                });
                        });
                }
            });

        let card_rect = frame_out.response.rect;
        ui.ctx().data_mut(|d| {
            d.insert_temp(open_id, is_open);
            d.insert_temp(card_rect_id, card_rect);
        });

        if idx + 1 < calls.len() {
            ui.add_space(6.0);
        }
    }
}

fn compact_status_pill(
    ui: &mut egui::Ui,
    label: &str,
    bg: egui::Color32,
    fg: egui::Color32,
) {
    let pill = egui::Button::new(RichText::new(label).size(10.5).strong().color(fg))
        .fill(bg)
        .stroke(Stroke::new(
            1.0_f32,
            egui::Color32::from_rgba_unmultiplied(fg.r(), fg.g(), fg.b(), 65),
        ))
        .corner_radius(CornerRadius::same(255))
        .sense(egui::Sense::hover());
    ui.add(pill.min_size(Vec2::new(0.0, 21.0)));
}

pub struct PendingApprovalUi {
    pub approval_id: String,
    pub tool_name: String,
    pub summary: String,
    pub detail: String,
}

/// Renders a polished Safe-mode tool approval banner. Returns `Some(true)` on Approve, `Some(false)` on Reject.
pub fn render_approval_banner(
    ui: &mut egui::Ui,
    approval: &PendingApprovalUi,
    palette: &Palette,
) -> Option<bool> {
    let mut decision = None;
    let amber_border = egui::Color32::from_rgba_unmultiplied(
        palette.badge_signed_out_fg.r(),
        palette.badge_signed_out_fg.g(),
        palette.badge_signed_out_fg.b(),
        120,
    );

    egui::Frame::new()
        .fill(palette.bg_card)
        .stroke(Stroke::new(1.0_f32, amber_border))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                compact_status_pill(
                    ui,
                    "Safe mode · Approval required",
                    palette.badge_signed_out_bg,
                    palette.badge_signed_out_fg,
                );
                ui.label(
                    RichText::new(&approval.tool_name)
                        .size(13.0)
                        .strong()
                        .color(palette.text_primary),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    if tactile_action_btn(
                        ui,
                        ("appr_reject", &approval.approval_id),
                        "Reject",
                        Vec2::new(72.0, 28.0),
                        palette.bg_elevated,
                        palette.border_strong,
                        palette.danger,
                    )
                    .clicked()
                    {
                        decision = Some(false);
                    }

                    if tactile_action_btn(
                        ui,
                        ("appr_approve", &approval.approval_id),
                        "Approve",
                        Vec2::new(80.0, 28.0),
                        palette.badge_signed_in_bg,
                        egui::Color32::from_rgba_unmultiplied(
                            palette.badge_signed_in_fg.r(),
                            palette.badge_signed_in_fg.g(),
                            palette.badge_signed_in_fg.b(),
                            110,
                        ),
                        palette.badge_signed_in_fg,
                    )
                    .clicked()
                    {
                        decision = Some(true);
                    }
                });
            });

            ui.add_space(8.0);

            // Recessed command summary strip matching input tokens (#0E1116 / #2A3342)
            egui::Frame::new()
                .fill(palette.bg_code)
                .stroke(Stroke::new(1.0_f32, palette.border_strong))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(&approval.summary)
                                .monospace()
                                .size(12.0)
                                .color(palette.text_primary),
                        )
                        .wrap()
                        .selectable(true),
                    );
                });

            if !approval.detail.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&approval.detail)
                        .size(11.5)
                        .color(palette.text_muted),
                );
            }
        });

    decision
}

fn tactile_action_btn(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    label: &str,
    size: Vec2,
    base_bg: egui::Color32,
    border: egui::Color32,
    fg: egui::Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let hovered = resp.hovered();
    let pressed = resp.is_pointer_button_down_on();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let h = hover_factor(ui.ctx(), ui.make_persistent_id(id_salt), hovered);
    let hover_bg = lerp_color(base_bg, fg, 0.14);
    let fill = if pressed {
        lerp_color(base_bg, egui::Color32::BLACK, 0.2)
    } else {
        lerp_color(base_bg, hover_bg, h)
    };
    let draw_rect = if pressed { rect.shrink(1.0) } else { rect };
    ui.painter().rect(
        draw_rect,
        CornerRadius::same(8),
        fill,
        Stroke::new(1.0_f32, lerp_color(border, fg, h * 0.45)),
        egui::StrokeKind::Inside,
    );
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::FontId::proportional(12.0),
        fg,
    );
    ui.painter()
        .galley(draw_rect.center() - galley.size() * 0.5, galley, fg);
    resp
}
