//! The whole run-a-store-script flow (lib install, start, download, heartbeat, end) against a fake server.

use std::{fs, io::{Cursor, Write}, path::PathBuf};

use serde_json::json;
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

use crate::{
    api::{Api, Config, Stats, StoreScript},
    fake_api::{FakeServer, Reply, Request},
    session::{prepare_in, revision_dir, sha256_hex},
};

const SCRIPT_ID: &str = "7d081fdd-59de-4a4e-9c29-b2f92d9bc697";
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
    /// 404 for the lib index, as before the first lib release.
    no_lib: bool,
}

impl Server {
    const fn healthy() -> Self {
        Self { revision: 4, start_status: 200, file_status: 200, wrong_checksum: false, no_lib: false }
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
            ("POST", "/launcher/sessions") if self.start_status == 402 => {
                Reply::json(402, json!({ "error": "Not enough credits.", "required": 300 }))
            }
            ("POST", "/launcher/sessions") => {
                let checksum = |text: &str| if self.wrong_checksum { "0".repeat(64) } else { sha256_hex(text.as_bytes()) };
                Reply::json(
                    200,
                    json!({
                        "session_id": "s1",
                        "revision": self.revision,
                        "price": 30,
                        "heartbeat_seconds": 60,
                        "files": [
                            { "name": "main.luau", "url": format!("{base}/files/main.luau"), "sha256": checksum(MAIN) },
                            { "name": "util.luau", "url": format!("{base}/files/util.luau"), "sha256": checksum(UTIL) },
                        ],
                    }),
                )
            }
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
    assert_eq!(fs::read_to_string(dir.join("util.luau")).unwrap(), UTIL);
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
fn a_second_start_reuses_the_download_and_the_lib() {
    let fake = Server::healthy().start();
    let api = api(&fake);
    let root = cache("reuse");
    prepare_in(&api, &script(), &root).unwrap();
    prepare_in(&api, &script(), &root).unwrap();
    assert_eq!(fake.count("GET", "/files/main.luau"), 1);
    assert_eq!(fake.count("GET", "/storage/v1/object/public/client/lib/index.json"), 2, "the index is checked every time");
    assert_eq!(fake.count("GET", &format!("/storage/v1/object/public/client/lib/{LIB_VERSION}/lib.zip")), 1);
    assert_eq!(fake.count("POST", "/launcher/sessions"), 2);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_new_revision_replaces_the_old_one_in_the_cache() {
    let root = cache("revisions");
    let old = Server { revision: 4, ..Server::healthy() }.start();
    prepare_in(&api(&old), &script(), &root).unwrap();
    let new = Server { revision: 5, ..Server::healthy() }.start();
    prepare_in(&api(&new), &script(), &root).unwrap();
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
