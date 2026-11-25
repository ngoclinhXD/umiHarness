use egui::{Color32, ColorImage, CornerRadius, Margin, Stroke, TextureHandle, TextureOptions, Visuals};
use umisong_core::ProviderId;

const LOGO_WHITE_PNG: &[u8] = include_bytes!("../../../assets/logos/logowhite.png");
const LOGO_COLORED_PNG: &[u8] = include_bytes!("../../../assets/logos/logocolored.png");

pub struct LogoTextures {
    pub dark_logo: Option<TextureHandle>,
    pub light_logo: Option<TextureHandle>,
}

impl LogoTextures {
    pub fn load(ctx: &egui::Context) -> Self {
        Self {
            dark_logo: decode_png_texture(ctx, "umika_logo_white", LOGO_WHITE_PNG),
            light_logo: decode_png_texture(ctx, "umika_logo_colored", LOGO_COLORED_PNG),
        }
    }

    pub fn for_theme(&self, dark: bool) -> Option<&TextureHandle> {
        if dark {
            self.dark_logo.as_ref()
        } else {
            self.light_logo.as_ref()
        }
    }
}

fn decode_png_image(bytes: &[u8], max_side: usize) -> Option<ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let data = &buf[..info.buffer_size()];
    let size = [info.width as usize, info.height as usize];

    let img = match info.color_type {
        png::ColorType::Rgba => ColorImage::from_rgba_unmultiplied(size, data),
        png::ColorType::Rgb => ColorImage::from_rgb(size, data),
        png::ColorType::GrayscaleAlpha => {
            let mut rgba = Vec::with_capacity(size[0] * size[1] * 4);
            for &[g, a] in data.as_chunks::<2>().0 {
                rgba.extend_from_slice(&[g, g, g, a]);
            }
            ColorImage::from_rgba_unmultiplied(size, &rgba)
        }
        png::ColorType::Grayscale => {
            let mut rgba = Vec::with_capacity(size[0] * size[1] * 4);
            for &g in data {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
            ColorImage::from_rgba_unmultiplied(size, &rgba)
        }
        _ => return None,
    };

    Some(downscale_color_image(img, max_side))
}

/// Area-averaged premultiplied-alpha downscaler so high-res logos stay crisp and
/// always fit comfortably below egui's 2048px max texture side limit.
fn downscale_color_image(src: ColorImage, max_side: usize) -> ColorImage {
    let [sw, sh] = src.size;
    if sw <= max_side && sh <= max_side {
        return src;
    }
    let scale = (max_side as f64 / sw.max(sh) as f64).min(1.0);
    let dw = ((sw as f64 * scale).round() as usize).max(1);
    let dh = ((sh as f64 * scale).round() as usize).max(1);

    let mut dst_pixels = vec![Color32::TRANSPARENT; dw * dh];
    for dy in 0..dh {
        let y0 = (dy * sh) / dh;
        let y1 = (((dy + 1) * sh) / dh).max(y0 + 1).min(sh);
        for dx in 0..dw {
            let x0 = (dx * sw) / dw;
            let x1 = (((dx + 1) * sw) / dw).max(x0 + 1).min(sw);

            let mut r_acc = 0u64;
            let mut g_acc = 0u64;
            let mut b_acc = 0u64;
            let mut a_acc = 0u64;
            let mut count = 0u64;

            for sy in y0..y1 {
                let row = sy * sw;
                for sx in x0..x1 {
                    let c = src.pixels[row + sx];
                    // Color32 in egui::ColorImage is already premultiplied RGBA
                    r_acc += c.r() as u64;
                    g_acc += c.g() as u64;
                    b_acc += c.b() as u64;
                    a_acc += c.a() as u64;
                    count += 1;
                }
            }

            let denom = count.max(1);
            dst_pixels[dy * dw + dx] = Color32::from_rgba_premultiplied(
                (r_acc / denom) as u8,
                (g_acc / denom) as u8,
                (b_acc / denom) as u8,
                (a_acc / denom) as u8,
            );
        }
    }

    ColorImage {
        size: [dw, dh],
        pixels: dst_pixels,
    }
}

fn decode_png_texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<TextureHandle> {
    let max_side = ctx.input(|i| i.max_texture_side).min(768);
    let image = decode_png_image(bytes, max_side)?;
    Some(ctx.load_texture(name, image, TextureOptions::LINEAR))
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg_canvas: Color32,
    pub bg_sidebar: Color32,
    pub bg_card: Color32,
    pub bg_elevated: Color32,
    pub bg_code: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_fg: Color32,
    pub accent_soft_bg: Color32,
    pub accent_soft_fg: Color32,
    pub accent_disabled_bg: Color32,
    pub badge_signed_in_bg: Color32,
    pub badge_signed_in_fg: Color32,
    pub badge_signed_out_bg: Color32,
    pub badge_signed_out_fg: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    pub diff_add_bg: Color32,
    pub diff_add_fg: Color32,
    pub diff_del_bg: Color32,
    pub diff_del_fg: Color32,
    pub diff_hunk_bg: Color32,
}

