//! The whole run-a-store-script flow (lib install, start, download, heartbeat, end) against a fake server.

use std::{fs, io::{Cursor, Write}, path::PathBuf};

use serde_json::json;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    api::{Api, ApiError, Config, Stats, StoreScript},
    fake_api::{FakeServer, Reply, Request},
    session::{prepare_in, prepare_warned, revision_dir, sha256_hex},
};

const SCRIPT_ID: &str = "7d081fdd-59de-4a4e-9c29-b2f92d9bc697";
const COMMUNITY_ID: &str = "0f3c1b52-8f0e-4c3a-9a58-1d6a4f7b2e90";
const LIB_VERSION: &str = "2026.10.26-abc1234";
const MAIN: &str = "print('main')";
const UTIL: &str = "return 1";

/// What the fake server does, so each test can break one thing.
#[derive(Clone, Copy)]
struct Server {
    revision: i64,
    start_status: u16,
    /// The status of the script files.
    file_status: u16,
    /// Serve a wrong checksum for the files.
    wrong_checksum: bool,
    entry: &'static str,
    /// 404 for the lib index, as before the first lib release.
    no_lib: bool,
    /// Credits per minute of the started session; a premium script's files stay in memory.
    price: i64,
    /// Serve the files encrypted, and the key with the session.
    encrypted: bool,
}

const KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

/// What the webapp stores: nonce || ciphertext || tag, bound to the file's place.
fn seal(revision: i64, name: &str, plain: &str) -> Vec<u8> {
    use ring::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
    let key: Vec<u8> = (0..32).collect();
    let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).unwrap());
    let nonce = [7u8; 12];
    let mut body = plain.as_bytes().to_vec();
    let aad = format!("volcren:{SCRIPT_ID}/{revision:09}/{name}");
    key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad.as_bytes()), &mut body).unwrap();
    [nonce.to_vec(), body].concat()
}

impl Server {
    const fn healthy() -> Self {
        Self { revision: 4, start_status: 200, file_status: 200, wrong_checksum: false, entry: "main.luau", no_lib: false, price: 30, encrypted: false }
    }

    fn start(self) -> FakeServer {
        let lib = archive(&[("host.luau", "return {}"), ("main/game_state.luau", "return 1")]);
        FakeServer::start(move |base, request| self.answer(base, request, &lib))
    }

