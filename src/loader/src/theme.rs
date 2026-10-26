use eframe::egui::{self, Color32, CornerRadius, Stroke, vec2};

const ACCENT: Color32 = Color32::from_rgb(244, 105, 36);

/// Fira Code Nerd Font Mono (SIL OFL, see `assets/FiraCode-LICENSE.txt`) ahead of egui's monospace font, which covers the rest.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "fira_code".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/FiraCodeNerdFontMono-Regular.ttf"))),
    );
    fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "fira_code".to_owned());
    ctx.set_fonts(fonts);
}

/// Rounder, roomier widgets for both themes, and a flat onyx palette with one accent for the dark one.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(10.0, 4.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.menu_margin = egui::Margin::same(6);

        let radius = CornerRadius::same(6);
        let visuals = &mut style.visuals;
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = radius;
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = radius;
        }
        // Buttons read as flat fills, not outlined boxes.
        visuals.widgets.inactive.bg_stroke = Stroke::NONE;
        visuals.widgets.inactive.weak_bg_fill = visuals.widgets.inactive.bg_fill;
        visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        visuals.hyperlink_color = ACCENT;
    });

    ctx.style_mut_of(egui::Theme::Dark, |style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = Color32::from_rgb(20, 21, 22);
        visuals.window_fill = Color32::from_rgb(28, 29, 30);
        visuals.extreme_bg_color = Color32::from_rgb(14, 15, 16);
        visuals.faint_bg_color = Color32::from_rgb(28, 29, 30);
        visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(54, 56, 57));
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(46, 48, 49));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 42, 43);
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 42, 43);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 56, 57);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(53, 56, 57);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.7));
        visuals.widgets.active.bg_fill = ACCENT.gamma_multiply(0.6);
        visuals.widgets.active.weak_bg_fill = ACCENT.gamma_multiply(0.6);
    });
}

/// A tab: the open one has accent text and an accent outline, the others are bare text.
pub fn tab_value<T: PartialEq>(ui: &mut egui::Ui, current: &mut T, value: T, label: &str) -> egui::Response {
    let selected = *current == value;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let (rect, mut response) = ui.allocate_exact_size(galley.size() + vec2(24.0, 10.0), egui::Sense::click());
    if response.clicked() && !selected {
        *current = value;
        response.mark_changed();
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let color = if selected {
            ACCENT
        } else if response.hovered() {
            ui.visuals().strong_text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        if selected {
            let top = CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 };
            ui.painter().rect_stroke(rect, top, Stroke::new(1.0, ACCENT), egui::StrokeKind::Inside);
        }
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response
}
