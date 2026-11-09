use std::path::{Path, PathBuf};

use eframe::egui;
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "volcren-loader";
const KEYRING_USER: &str = "accounts-key";
const FILE_NAME: &str = "accounts.bin";
const AAD: &[u8] = b"volcren-accounts-v1";
const PIN_LEN: usize = 4;
const HASH_SHOWN: usize = 12;
const MARGIN: i8 = 16;
const INPUT_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 28.0;
const MIN_TEXT_WIDTH: f32 = 90.0;
const ACCOUNT_REQUIRED: &str = "Enter the account name.";
const PIN_INVALID: &str = "The bank pin is exactly 4 digits.";
const LAMP_SKILLS: [&str; 24] = [
    "Agility",
    "Attack",
    "Construction",
    "Cooking",
    "Crafting",
    "Defence",
    "Farming",
    "Firemaking",
    "Fishing",
    "Fletching",
    "Herblore",
    "Hitpoints",
    "Hunter",
    "Magic",
    "Mining",
    "Prayer",
    "Ranged",
    "Runecraft",
    "Sailing",
    "Slayer",
    "Smithing",
    "Strength",
    "Thieving",
    "Woodcutting",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub hash: String,
    pub pin: String,
    #[serde(default)]
    pub lamp_skill: String,
}

pub fn hash_name(account: &str) -> String {
    injector::bank_pin::hash_name(account)
}

pub fn valid_pin(pin: &str) -> bool {
    injector::bank_pin::parse_pin(pin).is_some()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

pub(crate) fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let sealing = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).map_err(|e| e.to_string())?);
    let mut nonce = [0u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|e| e.to_string())?;
    let mut data = plain.to_vec();
    sealing
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(AAD),
            &mut data,
        )
        .map_err(|e| e.to_string())?;
    let mut out = nonce.to_vec();
    out.extend(data);
    Ok(out)
}

pub(crate) fn open(key: &[u8; 32], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < NONCE_LEN {
        return None;
    }
    let (nonce, rest) = sealed.split_at(NONCE_LEN);
    let opening = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).ok()?);
    let mut data = rest.to_vec();
    let plain = opening
        .open_in_place(
            Nonce::try_assume_unique_for_key(nonce).ok()?,
            Aad::from(AAD),
            &mut data,
        )
        .ok()?;
    Some(plain.to_vec())
}

fn encode(key: &[u8; 32], entries: &[Entry]) -> Result<Vec<u8>, String> {
    seal(
        key,
        &serde_json::to_vec(entries).map_err(|e| e.to_string())?,
    )
}

fn decode(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<Entry>, String> {
    let plain = open(key, sealed)
        .ok_or("the file can't be decrypted with this machine's key (changed or corrupt)")?;
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}

fn path() -> Option<PathBuf> {
    crate::paths::project_dirs().map(|dirs| dirs.data_dir().join(FILE_NAME))
}

fn keyring_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

fn parse_key(text: &str) -> Result<[u8; 32], String> {
    unhex(text)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| "the stored encryption key is malformed".to_owned())
}

pub(crate) fn existing_key() -> Result<Option<[u8; 32]>, String> {
    match keyring_entry()?.get_password() {
        Ok(text) => parse_key(&text).map(Some),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("the system keyring isn't available ({e})")),
    }
}

pub(crate) fn create_key() -> Result<[u8; 32], String> {
    let entry = keyring_entry()?;
    match entry.get_password() {
        Ok(text) => parse_key(&text),
        Err(keyring::Error::NoEntry) => {
            let mut fresh = [0u8; 32];
            SystemRandom::new()
                .fill(&mut fresh)
                .map_err(|e| e.to_string())?;
            entry
                .set_password(&hex(&fresh))
                .map_err(|e| e.to_string())?;
            Ok(fresh)
        }
        Err(e) => Err(format!("the system keyring isn't available ({e})")),
    }
}

fn load() -> Result<Vec<Entry>, String> {
    let path = path().ok_or("no data folder on this system")?;
    let sealed = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    let key = existing_key()?.ok_or("the encryption key is missing from the system keyring")?;
    decode(&key, &sealed)
}

