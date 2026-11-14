use std::collections::HashMap;
use std::time::Instant;

use eframe::egui;
use injector::manifest::Manifest;
use serde_json::Value;

use super::{Event, LoaderApp, PendingStart, Run};
use crate::api::models::ScriptManifest;
use crate::api::{Api, ApiError, Me, StoreScript};
use crate::libs::Lib;
use crate::queue::{self, Entry, Goal, OnChange, Outcome, Phase, Runner, WhenFinished};
use crate::script::{Launch, Script};
use crate::session;

const LOGOUT_SOURCE: &str = "local Logout = require(\"@lib/main/interfaces/gametabs/logout\")\nif not Logout.logout() then\n\terror(\"Couldn't log out\", 0)\nend\n";

pub(super) struct Check {
    me: Me,
    scripts: Vec<StoreScript>,
    manifests: HashMap<String, Result<ScriptManifest, String>>,
    quotes: HashMap<String, Result<crate::api::models::Quote, String>>,
    lib: Result<Lib, String>,
}

fn check(api: &Api, ids: &[String]) -> Result<Box<Check>, ApiError> {
    let me = api.me()?;
    let mut scripts = api.scripts("", "", false)?;
    if me.can_browse_community() && ids.iter().any(|id| scripts.iter().all(|s| &s.id != id)) {
        scripts.extend(api.scripts("", "", true)?);
    }
    let mut manifests = HashMap::new();
    let mut quotes = HashMap::new();
    for id in ids {
        if manifests.contains_key(id) {
            continue;
        }
        manifests.insert(id.clone(), api.manifest(id).map_err(|e| e.to_string()));
        quotes.insert(id.clone(), api.quote(id).map_err(|e| e.to_string()));
    }
    let lib = session::cache_root()
        .ok_or_else(|| "Can't find a cache folder on this system.".to_owned())
        .and_then(|root| crate::libs::ensure(api, &root));
    Ok(Box::new(Check {
        me,
        scripts,
        manifests,
        quotes,
        lib,
    }))
}

pub fn spec_hash(manifest: &Manifest) -> Option<String> {
    manifest
        .settings
        .as_ref()
        .map(|spec| session::sha256_hex(spec.to_string().as_bytes()))
}

#[derive(Debug)]
struct Ready {
    script: StoreScript,
    manifest: Manifest,
    price: i64,
}

impl Check {
    fn ready(&self, entry: &Entry, confirmed: &crate::store::Confirmed) -> Result<Ready, String> {
        let title = &entry.title;
        let script = self
            .scripts
            .iter()
            .find(|s| s.id == entry.script_id)
            .ok_or_else(|| format!("{title} is no longer available to this account."))?;
        if !script.can_access {
            return Err(format!("You can't run {title} right now."));
        }
        if !script.queueable {
            return Err(format!("{title}'s live revision can't run in a queue."));
        }
        let fetched = match self.manifests.get(&entry.script_id) {
            Some(Ok(fetched)) => fetched,
            Some(Err(error)) => return Err(format!("{title}: {error}")),
            None => return Err(format!("{title} wasn't checked.")),
        };
        let manifest =
            Manifest::parse(fetched.manifest.clone()).map_err(|e| format!("{title}: {e}"))?;
        if !manifest.queueable {
            return Err(format!("{title}'s live revision can't run in a queue."));
        }
        if spec_hash(&manifest) != entry.spec_hash {
            return Err(format!(
                "{title}'s settings changed. Open its entry to check them."
            ));
        }
        if script.community && !confirmed.has(&script.id, fetched.revision) {
            return Err(format!(
                "{title} has a revision you haven't reviewed. Run it once from the Scripts page."
            ));
        }
        let price = match self.quotes.get(&entry.script_id) {
            Some(Ok(quote)) => quote.price,
            Some(Err(error)) => return Err(format!("{title}: {error}")),
            None => return Err(format!("{title} wasn't quoted.")),
        };
        Ok(Ready {
            script: StoreScript {
                revision: Some(fetched.revision),
                ..script.clone()
            },
            manifest,
            price,
        })
    }
}

pub(super) struct Prompt {
    pid: u32,
    queue: queue::Queue,
    rows: Vec<(String, i64, Option<i64>)>,
    approved: HashMap<String, i64>,
}

#[derive(Default)]
pub(super) struct Page {
    selected: Option<usize>,
    form: Option<(usize, String, crate::form::Form)>,
    save_name: String,
    add: Option<String>,
    checking: Option<u32>,
    fetching: std::collections::HashSet<String>,
    prompt: Option<Prompt>,
}