impl Palette {
    pub fn for_mode(dark: bool) -> Self {
        if dark {
            Self {
                // Exact UMIKA | Harness dark design tokens
                bg_canvas: Color32::from_rgb(0x0E, 0x11, 0x16),          // #0E1116
                bg_sidebar: Color32::from_rgb(0x12, 0x16, 0x1D),         // #12161D
                bg_card: Color32::from_rgb(0x12, 0x16, 0x1D),            // #12161D
                bg_elevated: Color32::from_rgb(0x1E, 0x26, 0x35),        // #1E2635
                bg_code: Color32::from_rgb(0x0E, 0x11, 0x16),            // #0E1116 (input/code)
                border: Color32::from_rgb(0x23, 0x2A, 0x36),             // #232A36
                border_strong: Color32::from_rgb(0x2A, 0x33, 0x42),      // #2A3342 (input/hover border)
                text_primary: Color32::from_rgb(0xE6, 0xE9, 0xF0),       // #E6E9F0
                text_secondary: Color32::from_rgb(0xB7, 0xC0, 0xCF),     // #B7C0CF
                text_muted: Color32::from_rgb(0x9A, 0xA4, 0xB5),         // #9AA4B5
                accent: Color32::from_rgb(0xFF, 0x8C, 0xC6),             // #FF8CC6
                accent_fg: Color32::from_rgb(0x0B, 0x10, 0x20),          // #0B1020
                accent_soft_bg: Color32::from_rgb(0x4A, 0x23, 0x40),     // #4A2340
                accent_soft_fg: Color32::from_rgb(0xFF, 0xD6, 0xEA),     // #FFD6EA
                accent_disabled_bg: Color32::from_rgb(0x4A, 0x2C, 0x3F),
                badge_signed_in_bg: Color32::from_rgb(0x12, 0x26, 0x1D), // #12261D
                badge_signed_in_fg: Color32::from_rgb(0x6F, 0xD6, 0xA0), // #6FD6A0
                badge_signed_out_bg: Color32::from_rgb(0x2A, 0x21, 0x12), // #2A2112
                badge_signed_out_fg: Color32::from_rgb(0xF2, 0xC8, 0x79), // #F2C879
                success: Color32::from_rgb(0x6F, 0xD6, 0xA0),            // #6FD6A0
                warning: Color32::from_rgb(0xF2, 0xC8, 0x79),            // #F2C879
                danger: Color32::from_rgb(0xEB, 0x61, 0x6C),
                diff_add_bg: Color32::from_rgba_premultiplied(18, 38, 29, 210),
                diff_add_fg: Color32::from_rgb(0x6F, 0xD6, 0xA0),
                diff_del_bg: Color32::from_rgba_premultiplied(64, 24, 30, 200),
                diff_del_fg: Color32::from_rgb(0xFC, 0xA5, 0xAC),
                diff_hunk_bg: Color32::from_rgb(0x18, 0x20, 0x30),
            }
        } else {
            Self {
                bg_canvas: Color32::from_rgb(248, 249, 252),
                bg_sidebar: Color32::from_rgb(241, 243, 248),
                bg_card: Color32::from_rgb(255, 255, 255),
                bg_elevated: Color32::from_rgb(233, 237, 246),
                bg_code: Color32::from_rgb(243, 245, 249),
                border: Color32::from_rgb(218, 224, 236),
                border_strong: Color32::from_rgb(186, 196, 214),
                text_primary: Color32::from_rgb(22, 26, 36),
                text_secondary: Color32::from_rgb(76, 86, 108),
                text_muted: Color32::from_rgb(122, 132, 154),
                accent: Color32::from_rgb(224, 82, 154),
                accent_fg: Color32::WHITE,
                accent_soft_bg: Color32::from_rgb(252, 230, 242),
                accent_soft_fg: Color32::from_rgb(166, 36, 106),
                accent_disabled_bg: Color32::from_rgb(234, 186, 210),
                badge_signed_in_bg: Color32::from_rgb(224, 246, 235),
                badge_signed_in_fg: Color32::from_rgb(20, 140, 88),
                badge_signed_out_bg: Color32::from_rgb(252, 242, 222),
                badge_signed_out_fg: Color32::from_rgb(176, 114, 16),
                success: Color32::from_rgb(20, 145, 92),
                warning: Color32::from_rgb(196, 124, 16),
                danger: Color32::from_rgb(204, 52, 66),
                diff_add_bg: Color32::from_rgb(224, 247, 234),
                diff_add_fg: Color32::from_rgb(18, 107, 64),
                diff_del_bg: Color32::from_rgb(253, 230, 233),
                diff_del_fg: Color32::from_rgb(166, 33, 46),
                diff_hunk_bg: Color32::from_rgb(232, 239, 252),
            }
        }
    }

