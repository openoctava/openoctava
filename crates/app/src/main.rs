mod app;
mod demo;

use app::OctavaApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Octava")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_decorations(false),
        ..Default::default()
    };

    eframe::run_native(
        "Octava",
        options,
        Box::new(|cc| Ok(Box::new(OctavaApp::new(cc)))),
    )
}