    fn answer(&self, base: &str, request: &Request, lib: &[u8]) -> Reply {
        let path = request.path.split('?').next().unwrap_or("");
        match (request.method.as_str(), path) {
            ("GET", "/storage/v1/object/public/client/lib/index.json") if self.no_lib => Reply::status(404),
            ("GET", "/storage/v1/object/public/client/lib/index.json") => Reply::json(
                200,
                json!({ "versions": [{
                    "version": LIB_VERSION,
                    "requires_host": injector::HOST_API,
                    "path": format!("lib/{LIB_VERSION}/lib.zip"),
                    "sha256": sha256_hex(lib),
                }] }),
            ),
            ("GET", p) if p.ends_with("/lib.zip") => Reply::bytes(lib),
            ("GET", "/launcher/scripts") => {
                let only_community = request.path.contains("community=true");
                let scripts = [
                    json!({ "id": SCRIPT_ID, "url": "tempoross", "title": "Tempoross", "price": 30, "revision": 4, "can_access": true }),
                    json!({ "id": COMMUNITY_ID, "url": "mine", "title": "Mine", "price": 0, "revision": 4, "can_access": true, "community": true, "granted": true }),
                ];
                let scripts: Vec<_> = scripts.into_iter().filter(|s| !only_community || s["community"] == true).collect();
                Reply::json(200, json!({ "scripts": scripts }))
            }
            ("POST", "/launcher/scripts/redeem") => match request.json()["code"].as_str().and_then(|code| code.chars().next()) {
                Some('a') => Reply::json(200, json!({ "script_id": COMMUNITY_ID, "url": "mine" })),
                Some('b') => Reply::json(409, json!({ "error": "This share code has already been used.", "reason": "used" })),
                Some('c') => Reply::json(410, json!({ "error": "This share code has expired.", "reason": "expired" })),
                Some('d') => Reply::json(429, json!({ "error": "Too many failed attempts. Try again later.", "reason": "rate_limited" })),
                _ => Reply::json(404, json!({ "error": "This share code is not valid.", "reason": "invalid" })),
            },
            ("DELETE", p) if p == format!("/launcher/scripts/{COMMUNITY_ID}/grant") => Reply::json(200, json!({ "removed": true })),
            ("DELETE", p) if p.starts_with("/launcher/scripts/") => Reply::json(404, json!({ "error": "This script isn't granted to your account." })),
            ("POST", "/launcher/sessions") if self.start_status == 402 => {
                Reply::json(402, json!({ "error": "Not enough credits.", "required": 300 }))
            }
            ("POST", "/launcher/sessions") => {
                // the checksum is of the file as stored, so of the ciphertext when encrypted
                let checksum = |name: &str, text: &str| {
                    if self.wrong_checksum {
                        "0".repeat(64)
                    } else if self.encrypted {
                        sha256_hex(&seal(self.revision, name, text))
                    } else {
                        sha256_hex(text.as_bytes())
                    }
                };
                Reply::json(
                    200,
                    json!({
                        "session_id": "s1",
                        "revision": self.revision,
                        "price": self.price,
                        "key": self.encrypted.then_some(KEY_HEX),
                        "entry": self.entry,
                        "heartbeat_seconds": 60,
                        "files": [
                            { "name": "main.luau", "url": format!("{base}/files/main.luau"), "sha256": checksum("main.luau", MAIN) },
                            { "name": "util.luau", "url": format!("{base}/files/util.luau"), "sha256": checksum("util.luau", UTIL) },
                        ],
                    }),
                )
            }
            ("GET", "/files/main.luau") if self.file_status == 200 && self.encrypted => Reply::bytes(seal(self.revision, "main.luau", MAIN)),
            ("GET", "/files/util.luau") if self.file_status == 200 && self.encrypted => Reply::bytes(seal(self.revision, "util.luau", UTIL)),
            ("GET", "/files/main.luau") if self.file_status == 200 => Reply::bytes(MAIN),
            ("GET", "/files/util.luau") if self.file_status == 200 => Reply::bytes(UTIL),
            ("GET", p) if p.starts_with("/files/") => Reply::status(self.file_status),
            ("POST", "/launcher/sessions/s1/heartbeat") => match request.json()["stats"]["experience"].as_i64() {
                Some(9999) => Reply::json(200, json!({ "alive": false, "reason": "insufficient_credits", "credits": 0 })),
                _ => Reply::json(200, json!({ "alive": true, "credits": 120 })),
            },
            ("POST", "/launcher/sessions/s1/end") => Reply::json(200, json!({ "charged": 60 })),
            _ => Reply::status(404),
        }
    }
}