fn store(entries: &[Entry]) -> Result<(), String> {
    let path = path().ok_or("no data folder on this system")?;
    let key = create_key()?;
    write_sealed(&path, &encode(&key, entries)?)
}

pub(crate) fn write_sealed(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Pin,
}

#[derive(Clone, Copy)]
enum Column {
    Name,
    Hash,
    Pin,
}

struct Editing {
    index: usize,
    field: Field,
    text: String,
    fresh: bool,
}

impl Editing {
    fn start(index: usize, field: Field, text: String) -> Self {
        Self {
            index,
            field,
            text,
            fresh: true,
        }
    }
}

impl Column {
    fn field(self) -> Option<Field> {
        match self {
            Column::Name => Some(Field::Name),
            Column::Hash => None,
            Column::Pin => Some(Field::Pin),
        }
    }
}

#[derive(Default)]
struct Draft {
    target: Option<usize>,
    name: String,
    account: String,
    pin: String,
    error: Option<String>,
}

pub enum Action {
    ReadClientName,
}

#[derive(Default)]
pub struct Accounts {
    entries: Vec<Entry>,
    working: Vec<Entry>,
    locked: Option<String>,
    error: Option<String>,
    draft: Option<Draft>,
    editing: Option<Editing>,
    deleting: Option<usize>,
    reading: bool,
}

impl Accounts {
    pub fn load() -> Self {
        match load() {
            Ok(entries) => Self::with_entries(entries),
            Err(why) => Self {
                locked: Some(why),
                ..Self::default()
            },
        }
    }

    pub fn with_entries(entries: Vec<Entry>) -> Self {
        Self {
            working: entries.clone(),
            entries,
            ..Self::default()
        }
    }

    fn saved(&self) -> Result<&[Entry], String> {
        match &self.locked {
            Some(why) => Err(format!("the saved accounts can't be read: {why}")),
            None => Ok(&self.entries),
        }
    }

    pub fn pin_for(&self, hash: &str) -> Result<Option<String>, String> {
        Ok(self
            .saved()?
            .iter()
            .find(|entry| entry.hash == hash)
            .map(|entry| entry.pin.clone()))
    }

    pub fn lamp_skill_for(&self, hash: &str) -> Result<Option<String>, String> {
        Ok(self
            .saved()?
            .iter()
            .find(|entry| entry.hash == hash && !entry.lamp_skill.is_empty())
            .map(|entry| entry.lamp_skill.clone()))
    }

    fn persist(&mut self) {
        self.error = store(&self.entries)
            .err()
            .map(|why| format!("Couldn't save the accounts: {why}"));
    }

    fn submit(&mut self, draft: &Draft) -> Result<(), String> {
        let (name, account) = (draft.name.trim(), draft.account.trim());
        if let Some(entry) = draft.target.and_then(|index| self.working.get_mut(index)) {
            if account.is_empty() {
                return Err(ACCOUNT_REQUIRED.into());
            }
            entry.hash = hash_name(account);
            return Ok(());
        }
        if name.is_empty() {
            return Err("Give the entry a name.".into());
        }
        if account.is_empty() {
            return Err(ACCOUNT_REQUIRED.into());
        }
        if !valid_pin(&draft.pin) {
            return Err(PIN_INVALID.into());
        }
        let entry = Entry {
            name: name.to_owned(),
            hash: hash_name(account),
            pin: draft.pin.clone(),
            lamp_skill: String::new(),
        };
        self.entries.push(entry.clone());
        self.working.push(entry);
        self.persist();
        Ok(())
    }

    fn apply(&mut self, edit: &Editing) -> Result<(), String> {
        let text = edit.text.trim();
        let Some(entry) = self.working.get_mut(edit.index) else {
            return Ok(());
        };
        match edit.field {
            Field::Name if text.is_empty() => return Err("The name can't be empty.".into()),
            Field::Name => entry.name = text.to_owned(),
            Field::Pin if !valid_pin(text) => {
                return Err(PIN_INVALID.into());
            }
            Field::Pin => entry.pin = text.to_owned(),
        }
        Ok(())
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, client: bool) -> Option<Action> {
        self.page_ui(ui);
        self.dialog_ui(ui.ctx(), client)
    }