impl LoaderApp {
    pub(super) fn sync_queue_form(&mut self) {
        let Some((index, script_id, form)) = &self.queue_page.form else {
            return;
        };
        let hash = self
            .manifests
            .get(script_id)
            .and_then(|(_, m)| spec_hash(m));
        if let Some(entry) = self
            .queues
            .current
            .entries
            .get_mut(*index)
            .filter(|e| &e.script_id == script_id)
        {
            entry.settings = form.remembered();
            entry.spec_hash = hash;
        }
    }

    fn select_entry(&mut self, index: Option<usize>) {
        self.sync_queue_form();
        self.queue_page.form = None;
        self.queue_page.selected = index;
    }

    fn entry_form(&mut self, ctx: &egui::Context) {
        let Some(index) = self.queue_page.selected else {
            return;
        };
        let Some(entry) = self.queues.current.entries.get(index) else {
            self.queue_page.selected = None;
            return;
        };
        if self
            .queue_page
            .form
            .as_ref()
            .is_some_and(|(i, id, _)| *i == index && id == &entry.script_id)
        {
            return;
        }
        let script_id = entry.script_id.clone();
        match self.manifests.get(&script_id) {
            Some((_, manifest)) => {
                let spec = manifest
                    .settings
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({ "fields": [] }));
                let mut form = crate::form::Form::new(spec, Some(&entry.settings));
                form.title = entry.title.clone();
                self.queue_page.form = Some((index, script_id, form));
                self.sync_queue_form();
            }
            None if !self.queue_page.fetching.contains(&script_id) => {
                self.queue_page.fetching.insert(script_id.clone());
                self.api_job(ctx, move |api| Event::QueueManifest {
                    result: api.manifest(&script_id),
                    script_id,
                });
            }
            None => {}
        }
    }

    pub(super) fn queue_manifest(
        &mut self,
        script_id: String,
        result: Result<ScriptManifest, ApiError>,
    ) {
        self.queue_page.fetching.remove(&script_id);
        match result.map_err(|e| e.to_string()).and_then(|fetched| {
            Manifest::parse(fetched.manifest).map(|manifest| (fetched.revision, manifest))
        }) {
            Ok(cached) => {
                self.manifests.insert(script_id, cached);
            }
            Err(error) => {
                self.script_error(format!("Couldn't load the script's settings: {error}"))
            }
        }
    }

    fn queueable_scripts(&self) -> Vec<StoreScript> {
        let mut scripts: Vec<StoreScript> = self
            .store
            .scripts
            .iter()
            .filter(|s| s.queueable && s.can_access)
            .cloned()
            .collect();
        scripts.sort_by(|a, b| a.title.cmp(&b.title));
        scripts
    }

    pub(super) fn queue_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.heading("Queue");
        if self.auth.me().is_none() {
            ui.label("Sign in to queue store scripts.");
            return;
        }
        if !self.store.loaded && !self.store.loading && self.store.error.is_none() {
            self.fetch_scripts(&ctx);
        }
        self.running_queues_ui(ui);
        ui.add_space(8.0);
        self.saved_queues_ui(ui);
        ui.add_space(8.0);
        self.queue_options_ui(ui);
        ui.add_space(8.0);
        self.add_entry_ui(ui);
        ui.separator();
        self.entry_form(&ctx);
        egui::ScrollArea::vertical()
            .id_salt("queue")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.entries_ui(ui);
                ui.separator();
                self.entry_editor_ui(ui);
            });
        self.queue_prompt_ui(&ctx);
    }

    fn running_queues_ui(&mut self, ui: &mut egui::Ui) {
        let mut stop = None;
        let mut finish = None;
        for runner in &self.queue_runners {
            ui.horizontal_wrapped(|ui| {
                crate::theme::status_dot(ui, crate::theme::GREEN);
                ui.label(format!("Client {}: {}", runner.pid, runner.progress()));
                ui.weak(crate::xp::duration(runner.started.elapsed()));
                if ui
                    .button("Finish")
                    .on_hover_text("Lets the current script wrap up, then ends the queue.")
                    .clicked()
                {
                    finish = Some(runner.pid);
                }
                if ui
                    .button("Stop")
                    .on_hover_text("Stops the current script now and ends the queue.")
                    .clicked()
                {
                    stop = Some(runner.pid);
                }
            });
        }
        if let Some(pid) = finish
            && let Some(runner) = self.queue_runners.iter_mut().find(|r| r.pid == pid)
        {
            runner.finishing = true;
        }
        if let Some(pid) = stop {
            self.end_queue(pid, "Queue stopped.".into());
            if let Some(run) = self
                .runs
                .iter_mut()
                .find(|r| r.target.as_ref().is_some_and(|t| t.client.pid == pid) && r.is_running())
            {
                run.stop();
            }
        }
    }

    fn saved_queues_ui(&mut self, ui: &mut egui::Ui) {
        let mut load = None;
        let mut delete = None;
        let mut save = false;
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("saved_queues")
                .selected_text("Load a saved queue")
                .show_ui(ui, |ui| {
                    for saved in &self.queues.saved {
                        if ui.selectable_label(false, &saved.name).clicked() {
                            load = Some(saved.name.clone());
                        }
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.queue_page.save_name)
                    .hint_text(&self.queues.current.name)
                    .desired_width(160.0),
            );
            save = ui.button("Save").clicked();
            if self
                .queues
                .saved
                .iter()
                .any(|q| q.name == self.queues.current.name)
                && ui.button("Delete saved").clicked()
            {
                delete = Some(self.queues.current.name.clone());
            }
        });
        if let Some(name) = load {
            self.select_entry(None);
            self.queues.load(&name);
            self.queue_page.save_name = name;
        }
        if save {
            self.sync_queue_form();
            let name = if self.queue_page.save_name.trim().is_empty() {
                self.queues.current.name.clone()
            } else {
                self.queue_page.save_name.clone()
            };
            match self.queues.save_as(&name) {
                Ok(()) => self.info(format!("Saved the queue {}.", name.trim())),
                Err(error) => self.error(error),
            }
        }
        if let Some(name) = delete {
            self.queues.delete(&name);
            self.info(format!("Deleted the saved queue {name}."));
        }
    }

    fn queue_options_ui(&mut self, ui: &mut egui::Ui) {
        let queue = &mut self.queues.current;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut queue.repeat, "Repeat")
                .on_hover_text("Starts over from the first entry after the last one.");
            ui.label("When finished:");
            egui::ComboBox::from_id_salt("queue_when_finished")
                .selected_text(match queue.when_finished {
                    WhenFinished::LogOut => "log out",
                    WhenFinished::StayLoggedIn => "stay logged in",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut queue.when_finished, WhenFinished::LogOut, "log out");
                    ui.selectable_value(
                        &mut queue.when_finished,
                        WhenFinished::StayLoggedIn,
                        "stay logged in",
                    );
                });
            ui.label("If a script changed:").on_hover_text(
                "Its price, its settings or a new community revision since the queue started.",
            );
            egui::ComboBox::from_id_salt("queue_on_change")
                .selected_text(match queue.on_change {
                    OnChange::Stop => "stop the queue",
                    OnChange::Skip => "skip it",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut queue.on_change, OnChange::Stop, "stop the queue");
                    ui.selectable_value(&mut queue.on_change, OnChange::Skip, "skip it");
                });
        });
    }

    fn add_entry_ui(&mut self, ui: &mut egui::Ui) {
        let scripts = self.queueable_scripts();
        let ctx = ui.ctx().clone();
        let mut add = None;
        let mut start = None;
        ui.horizontal_wrapped(|ui| {
            let chosen = self
                .queue_page
                .add
                .as_ref()
                .and_then(|id| scripts.iter().find(|s| &s.id == id));
            egui::ComboBox::from_id_salt("queue_add")
                .selected_text(chosen.map_or("Pick a queueable script", |s| s.title.as_str()))
                .show_ui(ui, |ui| {
                    for script in &scripts {
                        if ui.selectable_label(false, &script.title).clicked() {
                            self.queue_page.add = Some(script.id.clone());
                        }
                    }
                });
            if ui
                .add_enabled(
                    chosen.is_some() && self.queues.current.entries.len() < queue::MAX_ENTRIES,
                    egui::Button::new("Add"),
                )
                .clicked()
            {
                add = chosen.cloned();
            }
            ui.separator();
            let target = self.run_target_ui(ui);
            let problem = self
                .queues
                .current
                .problem()
                .or_else(|| self.run_block(target.as_ref()))
                .or_else(|| {
                    self.queue_page
                        .checking
                        .map(|_| "Checking the queue…".into())
                });
            let button = ui.add_enabled(problem.is_none(), egui::Button::new("Start queue"));
            let button = match &problem {
                Some(why) => button.on_disabled_hover_text(why),
                None => button,
            };
            if button.clicked() {
                start = target;
            }
        });
        if scripts.is_empty() && self.store.loaded {
            ui.weak("None of your scripts can run in a queue yet.");
        }
        if let Some(script) = add {
            self.sync_queue_form();
            self.queues
                .current
                .entries
                .push(Entry::new(script.id.clone(), script.title.clone()));
            let last = self.queues.current.entries.len() - 1;
            self.select_entry(Some(last));
        }
        if let Some(target) = start {
            self.check_queue(&ctx, target.client.pid);
        }
    }

    fn entries_ui(&mut self, ui: &mut egui::Ui) {
        let mut select = None;
        let mut action: Option<(usize, i32)> = None;
        for (i, entry) in self.queues.current.entries.iter().enumerate() {
            ui.horizontal(|ui| {
                let picked = self.queue_page.selected == Some(i);
                if ui
                    .selectable_label(picked, format!("{}. {}", i + 1, entry.title))
                    .clicked()
                {
                    select = Some(i);
                }
                ui.weak(limits_text(entry));
                if picked {
                    if ui.small_button("Up").clicked() {
                        action = Some((i, -1));
                    }
                    if ui.small_button("Down").clicked() {
                        action = Some((i, 1));
                    }
                    if ui.small_button("Duplicate").clicked() {
                        action = Some((i, 2));
                    }
                    if ui.small_button("Remove").clicked() {
                        action = Some((i, 0));
                    }
                }
            });
        }
        if self.queues.current.entries.is_empty() {
            ui.weak("Add scripts to build a queue. Each entry keeps its own settings.");
        }
        if let Some(i) = select {
            self.select_entry(Some(i));
        }
        if let Some((i, what)) = action {
            self.sync_queue_form();
            self.queue_page.form = None;
            let entries = &mut self.queues.current.entries;
            match what {
                0 => {
                    entries.remove(i);
                    self.queue_page.selected = None;
                }
                2 if entries.len() < queue::MAX_ENTRIES => {
                    entries.insert(i + 1, entries[i].clone());
                    self.queue_page.selected = Some(i + 1);
                }
                -1 if i > 0 => {
                    entries.swap(i, i - 1);
                    self.queue_page.selected = Some(i - 1);
                }
                1 if i + 1 < entries.len() => {
                    entries.swap(i, i + 1);
                    self.queue_page.selected = Some(i + 1);
                }
                _ => {}
            }
        }
    }

    fn entry_editor_ui(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.queue_page.selected else {
            return;
        };
        let Some(entry) = self.queues.current.entries.get(index) else {
            return;
        };
        let goals = self
            .manifests
            .get(&entry.script_id)
            .map(|(_, m)| m.goals.clone())
            .unwrap_or_default();
        let entry = &mut self.queues.current.entries[index];
        ui.strong(&entry.title);
        ui.horizontal_wrapped(|ui| {
            ui.label("Goal:");
            let current = entry
                .goal
                .as_ref()
                .and_then(|g| goals.iter().find(|goal| goal.id == g.counter))
                .map_or("none", |g| g.label.as_str());
            let mut pick: Option<Option<String>> = None;
            egui::ComboBox::from_id_salt(("queue_goal", index))
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(entry.goal.is_none(), "none").clicked() {
                        pick = Some(None);
                    }
                    for goal in &goals {
                        if ui.selectable_label(false, &goal.label).clicked() {
                            pick = Some(Some(goal.id.clone()));
                        }
                    }
                });
            match pick {
                Some(None) => entry.goal = None,
                Some(Some(counter)) => {
                    let target = entry.goal.as_ref().map_or(1.0, |g| g.target);
                    entry.goal = Some(Goal { counter, target });
                }
                None => {}
            }
            if let Some(goal) = entry.goal.as_mut() {
                ui.add(
                    egui::DragValue::new(&mut goal.target)
                        .range(1.0..=1e9)
                        .speed(1.0),
                );
            }
            if goals.is_empty() {
                ui.weak("(this script names no goals)");
            }
        });
        minutes_ui(ui, "At least", &mut entry.min_minutes, index, "min");
        minutes_ui(ui, "At most", &mut entry.max_minutes, index, "max");
        ui.add_space(8.0);
        let title = entry.title.clone();
        match self.queue_page.form.take() {
            Some((i, id, mut form)) if i == index => {
                form.show_inline(
                    ui,
                    self.layouts.get(&title).unwrap_or(&super::EMPTY_LAYOUTS),
                );
                self.queue_page.form = Some((i, id, form));
            }
            other => {
                self.queue_page.form = other;
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading the script's settings…");
                });
            }
        }
    }

    fn check_queue(&mut self, ctx: &egui::Context, pid: u32) {
        self.sync_queue_form();
        self.queue_page.checking = Some(pid);
        let ids: Vec<String> = self
            .queues
            .current
            .entries
            .iter()
            .map(|e| e.script_id.clone())
            .collect();
        self.api_job(ctx, move |api| Event::QueueChecked {
            pid,
            entry: false,
            result: check(&api, &ids),
        });
    }

    pub(super) fn queue_checked(
        &mut self,
        ctx: &egui::Context,
        pid: u32,
        entry: bool,
        result: Result<Box<Check>, ApiError>,
    ) {
        let check = match result {
            Ok(check) => check,
            Err(ApiError::SignedOut(why)) => return self.sign_out(Some(why)),
            Err(error) => {
                self.queue_page.checking = None;
                return if entry {
                    self.end_queue(pid, format!("Queue stopped: {error}"))
                } else {
                    self.script_error(format!("Couldn't check the queue: {error}"))
                };
            }
        };
        self.refresh_me(check.me.clone());
        for (id, fetched) in &check.manifests {
            if let Ok(fetched) = fetched
                && let Ok(manifest) = Manifest::parse(fetched.manifest.clone())
            {
                self.manifests
                    .insert(id.clone(), (fetched.revision, manifest));
            }
        }
        if entry {
            self.start_checked_entry(ctx, pid, &check);
        } else {
            self.queue_page.checking = None;
            self.prompt_queue(pid, &check);
        }
    }

    fn prompt_queue(&mut self, pid: u32, check: &Check) {
        if let Err(error) = &check.lib {
            return self.script_error(format!("Couldn't start the queue: {error}"));
        }
        let queue = self.queues.current.clone();
        let mut rows = Vec::new();
        let mut approved = HashMap::new();
        let mut problems = Vec::new();
        for entry in &queue.entries {
            match check.ready(entry, &self.confirmed) {
                Ok(ready) => {
                    rows.push((
                        entry.title.clone(),
                        ready.price,
                        entry.most_cost(ready.price),
                    ));
                    approved.insert(entry.script_id.clone(), ready.price);
                }
                Err(problem) => problems.push(problem),
            }
        }
        if !problems.is_empty() {
            return self.script_error(format!("Couldn't start the queue: {}", problems.join(" ")));
        }
        self.queue_page.prompt = Some(Prompt {
            pid,
            queue,
            rows,
            approved,
        });
    }

    fn queue_prompt_ui(&mut self, ctx: &egui::Context) {
        let Some(prompt) = &self.queue_page.prompt else {
            return;
        };
        let mut choice = None;
        egui::Window::new("Start the queue?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!(
                    "{} on client {}{}",
                    prompt.queue.name,
                    prompt.pid,
                    if prompt.queue.repeat { ", repeating" } else { "" }
                ));
                egui::Grid::new("queue_prices").striped(true).show(ui, |ui| {
                    ui.strong("Script");
                    ui.strong("Per hour");
                    ui.strong("At most");
                    ui.end_row();
                    for (title, price, most) in &prompt.rows {
                        ui.label(title);
                        ui.label(crate::api::models::rate_label(*price as f64));
                        ui.label(most.map_or_else(
                            || "no limit".to_owned(),
                            |m| crate::api::models::credits_label(m as f64),
                        ));
                        ui.end_row();
                    }
                });
                if prompt.queue.repeat || prompt.rows.iter().any(|(_, p, m)| *p > 0 && m.is_none()) {
                    ui.weak("Entries without a maximum time are billed for as long as they run.");
                }
                ui.weak("If a script's price or settings change, or a community script gets a new revision, the queue follows the option you picked.");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Start").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
        match choice {
            Some(true) => {
                if let Some(prompt) = self.queue_page.prompt.take() {
                    if self.free_client(prompt.pid).is_none() {
                        return self.script_error("The queue's client is gone or busy.");
                    }
                    let pid = prompt.pid;
                    self.queue_runners
                        .push(Runner::new(prompt.queue, pid, prompt.approved));
                    self.script_info("Queue started.");
                    self.start_queue_entry(ctx, pid);
                }
            }
            Some(false) => self.queue_page.prompt = None,
            None => {}
        }
    }

    fn start_queue_entry(&mut self, ctx: &egui::Context, pid: u32) {
        let Some(runner) = self.queue_runners.iter_mut().find(|r| r.pid == pid) else {
            return;
        };
        runner.phase = Phase::Starting;
        let Some(entry) = runner.entry() else {
            return;
        };
        let ids = vec![entry.script_id.clone()];
        self.api_job(ctx, move |api| Event::QueueChecked {
            pid,
            entry: true,
            result: check(&api, &ids),
        });
    }

    fn start_checked_entry(&mut self, ctx: &egui::Context, pid: u32, check: &Check) {
        let Some(runner) = self
            .queue_runners
            .iter()
            .find(|r| r.pid == pid && r.phase == Phase::Starting)
        else {
            return;
        };
        let Some(entry) = runner.entry().cloned() else {
            return;
        };
        let approved = runner.approved.get(&entry.script_id).copied();
        let on_change = runner.queue.on_change;
        let resume = runner
            .resume
            .as_ref()
            .map(|(plan, at)| queue::resumed(plan, *at, Instant::now()));
        let ready = check
            .ready(&entry, &self.confirmed)
            .and_then(|ready| match approved {
                Some(price) if price == ready.price => Ok(ready),
                _ => Err(format!(
                    "{}'s price changed since the queue started.",
                    entry.title
                )),
            });
        let ready = match ready {
            Ok(ready) => ready,
            Err(change) => {
                return match on_change {
                    OnChange::Skip => {
                        self.script_info(format!("Skipping it: {change}"));
                        self.next_queue_entry(ctx, pid);
                    }
                    OnChange::Stop => self.end_queue(pid, format!("Queue stopped: {change}")),
                };
            }
        };
        let lib = match &check.lib {
            Ok(lib) => Lib {
                version: lib.version.clone(),
                warning: lib.warning.clone(),
            },
            Err(error) => return self.end_queue(pid, format!("Queue stopped: {error}")),
        };
        let quote = match check.quotes.get(&entry.script_id) {
            Some(Ok(quote)) => quote.clone(),
            _ => return self.end_queue(pid, "Queue stopped: no quote.".into()),
        };
        if let Some(why) = session::start_block(&ready.script, &quote, check.me.microcredits) {
            return self.end_queue(pid, format!("Queue stopped: {why}"));
        }
        let Some(target) = self
            .targets
            .iter()
            .find(|t| t.client.pid == pid && t.client.loaded.is_some())
            .cloned()
        else {
            return self.end_queue(pid, "Queue stopped: its client is gone.".into());
        };
        let spec = ready
            .manifest
            .settings
            .clone()
            .unwrap_or_else(|| serde_json::json!({ "fields": [] }));
        let form = crate::form::Form::new(spec, Some(&entry.settings));
        let layouts = self
            .layouts
            .get(&entry.title)
            .unwrap_or(&super::EMPTY_LAYOUTS);
        let values = match form.submitted(layouts, &self.known_items) {
            crate::form::Outcome::Submitted(values) => values,
            _ => Value::Object(Default::default()),
        };
        self.start_store_script(
            ctx,
            PendingStart {
                script: ready.script,
                target,
                replaces: None,
                lib,
                run: None,
                launch: Launch { values, resume },
            },
        );
    }

    pub(super) fn queue_entry_started(&mut self, pid: u32, run: Option<u64>) {
        let Some(runner) = self
            .queue_runners
            .iter_mut()
            .find(|r| r.pid == pid && r.phase == Phase::Starting)
        else {
            return;
        };
        match run {
            Some(id) => {
                runner.phase = Phase::Running {
                    run: id,
                    finish_sent: None,
                };
                let progress = runner.progress();
                if let Some(run) = self.run_by_id_mut(id) {
                    run.info(format!("Queue {progress}."));
                }
            }
            None => self.end_queue(pid, "Queue stopped: the script didn't start.".into()),
        }
    }

    fn next_queue_entry(&mut self, ctx: &egui::Context, pid: u32) {
        let Some(runner) = self.queue_runners.iter_mut().find(|r| r.pid == pid) else {
            return;
        };
        if !runner.finishing && runner.advance() {
            self.start_queue_entry(ctx, pid);
        } else {
            self.finish_queue(ctx, pid, "Queue finished.".into());
        }
    }

    fn finish_queue(&mut self, ctx: &egui::Context, pid: u32, message: String) {
        let Some(runner) = self.queue_runners.iter().find(|r| r.pid == pid) else {
            return;
        };
        if runner.queue.when_finished == WhenFinished::StayLoggedIn {
            return self.end_queue_info(pid, message);
        }
        match self.start_logout(ctx, pid) {
            Ok(run) => {
                self.script_info(format!("{message} Logging out."));
                if let Some(runner) = self.queue_runners.iter_mut().find(|r| r.pid == pid) {
                    runner.phase = Phase::LoggingOut { run };
                }
            }
            Err(error) => self.end_queue(pid, format!("{message} Couldn't log out: {error}")),
        }
    }

    fn start_logout(&mut self, ctx: &egui::Context, pid: u32) -> Result<u64, String> {
        let target = self
            .targets
            .iter()
            .find(|t| t.client.pid == pid && t.client.loaded.is_some())
            .cloned()
            .ok_or("its client is gone")?;
        let root = session::cache_root().ok_or("no cache folder")?;
        let script = Script::start(
            &self.script_host(ctx),
            LOGOUT_SOURCE,
            Some(&root.join("queue-logout.luau")),
            Some(&root.join("lib")),
            Some(&target),
            &self.settings,
            &Launch::default(),
        )?;
        let id = self.next_run_id();
        let mut run = Run::new(
            id,
            "Log out".into(),
            Some(script),
            Some(target),
            None,
            &self.settings,
        );
        run.info("The queue is logging the account out.");
        self.runs.push(run);
        Ok(id)
    }

    fn end_queue_info(&mut self, pid: u32, message: String) {
        self.queue_runners.retain(|r| r.pid != pid);
        self.script_info(message);
    }

    pub(super) fn end_queue(&mut self, pid: u32, message: String) {
        self.queue_runners.retain(|r| r.pid != pid);
        self.script_error(message);
    }

    pub(super) fn drive_queues(&mut self, ctx: &egui::Context) {
        if self.queue_runners.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let pids: Vec<u32> = self.queue_runners.iter().map(|r| r.pid).collect();
        for pid in pids {
            self.drive_queue(ctx, pid);
        }
    }

    fn drive_queue(&mut self, ctx: &egui::Context, pid: u32) {
        let Some(runner) = self.queue_runners.iter().find(|r| r.pid == pid) else {
            return;
        };
        match runner.phase.clone() {
            Phase::Starting => {}
            Phase::LoggingOut { run } => {
                if self.run_by_id(run).is_none_or(|r| !r.is_running()) {
                    self.end_queue_info(pid, "Queue finished.".into());
                }
            }
            Phase::Running { run, finish_sent } => {
                let Some(current) = self.run_by_id(run) else {
                    return self.end_queue(pid, "Queue stopped: its run was closed.".into());
                };
                if let Some((result, stopped)) = current.finished.clone() {
                    let session_open = current.store.as_ref().is_some_and(|store| {
                        !matches!(
                            store.closed.try_recv(),
                            Err(std::sync::mpsc::TryRecvError::Disconnected)
                        )
                    });
                    if session_open {
                        return;
                    }
                    let killed = runner.killed;
                    let title = current.title.clone();
                    let outcome = queue::outcome(&result, current.exit.as_ref(), killed, stopped);
                    let breaks = current.last_breaks.clone();
                    if let Some(runner) = self.queue_runners.iter_mut().find(|r| r.pid == pid) {
                        runner.resume = breaks.or(runner.resume.take());
                    }
                    return match outcome {
                        Outcome::Done => self.next_queue_entry(ctx, pid),
                        Outcome::Blocked => {
                            self.script_info(format!(
                                "{title} couldn't go on, so the queue moves on."
                            ));
                            self.next_queue_entry(ctx, pid);
                        }
                        Outcome::Failed => {
                            self.finish_queue(ctx, pid, format!("Queue stopped: {title} failed."))
                        }
                        Outcome::Stopped => {
                            self.end_queue(pid, format!("Queue stopped: {title} was stopped."))
                        }
                    };
                }
                let Some(entry) = runner.entry() else {
                    return;
                };
                match finish_sent {
                    None => {
                        let counter = entry
                            .goal
                            .as_ref()
                            .and_then(|g| current.counters.value(&g.counter));
                        if runner.finishing || entry.should_finish(current.run_time(), counter) {
                            let sent = current.send(Script::finish);
                            if let Some(run) = self.run_by_id_mut(run) {
                                run.report(sent, "The queue asked the script to wrap up.");
                            }
                            if let Some(runner) =
                                self.queue_runners.iter_mut().find(|r| r.pid == pid)
                            {
                                runner.phase = Phase::Running {
                                    run,
                                    finish_sent: Some(Instant::now()),
                                };
                            }
                        }
                    }
                    Some(_) => {
                        let grace = self.manifests.get(&entry.script_id).map_or(
                            injector::manifest::DEFAULT_FINISH_GRACE_MINUTES,
                            |(_, m)| m.finish_grace_minutes,
                        );
                        let late = runner.grace(grace).is_some_and(|left| left.is_zero());
                        if late && !runner.killed {
                            if let Some(runner) =
                                self.queue_runners.iter_mut().find(|r| r.pid == pid)
                            {
                                runner.killed = true;
                            }
                            if let Some(run) = self.run_by_id_mut(run) {
                                run.error(format!(
                                    "The script didn't wrap up within {grace} minutes, so it was stopped."
                                ));
                                run.stop();
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn queue_banner(&self, run: u64) -> Option<String> {
        self.queue_runners
            .iter()
            .find(|r| r.run_id() == Some(run))
            .map(Runner::progress)
    }
}

fn limits_text(entry: &Entry) -> String {
    let mut parts = Vec::new();
    if let Some(goal) = &entry.goal {
        parts.push(format!(
            "goal {} {}",
            crate::counters::number(goal.target),
            goal.counter
        ));
    }
    if let Some(min) = entry.min_minutes {
        parts.push(format!("at least {min} min"));
    }
    if let Some(max) = entry.max_minutes {
        parts.push(format!("at most {max} min"));
    }
    if parts.is_empty() {
        "until it ends".into()
    } else {
        parts.join(", ")
    }
}

fn minutes_ui(ui: &mut egui::Ui, label: &str, value: &mut Option<u32>, index: usize, salt: &str) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui.checkbox(&mut on, label).changed() {
            *value = on.then_some(60);
        }
        if let Some(minutes) = value.as_mut() {
            ui.push_id((salt, index), |ui| {
                ui.add(
                    egui::DragValue::new(minutes)
                        .range(1..=queue::MAX_MINUTES)
                        .suffix(" min"),
                );
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(queueable: bool, community: bool) -> StoreScript {
        serde_json::from_value(serde_json::json!({
            "id": "s1", "url": "s1", "title": "Bosses", "revision": 3,
            "can_access": true, "queueable": queueable, "community": community
        }))
        .unwrap()
    }

    fn check(script: StoreScript, manifest: Value, price: i64) -> Check {
        Check {
            me: serde_json::from_value(serde_json::json!({
                "username": "u", "microcredits": 1_000_000, "min_balance_minutes": 10, "api_version": 3
            }))
            .unwrap(),
            scripts: vec![script],
            manifests: HashMap::from([(
                "s1".to_owned(),
                Ok(ScriptManifest { revision: 4, manifest }),
            )]),
            quotes: HashMap::from([(
                "s1".to_owned(),
                Ok(crate::api::models::Quote { price, rank: 0, fee: 0, required: 0 }),
            )]),
            lib: Err("unused".into()),
        }
    }

    fn entry(manifest: &Value) -> Entry {
        let mut entry = Entry::new("s1".into(), "Bosses".into());
        entry.spec_hash = spec_hash(&Manifest::parse(manifest.clone()).unwrap());
        entry
    }

    #[test]
    fn a_queueable_script_with_unchanged_settings_is_ready_at_its_live_revision() {
        let manifest = serde_json::json!({ "queueable": true, "settings": { "fields": [{ "id": "a", "kind": "check" }] } });
        let ready = check(script(true, false), manifest.clone(), 500)
            .ready(&entry(&manifest), &Default::default())
            .unwrap();
        assert_eq!(ready.price, 500);
        assert_eq!(ready.script.revision, Some(4));
    }

    #[test]
    fn changes_since_the_entry_was_set_up_are_refused() {
        let manifest = serde_json::json!({ "queueable": true, "settings": { "fields": [{ "id": "a", "kind": "check" }] } });
        let old = entry(&serde_json::json!({ "queueable": true }));
        assert!(
            check(script(true, false), manifest.clone(), 0)
                .ready(&old, &Default::default())
                .unwrap_err()
                .contains("settings changed")
        );
        let not_queueable =
            serde_json::json!({ "settings": { "fields": [{ "id": "a", "kind": "check" }] } });
        assert!(
            check(script(true, false), not_queueable, 0)
                .ready(&entry(&manifest), &Default::default())
                .is_err()
        );
        assert!(
            check(script(false, false), manifest.clone(), 0)
                .ready(&entry(&manifest), &Default::default())
                .is_err()
        );
    }

    #[test]
    fn a_community_revision_runs_only_once_reviewed() {
        let manifest = serde_json::json!({ "queueable": true });
        let check = check(script(true, true), manifest.clone(), 0);
        let mut confirmed = crate::store::Confirmed::default();
        assert!(
            check
                .ready(&entry(&manifest), &confirmed)
                .unwrap_err()
                .contains("reviewed")
        );
        confirmed.confirm("s1", 4);
        assert!(check.ready(&entry(&manifest), &confirmed).is_ok());
    }

    #[test]
    fn a_script_gone_from_the_list_is_refused() {
        let manifest = serde_json::json!({ "queueable": true });
        let mut check = check(script(true, false), manifest.clone(), 0);
        check.scripts.clear();
        assert!(
            check
                .ready(&entry(&manifest), &Default::default())
                .unwrap_err()
                .contains("no longer available")
        );
    }
}
