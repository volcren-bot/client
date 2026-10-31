//! The HTTP whitelist: the hosts a script's `http_request` may reach, edited under Settings.
//!
//! Entries are hosts, not full URLs: `example.com` matches exactly that host (any scheme, port and path), `*.example.com`
//! matches its subdomains. The Volcren hosts come from the loader's config and are always allowed; the user's own entries
//! are kept in the loader's storage and can be added and removed.

use eframe::egui;
use injector::http;

use crate::api::Config;

/// Height of a list row.
const ROW_HEIGHT: f32 = 28.0;
/// Width of the list's last column, which holds the Delete button.
const ACTIONS_WIDTH: f32 = 90.0;
/// Width of the dialog's text box; the dialog fits around it.
const INPUT_WIDTH: f32 = 300.0;
/// How many entries the user can add.
const MAX_USER_ENTRIES: usize = 200;

/// What scripts may reach. Cheap to clone, so a request thread can carry the list as it was when the script asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Whitelist {
    /// The Volcren hosts: shown in the list, never removable.
    builtin: Vec<String>,
    user: Vec<String>,
}

/// The host a whitelist entry stands for, lowercase, from a host, `*.host` or a URL: `https://Example.com:8080/a` is `example.com`.
pub fn normalize(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter a host or URL.".into());
    }
    let (wildcard, rest) = match input.strip_prefix("*.") {
        Some(rest) => (true, rest),
        None => (false, input),
    };
    let rest = if rest.contains("://") {
        rest.to_owned()
    } else {
        format!("https://{rest}")
    };
    if rest.contains('*') || input.contains(char::is_whitespace) {
        return Err("Use a host like example.com, or *.example.com for its subdomains.".into());
    }
    let uri: ureq::http::Uri = rest
        .parse()
        .map_err(|_| "That isn't a valid host or URL.".to_owned())?;
    let host = crate::http::host_of(&uri)
        .map_err(|_| "Only http and https URLs can be whitelisted.".to_owned())?;
    let label = |label: &str| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    let valid = host.starts_with('[') || host.split('.').all(label);
    if host.is_empty() || !valid {
        return Err("That isn't a valid host.".into());
    }
    if wildcard && !host.contains('.') {
        return Err("A wildcard needs a domain, like *.example.com.".into());
    }
    Ok(if wildcard { format!("*.{host}") } else { host })
}

fn matches(entry: &str, host: &str) -> bool {
    match entry.strip_prefix("*.") {
        Some(domain) => host
            .strip_suffix(domain)
            .is_some_and(|label| label.len() > 1 && label.ends_with('.')),
        None => entry == host,
    }
}

impl Whitelist {
    /// `user` is what was saved; the built-in hosts are the ones in `config`.
    pub fn new(user: Vec<String>, config: &Config) -> Self {
        let mut builtin = Vec::new();
        for url in [&config.api_url, &config.supabase_url, &config.site_url] {
            if let Ok(uri) = url.parse::<ureq::http::Uri>()
                && let Ok(host) = crate::http::host_of(&uri)
                && !builtin.contains(&host)
            {
                builtin.push(host);
            }
        }
        Self {
            builtin,
            user: user
                .into_iter()
                .filter_map(|entry| normalize(&entry).ok())
                .collect(),
        }
    }

    /// Whether a script may reach this host (lowercase, no trailing dot).
    pub fn allows(&self, host: &str) -> bool {
        self.builtin
            .iter()
            .chain(&self.user)
            .any(|entry| matches(entry, host))
    }

    pub fn user(&self) -> &[String] {
        &self.user
    }

    /// Adds what the user typed; `Err` is why not.
    pub fn add(&mut self, input: &str) -> Result<(), String> {
        let entry = normalize(input)?;
        if self.builtin.contains(&entry) || self.user.contains(&entry) {
            return Err(format!("{entry} is already on the list."));
        }
        if self.user.len() >= MAX_USER_ENTRIES {
            return Err(format!(
                "The list holds at most {MAX_USER_ENTRIES} entries of your own."
            ));
        }
        self.user.push(entry);
        Ok(())
    }

    /// Removes one of the user's entries, never a built-in one.
    pub fn remove(&mut self, entry: &str) {
        self.user.retain(|have| have != entry);
    }

    /// Answers one script request; the request itself is made here, so call it from a thread.
    pub fn perform(&self, request: &http::Request) -> Result<http::Response, String> {
        crate::http::perform(self, request)
    }
}

/// The Add dialog's text and why the last Save was refused.
#[derive(Default)]
struct Draft {
    input: String,
    error: Option<String>,
}

/// The Settings section: the list, its Add dialog and the entry whose Delete is waiting for a second click.
#[derive(Default)]
pub struct Page {
    draft: Option<Draft>,
    deleting: Option<String>,
}

impl Page {
    pub fn ui(&mut self, ui: &mut egui::Ui, list: &mut Whitelist) {
        ui.add_space(12.0);
        ui.heading("HTTP whitelist");
        ui.label(egui::RichText::new("Scripts can only make HTTP requests to these hosts. Redirects to any other host are refused.").weak());
        ui.add_space(6.0);
        ui.add_enabled_ui(self.draft.is_none(), |ui| {
            if ui.button("Add URL").clicked() {
                self.draft = Some(Draft::default());
            }
            ui.add_space(6.0);
            self.list_ui(ui, list);
        });
        self.dialog_ui(ui.ctx(), list);
    }