    pub fn client_name(&mut self, result: Result<Option<String>, String>) {
        self.reading = false;
        let Some(draft) = &mut self.draft else { return };
        match result {
            Ok(Some(name)) => {
                draft.account = name.replace('_', " ");
                draft.error = None;
            }
            Ok(None) => draft.error = Some("Log in to the game first.".into()),
            Err(why) => {
                draft.error = Some(format!("Couldn't read the name from the client: {why}"))
            }
        }
    }

    fn page_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(why) = &self.locked {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Saved accounts can't be opened: {why}."),
            );
            ui.label("Nothing is shown or saved so the file isn't overwritten. Fix the keyring, or delete accounts.bin in the Volcren data folder to start over.");
            return;
        }
        ui.add_enabled_ui(self.draft.is_none(), |ui| {
            if ui.button("Add account").clicked() {
                self.draft = Some(Draft::default());
            }
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Encrypted on this machine. Account names are not stored.")
                    .weak(),
            );
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(f32::from(MARGIN));
            if self.working.is_empty() {
                ui.label(egui::RichText::new("No accounts saved yet.").weak());
            } else {
                self.list_ui(ui);
            }
        });
    }

    fn list_ui(&mut self, ui: &mut egui::Ui) {
        let mut delete = None;
        let mut save = None;
        let mut edit_start = None;
        let mut finished = None;
        let mut change_account = None;
        let mut lamp_skill = None;
        let Self {
            entries,
            working,
            editing,
            deleting,
            ..
        } = self;
        let font = egui::TextStyle::Button.resolve(ui.style());
        let button_width = ["Save", "Delete", "Sure?"]
            .iter()
            .map(|label| {
                ui.painter()
                    .layout_no_wrap((*label).into(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
            .fold(0.0, f32::max)
            + 2.0 * ui.spacing().button_padding.x;
        let button_size = egui::vec2(button_width, 0.0);
        let spacing = ui.spacing().item_spacing;
        let row_actions = 2.0 * button_width + spacing.x;
        let stacked = ui.available_width() < 4.0 * MIN_TEXT_WIDTH + 5.0 * spacing.x + row_actions;
        let actions_width = if stacked { button_width } else { row_actions };
        let row_height = if stacked {
            ROW_HEIGHT.max(2.0 * ui.spacing().interact_size.y + spacing.y)
        } else {
            ROW_HEIGHT
        };
        let text_column = || {
            egui_extras::Column::remainder()
                .at_least(MIN_TEXT_WIDTH)
                .clip(true)
        };
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(text_column())
            .column(text_column())
            .column(text_column())
            .column(text_column())
            .column(egui_extras::Column::exact(actions_width))
            .header(ROW_HEIGHT, |mut header| {
                for head in ["Name", "Account hash", "Bank pin", "Lamp skill", ""] {
                    header.col(|ui| {
                        ui.strong(head);
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, working.len(), |mut row| {
                    let index = row.index();
                    let entry = &working[index];
                    for column in [Column::Name, Column::Hash, Column::Pin] {
                        row.col(|ui| {
                            let field = column.field();
                            if let Some(edit) = editing
                                .as_mut()
                                .filter(|e| e.index == index && Some(e.field) == field)
                            {
                                if let Some(done) = edit_cell(ui, edit) {
                                    finished = Some(done);
                                }
                                return;
                            }
                            let (shown, hover) = match column {
                                Column::Name => (entry.name.clone(), "Click to rename".to_owned()),
                                Column::Hash => (
                                    format!("{}…", &entry.hash[..HASH_SHOWN.min(entry.hash.len())]),
                                    format!("{}\nClick to change the account", entry.hash),
                                ),
                                Column::Pin => (
                                    "*".repeat(entry.pin.len()),
                                    "Click to show and change the pin".to_owned(),
                                ),
                            };
                            if click_cell(ui, &shown).on_hover_text(hover).clicked() {
                                match column {
                                    Column::Hash => change_account = Some(index),
                                    Column::Name => {
                                        edit_start = Some(Editing::start(
                                            index,
                                            Field::Name,
                                            entry.name.clone(),
                                        ));
                                    }
                                    Column::Pin => {
                                        edit_start = Some(Editing::start(
                                            index,
                                            Field::Pin,
                                            entry.pin.clone(),
                                        ));
                                    }
                                }
                            }
                        });
                    }
                    row.col(|ui| {
                        let mut chosen = entry.lamp_skill.clone();
                        let shown = if chosen.is_empty() {
                            "None"
                        } else {
                            &entry.lamp_skill
                        };
                        egui::ComboBox::from_id_salt(("lamp_skill", index))
                            .selected_text(shown)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut chosen, String::new(), "None");
                                for skill in LAMP_SKILLS {
                                    ui.selectable_value(&mut chosen, skill.to_owned(), skill);
                                }
                            })
                            .response
                            .on_hover_text(
                                "What a random event's lamp is spent on. None keeps the lamp.",
                            );
                        if chosen != entry.lamp_skill {
                            lamp_skill = Some((index, chosen));
                        }
                    });
                    row.col(|ui| {
                        let armed = *deleting == Some(index);
                        let changed = entries.get(index) != Some(entry);
                        let buttons = |ui: &mut egui::Ui| {
                            let delete_button =
                                egui::Button::new(if armed { "Sure?" } else { "Delete" })
                                    .min_size(button_size);
                            let save_button = egui::Button::new("Save").min_size(button_size);
                            if stacked {
                                if ui.add_enabled(changed, save_button).clicked() {
                                    save = Some(index);
                                }
                                if ui.add(delete_button).clicked() {
                                    delete = Some(index);
                                }
                            } else {
                                if ui.add(delete_button).clicked() {
                                    delete = Some(index);
                                }
                                if ui.add_enabled(changed, save_button).clicked() {
                                    save = Some(index);
                                }
                            }
                        };
                        if stacked {
                            ui.vertical(buttons);
                        } else {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                buttons,
                            );
                        }
                    });
                });
            });
        if let Some((index, skill)) = lamp_skill
            && let Some(entry) = self.working.get_mut(index)
        {
            entry.lamp_skill = skill;
        }
        match finished {
            Some(Some(edit)) => {
                self.editing = None;
                self.error = self.apply(&edit).err();
            }
            Some(None) => self.editing = None,
            None => {}
        }
        if let Some(edit) = edit_start {
            self.editing = Some(edit);
            self.deleting = None;
        }
        if let Some(index) = change_account {
            self.draft = Some(Draft {
                target: Some(index),
                ..Draft::default()
            });
            self.editing = None;
            self.deleting = None;
        }
        if let Some(index) = save {
            self.entries[index] = self.working[index].clone();
            self.persist();
        }
        if let Some(index) = delete {
            if self.deleting == Some(index) {
                self.entries.remove(index);
                self.working.remove(index);
                self.deleting = None;
                self.editing = None;
                self.persist();
            } else {
                self.deleting = Some(index);
            }
        }
    }

    fn dialog_ui(&mut self, ctx: &egui::Context, client: bool) -> Option<Action> {
        let mut draft = self.draft.take()?;
        let mut decision = None;
        let mut action = None;
        let can_read = client && !self.reading;
        let changing = draft.target.is_some();
        egui::Window::new(if changing {
            "Change account"
        } else {
            "Add account"
        })
        .id(egui::Id::new(("accounts-dialog", changing)))
        .collapsible(false)
        .auto_sized()
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let fields = egui::Grid::new("add-account")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    if !changing {
                        ui.label("Name");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.name)
                                .hint_text("e.g. Ironman (it's just for you)")
                                .desired_width(INPUT_WIDTH),
                        );
                        ui.end_row();
                    }
                    ui.label("Account");
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.account)
                            .hint_text("In-game name (not stored)")
                            .desired_width(INPUT_WIDTH),
                    );
                    ui.end_row();
                    if !changing {
                        ui.label("Bank pin");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.pin)
                                .password(true)
                                .hint_text("4 digits")
                                .char_limit(PIN_LEN)
                                .desired_width(INPUT_WIDTH),
                        );
                        ui.end_row();
                    }
                })
                .response
                .rect
                .width();
            draft.pin.retain(|c| c.is_ascii_digit());
            if let Some(error) = &draft.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(6.0);
            let row = egui::vec2(fields, ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(
                row,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let load =
                        ui.add_enabled(can_read, egui::Button::new("Load account from client"));
                    let load = if can_read {
                        load.on_hover_text(
                            "Fill in the name of the account logged in on the selected client",
                        )
                    } else {
                        load.on_disabled_hover_text(
                            "Select a client with the agent loaded, and log in",
                        )
                    };
                    if load.clicked() {
                        action = Some(Action::ReadClientName);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Save").clicked() {
                            decision = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(false);
                        }
                    });
                },
            );
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            decision = Some(false);
        }
        match decision {
            Some(true) => {
                if let Err(why) = self.submit(&draft) {
                    draft.error = Some(why);
                    self.draft = Some(draft);
                }
            }
            Some(false) => {}
            None => self.draft = Some(draft),
        }
        if action.is_some() {
            self.reading = true;
        }
        action
    }
}