    pub fn provider_color(&self, provider: ProviderId) -> Color32 {
        match provider {
            ProviderId::ClaudeCode => Color32::from_rgb(0xD9, 0x77, 0x57),  // #D97757
            ProviderId::Antigravity => Color32::from_rgb(0x4C, 0xC3, 0x8A), // #4CC38A
            ProviderId::Codex => Color32::from_rgb(0x5B, 0x8C, 0xFF),       // #5B8CFF
        }
    }

    pub fn card_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.bg_card)
            .stroke(Stroke::new(1.0_f32, self.border))
            .corner_radius(CornerRadius::same(14))
            .inner_margin(Margin::same(14))
    }

    pub fn subtle_frame(&self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.bg_elevated)
            .stroke(Stroke::new(1.0_f32, self.border))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(8))
    }
}

pub fn install_system_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let mut search_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        search_dirs.push(home.join("Library/Fonts"));
        search_dirs.push(home.join(".local/share/fonts"));
        search_dirs.push(home.join(".fonts"));
    }
    search_dirs.push(std::path::PathBuf::from("/Library/Fonts"));
    search_dirs.push(std::path::PathBuf::from("/usr/share/fonts"));

    let discovered = collect_font_files(&search_dirs, 3);

    // 1. Prefer locally installed Google Sans (Proportional) and Google Sans Code/Mono (Monospace)
    let mut proportional_priority: Vec<String> = Vec::new();
    let mut mono_priority: Vec<String> = Vec::new();

    if let Some(path) = pick_best_google_sans_proportional(&discovered) {
        if let Ok(bytes) = std::fs::read(&path) {
            tracing::info!(font = %path.display(), "Loaded Google Sans proportional font");
            fonts.font_data.insert(
                "google_sans".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            proportional_priority.push("google_sans".to_owned());
        }
    }

    if let Some(path) = pick_best_google_sans_mono(&discovered) {
        if let Ok(bytes) = std::fs::read(&path) {
            tracing::info!(font = %path.display(), "Loaded Google Sans monospace font");
            fonts.font_data.insert(
                "google_sans_mono".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            mono_priority.push("google_sans_mono".to_owned());
        }
    }

    // 2. macOS system fonts as crisp fallback (ensures ⌘, ↵, CJK/Vietnamese/symbols never render tofu)
    #[cfg(target_os = "macos")]
    {
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
            fonts.font_data.insert(
                "macos_sfns".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            proportional_priority.push("macos_sfns".to_owned());
        }
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNSMono.ttf") {
            fonts.font_data.insert(
                "macos_sfmono".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            mono_priority.push("macos_sfmono".to_owned());
        }
        if let Ok(bytes) = std::fs::read("/System/Library/Fonts/Apple Symbols.ttf") {
            fonts.font_data.insert(
                "macos_symbols".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(bytes)),
            );
            proportional_priority.push("macos_symbols".to_owned());
            mono_priority.push("macos_symbols".to_owned());
        }
    }

    // Prepend priority fonts in order ahead of egui defaults
    let prop_family = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    for name in proportional_priority.into_iter().rev() {
        prop_family.insert(0, name);
    }

    let mono_family = fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default();
    for name in mono_priority.into_iter().rev() {
        mono_family.insert(0, name);
    }

    ctx.set_fonts(fonts);
}

fn collect_font_files(dirs: &[std::path::PathBuf], max_depth: usize) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for dir in dirs {
        collect_font_files_rec(dir, max_depth, &mut out);
    }
    out
}

fn collect_font_files_rec(
    dir: &std::path::Path,
    depth_left: usize,
    out: &mut Vec<std::path::PathBuf>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            if depth_left > 0 {
                collect_font_files_rec(&path, depth_left - 1, out);
            }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_l = ext.to_ascii_lowercase();
            if ext_l == "ttf" || ext_l == "otf" {
                out.push(path);
            }
        }
    }
}

fn pick_best_google_sans_proportional(paths: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    paths
        .iter()
        .filter_map(|p| score_google_sans_proportional(p).map(|score| (score, p)))
        .max_by_key(|(score, _)| *score)
        .map(|(_, p)| p.clone())
}

