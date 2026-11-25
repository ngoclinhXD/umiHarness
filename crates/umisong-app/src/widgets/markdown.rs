use crate::theme::Palette;
use egui::{CornerRadius, Margin, RichText, Stroke};

/// Lightweight pure-Rust markdown renderer for egui:
/// headings, blockquotes, bullet/numbered lists, inline `code` / **bold**, and fenced code blocks with Copy.
pub fn render_markdown(ui: &mut egui::Ui, text: &str, palette: &Palette) {
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();
    let mut block_idx = 0usize;

    for raw_line in text.lines() {
        let trimmed = raw_line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if in_code_block {
                render_code_block(ui, &code_lang, &code_buf, palette, block_idx);
                block_idx += 1;
                in_code_block = false;
                code_lang.clear();
                code_buf.clear();
            } else {
                in_code_block = true;
                code_lang = rest.trim().to_string();
                code_buf.clear();
            }
            continue;
        }

        if in_code_block {
            if !code_buf.is_empty() {
                code_buf.push('\n');
            }
            code_buf.push_str(raw_line);
            continue;
        }

        if raw_line.trim().is_empty() {
            ui.add_space(4.0);
            continue;
        }

        if let Some(h) = trimmed.strip_prefix("### ") {
            ui.add_space(10.0);
            ui.add(
                egui::Label::new(RichText::new(h).size(15.5).strong().color(palette.text_primary))
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(2.0);
        } else if let Some(h) = trimmed.strip_prefix("## ") {
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(RichText::new(h).size(16.5).strong().color(palette.text_primary))
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(2.0);
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(RichText::new(h).size(18.0).strong().color(palette.text_primary))
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(2.0);
        } else if let Some(quote) = trimmed.strip_prefix("> ") {
            egui::Frame::new()
                .fill(palette.bg_elevated)
                .stroke(Stroke::new(1.0_f32, palette.border))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    render_inline_text(ui, quote, palette, true);
                });
        } else if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new("•").color(palette.text_muted).strong());
                render_inline_text(ui, item, palette, false);
            });
            ui.add_space(2.0);
        } else {
            render_inline_text(ui, raw_line, palette, false);
        }
    }

    // Flush unclosed streaming code fence smoothly
    if in_code_block && !code_buf.is_empty() {
        render_code_block(ui, &code_lang, &code_buf, palette, block_idx);
    }
}

fn render_code_block(
    ui: &mut egui::Ui,
    lang: &str,
    code: &str,
    palette: &Palette,
    idx: usize,
) {
    ui.push_id(("md_code_blk", idx), |ui| {
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(palette.bg_card)
            .stroke(Stroke::new(1.0_f32, palette.border))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let header = if lang.is_empty() { "code" } else { lang };
                    ui.label(
                        RichText::new(header)
                            .size(11.0)
                            .monospace()
                            .color(palette.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let copy_btn = egui::Button::new(
                            RichText::new("Copy")
                                .size(11.0)
                                .color(palette.text_secondary),
                        )
                        .fill(palette.bg_card)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(CornerRadius::same(6));
                        let resp = ui.add(copy_btn).on_hover_text("Copy code snippet");
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            ui.ctx().copy_text(code.to_string());
                        }
                    });
                });
                ui.add_space(6.0);
                egui::ScrollArea::horizontal()
                    .id_salt(("code_block_scroll", idx))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(code)
                                    .monospace()
                                    .size(12.0)
                                    .color(palette.text_primary),
                            )
                            .selectable(true),
                        );
                    });
            });
        ui.add_space(6.0);
    });
}

fn render_inline_text(ui: &mut egui::Ui, line: &str, palette: &Palette, muted: bool) {
    let base_color = if muted {
        palette.text_secondary
    } else {
        palette.text_primary
    };

    // Fast path if no inline formatting markers
    if !line.contains('`') && !line.contains("**") && !line.contains("](") {
        ui.add(
            egui::Label::new(RichText::new(line).size(13.5).color(base_color))
                .wrap()
                .selectable(true),
        );
        return;
    }

    let mut job = egui::text::LayoutJob::default();
    let default_fmt = egui::TextFormat {
        font_id: egui::FontId::proportional(13.5),
        color: base_color,
        ..Default::default()
    };
    let bold_fmt = egui::TextFormat {
        font_id: egui::FontId::proportional(13.5),
        color: palette.text_primary,
        ..Default::default()
    };
    let code_fmt = egui::TextFormat {
        font_id: egui::FontId::monospace(12.0),
        color: palette.text_primary,
        background: palette.bg_elevated,
        ..Default::default()
    };

    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Inline markdown link `[label](url)` -> render clean `label` chip
        if bytes[i] == b'[' {
            if let Some(close_br) = line[i + 1..].find("](") {
                let label_end = i + 1 + close_br;
                let url_start = label_end + 2;
                if let Some(close_paren) = line[url_start..].find(')') {
                let label_seg = format!(" {} ", &line[i + 1..label_end]);
                    job.append(&label_seg, 0.0, code_fmt.clone());
                    i = url_start + close_paren + 1;
                    continue;
                }
            }
        }
        if bytes[i] == b'`' {
            if let Some(end_rel) = line[i + 1..].find('`') {
                let code_seg = format!(" {} ", &line[i + 1..i + 1 + end_rel]);
                job.append(&code_seg, 0.0, code_fmt.clone());
                i += end_rel + 2;
                continue;
            }
        }
        if line[i..].starts_with("**") {
            if let Some(end_rel) = line[i + 2..].find("**") {
                let bold_seg = &line[i + 2..i + 2 + end_rel];
                job.append(bold_seg, 0.0, bold_fmt.clone());
                i += end_rel + 4;
                continue;
            }
        }
        // Advance until next marker
        let mut next = i + line[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        while next < bytes.len()
            && bytes[next] != b'`'
            && bytes[next] != b'['
            && !line[next..].starts_with("**")
        {
            next += line[next..].chars().next().map(char::len_utf8).unwrap_or(1);
        }
        job.append(&line[i..next], 0.0, default_fmt.clone());
        i = next;
    }

    ui.add(egui::Label::new(job).wrap().selectable(true));
}
