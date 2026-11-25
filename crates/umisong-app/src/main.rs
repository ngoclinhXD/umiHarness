mod app;
mod client;
mod theme;
mod views;
mod widgets;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let viewport = egui::ViewportBuilder::default()
        .with_title("UMIKA | Harness")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([1024.0, 680.0])
        .with_visible(true)
        .with_active(true);

    #[cfg(target_os = "macos")]
    let viewport = viewport
        .with_fullsize_content_view(true)
        .with_title_shown(false)
        .with_titlebar_shown(false);

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "UMIKA | Harness",
        native_options,
        Box::new(|cc| Ok(Box::new(app::UmisongApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}
