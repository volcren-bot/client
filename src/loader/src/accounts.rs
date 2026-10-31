//! The Accounts tab: saved game accounts (a label, the hash of the account name, the bank pin), kept encrypted on disk.
//!
//! The account name is never stored, only `hash_name` of it, so a script can later recognise the logged in account by hashing its name.
//! The file is AES-256-GCM sealed with a random key that lives in the OS keyring, so the file alone is useless.

use std::path::PathBuf;

use eframe::egui;
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey},
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};

const KEYRING_SERVICE: &str = "volcren-loader";
const KEYRING_USER: &str = "accounts-key";
const FILE_NAME: &str = "accounts.bin";
/// Binds the ciphertext to this file format; bump with the format.
const AAD: &[u8] = b"volcren-accounts-v1";
const PIN_LEN: usize = 4;
/// Leading characters of the hash shown in the list; the full hash is on hover.
const HASH_SHOWN: usize = 12;
/// Space around the page's content.
const MARGIN: i8 = 16;
/// Width of the dialog's text boxes; the dialog fits around them.
const INPUT_WIDTH: f32 = 300.0;
/// Height of a list row.
const ROW_HEIGHT: f32 = 28.0;
/// Width of the list's last column, which holds the Save and Delete buttons.
const ACTIONS_WIDTH: f32 = 130.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    /// `hash_name` of the account name.
    pub hash: String,
    pub pin: String,
}

/// SHA-256 of the account name as the client normalizes it (trimmed, lowercase, underscores and hyphens as spaces), as lowercase hex.
/// So a name typed as "My Name" and the client's own `my_name` hash the same.
pub fn hash_name(account: &str) -> String {
    injector::bank_pin::hash_name(account)
}

pub fn valid_pin(pin: &str) -> bool {
    pin.len() == PIN_LEN && pin.bytes().all(|b| b.is_ascii_digit())
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

/// `nonce || ciphertext || tag`, with a fresh random nonce.
fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
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

fn open(key: &[u8; 32], sealed: &[u8]) -> Option<Vec<u8>> {
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
    directories::ProjectDirs::from("com", "volcren", "Volcren")
        .map(|dirs| dirs.data_dir().join(FILE_NAME))
}

/// The encryption key from the keyring; made and stored on first use.
fn key(create: bool) -> Result<Option<[u8; 32]>, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(text) => unhex(&text)
            .and_then(|bytes| bytes.try_into().ok())
            .map(Some)
            .ok_or_else(|| "the stored encryption key is malformed".to_owned()),
        Err(keyring::Error::NoEntry) if create => {
            let mut fresh = [0u8; 32];
            SystemRandom::new()
                .fill(&mut fresh)
                .map_err(|e| e.to_string())?;
            entry
                .set_password(&hex(&fresh))
                .map_err(|e| e.to_string())?;
            Ok(Some(fresh))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
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
    let key = key(false)?.ok_or("the encryption key is missing from the system keyring")?;
    decode(&key, &sealed)
}

fn store(entries: &[Entry]) -> Result<(), String> {
    let path = path().ok_or("no data folder on this system")?;
    let key = key(true)?.ok_or("no encryption key")?;
    let sealed = encode(&key, entries)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Write beside it and rename, so a crash never leaves half a file.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, sealed).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Name,
    Pin,
}

/// The list's data columns.
#[derive(Clone, Copy)]
enum Column {
    Name,
    Hash,
    Pin,
}

/// The cell being edited in place. `fresh` until its text box has been given focus once.
struct Editing {
    index: usize,
    field: Field,
    text: String,
    fresh: bool,
}

/// The add dialog, or (with `target`) the dialog that changes the account of one row.
#[derive(Default)]
struct Draft {
    /// The row whose account is being changed; `None` when adding.
    target: Option<usize>,
    name: String,
    account: String,
    pin: String,
    error: Option<String>,
}

/// What the page asks the app to do.
pub enum Action {
    /// Read the account name of the selected client; answer with `Accounts::client_name`.
    ReadClientName,
}

#[derive(Default)]
pub struct Accounts {
    /// What is on disk.
    entries: Vec<Entry>,
    /// `entries` with the edits not saved yet; a row is changed while it differs from its saved entry.
    working: Vec<Entry>,
    /// Set when the file couldn't be read: saving is blocked so the unreadable file isn't overwritten.
    locked: Option<String>,
    /// The last failure to save or an edit that was refused.
    error: Option<String>,
    draft: Option<Draft>,
    editing: Option<Editing>,
    /// The entry whose delete button is waiting for a second click.
    deleting: Option<usize>,
    /// A read of the client's account name is under way.
    reading: bool,
}

impl Accounts {
    pub fn load() -> Self {
        match load() {
            Ok(entries) => Self {
                working: entries.clone(),
                entries,
                ..Self::default()
            },
            Err(why) => Self {
                locked: Some(why),
                ..Self::default()
            },
        }
    }

    /// The saved PIN of the account with this name hash, for a script's `bank_pin_enter()`; `Err` while the file can't be read.
    /// Edits that aren't saved yet don't count.
    pub fn pin_for(&self, hash: &str) -> Result<Option<String>, String> {
        if let Some(why) = &self.locked {
            return Err(format!("the saved accounts can't be read: {why}"));
        }
        Ok(self
            .entries
            .iter()
            .find(|entry| entry.hash == hash)
            .map(|entry| entry.pin.clone()))
    }

    fn persist(&mut self) {
        self.error = store(&self.entries)
            .err()
            .map(|why| format!("Couldn't save the accounts: {why}"));
    }