fn click_cell(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height().max(ROW_HEIGHT)),
        egui::Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, rect.width());
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.painter().layout_job(job);
    let top_left = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(top_left, galley, color);
    response
}

fn edit_cell(ui: &mut egui::Ui, edit: &mut Editing) -> Option<Option<Editing>> {
    let hint = match edit.field {
        Field::Name => "name",
        Field::Pin => "4 digits",
    };
    let mut input = egui::TextEdit::singleline(&mut edit.text)
        .hint_text(hint)
        .desired_width(ui.available_width());
    if edit.field == Field::Pin {
        input = input.char_limit(PIN_LEN);
    }
    let response = ui.add(input);
    if edit.field == Field::Pin {
        edit.text.retain(|c| c.is_ascii_digit());
    }
    if edit.fresh {
        response.request_focus();
        edit.fresh = false;
        return None;
    }
    if !response.lost_focus() {
        return None;
    }
    let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
    Some((!cancelled).then(|| Editing {
        index: edit.index,
        field: edit.field,
        text: edit.text.clone(),
        fresh: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_round_trip_and_the_wrong_key_fails() {
        let entries = vec![Entry {
            name: "main".into(),
            hash: hash_name("a"),
            pin: "1234".into(),
            lamp_skill: "Slayer".into(),
        }];
        let sealed = encode(&[7; 32], &entries).unwrap();
        assert!(
            !sealed.windows(4).any(|w| w == b"1234"),
            "the pin must not appear in the file"
        );
        assert_eq!(decode(&[7; 32], &sealed).unwrap(), entries);
        assert!(decode(&[8; 32], &sealed).is_err());
        let mut tampered = sealed.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(decode(&[7; 32], &tampered).is_err());
        assert!(decode(&[7; 32], &[1, 2, 3]).is_err());
    }

    #[test]
    fn a_file_from_before_lamp_skills_still_loads() {
        let old = br#"[{"name":"main","hash":"abc","pin":"1234"}]"#;
        let sealed = seal(&[7; 32], old).unwrap();
        let entries = decode(&[7; 32], &sealed).unwrap();
        assert_eq!(entries[0].lamp_skill, "");
    }

    #[test]
    fn the_lamp_skill_comes_from_what_is_saved() {
        let entry = |hash: &str, skill: &str| Entry {
            name: "main".into(),
            hash: hash.into(),
            pin: "1234".into(),
            lamp_skill: skill.into(),
        };
        let accounts = Accounts {
            entries: vec![entry("set", "Slayer"), entry("unset", "")],
            working: vec![entry("set", "Mining")],
            ..Accounts::default()
        };
        assert_eq!(accounts.lamp_skill_for("set"), Ok(Some("Slayer".into())));
        assert_eq!(accounts.lamp_skill_for("unset"), Ok(None));
        assert_eq!(accounts.lamp_skill_for("nobody"), Ok(None));
    }

    #[test]
    fn hex_round_trips() {
        assert_eq!(unhex(&hex(&[0, 15, 255])), Some(vec![0, 15, 255]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }
}
