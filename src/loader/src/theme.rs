use eframe::egui::{self, Color32, CornerRadius, Stroke, vec2};

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

pub fn bold_family() -> egui::FontFamily {
    egui::FontFamily::Name("fira_code_bold".into())
}

pub const BG_DARK: Color32 = Color32::from_rgb(0x16, 0x16, 0x1e);
const BG: Color32 = Color32::from_rgb(0x1a, 0x1b, 0x26);
const BG_FLOAT: Color32 = Color32::from_rgb(0x1f, 0x23, 0x35);
const BG_HL: Color32 = Color32::from_rgb(0x29, 0x2e, 0x42);
const BORDER: Color32 = Color32::from_rgb(0x3b, 0x42, 0x61);
pub const FG: Color32 = Color32::from_rgb(0xc0, 0xca, 0xf5);
const FG_DARK: Color32 = Color32::from_rgb(0xa9, 0xb1, 0xd6);
pub const COMMENT: Color32 = Color32::from_rgb(0x56, 0x5f, 0x89);
pub const BLUE: Color32 = Color32::from_rgb(0x7a, 0xa2, 0xf7);
pub const SEL: Color32 = Color32::from_rgb(0x2e, 0x3c, 0x64);
pub const GREEN: Color32 = Color32::from_rgb(0x9e, 0xce, 0x6a);
pub const YELLOW: Color32 = Color32::from_rgb(0xe0, 0xaf, 0x68);
pub const PURPLE: Color32 = Color32::from_rgb(0xbb, 0x9a, 0xf7);
pub const RED: Color32 = Color32::from_rgb(0xf7, 0x76, 0x8e);
pub const CYAN: Color32 = Color32::from_rgb(0x7d, 0xcf, 0xff);
pub const ROW_HEIGHT: f32 = 28.0;

pub fn table_header<'a>(
    mut header: egui_extras::TableRow,
    heads: impl IntoIterator<Item = &'a str>,
) {
    for head in heads {
        header.col(|ui| {
            ui.strong(head);
        });
    }
}

pub fn text_width(ui: &egui::Ui, font: &egui::FontId, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
}

pub fn widest(ui: &egui::Ui, font: &egui::FontId, labels: &[&str]) -> f32 {
    labels
        .iter()
        .map(|label| text_width(ui, font, label))
        .fold(0.0, f32::max)
}

pub fn button_width(ui: &egui::Ui, font: &egui::FontId, labels: &[&str]) -> f32 {
    widest(ui, font, labels) + 2.0 * ui.spacing().button_padding.x
}

pub fn credits(ui: &mut egui::Ui, text: egui::RichText, suffix: &str) -> egui::Response {
    let style = ui.style().clone();
    let size = ui.ctx().fonts_mut(|fonts| text.font_height(fonts, &style));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(text);
        if !suffix.is_empty() {
            ui.label(egui::RichText::new(suffix).size(size * 0.85).weak());
        }
    })
    .response
}

pub fn sidebar_child<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    name: &str,
    dim: bool,
    last: bool,
) -> egui::Response {
    sidebar_row(ui, current, value, dim, |text| {
        [
            ("│  ", COMMENT),
            (if last { "└─ " } else { "├─ " }, COMMENT),
            (name, text),
        ]
    })
}

fn sidebar_row<'a, T: PartialEq, const N: usize>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    dim: bool,
    pieces: impl FnOnce(Color32) -> [(&'a str, Color32); N],
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
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hovered = response.hovered();
    let text = if selected {
        BLUE
    } else if dim {
        COMMENT
    } else if hovered {
        FG
    } else {
        FG_DARK
    };
    if selected || hovered {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, BG_HL);
    }
    let font = egui::FontId::monospace(SIDEBAR_FONT_SIZE);
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: rect.width() - 16.0,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        },
        ..Default::default()
    };
    for (piece, color) in pieces(text) {
        job.append(piece, 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(rect.min.x + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    response
}

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

fn tab_frame<R>(ui: &mut egui::Ui, stroke: Stroke, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .stroke(stroke)
        .inner_margin(12.0)
        .show(ui, add)
        .inner
}

pub fn tab_content<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    tab_frame(ui, Stroke::NONE, add)
}

pub fn tab_panel<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    tab_frame(ui, Stroke::new(1.0, BORDER), add)
}

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

pub fn sidebar_header(ui: &mut egui::Ui, label: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(label.to_uppercase())
            .font(egui::FontId::new(11.0, bold_family()))
            .color(PURPLE),
    );
    ui.add_space(2.0);
}

pub fn sidebar_root(ui: &mut egui::Ui, icon: &str, name: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(format!("{icon} {name}/"))
                .size(SIDEBAR_FONT_SIZE)
                .color(YELLOW),
        )
        .selectable(false),
    );
}

pub fn sidebar_item<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    value: T,
    icon: &str,
    name: &str,
    last: bool,
) -> egui::Response {
    sidebar_row(ui, current, value, false, |text| {
        [
            (if last { "└─ " } else { "├─ " }, COMMENT),
            (icon, text),
            (" ", text),
            (name, text),
            ("/", text),
        ]
    })
}

const SIDEBAR_FONT_SIZE: f32 = 15.0;

pub const SIDEBAR_INSET: i8 = 8;

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

pub fn sidebar_handle(ui: &mut egui::Ui, open: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
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
