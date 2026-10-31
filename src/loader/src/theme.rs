use eframe::egui::{self, Color32, CornerRadius, Stroke, vec2};

/// Fira Code Nerd Font Mono (SIL OFL, see `assets/FiraCode-LICENSE.txt`) ahead of egui's monospace font, which covers the rest.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "fira_code".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/FiraCodeNerdFontMono-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "fira_code".to_owned());
    fonts.font_data.insert(
        "fira_code_bold".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/FiraCodeNerdFontMono-Bold.ttf"
        ))),
    );
    fonts.families.insert(
        bold_family(),
        vec!["fira_code_bold".to_owned(), "fira_code".to_owned()],
    );
    ctx.set_fonts(fonts);
}

/// The bold weight of Fira Code, used for the app title.
pub fn bold_family() -> egui::FontFamily {
    egui::FontFamily::Name("fira_code_bold".into())
}

/// Tokyonight (night), the webapp's default colorscheme.
const BG_DARK: Color32 = Color32::from_rgb(0x16, 0x16, 0x1e);
const BG: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x26);
const BG_FLOAT: Color32 = Color32::from_rgb(0x1f, 0x23, 0x35);
const BG_HL: Color32 = Color32::from_rgb(0x29, 0x2e, 0x42);
const BORDER: Color32 = Color32::from_rgb(0x3b, 0x42, 0x61);
const FG: Color32 = Color32::from_rgb(0xc0, 0xca, 0xf5);
const FG_DARK: Color32 = Color32::from_rgb(0xa9, 0xb1, 0xd6);
pub const COMMENT: Color32 = Color32::from_rgb(0x56, 0x5f, 0x89);
const BLUE: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xf7);
const SEL: Color32 = Color32::from_rgb(0x2e, 0x3c, 0x64);
pub const GREEN: Color32 = Color32::from_rgb(0x9e, 0xce, 0x6a);
pub const YELLOW: Color32 = Color32::from_rgb(0xe0, 0xaf, 0x68);
const PURPLE: Color32 = Color32::from_rgb(0xbb, 0x9a, 0xf7);
pub const RED: Color32 = Color32::from_rgb(0xf7, 0x76, 0x8e);

/// A flat, square, all-monospace look after the webapp's Neovim/LazyVim layer, always dark.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = vec2(8.0, 4.0);
        style.spacing.button_padding = vec2(8.0, 2.0);
        style.spacing.interact_size.y = 22.0;
        style.spacing.menu_margin = egui::Margin::same(4);

        for (text_style, size) in [
            (egui::TextStyle::Small, 11.0),
            (egui::TextStyle::Body, 13.0),
            (egui::TextStyle::Button, 13.0),
            (egui::TextStyle::Monospace, 13.0),
            (egui::TextStyle::Heading, 16.0),
        ] {
            style.text_styles.insert(
                text_style,
                egui::FontId::new(size, egui::FontFamily::Monospace),
            );
        }

        let visuals = &mut style.visuals;
        visuals.dark_mode = true;
        visuals.window_corner_radius = CornerRadius::ZERO;
        visuals.menu_corner_radius = CornerRadius::ZERO;
        visuals.panel_fill = BG;
        visuals.window_fill = BG_FLOAT;
        visuals.extreme_bg_color = BG_DARK;
        visuals.faint_bg_color = BG_FLOAT;
        visuals.code_bg_color = BG_DARK;
        visuals.window_stroke = Stroke::new(1.0, BORDER);
        visuals.hyperlink_color = BLUE;
        visuals.selection.bg_fill = SEL;
        visuals.selection.stroke = Stroke::new(1.0, BLUE);
        visuals.popup_shadow = egui::Shadow::NONE;
        visuals.window_shadow = egui::Shadow::NONE;

        let w = &mut visuals.widgets;
        w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        w.noninteractive.fg_stroke = Stroke::new(1.0, FG_DARK);
        for (widget, fill) in [
            (&mut w.inactive, BG_HL),
            (&mut w.hovered, SEL),
            (&mut w.active, SEL),
            (&mut w.open, BG_HL),
        ] {
            widget.corner_radius = CornerRadius::ZERO;
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.fg_stroke = Stroke::new(1.0, FG);
        }
        w.noninteractive.corner_radius = CornerRadius::ZERO;
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.bg_stroke = Stroke::new(1.0, BLUE);
        w.active.bg_stroke = Stroke::new(1.0, BLUE);
    });
}