fn pick_best_google_sans_mono(paths: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    paths
        .iter()
        .filter_map(|p| score_google_sans_mono(p).map(|score| (score, p)))
        .max_by_key(|(score, _)| *score)
        .map(|(_, p)| p.clone())
}

fn normalize_font_stem(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace([' ', '_', '-'], "")
}

fn score_google_sans_proportional(path: &std::path::Path) -> Option<i32> {
    let stem = normalize_font_stem(path);
    let is_google_sans = stem.contains("googlesans") || stem.contains("productsans");
    if !is_google_sans {
        return None;
    }
    // Exclude monospace/code variants from proportional UI font
    if stem.contains("mono") || stem.contains("code") {
        return None;
    }

    let mut score = 100;
    if stem.contains("italic") || stem.contains("oblique") {
        score -= 80;
    }
    if stem.contains("bold") || stem.contains("black") || stem.contains("heavy") || stem.contains("extrabold") || stem.contains("semibold") {
        score -= 45;
    }
    if stem.contains("thin") || stem.contains("light") || stem.contains("extralight") {
        score -= 40;
    }
    if stem.contains("regular") {
        score += 50;
    } else if stem.contains("medium") {
        score += 20;
    } else if stem.contains("variable") || stem.contains("flex") || stem.contains('[') {
        score += 35;
    }
    // Prefer Text / base Google Sans over Display for UI legibility
    if stem.contains("googlesanstext") {
        score += 18;
    } else if stem == "googlesans" || stem == "googlesansregular" {
        score += 22;
    } else if stem.contains("display") {
        score -= 5;
    }
    // Slightly prefer .ttf for broadest ab_glyph compatibility
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ttf"))
    {
        score += 5;
    }

    Some(score)
}

fn score_google_sans_mono(path: &std::path::Path) -> Option<i32> {
    let stem = normalize_font_stem(path);
    if !stem.contains("googlesans") || (!stem.contains("mono") && !stem.contains("code")) {
        return None;
    }
    let mut score = 100;
    if stem.contains("italic") {
        score -= 80;
    }
    if stem.contains("bold") || stem.contains("light") || stem.contains("thin") {
        score -= 40;
    }
    if stem.contains("regular") {
        score += 50;
    } else if stem.contains("variable") || stem.contains('[') {
        score += 35;
    }
    Some(score)
}

/// Consumes a plain `Enter` (or `⌘+Enter`) key press for "Enter to send",
/// while leaving `Shift+Enter` untouched so multiline `TextEdit` inserts `\n`.
pub fn consume_enter_to_send(ui: &mut egui::Ui) -> bool {
    ui.input_mut(|i| {
        if i.modifiers.shift {
            return false;
        }
        let mut found = false;
        i.events.retain(|ev| {
            if let egui::Event::Key {
                key: egui::Key::Enter,
                pressed: true,
                modifiers,
                ..
            } = ev
            {
                if !modifiers.shift && !modifiers.alt {
                    found = true;
                    return false;
                }
            }
            true
        });
        found
    })
}

/// Smoothly linearly interpolates two `Color32` values (`t` in `0.0..=1.0`).
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 { (x as f32 + (y as f32 - x as f32) * t).round() as u8 };
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// Fast 120ms ease-out hover factor (`0.0` at rest -> `1.0` hovered) for silky UI feedback.
pub fn hover_factor(ctx: &egui::Context, id: egui::Id, hovered: bool) -> f32 {
    let raw = ctx.animate_bool_with_time(id, hovered, 0.12);
    // Ease-out cubic: starts fast, settles crisply
    let inv = 1.0 - raw;
    1.0 - inv * inv * inv
}

/// Tactile Sakura Pink primary CTA button with smooth hover brighten and 1px press inset.
pub fn sakura_cta_button(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    label: &str,
    size: egui::Vec2,
    enabled: bool,
    palette: &Palette,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);

    let hovered = enabled && resp.hovered();
    let pressed = enabled && resp.is_pointer_button_down_on();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let h = hover_factor(ui.ctx(), ui.make_persistent_id(id_salt), hovered);
    let hover_pink = Color32::from_rgb(
        palette.accent.r().saturating_add(14),
        palette.accent.g().saturating_add(18),
        palette.accent.b().saturating_add(12),
    );
    let pressed_pink = Color32::from_rgb(
        palette.accent.r().saturating_sub(24),
        palette.accent.g().saturating_sub(26),
        palette.accent.b().saturating_sub(18),
    );

    let fill = if !enabled {
        palette.accent_disabled_bg
    } else if pressed {
        pressed_pink
    } else {
        lerp_color(palette.accent, hover_pink, h)
    };

    let draw_rect = if pressed { rect.shrink(1.0) } else { rect };
    ui.painter()
        .rect_filled(draw_rect, CornerRadius::same(9), fill);

    let text_col = if enabled {
        palette.accent_fg
    } else {
        palette.text_muted
    };
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::FontId::proportional(13.0),
        text_col,
    );
    let text_pos = draw_rect.center() - galley.size() * 0.5;
    ui.painter().galley(text_pos, galley, text_col);

    resp
}

