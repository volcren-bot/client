use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, NumberInput};
use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::{Entity, Focusable, Hsla, Subscription};

use super::*;
use crate::ui::{theme, widgets};

const COLUMN_GAP: Pixels = px(16.0);
const SWATCH: Pixels = px(20.0);
const DISPLAY_NAME_WIDTH: Pixels = px(140.0);
const CURSOR_SIZE_WIDTH: Pixels = px(100.0);
const HIDER_COLUMN: Pixels = px(170.0);
const CURSOR_SIZES: std::ops::RangeInclusive<u8> = 4..=64;
const DISPLAY_NAME_HINT: &str = "Shown instead of your display name, in this client only, and kept across logins until reset. Empty puts the real name back.";

pub(super) struct Ui {
    pid: u32,
    display_name: Entity<InputState>,
    cursor_color: Entity<ColorPickerState>,
    cursor_size: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

fn cursor_hsla(color: [u8; 3]) -> Hsla {
    Rgba {
        r: f32::from(color[0]) / 255.0,
        g: f32::from(color[1]) / 255.0,
        b: f32::from(color[2]) / 255.0,
        a: 1.0,
    }
    .into()
}

fn cursor_rgb(color: Hsla) -> [u8; 3] {
    let rgb = color.to_rgb();
    [rgb.r, rgb.g, rgb.b].map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn parse_cursor_size(text: &str) -> Option<u8> {
    text.trim()
        .parse()
        .ok()
        .filter(|size| CURSOR_SIZES.contains(size))
}

impl Ui {
    fn new(pid: u32, app: &LoaderApp, window: &mut Window, cx: &mut Context<LoaderApp>) -> Self {
        let display_name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Real name")
                .default_value(app.display_name.clone())
                .validate(|text, _| text.chars().count() <= injector::state::MAX_NAME_BYTES)
        });
        let cursor_color = cx.new(|cx| {
            ColorPickerState::new(window, cx).default_value(cursor_hsla(app.cursor.color))
        });
        let cursor_size = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(app.cursor.size.to_string())
                .step(1.0)
                .min(f64::from(*CURSOR_SIZES.start()))
                .max(f64::from(*CURSOR_SIZES.end()))
        });
        let subscriptions = vec![
            cx.subscribe_in(
                &display_name,
                window,
                move |app, input, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        app.display_name = input.read(cx).value().to_string();
                    }
                    InputEvent::PressEnter { .. } => {
                        window.focus(&app.chrome.focus, cx);
                        app.submit_display_name(pid);
                        cx.notify();
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &cursor_color,
                window,
                move |app, _, event: &ColorPickerEvent, _, cx| {
                    let ColorPickerEvent::Change(Some(color)) = event else {
                        return;
                    };
                    let color = cursor_rgb(*color);
                    if color != app.cursor.color {
                        app.cursor.color = color;
                        app.cursor_changed(pid);
                        cx.notify();
                    }
                },
            ),
            cx.subscribe_in(
                &cursor_size,
                window,
                move |app, input, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event
                        && let Some(size) = parse_cursor_size(&input.read(cx).value())
                        && size != app.cursor.size
                    {
                        app.cursor.size = size;
                        app.cursor_changed(pid);
                        cx.notify();
                    }
                },
            ),
        ];
        Self {
            pid,
            display_name,
            cursor_color,
            cursor_size,
            _subscriptions: subscriptions,
        }
    }

    fn sync(&self, app: &LoaderApp, window: &mut Window, cx: &mut App) {
        if !self.display_name.focus_handle(cx).is_focused(window)
            && self.display_name.read(cx).value().as_ref() != app.display_name
        {
            let text = app.display_name.clone();
            self.display_name
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        let shown = self.cursor_color.read(cx).value().map(cursor_rgb);
        if shown != Some(app.cursor.color) {
            let color = cursor_hsla(app.cursor.color);
            self.cursor_color
                .update(cx, |picker, cx| picker.set_value(color, window, cx));
        }
        if !self.cursor_size.focus_handle(cx).is_focused(window)
            && parse_cursor_size(&self.cursor_size.read(cx).value()) != Some(app.cursor.size)
        {
            let text = app.cursor.size.to_string();
            self.cursor_size
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
    }
}

fn status_of(client: &injector::Client, agent_version: Option<&str>) -> (String, Rgba, String) {
    if client.foreign_bot.is_some() {
        return (
            "Unsupported".to_owned(),
            theme::RED,
            "Being used by another platform".to_owned(),
        );
    }
    if let Some(module) = &client.loaded {
        let version = client.version.as_deref().unwrap_or("unknown version");
        let (text, color) = if agent_version.is_some_and(|v| v != version) {
            (format!("Loaded {version}, outdated"), theme::YELLOW)
        } else {
            (format!("Loaded {version}"), theme::GREEN)
        };
        let tip = format!("{text}\n{module}");
        return (text, color, tip);
    }
    let (text, color) = if client.is_64bit {
        ("Not loaded", theme::COMMENT)
    } else {
        ("32-bit, unsupported", theme::RED)
    };
    (text.to_owned(), color, text.to_owned())
}

impl LoaderApp {
    fn cursor_changed(&mut self, pid: u32) {
        if let Some(target) = self.target_of(pid).cloned() {
            self.set_cursor(target);
        }
    }

    fn can_set_display_name(&self, pid: u32) -> bool {
        !self.busy
            && self
                .target_of(pid)
                .is_some_and(|t| t.client.loaded.is_some())
    }

    fn submit_display_name(&mut self, pid: u32) {
        if !self.can_set_display_name(pid) {
            return;
        }
        if let Some(target) = self.target_of(pid).cloned() {
            self.set_display_name(target);
        }
    }

    fn client_action(&mut self, pid: u32, action: ClientAction) {
        let Some(target) = self.target_of(pid).cloned() else {
            return;
        };
        match action {
            ClientAction::Load => self.load(target),
            ClientAction::Unload => self.unload(target),
            ClientAction::Console => self.console(target),
        }
    }

    fn render_client_buttons(&self, pid: u32, window: &Window, cx: &mut Context<Self>) -> Div {
        let client = self.target_of(pid).map(|t| &t.client);
        let idle = !self.busy;
        let loaded = client.is_some_and(|c| c.loaded.is_some());
        let can_load = idle
            && self.agent_version.is_some()
            && client.is_some_and(|c| {
                c.is_64bit && (c.loaded.is_none() || c.version != self.agent_version)
            });
        let can_unload = idle && loaded;
        let (load_icon, load_label) = if loaded {
            (ICON_UPDATE, "Reload")
        } else {
            (ICON_LOAD, "Load")
        };
        let width = widgets::icon_button_width(
            window,
            &[
                (ICON_LOAD, "Load"),
                (ICON_UPDATE, "Reload"),
                (ICON_UNLOAD, "Unload"),
                (ICON_CONSOLE, "Console"),
            ],
        );
        let button = |id: &'static str,
                      icon: &'static str,
                      label: &'static str,
                      enabled: bool,
                      action: ClientAction,
                      cx: &mut Context<Self>| {
            widgets::icon_button(
                id,
                icon,
                label,
                width,
                enabled,
                cx.listener(move |app, _: &ClickEvent, _, cx| {
                    app.client_action(pid, action);
                    cx.notify();
                }),
            )
        };
        widgets::button_group(false)
            .child(
                button(
                    "load",
                    load_icon,
                    load_label,
                    can_load,
                    ClientAction::Load,
                    cx,
                )
                .tooltip(widgets::tooltip(
                    "Loads the DLL from the settings, unloading another build first.",
                )),
            )
            .child(button(
                "unload",
                ICON_UNLOAD,
                "Unload",
                can_unload,
                ClientAction::Unload,
                cx,
            ))
            .child(
                button(
                    "console",
                    ICON_CONSOLE,
                    "Console",
                    can_unload,
                    ClientAction::Console,
                    cx,
                )
                .tooltip(widgets::tooltip(
                    "Opens agent's console window in the client.",
                )),
            )
    }

    fn render_client_options(&self, ui: &Ui, window: &Window, cx: &mut Context<Self>) -> Div {
        let pid = ui.pid;
        let labels = ["Mouse cursor", "Display name", "Proxy"];
        let label_width = widgets::widest(window, &labels, theme::BODY).ceil();
        let option = |label: &'static str, control: Div| {
            div()
                .flex()
                .items_center()
                .gap(COLUMN_GAP)
                .child(div().flex_none().w(label_width).child(label))
                .child(control)
        };
        let app = cx.entity().downgrade();
        let styles = CURSOR_STYLES
            .into_iter()
            .map(|style| {
                let app = app.clone();
                widgets::Choice::new(
                    cursor_style_label(style),
                    style == self.cursor.style,
                    move |_, cx| {
                        app.update(cx, |app, cx| {
                            if app.cursor.style != style {
                                app.cursor.style = style;
                                app.cursor_changed(pid);
                                cx.notify();
                            }
                        })
                        .ok();
                    },
                )
            })
            .collect();
        let cursor_on = self.cursor.style != CursorStyle::Off;
        let color = if cursor_on {
            ColorPicker::new(&ui.cursor_color)
                .small()
                .into_any_element()
        } else {
            div()
                .flex_none()
                .size(SWATCH)
                .border_1()
                .border_color(theme::BORDER)
                .bg(cursor_hsla(self.cursor.color))
                .opacity(0.5)
                .into_any_element()
        };
        let cursor = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(theme::ITEM_GAP)
            .child(widgets::combo(
                "cursor-style",
                cursor_style_label(self.cursor.style),
                styles,
            ))
            .child(color)
            .child(
                NumberInput::new(&ui.cursor_size)
                    .small()
                    .w(CURSOR_SIZE_WIDTH)
                    .suffix("px")
                    .disabled(!cursor_on),
            );
        let can_set = self.can_set_display_name(pid);
        let label = if self.display_name.trim().is_empty() {
            "Reset"
        } else {
            "Set"
        };
        let name = div()
            .flex()
            .items_center()
            .gap(theme::ITEM_GAP)
            .child(
                div()
                    .id("display-name")
                    .w(DISPLAY_NAME_WIDTH)
                    .child(Input::new(&ui.display_name).small().disabled(!can_set))
                    .tooltip(widgets::tooltip(DISPLAY_NAME_HINT)),
            )
            .child(
                widgets::button(
                    "set-display-name",
                    label,
                    can_set,
                    cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.submit_display_name(pid);
                        cx.notify();
                    }),
                )
                .tooltip(widgets::tooltip(DISPLAY_NAME_HINT)),
            );
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(option(labels[0], cursor))
            .child(option(labels[1], name))
            .child(option(labels[2], self.render_client_proxy(pid, cx)))
    }

    fn render_client_proxy(&self, pid: u32, cx: &mut Context<Self>) -> Div {
        let app = cx.entity().downgrade();
        let current = self.assigned_proxies.name(pid).map(str::to_owned);
        let names = std::iter::once(None).chain(
            self.proxies
                .saved()
                .iter()
                .map(|saved| Some(saved.name.clone())),
        );
        let choices = names
            .map(|name| {
                let app = app.clone();
                let label = name.clone().unwrap_or_else(|| "None".into());
                widgets::Choice::new(label, name == current, move |_, cx| {
                    app.update(cx, |app, cx| {
                        if app.assigned_proxies.name(pid) != name.as_deref() {
                            app.set_client_proxy(pid, name.clone());
                            cx.notify();
                        }
                    })
                    .ok();
                })
            })
            .collect();
        let loaded = self
            .target_of(pid)
            .is_some_and(|t| t.client.loaded.is_some());
        let status = match self.assigned_proxies.shown(pid, loaded) {
            crate::proxies::Shown::Off => widgets::weak(if self.proxies.saved().is_empty() {
                "Add proxies in Settings."
            } else {
                "Connections go out directly."
            }),
            crate::proxies::Shown::Waiting => widgets::weak("Applying…"),
            crate::proxies::Shown::NoAgent => {
                widgets::weak("Applies when the agent is loaded. Load it before you log in.")
            }
            crate::proxies::Shown::On(status) => {
                let mut text = format!(
                    "On: {} connections through it, {} shut down to reconnect.",
                    status.proxied, status.dropped
                );
                if let Some(error) = status.last_error() {
                    text.push_str(&format!(" {} failed, last because {error}.", status.failed));
                }
                widgets::weak(text)
            }
            crate::proxies::Shown::Problem(why) => widgets::error(why),
        };
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(theme::ITEM_GAP)
            .child(
                div()
                    .id("client-proxy")
                    .child(widgets::combo(
                        "client-proxy-choice",
                        current.clone().unwrap_or_else(|| "None".into()),
                        choices,
                    ))
                    .tooltip(widgets::tooltip(crate::proxies::HINT)),
            )
            .child(status)
    }

    fn render_client_hider(&self, pid: u32, cx: &mut Context<Self>) -> Div {
        let mask = self.hiders.mask(pid);
        let columns = crate::hider::GROUPS.iter().map(|(title, categories)| {
            let boxes = categories.iter().map(|category| {
                let bit = category.bit;
                Checkbox::new(("hide", bit))
                    .label(category.label)
                    .small()
                    .checked(mask.is_some_and(|mask| mask & bit != 0))
                    .disabled(mask.is_none())
                    .on_click(cx.listener(move |app, on: &bool, _, cx| {
                        if let Some(mask) = app.hiders.mask(pid) {
                            let mask = if *on { mask | bit } else { mask & !bit };
                            app.set_hider(pid, mask);
                        }
                        cx.notify();
                    }))
            });
            div()
                .flex()
                .flex_col()
                .gap(theme::LINE_GAP)
                .min_w(HIDER_COLUMN)
                .child(widgets::strong(*title))
                .children(boxes)
        });
        div()
            .flex()
            .flex_col()
            .gap(theme::LINE_GAP)
            .mt(px(20.0))
            .child(widgets::strong("Entity hider"))
            .child(div().flex().flex_wrap().gap(COLUMN_GAP).children(columns))
    }

    fn render_client_children(&self, pid: u32, cx: &mut Context<Self>) -> Div {
        let buttons = self
            .client_node(pid)
            .children
            .into_iter()
            .enumerate()
            .map(|(i, child)| {
                let pick = child.pick;
                widgets::button(
                    ("client-child", i),
                    child.name,
                    true,
                    cx.listener(move |app, _: &ClickEvent, _, cx| {
                        app.go(pick.0, pick.1);
                        cx.notify();
                    }),
                )
            });
        div()
            .flex()
            .flex_wrap()
            .gap(theme::ITEM_GAP)
            .mt(px(20.0))
            .children(buttons)
    }

    fn render_client_facts(&self, pid: u32) -> Div {
        let show_prefix = !cfg!(windows);
        let fact = |label: &'static str, value: Stateful<Div>| {
            div()
                .flex()
                .items_center()
                .gap(COLUMN_GAP)
                .child(
                    div()
                        .flex_none()
                        .w(px(70.0))
                        .text_color(theme::COMMENT)
                        .child(label),
                )
                .child(value.min_w_0())
        };
        let Some(target) = self.detected(pid) else {
            return div().child(widgets::weak(
                "This client has closed. Its runs stay here until you close them.",
            ));
        };
        let client = &target.client;
        let (status, color, tip) = status_of(client, self.agent_version.as_deref());
        let prefix = target.location.label().unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(theme::LINE_GAP)
            .child(fact(
                "Title",
                div()
                    .id("client-title")
                    .truncate()
                    .child(client.title.clone())
                    .tooltip(widgets::tooltip(client.exe.clone())),
            ))
            .when(show_prefix, |this| {
                this.child(fact(
                    "Prefix",
                    div()
                        .id("client-prefix")
                        .truncate()
                        .child(prefix.clone())
                        .tooltip(widgets::tooltip(prefix)),
                ))
            })
            .child(fact(
                "Agent",
                div()
                    .id("client-status")
                    .text_color(color)
                    .child(status)
                    .tooltip(widgets::tooltip(tip)),
            ))
    }

    pub(super) fn render_client(
        &mut self,
        pid: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ui = match self.clients_ui.take().filter(|ui| ui.pid == pid) {
            Some(ui) => ui,
            None => Ui::new(pid, self, window, cx),
        };
        ui.sync(self, window, cx);
        let usable = self.target_of(pid).is_some_and(|t| t.client.is_64bit);
        let page = div()
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .gap(theme::LINE_GAP)
            .child(crate::logo::background())
            .child(widgets::heading(format!("Client {pid}")))
            .child(self.render_client_facts(pid));
        let page = if usable {
            page.child(
                div()
                    .flex()
                    .mt(theme::ITEM_GAP)
                    .child(self.render_client_buttons(pid, window, cx)),
            )
            .child(
                div()
                    .mt(px(6.0))
                    .child(self.render_client_options(&ui, window, cx)),
            )
            .when(
                self.target_of(pid)
                    .is_some_and(|t| t.client.loaded.is_some()),
                |page| page.child(self.render_client_hider(pid, cx)),
            )
        } else {
            page
        };
        let page = page.child(self.render_client_children(pid, cx));
        self.clients_ui = Some(ui);
        page.into_any_element()
    }
}