/// The content under a row of nested tabs, with room around it.
pub fn tab_content<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new().inner_margin(12.0).show(ui, add).inner
}

/// A bordered panel directly under a row of tabs, with room inside it.
pub fn tab_panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, BORDER))
        .inner_margin(12.0)
        .show(ui, add)
        .inner
}

/// A bufferline tab: the open one sits on a lifted background with a blue top bar, the others are dim text.
pub fn tab_value<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    label: &str,
) -> egui::Response {
    let selected = *current == value;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let (rect, mut response) =
        ui.allocate_exact_size(galley.size() + vec2(20.0, 8.0), egui::Sense::click());
    if response.clicked() && !selected {
        *current = value;
        response.mark_changed();
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let color = if selected {
            FG
        } else if response.hovered() {
            FG_DARK
        } else {
            COMMENT
        };
        if selected {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, BG_HL);
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.min, vec2(rect.width(), 2.0)),
                CornerRadius::ZERO,
                BLUE,
            );
        }
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    response
}

/// The webapp's neo-tree header: a bold, spaced, uppercase purple label.
pub fn sidebar_header(ui: &mut egui::Ui, label: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(label.to_uppercase())
            .font(egui::FontId::new(11.0, bold_family()))
            .color(PURPLE),
    );
    ui.add_space(2.0);
}

/// The tree's root row (not clickable): `icon name/` in yellow.
pub fn sidebar_root(ui: &mut egui::Ui, icon: &str, name: &str) {
    ui.label(
        egui::RichText::new(format!("{icon} {name}/"))
            .size(SIDEBAR_FONT_SIZE)
            .color(YELLOW),
    );
}

/// A neo-tree row: dim guide, icon and `name/`; the open one is lifted and blue, hover brightens it.
pub fn sidebar_item<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    icon: &str,
    name: &str,
    last: bool,
) -> egui::Response {
    let selected = *current == value;
    let (rect, mut response) = ui.allocate_exact_size(
        vec2(ui.available_width(), ui.spacing().interact_size.y),
        egui::Sense::click(),
    );
    if response.clicked() && !selected {
        *current = value;
        response.mark_changed();
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let text = if selected {
            BLUE
        } else if response.hovered() {
            FG
        } else {
            FG_DARK
        };
        if selected || response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, BG_HL);
        }
        let font = egui::FontId::monospace(SIDEBAR_FONT_SIZE);
        let mut job = egui::text::LayoutJob::default();
        job.wrap = egui::text::TextWrapping {
            max_width: rect.width() - 16.0,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        for (s, color) in [
            (if last { "└─ " } else { "├─ " }, COMMENT),
            (icon, text),
            (" ", text),
            (name, text),
            ("/", text),
        ] {
            job.append(s, 0.0, egui::TextFormat::simple(font.clone(), color));
        }
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            egui::pos2(rect.min.x + 8.0, rect.center().y - galley.size().y / 2.0),
            galley,
            text,
        );
    }
    response
}

/// Text size of the sidebar's list rows.
const SIDEBAR_FONT_SIZE: f32 = 15.0;

/// Space left of the sidebar's list.
pub const SIDEBAR_INSET: i8 = 8;

/// The sidebar's own surface: darker than the page with a single line on its right edge.
pub fn sidebar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_DARK)
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin {
            left: SIDEBAR_INSET,
            right: 0,
            top: 6,
            bottom: 6,
        })
}

/// The narrow-window sidebar toggle: a skinny strip with a chevron pointing right (closed, `open` 0) to left (open, `open` 1).
pub fn sidebar_handle(ui: &mut egui::Ui, open: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text("Toggle the sidebar");
    ui.painter().rect_filled(
        rect,
        CornerRadius::ZERO,
        if response.hovered() { BG_HL } else { BG_DARK },
    );
    let color = if response.hovered() || open > 0.5 {
        FG
    } else {
        FG_DARK
    };
    let (sin, cos) = (open * std::f32::consts::PI).sin_cos();
    let points = [vec2(-3.0, -7.0), vec2(3.0, 0.0), vec2(-3.0, 7.0)]
        .map(|p| rect.center() + vec2(p.x * cos - p.y * sin, p.x * sin + p.y * cos));
    ui.painter()
        .add(egui::Shape::line(points.to_vec(), Stroke::new(2.0, color)));
    response
}