/// Paints a crisp anti-aliased vector circle (filled dot or hollow ring) so status
/// indicators never depend on font glyphs.
pub fn paint_status_dot(ui: &mut egui::Ui, color: Color32, filled: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let center = rect.center();
    if filled {
        ui.painter().circle_filled(center, 3.8, color);
    } else {
        ui.painter()
            .circle_stroke(center, 3.6, Stroke::new(1.4_f32, color));
    }
}

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let p = Palette::for_mode(dark);
    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    visuals.panel_fill = p.bg_canvas;
    visuals.window_fill = p.bg_card;
    visuals.extreme_bg_color = p.bg_code;
    visuals.faint_bg_color = p.bg_elevated;
    visuals.window_stroke = Stroke::new(1.0_f32, p.border_strong);
    visuals.window_corner_radius = CornerRadius::same(14);

    visuals.widgets.noninteractive.bg_fill = p.bg_card;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = p.bg_card;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = p.bg_elevated;
    visuals.widgets.hovered.weak_bg_fill = p.bg_elevated;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

    visuals.widgets.active.bg_fill = p.bg_elevated;
    visuals.widgets.active.weak_bg_fill = p.bg_elevated;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    visuals.widgets.open.bg_fill = p.bg_elevated;
    visuals.widgets.open.weak_bg_fill = p.bg_elevated;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, p.border_strong);
    visuals.widgets.open.corner_radius = CornerRadius::same(8);

    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_rgba_unmultiplied(0, 0, 0, if dark { 140 } else { 35 }),
    };

    // High-contrast text selection highlight so selected text in chat/code/inputs stands out clearly
    visuals.selection.bg_fill = if dark {
        Color32::from_rgb(0x6E, 0x32, 0x5C) // Bright Sakura plum highlight (#6E325C)
    } else {
        Color32::from_rgb(0xF7, 0xB8, 0xDA) // Crisp light Sakura selection (#F7B8DA)
    };
    visuals.selection.stroke = Stroke::new(
        1.0_f32,
        if dark {
            Color32::WHITE
        } else {
            p.text_primary
        },
    );

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.scroll.bar_width = 7.0;
    // Native desktop feel: UI chrome is never text-selectable; chat/code/outputs opt in explicitly.
    style.interaction.selectable_labels = false;
    ctx.set_style(style);
}

pub fn pill_badge(ui: &mut egui::Ui, label: &str, color: Color32) {
    let fill = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28);
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(
            1.0_f32,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 82),
        ))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).color(color).size(11.0).strong());
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_embedded_umika_logos() {
        let white = decode_png_image(LOGO_WHITE_PNG, 768).expect("white logo PNG decodes");
        assert_eq!(white.size[0], 768);
        assert!(white.size[1] > 100 && white.size[1] <= 120);
        let colored = decode_png_image(LOGO_COLORED_PNG, 768).expect("colored logo PNG decodes");
        assert_eq!(colored.size, white.size);
    }

    #[test]
    fn prefers_google_sans_regular_and_code_variants() {
        let candidates = vec![
            std::path::PathBuf::from("/Library/Fonts/GoogleSans-Bold.ttf"),
            std::path::PathBuf::from("/Library/Fonts/GoogleSans-Italic.ttf"),
            std::path::PathBuf::from("/Library/Fonts/GoogleSans-Regular.ttf"),
            std::path::PathBuf::from("/Library/Fonts/GoogleSansDisplay-Regular.ttf"),
            std::path::PathBuf::from("/Library/Fonts/GoogleSansCode-Regular.ttf"),
            std::path::PathBuf::from("/Library/Fonts/GoogleSansCode-BoldItalic.ttf"),
        ];
        assert_eq!(
            pick_best_google_sans_proportional(&candidates),
            Some(std::path::PathBuf::from("/Library/Fonts/GoogleSans-Regular.ttf"))
        );
        assert_eq!(
            pick_best_google_sans_mono(&candidates),
            Some(std::path::PathBuf::from("/Library/Fonts/GoogleSansCode-Regular.ttf"))
        );
    }
}
