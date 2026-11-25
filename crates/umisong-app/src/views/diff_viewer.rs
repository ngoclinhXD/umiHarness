use crate::app::UmisongApp;
use crate::theme::{Palette, pill_badge};
use egui::{CornerRadius, Margin, RichText, Stroke};
use umisong_core::{DiffLineKind, DiffScope, FileDiff};

pub fn render_diff_viewer(app: &mut UmisongApp, ui: &mut egui::Ui, palette: &Palette) {
    // Header toolbar
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Changes")
                .size(17.0)
                .strong()
                .color(palette.text_primary),
        );
        if let Some(snap) = &app.git_diff {
            if let Some(branch) = &snap.branch {
                pill_badge(ui, branch, palette.accent);
            }
            pill_badge(
                ui,
                &format!("{} files", snap.files.len()),
                palette.text_secondary,
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Refresh").clicked() {
                app.refresh_git_diff();
            }
            for (scope, label) in [
                (DiffScope::All, "All vs HEAD"),
                (DiffScope::Staged, "Staged"),
                (DiffScope::Unstaged, "Unstaged"),
            ] {
                let selected = app.diff_scope == scope;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    app.diff_scope = scope;
                    app.refresh_git_diff();
                }
            }
        });
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(8.0);

    let Some(snap) = app.git_diff.clone() else {
        ui.label(
            RichText::new("Open a workspace folder to inspect git changes.")
                .color(palette.text_muted),
        );
        return;
    };

    if !snap.is_git_repo {
        palette.card_frame().show(ui, |ui| {
            ui.label(
                RichText::new("This workspace is not a Git repository.")
                    .strong()
                    .color(palette.text_primary),
            );
            ui.label(
                RichText::new(
                    "Initialize git (`git init`) or open a git repository to inspect live file diffs and enable isolated Git Worktree Compare.",
                )
                .color(palette.text_secondary),
            );
        });
        return;
    }

    if snap.files.is_empty() {
        palette.card_frame().show(ui, |ui| {
            ui.label(
                RichText::new("Working tree is clean — no diffs in the selected scope.")
                    .color(palette.success),
            );
        });
        return;
    }

    // Ensure a valid selected file
    if app.selected_diff_file.is_none()
        || !snap
            .files
            .iter()
            .any(|f| Some(&f.path) == app.selected_diff_file.as_ref())
    {
        app.selected_diff_file = snap.files.first().map(|f| f.path.clone());
    }

    let available = ui.available_size();
    let left_w = (available.x * 0.28).clamp(220.0, 340.0);

    ui.horizontal_top(|ui| {
        // Left changed-files list
        ui.vertical(|ui| {
            ui.set_width(left_w);
            ui.set_min_height(available.y - 8.0);
            ui.add(
                egui::TextEdit::singleline(&mut app.diff_file_filter)
                    .hint_text("Filter changed files…")
                    .desired_width(left_w),
            );
            ui.add_space(6.0);

            let filter = app.diff_file_filter.trim().to_ascii_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("diff_file_list")
                .show(ui, |ui| {
                    for f in &snap.files {
                        if !filter.is_empty() && !f.path.to_ascii_lowercase().contains(&filter) {
                            continue;
                        }
                        let selected = app.selected_diff_file.as_deref() == Some(f.path.as_str());
                        egui::Frame::new()
                            .fill(if selected {
                                palette.bg_elevated
                            } else {
                                palette.bg_card
                            })
                            .stroke(Stroke::new(1.0_f32,
                                if selected {
                                    palette.accent
                                } else {
                                    palette.border
                                },
                            ))
                            .corner_radius(CornerRadius::same(7))
                            .inner_margin(Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.set_width(left_w - 18.0);
                                ui.horizontal(|ui| {
                                    let badge_col = match f.kind {
                                        umisong_core::FileChangeKind::Added
                                        | umisong_core::FileChangeKind::Untracked => palette.success,
                                        umisong_core::FileChangeKind::Deleted => palette.danger,
                                        _ => palette.warning,
                                    };
                                    ui.label(
                                        RichText::new(f.kind.badge())
                                            .monospace()
                                            .strong()
                                            .color(badge_col),
                                    );
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&f.path)
                                                    .size(12.0)
                                                    .color(palette.text_primary),
                                            )
                                            .frame(false)
                                            .truncate(),
                                        )
                                        .clicked()
                                    {
                                        app.selected_diff_file = Some(f.path.clone());
                                    }
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
                                                    .color(palette.success),
                                            );
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                    }
                });
        });

        ui.separator();

        // Right diff hunks canvas
        ui.vertical(|ui| {
            if let Some(file) = snap
                .files
                .iter()
                .find(|f| Some(&f.path) == app.selected_diff_file.as_ref())
            {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&file.path)
                            .monospace()
                            .strong()
                            .size(13.5)
                            .color(palette.text_primary),
                    );
                    ui.label(
                        RichText::new(format!("+{} / -{}", file.additions, file.deletions))
                            .monospace()
                            .size(12.0)
                            .color(palette.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(RichText::new("Revert File…").color(palette.danger))
                            .clicked()
                        {
                            app.pending_revert_file = Some(file.path.clone());
                        }
                    });
                });
                ui.add_space(6.0);
                render_file_diff_body(ui, file, palette, "main_diff_scroll");
            }
        });
    });
}

