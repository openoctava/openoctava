mod app;
mod config;
mod demo;

use std::sync::Arc;

use app::OctavaApp;
use eframe::egui;
use egui::IconData;

fn main() -> eframe::Result {
    let image = image::load_from_memory(include_bytes!("../assets/OpenOctavaLogo.png"))
        .expect("Failed to load application icon")
        .to_rgba8();
    let (width, height) = image.dimensions();
    let icon = Arc::new(IconData {
        rgba: image.into_raw(),
        width,
        height,
    });

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Open Octava")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_decorations(false)
            .with_icon(icon)
            .with_app_id("Open Octava"),
        ..Default::default()
    };

    eframe::run_native(
        "Open Octava",
        options,
        Box::new(|cc| Ok(Box::new(OctavaApp::new(cc)))),
    )
}
