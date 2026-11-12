use eframe::egui::{self, Color32, CornerRadius, Stroke, vec2};

#[derive(Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UiFont {
    #[default]
    Proportional,
    Monospace,
}

impl UiFont {
    pub const ALL: [Self; 2] = [Self::Proportional, Self::Monospace];

    pub fn label(self) -> &'static str {
        match self {
            Self::Proportional => "Proportional",
            Self::Monospace => "Monospace",
        }
    }

    fn family(self) -> egui::FontFamily {
        match self {
            Self::Proportional => egui::FontFamily::Proportional,
            Self::Monospace => egui::FontFamily::Monospace,
        }
    }
}

fn install_fonts(ctx: &egui::Context, ui_font: UiFont) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "fira_code",
            &include_bytes!("../assets/FiraCodeNerdFontMono-Regular.ttf")[..],
        ),
        (
            "fira_code_bold",
            &include_bytes!("../assets/FiraCodeNerdFontMono-Bold.ttf")[..],
        ),
        (
            "noto_sans",
            &include_bytes!("../assets/NotoSans-Regular.ttf")[..],
        ),
        (
            "noto_sans_bold",
            &include_bytes!("../assets/NotoSans-Bold.ttf")[..],
        ),
    ] {
        fonts.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
    }
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "fira_code".to_owned());
    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    proportional.insert(0, "noto_sans".to_owned());
    proportional.insert(1, "fira_code".to_owned());
    let bold = match ui_font {
        UiFont::Proportional => vec![
            "noto_sans_bold".to_owned(),
            "fira_code_bold".to_owned(),
            "fira_code".to_owned(),
        ],
        UiFont::Monospace => vec!["fira_code_bold".to_owned(), "fira_code".to_owned()],
    };
    fonts.families.insert(bold_family(), bold);
    ctx.set_fonts(fonts);
}

pub fn bold_family() -> egui::FontFamily {
    egui::FontFamily::Name("bold".into())
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

fn credits_job(ui: &egui::Ui, text: egui::RichText, suffix: &str) -> egui::text::LayoutJob {
    let style = ui.style();
    let size = ui.ctx().fonts_mut(|fonts| text.font_height(fonts, style));
    let mut job = egui::text::LayoutJob::default();
    text.append_to(
        &mut job,
        style,
        egui::FontSelection::Default,
        egui::Align::Center,
    );
    if !suffix.is_empty() {
        egui::RichText::new(suffix)
            .size(size * 0.85)
            .weak()
            .append_to(
                &mut job,
                style,
                egui::FontSelection::Default,
                egui::Align::Center,
            );
    }
    job
}

pub fn credits(ui: &mut egui::Ui, text: egui::RichText, suffix: &str) -> egui::Response {
    let job = credits_job(ui, text, suffix);
    ui.add(egui::Label::new(job).selectable(false).extend())
}

pub fn credits_link(ui: &mut egui::Ui, text: egui::RichText, suffix: &str) -> egui::Response {
    let text = if link_hovered(ui, ui.next_auto_id()) {
        text.color(BLUE)
    } else {
        text
    };
    let job = credits_job(ui, text, suffix);
    ui.add(
        egui::Label::new(job)
            .selectable(false)
            .extend()
            .sense(egui::Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
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

pub fn set_ui_font(ctx: &egui::Context, ui_font: UiFont) {
    install_fonts(ctx, ui_font);
    ctx.all_styles_mut(|style| {
        for (text_style, size) in [
            (egui::TextStyle::Small, 11.0),
            (egui::TextStyle::Body, 13.0),
            (egui::TextStyle::Button, 13.0),
            (egui::TextStyle::Monospace, 13.0),
            (egui::TextStyle::Heading, 16.0),
        ] {
            let family = if text_style == egui::TextStyle::Monospace {
                egui::FontFamily::Monospace
            } else {
                ui_font.family()
            };
            style
                .text_styles
                .insert(text_style, egui::FontId::new(size, family));
        }
    });
}

pub fn apply(ctx: &egui::Context, ui_font: UiFont) {
    set_ui_font(ctx, ui_font);
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = vec2(8.0, 4.0);
        style.spacing.button_padding = vec2(8.0, 2.0);
        style.spacing.interact_size.y = 22.0;
        style.spacing.menu_margin = egui::Margin::same(4);

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

pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_FLOAT)
        .inner_margin(egui::Margin::same(12))
}

pub fn status_dot(ui: &mut egui::Ui, color: Color32) -> egui::Response {
    let size = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, response) = ui.allocate_exact_size(vec2(size * 0.6, size), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), size * 0.25, color);
    response
}

pub fn button_group<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    button_group_in(ui, false, add)
}

pub fn button_group_in<R>(
    ui: &mut egui::Ui,
    vertical: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(BORDER)
        .inner_margin(egui::Margin::same(1))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
            if vertical {
                ui.vertical(add).inner
            } else {
                ui.horizontal(add).inner
            }
        })
        .inner
}