fn archive(files: &[(&str, &str)]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in files {
        writer.start_file(*name, SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)).unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn cache(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("volcren-flow-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn api(server: &FakeServer) -> Api {
    let config = Config { api_url: server.url.clone(), supabase_url: server.url.clone(), anon_key: "anon".into(), site_url: String::new() };
    Api::signed_in(config, "token-1")
}

fn script() -> StoreScript {
    serde_json::from_value(json!({
        "id": SCRIPT_ID, "url": "tempoross", "title": "Tempoross", "price": 30, "revision": 4, "can_access": true
    }))
    .unwrap()
}

fn free_script() -> StoreScript {
    serde_json::from_value(json!({
        "id": SCRIPT_ID, "url": "tempoross", "title": "Tempoross", "price": 0, "revision": 4, "can_access": true
    }))
    .unwrap()
}

#[test]
fn a_store_script_runs_from_start_to_end() {
    let fake = Server::healthy().start();
    let api = api(&fake);
    let root = cache("full");

    let prepared = prepare_in(&api, &script(), &root).expect("prepare");
    assert_eq!(prepared.source, MAIN);
    assert_eq!(prepared.warning, None);
    assert_eq!(prepared.session.session_id, "s1");
    let dir = revision_dir(&root, SCRIPT_ID, 4);
    assert_eq!(prepared.entry, dir.join("main.luau"));
    // A premium script is never written to disk: its other files are handed over in memory.
    assert!(prepared.premium);
    assert_eq!(prepared.modules, vec![("util.luau".to_owned(), UTIL.to_owned())]);
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "the revision folder is empty");
    // The lib sits beside the revision folders, which is where `require("../lib/...")` looks.
    assert_eq!(fs::read_to_string(root.join("lib/main/game_state.luau")).unwrap(), "return 1");
    assert_eq!(fs::read_to_string(root.join("lib/.version")).unwrap(), LIB_VERSION);

    let order: Vec<String> = fake.requests().iter().map(|r| format!("{} {}", r.method, r.path.split('?').next().unwrap())).collect();
    let position = |needle: &str| order.iter().position(|line| line.contains(needle)).unwrap_or_else(|| panic!("{needle} not requested: {order:?}"));
    assert!(position("lib/index.json") < position("POST /launcher/sessions"), "the lib is fetched before the session starts, so it is never billed");
    assert!(position("POST /launcher/sessions") < position("/files/main.luau"));

    let start = fake.requests().into_iter().find(|r| r.path == "/launcher/sessions").unwrap();
    assert_eq!(start.header("authorization"), Some("Bearer token-1"));
    assert_eq!(start.header("origin"), Some(fake.url.as_str()));
    assert_eq!(start.json(), json!({ "script_id": SCRIPT_ID }));
    assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 0, "a good start is not ended");

    let stats = Stats { experience: 500, gold: 20, minutes: 1.0, custom: vec![3] };
    let beat = api.heartbeat("s1", Some(&stats)).unwrap();
    assert!(beat.alive);
    assert_eq!(beat.credits, Some(120));
    let sent = fake.requests().into_iter().rfind(|r| r.path.ends_with("/heartbeat")).unwrap();
    assert_eq!(sent.json(), json!({ "stats": { "experience": 500, "gold": 20, "minutes": 1.0, "custom": [3] } }));

    let beat = api.heartbeat("s1", Some(&Stats { experience: 9999, ..Stats::default() })).unwrap();
    assert!(!beat.alive);
    assert_eq!(beat.reason.as_deref(), Some("insufficient_credits"));

    assert_eq!(api.end_session("s1").unwrap().charged, 60);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn encrypted_files_are_opened_with_the_session_key() {
    let root = cache("encrypted");
    // premium: decrypted in memory only
    let fake = Server { encrypted: true, ..Server::healthy() }.start();
    let prepared = prepare_in(&api(&fake), &script(), &root).expect("prepare");
    assert_eq!(prepared.source, MAIN);
    assert_eq!(prepared.modules, vec![("util.luau".to_owned(), UTIL.to_owned())]);
    assert_eq!(fs::read_dir(revision_dir(&root, SCRIPT_ID, 4)).unwrap().count(), 0);
    // free: cached decrypted, so the next start needs no key to read it
    let fake = Server { encrypted: true, price: 0, revision: 6, ..Server::healthy() }.start();
    let prepared = prepare_in(&api(&fake), &free_script(), &root).expect("prepare");
    assert_eq!(prepared.source, MAIN);
    assert_eq!(fs::read_to_string(revision_dir(&root, SCRIPT_ID, 6).join("util.luau")).unwrap(), UTIL);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_session_priced_zero_for_a_premium_script_is_still_premium() {
    // authors and testers run for free, but the script is still premium: nothing of it is written to disk
    let root = cache("premium-free-session");
    let fake = Server { price: 0, ..Server::healthy() }.start();
    let premium_listing: StoreScript = serde_json::from_value(json!({
        "id": SCRIPT_ID, "url": "tempoross", "title": "Tempoross", "price": 30, "revision": 4, "can_access": true
    }))
    .unwrap();
    let prepared = prepare_in(&api(&fake), &premium_listing, &root).expect("prepare");
    assert!(prepared.premium);
    assert_eq!(fs::read_dir(revision_dir(&root, SCRIPT_ID, 4)).unwrap().count(), 0);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_free_script_is_cached_and_a_premium_one_is_not() {
    let root = cache("free");
    let free = Server { price: 0, ..Server::healthy() }.start();
    let prepared = prepare_in(&api(&free), &free_script(), &root).unwrap();
    assert!(!prepared.premium);
    assert!(prepared.modules.is_empty());
    let dir = revision_dir(&root, SCRIPT_ID, 4);
    assert_eq!(fs::read_to_string(dir.join("util.luau")).unwrap(), UTIL);
    assert_eq!(fs::read_to_string(dir.join("main.luau")).unwrap(), MAIN);
    // Once it costs credits, the cached copy is removed.
    prepare_in(&api(&Server::healthy().start()), &script(), &root).unwrap();
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_second_start_reuses_the_download_and_the_lib() {
    let fake = Server { price: 0, ..Server::healthy() }.start();
    let api = api(&fake);
    let root = cache("reuse");
    prepare_in(&api, &free_script(), &root).unwrap();
    prepare_in(&api, &free_script(), &root).unwrap();
    assert_eq!(fake.count("GET", "/files/main.luau"), 1);
    assert_eq!(fake.count("GET", "/storage/v1/object/public/client/lib/index.json"), 2, "the index is checked every time");
    assert_eq!(fake.count("GET", &format!("/storage/v1/object/public/client/lib/{LIB_VERSION}/lib.zip")), 1);
    assert_eq!(fake.count("POST", "/launcher/sessions"), 2);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_new_revision_replaces_the_old_one_in_the_cache() {
    let root = cache("revisions");
    let old = Server { revision: 4, price: 0, ..Server::healthy() }.start();
    prepare_in(&api(&old), &free_script(), &root).unwrap();
    let new = Server { revision: 5, price: 0, ..Server::healthy() }.start();
    prepare_in(&api(&new), &free_script(), &root).unwrap();
    assert!(!revision_dir(&root, SCRIPT_ID, 4).exists());
    assert!(revision_dir(&root, SCRIPT_ID, 5).join("main.luau").is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_failed_download_ends_the_session_so_nothing_is_billed() {
    let fake = Server { file_status: 500, ..Server::healthy() }.start();
    let root = cache("download-fails");
    let error = prepare_in(&api(&fake), &script(), &root).err().expect("fails");
    assert!(error.contains("Couldn't download"), "{error}");
    assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_entry_file_comes_from_the_server_and_needs_no_particular_name() {
    let fake = Server { entry: "util.luau", ..Server::healthy() }.start();
    let root = cache("entry");

    let prepared = prepare_in(&api(&fake), &script(), &root).expect("prepare");
    assert_eq!(prepared.source, UTIL);
    assert_eq!(prepared.entry, revision_dir(&root, SCRIPT_ID, 4).join("util.luau"));
}

#[test]
fn an_entry_that_is_not_one_of_the_files_is_rejected_and_the_session_ended() {
    let fake = Server { entry: "gone.luau", ..Server::healthy() }.start();

    let error = prepare_in(&api(&fake), &script(), &cache("bad_entry")).err().expect("must fail");
    assert!(error.contains("gone.luau"), "{error}");
    assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 1);
}

#[test]
fn a_file_with_the_wrong_checksum_is_rejected_and_the_session_ended() {
    let fake = Server { wrong_checksum: true, ..Server::healthy() }.start();
    let root = cache("checksum");
    let error = prepare_in(&api(&fake), &script(), &root).err().expect("fails");
    assert!(error.contains("checksum"), "{error}");
    assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 1);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn too_few_credits_downloads_nothing_and_ends_nothing() {
    let fake = Server { start_status: 402, ..Server::healthy() }.start();
    let root = cache("credits");
    let error = prepare_in(&api(&fake), &script(), &root).err().expect("fails");
    assert!(error.contains("300"), "{error}");
    assert_eq!(fake.count("GET", "/files/"), 0);
    assert_eq!(fake.count("POST", "/launcher/sessions/"), 0, "there is no session to end");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn without_any_lib_the_start_fails_before_a_session_exists() {
    let fake = Server { no_lib: true, ..Server::healthy() }.start();
    let root = cache("no-lib");
    let error = prepare_in(&api(&fake), &script(), &root).err().expect("fails");
    assert!(error.contains("script library"), "{error}");
    assert_eq!(fake.count("POST", "/launcher/sessions"), 0, "nothing is started, so nothing can be billed");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_old_cached_lib_is_used_when_the_index_cannot_be_read() {
    let root = cache("offline");
    prepare_in(&api(&Server::healthy().start()), &script(), &root).unwrap();
    let fake = Server { no_lib: true, ..Server::healthy() }.start();
    prepare_in(&api(&fake), &script(), &root).expect("the cached lib is enough");
    let _ = fs::remove_dir_all(&root);
}

fn community_script() -> StoreScript {
    serde_json::from_value(json!({
        "id": SCRIPT_ID, "url": "tempoross", "title": "Tempoross", "price": 0, "revision": 4, "can_access": true, "community": true
    }))
    .unwrap()
}

fn code(first: char) -> String {
    std::iter::repeat_n(first, 64).collect()
}

#[test]
fn a_redeemed_code_adds_the_script() {
    let fake = Server::healthy().start();
    let redeemed = api(&fake).redeem(&code('a')).expect("redeem");
    assert_eq!(redeemed.script_id, COMMUNITY_ID);
    assert_eq!(redeemed.url.as_deref(), Some("mine"));
    let sent = fake.requests().into_iter().find(|r| r.path == "/launcher/scripts/redeem").unwrap();
    assert_eq!(sent.header("authorization"), Some("Bearer token-1"));
    assert_eq!(sent.json(), json!({ "code": code('a') }));
}

#[test]
fn a_code_the_server_refuses_says_why() {
    let fake = Server::healthy().start();
    let api = api(&fake);
    assert_eq!(api.redeem(&code('b')).unwrap_err(), ApiError::Conflict("This share code has already been used.".into()));
    assert_eq!(api.redeem(&code('c')).unwrap_err().to_string(), "This share code has expired.");
    assert_eq!(api.redeem(&code('d')).unwrap_err().to_string(), "Too many failed attempts. Try again later.");
    assert_eq!(api.redeem(&code('e')).unwrap_err(), ApiError::NotFound("This share code is not valid.".into()));
}

#[test]
fn a_granted_script_can_be_removed_from_the_account() {
    let fake = Server::healthy().start();
    let api = api(&fake);
    assert!(api.remove_grant(COMMUNITY_ID).unwrap().removed);
    assert_eq!(fake.count("DELETE", &format!("/launcher/scripts/{COMMUNITY_ID}/grant")), 1);
    let sent = fake.requests().into_iter().find(|r| r.method == "DELETE").unwrap();
    assert_eq!(sent.header("authorization"), Some("Bearer token-1"));
    // The server says no for a script that isn't granted
    assert!(matches!(api.remove_grant(SCRIPT_ID), Err(ApiError::NotFound(_))));
}

#[test]
fn an_id_that_isnt_plain_never_reaches_the_server() {
    let fake = Server::healthy().start();
    let api = api(&fake);
    for id in ["", "../me", "a/b", "a?b=1", "a b"] {
        assert!(matches!(api.remove_grant(id), Err(ApiError::NotFound(_))), "{id:?}");
    }
    assert_eq!(fake.count("DELETE", ""), 0);
}

#[test]
fn the_list_carries_the_community_tag_and_the_filter_asks_for_it() {
    let fake = Server::healthy().start();
    let api = api(&fake);

    let all = api.scripts("", "", false).unwrap();
    assert_eq!(all.len(), 2);
    assert!(!all[0].community && !all[0].granted);
    assert!(all[1].community && all[1].granted);
    assert_eq!(crate::store::origin_label(&all[0]), "official");
    assert_eq!(crate::store::origin_label(&all[1]), "community");
    assert!(!fake.requests().iter().any(|r| r.path.contains("community")), "the default list doesn't ask for the filter");

    let community = api.scripts("", "", true).unwrap();
    assert_eq!(community.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Mine"]);
    assert_eq!(fake.count("GET", "/launcher/scripts?community=true"), 1);
}

#[test]
fn a_community_script_runs_only_at_the_revision_the_user_was_warned_about() {
    let root = cache("warned");
    let fake = Server::healthy().start();

    // Warned about revision 4, and revision 4 is what the server starts
    let prepared = prepare_warned(&api(&fake), &community_script(), &root, Some(4)).expect("runs");
    assert_eq!(prepared.session.revision, 4);
    assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 0);
    let _ = fs::remove_dir_all(&root);

    // A new revision went live after the warning: nothing runs, the session is ended and nothing is downloaded
    let fake = Server { revision: 5, ..Server::healthy() }.start();
    let root = cache("warned-new-revision");
    let error = prepare_warned(&api(&fake), &community_script(), &root, Some(4)).err().expect("refused");
    assert!(error.contains("new revision"), "{error}");
    assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 1);
    assert_eq!(fake.count("GET", "/files/"), 0);
    assert!(!revision_dir(&root, SCRIPT_ID, 5).exists());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_community_script_never_runs_without_a_warning() {
    let fake = Server::healthy().start();
    let root = cache("unwarned");
    assert!(prepare_warned(&api(&fake), &community_script(), &root, None).is_err());
    assert_eq!(fake.count("POST", "/launcher/sessions/s1/end"), 1);
    assert_eq!(fake.count("GET", "/files/"), 0);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_official_script_needs_no_warning() {
    let fake = Server::healthy().start();
    let root = cache("official");
    prepare_warned(&api(&fake), &script(), &root, None).expect("official scripts run without one");
    let _ = fs::remove_dir_all(&root);
}