pub fn render_file_diff_body(
    ui: &mut egui::Ui,
    file: &FileDiff,
    palette: &Palette,
    id_salt: &str,
) {
    if file.is_binary {
        palette.subtle_frame().show(ui, |ui| {
            ui.label(
                RichText::new("Binary file changed (preview omitted).")
                    .color(palette.text_secondary),
            );
        });
        return;
    }

    egui::Frame::new()
        .fill(palette.bg_code)
        .stroke(Stroke::new(1.0_f32, palette.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(6))
        .show(ui, |ui| {
            egui::ScrollArea::both()
                .id_salt((id_salt, &file.path))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for hunk in &file.hunks {
                        egui::Frame::new()
                            .fill(palette.diff_hunk_bg)
                            .inner_margin(Margin::symmetric(8, 3))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width().max(540.0));
                                ui.label(
                                    RichText::new(&hunk.header)
                                        .monospace()
                                        .size(11.5)
                                        .color(palette.accent),
                                );
                            });

                        for line in &hunk.lines {
                            let (bg, fg, prefix) = match line.kind {
                                DiffLineKind::Added => {
                                    (palette.diff_add_bg, palette.diff_add_fg, "+")
                                }
                                DiffLineKind::Removed => {
                                    (palette.diff_del_bg, palette.diff_del_fg, "-")
                                }
                                DiffLineKind::Context => {
                                    (egui::Color32::TRANSPARENT, palette.text_primary, " ")
                                }
                                DiffLineKind::Meta => {
                                    (egui::Color32::TRANSPARENT, palette.text_muted, " ")
                                }
                            };

                            let old_no = line
                                .old_line
                                .map(|n| format!("{n:>4}"))
                                .unwrap_or_else(|| "    ".into());
                            let new_no = line
                                .new_line
                                .map(|n| format!("{n:>4}"))
                                .unwrap_or_else(|| "    ".into());

                            egui::Frame::new()
                                .fill(bg)
                                .inner_margin(Margin::symmetric(6, 1))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width().max(540.0));
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        ui.label(
                                            RichText::new(format!("{old_no} {new_no}"))
                                                .monospace()
                                                .size(11.0)
                                                .color(palette.text_muted),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!("{prefix}{}", line.content))
                                                    .monospace()
                                                    .size(12.0)
                                                    .color(fg),
                                            )
                                            .selectable(true),
                                        );
                                    });
                                });
                        }
                    }
                });
        });
}
