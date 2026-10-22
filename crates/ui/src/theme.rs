use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle};

pub const BACKGROUND: Color32 = Color32::from_rgb(15, 17, 21);
pub const PANEL: Color32 = Color32::from_rgb(21, 24, 29);
pub const PANEL_RAISED: Color32 = Color32::from_rgb(28, 32, 39);
pub const GRID_ROW_WHITE: Color32 = Color32::from_rgb(28, 32, 38);
pub const GRID_ROW_BLACK: Color32 = Color32::from_rgb(22, 26, 31);
pub const GRID_LINE: Color32 = Color32::from_rgb(38, 43, 51);
pub const GRID_BAR: Color32 = Color32::from_rgb(62, 70, 82);
pub const TEXT: Color32 = Color32::from_rgb(218, 222, 228);
pub const TEXT_DIM: Color32 = Color32::from_rgb(138, 146, 158);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(88, 95, 106);
pub const ACCENT: Color32 = Color32::from_rgb(242, 166, 90);
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(72, 49, 27, 255);
pub const KEY_WHITE: Color32 = Color32::from_rgb(190, 195, 202);
pub const KEY_BLACK: Color32 = Color32::from_rgb(42, 47, 55);
pub const RULER: Color32 = Color32::from_rgb(25, 28, 34);
pub const TITLEBAR: Color32 = Color32::from_rgb(17, 19, 23);

const TRACK_COLORS: [Color32; 6] = [
    Color32::from_rgb(232, 165, 83),
    Color32::from_rgb(84, 181, 196),
    Color32::from_rgb(139, 190, 106),
    Color32::from_rgb(201, 123, 168),
    Color32::from_rgb(201, 194, 106),
    Color32::from_rgb(126, 156, 216),
];

#[must_use]
pub fn track_color(index: usize) -> Color32 {
    TRACK_COLORS[index % TRACK_COLORS.len()]
}

/// Colour of a clip, derived from the pattern it plays so that every
/// placement of the same pattern looks the same.
#[must_use]
pub fn pattern_color(index: usize) -> Color32 {
    TRACK_COLORS[(index + 1) % TRACK_COLORS.len()]
}

pub fn install(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = PANEL_RAISED;
    visuals.code_bg_color = BACKGROUND;
    visuals.selection.bg_fill = Color32::from_rgba_premultiplied(70, 48, 26, 255);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.window_stroke = Stroke::new(1.0, GRID_LINE);
    visuals.window_corner_radius = CornerRadius::same(6);
    visuals.menu_corner_radius = CornerRadius::same(6);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, GRID_LINE);
    visuals.widgets.inactive.bg_fill = PANEL_RAISED;
    visuals.widgets.inactive.weak_bg_fill = PANEL_RAISED;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 47, 56);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(42, 47, 56);
    visuals.widgets.active.bg_fill = Color32::from_rgb(52, 58, 68);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(52, 58, 68);
    visuals.slider_trailing_fill = true;

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.visuals = visuals;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
        style.spacing.interact_size.y = 24.0;
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(13.5));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(19.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.0));
    });
}