pub fn button_group_width(buttons: usize, button_width: f32) -> f32 {
    buttons as f32 * (button_width + 1.0) + 1.0
}

pub fn button_group_size(
    ui: &egui::Ui,
    buttons: usize,
    button_width: f32,
    vertical: bool,
) -> egui::Vec2 {
    let height = ui.spacing().interact_size.y;
    if vertical {
        vec2(
            button_group_width(1, button_width),
            button_group_width(buttons, height),
        )
    } else {
        vec2(
            button_group_width(buttons, button_width),
            button_group_width(1, height),
        )
    }
}

pub fn button_group_at<R>(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vertical: bool,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let builder = egui::UiBuilder::new()
        .max_rect(rect)
        .layout(egui::Layout::top_down(egui::Align::Min));
    ui.scope_builder(builder, |ui| button_group_in(ui, vertical, add))
        .inner
}

const SLIDE_SECONDS: f32 = 0.15;
const ICON_GAP: f32 = 6.0;

struct IconButton {
    icon: std::sync::Arc<egui::Galley>,
    text: std::sync::Arc<egui::Galley>,
    padding: f32,
}

impl IconButton {
    fn new(ui: &egui::Ui, icon: &str, label: &str) -> Self {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let painter = ui.painter();
        Self {
            icon: painter.layout_no_wrap(icon.to_owned(), font.clone(), Color32::PLACEHOLDER),
            text: painter.layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER),
            padding: ui.spacing().button_padding.x,
        }
    }

    fn content_width(&self) -> f32 {
        self.icon.size().x + ICON_GAP + self.text.size().x
    }

    fn show(
        self,
        ui: &mut egui::Ui,
        width: f32,
        content_left: f32,
        text_alpha: f32,
        selected: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(
            vec2(width, ui.spacing().interact_size.y),
            egui::Sense::click(),
        );
        let enabled = ui.is_enabled();
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                enabled,
                selected,
                self.text.text(),
            )
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let visuals = ui.style().interact_selectable(&response, selected);
            painter.rect(
                rect,
                CornerRadius::ZERO,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
            let color = visuals.text_color();
            let icon_pos = egui::pos2(
                rect.left() + content_left,
                rect.center().y - self.icon.size().y / 2.0,
            );
            let text_pos = egui::pos2(
                icon_pos.x + self.icon.size().x + ICON_GAP,
                rect.center().y - self.text.size().y / 2.0,
            );
            painter.galley(icon_pos, self.icon, color);
            if text_alpha > 0.0 {
                painter
                    .with_clip_rect(
                        rect.shrink2(vec2(self.padding, 0.0))
                            .intersect(ui.clip_rect()),
                    )
                    .galley(text_pos, self.text, color.gamma_multiply(text_alpha));
            }
        }
        response
    }
}

pub fn icon_button_width(ui: &egui::Ui, buttons: &[(&str, &str)]) -> f32 {
    buttons
        .iter()
        .map(|(icon, label)| IconButton::new(ui, icon, label).content_width())
        .fold(0.0, f32::max)
        + 2.0 * ui.spacing().button_padding.x
}

pub fn icon_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    width: f32,
    enabled: bool,
) -> egui::Response {
    ui.scope(|ui| {
        if !enabled {
            ui.disable();
        }
        let button = IconButton::new(ui, icon, label);
        let left = ((width - button.content_width()) / 2.0).max(button.padding);
        button.show(ui, width, left, 1.0, false)
    })
    .inner
}

pub fn slide_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    selected: bool,
) -> egui::Response {
    ui.scope(|ui| {
        if !enabled {
            ui.disable();
        }
        let id = ui.next_auto_id();
        let hovered = ui
            .ctx()
            .read_response(id)
            .is_some_and(|r| r.contains_pointer());
        let open = ui.ctx().animate_bool_with_time_and_easing(
            id.with("open"),
            hovered,
            SLIDE_SECONDS,
            egui::emath::easing::cubic_out,
        );
        let button = IconButton::new(ui, icon, label);
        let padding = button.padding;
        let width = button.icon.size().x + 2.0 * padding + open * (ICON_GAP + button.text.size().x);
        button.show(ui, width, padding, open, selected)
    })
    .inner
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

pub fn link_hovered(ui: &egui::Ui, id: egui::Id) -> bool {
    ui.ctx()
        .read_response(id)
        .is_some_and(|r| r.contains_pointer())
}

pub fn sidebar_root(ui: &mut egui::Ui, icon: &str, name: &str, link: bool) -> egui::Response {
    let hovered = link && link_hovered(ui, ui.next_auto_id());
    let response = ui.add(
        egui::Label::new(
            egui::RichText::new(format!("{icon} {name}/"))
                .size(SIDEBAR_FONT_SIZE)
                .color(if hovered { BLUE } else { YELLOW }),
        )
        .selectable(false)
        .sense(if link {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        }),
    );
    if link {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
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