    /// Saves the dialog: a new entry, or a new account for the row it targets (which stays unsaved until the row's Save).
    fn submit(&mut self, draft: &Draft) -> Result<(), String> {
        let (name, account) = (draft.name.trim(), draft.account.trim());
        if let Some(entry) = draft.target.and_then(|index| self.working.get_mut(index)) {
            if account.is_empty() {
                return Err("Enter the account name.".into());
            }
            entry.hash = hash_name(account);
            return Ok(());
        }
        if name.is_empty() {
            return Err("Give the entry a name.".into());
        }
        if account.is_empty() {
            return Err("Enter the account name.".into());
        }
        if !valid_pin(&draft.pin) {
            return Err("The bank pin is exactly 4 digits.".into());
        }
        let entry = Entry {
            name: name.to_owned(),
            hash: hash_name(account),
            pin: draft.pin.clone(),
        };
        self.entries.push(entry.clone());
        self.working.push(entry);
        self.persist();
        Ok(())
    }

    /// Applies an in place edit to the row's unsaved copy; `Err` is why it was refused and nothing changed.
    fn apply(&mut self, edit: &Editing) -> Result<(), String> {
        let text = edit.text.trim();
        let Some(entry) = self.working.get_mut(edit.index) else {
            return Ok(());
        };
        match edit.field {
            Field::Name if text.is_empty() => return Err("The name can't be empty.".into()),
            Field::Name => entry.name = text.to_owned(),
            Field::Pin if !valid_pin(text) => {
                return Err("The bank pin is exactly 4 digits.".into());
            }
            Field::Pin => entry.pin = text.to_owned(),
        }
        Ok(())
    }

    /// `client` is whether a client with the agent loaded is selected, which "Load account from client" needs.
    pub fn ui(&mut self, ui: &mut egui::Ui, client: bool) -> Option<Action> {
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: MARGIN,
                right: MARGIN,
                top: MARGIN,
                bottom: 0,
            })
            .show(ui, |ui| self.page_ui(ui));
        self.dialog_ui(ui.ctx(), client)
    }

    /// The answer to `Action::ReadClientName`: puts the name in the dialog.
    pub fn client_name(&mut self, result: Result<Option<String>, String>) {
        self.reading = false;
        let Some(draft) = &mut self.draft else { return };
        match result {
            // The client's form is `my_name`; show it as it looks in the game.
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
        // The dialog is modal in spirit: the page behind it is inert.
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
        let Self {
            entries,
            working,
            editing,
            deleting,
            ..
        } = self;
        // The first three columns share what the buttons leave; the last one fits the buttons.
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            // Cells inherit the page's top down layout otherwise, which leaves the buttons at the top of their row.
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::remainder().clip(true))
            .column(egui_extras::Column::remainder().clip(true))
            .column(egui_extras::Column::remainder().clip(true))
            .column(egui_extras::Column::exact(ACTIONS_WIDTH))
            .header(ROW_HEIGHT, |mut header| {
                for head in ["Name", "Account hash", "Bank pin", ""] {
                    header.col(|ui| {
                        ui.strong(head);
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_HEIGHT, working.len(), |mut row| {
                    let index = row.index();
                    let entry = &working[index];
                    for column in [Column::Name, Column::Hash, Column::Pin] {
                        row.col(|ui| {
                            let field = match column {
                                Column::Name => Some(Field::Name),
                                Column::Hash => None,
                                Column::Pin => Some(Field::Pin),
                            };
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
                                        edit_start = Some(Editing {
                                            index,
                                            field: Field::Name,
                                            text: entry.name.clone(),
                                            fresh: true,
                                        })
                                    }
                                    Column::Pin => {
                                        edit_start = Some(Editing {
                                            index,
                                            field: Field::Pin,
                                            text: entry.pin.clone(),
                                            fresh: true,
                                        })
                                    }
                                }
                            }
                        });
                    }
                    row.col(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let armed = *deleting == Some(index);
                            if ui.button(if armed { "Sure?" } else { "Delete" }).clicked() {
                                delete = Some(index);
                            }
                            let changed = entries.get(index) != Some(entry);
                            if ui.add_enabled(changed, egui::Button::new("Save")).clicked() {
                                save = Some(index);
                            }
                        });
                    });
                });
            });
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
        // Fit the content every frame: eframe remembers a window's size between runs, which would keep an old width.
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
            // As wide as the fields, so the dialog fits them. A bare `with_layout` would take all the height the window offers; give the row just one button's.
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

/// A list cell that can be clicked anywhere in its width, not just on its text.
fn click_cell(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().with_clip_rect(rect).text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
    response
}

/// The text box of the cell being edited, as wide as its column. `Some(Some(edit))` when it should be applied, `Some(None)` when cancelled.
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
    fn hash_ignores_case_and_surrounding_space() {
        assert_eq!(hash_name("  Some Name "), hash_name("some name"));
        assert_ne!(hash_name("some name"), hash_name("some  name"));
        assert_eq!(
            hash_name("P 4I N"),
            hash_name("p_4i_n"),
            "the client's own form of the name matches"
        );
        assert_eq!(hash_name("a-b\u{a0}c"), hash_name("a_b_c"));
        assert_eq!(
            hash_name("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn pin_is_exactly_four_digits() {
        assert!(valid_pin("0123"));
        for bad in ["", "123", "12345", "12a4", "١٢٣٤", " 123"] {
            assert!(!valid_pin(bad), "{bad:?}");
        }
    }

    #[test]
    fn entries_round_trip_and_the_wrong_key_fails() {
        let entries = vec![Entry {
            name: "main".into(),
            hash: hash_name("a"),
            pin: "1234".into(),
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
    fn hex_round_trips() {
        assert_eq!(unhex(&hex(&[0, 15, 255])), Some(vec![0, 15, 255]));
        assert_eq!(unhex("abc"), None);
        assert_eq!(unhex("zz"), None);
    }
}