#[cfg(test)]
pub(super) fn target(pid: u32, title: &str, loaded: Option<&str>, foreign: bool) -> Target {
    let location = if cfg!(windows) {
        serde_json::Value::Null
    } else {
        serde_json::json!({
            "prefix": "/home/tester/.local/share/launcher/game_wine_prefix",
            "wine": "/usr/bin/wine",
            "env": [],
        })
    };
    serde_json::from_value(serde_json::json!({
        "client": {
            "pid": pid,
            "exe": format!("C:\\Program Files\\Game\\{title}.exe"),
            "title": title,
            "is_64bit": true,
            "loaded": loaded.map(|_| "Z:\\home\\tester\\agent.dll"),
            "version": loaded,
            "foreign_bot": foreign.then_some("other"),
        },
        "location": location,
    }))
    .expect("a fake target")
}

#[cfg(test)]
pub(super) fn populated(app: &mut LoaderApp) {
    app.agent_version = Some("2026.11.20-abc1234".into());
    app.targets = vec![
        target(4120, "Game client", Some("2026.11.20-abc1234"), false),
        target(5388, "Game client - second account", None, false),
        target(6012, "Game client", Some("2026.10.02-0f0e0d0"), false),
        target(7301, "Game client", None, true),
    ];
}

#[cfg(test)]
pub(super) fn snapshots() -> Vec<super::snapshots::State> {
    vec![
        ("client-loaded", |app| {
            populated(app);
            app.page = Page::Client(4120);
            app.display_name = "Alt name".into();
        }),
        ("client-hider", |app| {
            populated(app);
            app.page = Page::Client(4120);
            let asked_at = app.hiders.change();
            app.hiders.answered(
                4120,
                asked_at,
                false,
                Some(injector::hide::OTHERS | injector::hide::WALLS | injector::hide::PROJECTILES),
            );
            app.hiders.poll(Instant::now());
        }),
        ("client-proxy", |app| {
            populated(app);
            app.page = Page::Client(4120);
            app.proxies = crate::proxies::Proxies::with(super::settings_page::snapshot_proxies());
            app.assigned_proxies
                .assign(4120, Some("Residential US".into()));
            app.assigned_proxies.answered(
                4120,
                Ok(injector::ProxyStatus {
                    written: 2,
                    adopted: 2,
                    state: injector::proxy_state::ON,
                    proxied: 5,
                    dropped: 2,
                    ..injector::ProxyStatus::default()
                }),
            );
        }),
        ("client-unloaded", |app| {
            populated(app);
            app.page = Page::Client(5388);
            app.cursor.style = CursorStyle::Off;
        }),
        ("client-running", |app| {
            super::sessions::populated(app);
            app.page = Page::Client(4120);
        }),
        ("client-busy", |app| {
            populated(app);
            app.page = Page::Client(4120);
            app.busy = true;
        }),
    ]
}