    fn list_ui(&mut self, ui: &mut egui::Ui, list: &mut Whitelist) {
        let mut remove = None;
        let rows: Vec<(&str, bool)> = list
            .builtin
            .iter()
            .map(|h| (h.as_str(), true))
            .chain(list.user.iter().map(|h| (h.as_str(), false)))
            .collect();
        egui_extras::TableBuilder::new(ui)
            .id_salt("http-whitelist")
            .striped(true)
            .vscroll(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::remainder().clip(true))
            .column(egui_extras::Column::exact(ACTIONS_WIDTH))
            .header(ROW_HEIGHT, |mut header| {
                for head in ["Host", ""] {
                    header.col(|ui| {
                        ui.strong(head);
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let (host, builtin) = rows[row.index()];
                    row.col(|ui| {
                        ui.label(host);
                    });
                    row.col(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if builtin {
                                ui.label(egui::RichText::new("Built in").weak())
                                    .on_hover_text("The Volcren hosts are always allowed");
                            } else {
                                let armed = self.deleting.as_deref() == Some(host);
                                if ui.button(if armed { "Sure?" } else { "Delete" }).clicked() {
                                    remove = Some(host.to_owned());
                                }
                            }
                        });
                    });
                });
            });
        let Some(host) = remove else { return };
        if self.deleting.as_deref() == Some(host.as_str()) {
            list.remove(&host);
            self.deleting = None;
        } else {
            self.deleting = Some(host);
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context, list: &mut Whitelist) {
        let Some(mut draft) = self.draft.take() else {
            return;
        };
        let mut decision = None;
        egui::Window::new("Add URL")
            .id(egui::Id::new("http-whitelist-dialog"))
            .collapsible(false)
            // Fit the content every frame: eframe remembers a window's size between runs, which would keep an old width.
            .auto_sized()
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("A host or URL scripts may send requests to. Use *.example.com for every subdomain.");
                let response = ui.add(egui::TextEdit::singleline(&mut draft.input).hint_text("e.g. api.example.com").desired_width(INPUT_WIDTH));
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    decision = Some(true);
                }
                if let Some(error) = &draft.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                ui.add_space(6.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save").clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            decision = Some(false);
        }
        match decision {
            Some(true) => {
                if let Err(why) = list.add(&draft.input) {
                    draft.error = Some(why);
                    self.draft = Some(draft);
                }
            }
            Some(false) => {}
            None => self.draft = Some(draft),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            api_url: "https://api.volcren.com".into(),
            supabase_url: "https://db.volcren.com".into(),
            anon_key: String::new(),
            site_url: "https://volcren.com".into(),
        }
    }

    #[test]
    fn normalizes_hosts_and_urls() {
        for (input, host) in [
            ("example.com", "example.com"),
            ("  Example.COM ", "example.com"),
            ("https://Example.com:8080/a/b?c=d", "example.com"),
            ("http://example.com./", "example.com"),
            ("*.Example.com", "*.example.com"),
            ("https://user:pw@example.com/", "example.com"),
            ("127.0.0.1", "127.0.0.1"),
            ("localhost", "localhost"),
        ] {
            assert_eq!(normalize(input).as_deref(), Ok(host), "{input:?}");
        }
        for bad in [
            "",
            "  ",
            "ftp://example.com",
            "exa mple.com",
            "ex*mple.com",
            "*.com",
            "*",
            "https://",
            "a..b",
            "-.",
            "a/b c",
        ] {
            assert!(normalize(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn builtin_hosts_come_from_the_config_and_are_allowed() {
        let list = Whitelist::new(Vec::new(), &config());
        for host in ["api.volcren.com", "db.volcren.com", "volcren.com"] {
            assert!(list.allows(host), "{host}");
        }
        assert!(!list.allows("example.com"));
        assert!(!list.allows("evil-volcren.com"));
        assert!(!list.allows("volcren.com.evil.com"));
        assert!(
            !list.allows("www.volcren.com"),
            "an exact host does not cover subdomains"
        );
    }

    #[test]
    fn wildcards_match_subdomains_only() {
        let mut list = Whitelist::new(Vec::new(), &config());
        list.add("*.example.com").unwrap();
        assert!(list.allows("a.example.com"));
        assert!(list.allows("a.b.example.com"));
        assert!(!list.allows("example.com"));
        assert!(!list.allows("badexample.com"));
        assert!(!list.allows(".example.com"));
        assert!(!list.allows("example.com.evil.com"));
    }

    #[test]
    fn adds_and_removes_user_entries_but_never_builtin_ones() {
        let mut list = Whitelist::new(Vec::new(), &config());
        assert_eq!(list.add("https://Example.com/x"), Ok(()));
        assert!(list.allows("example.com"));
        assert!(list.add("example.com").is_err(), "twice");
        assert!(list.add("volcren.com").is_err(), "already built in");
        list.remove("volcren.com");
        assert!(list.allows("volcren.com"));
        list.remove("example.com");
        assert!(!list.allows("example.com"));
        assert!(list.user().is_empty());
    }

    #[test]
    fn saved_entries_that_are_no_longer_valid_are_dropped() {
        let list = Whitelist::new(
            vec!["Example.com".into(), "not a host".into(), "*.com".into()],
            &config(),
        );
        assert_eq!(list.user(), ["example.com"]);
    }

    #[test]
    fn an_env_override_changes_the_builtin_hosts() {
        let mut local = config();
        local.api_url = "http://localhost:3000".into();
        assert!(Whitelist::new(Vec::new(), &local).allows("localhost"));
        assert!(!Whitelist::new(Vec::new(), &config()).allows("localhost"));
    }
}
