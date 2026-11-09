use std::{
    env, fs,
    fs::OpenOptions,
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use flate2::read::GzDecoder;

use super::{Kind, Progress, Release, download, own_exe, own_folder, release_file};
use crate::{api::Api, script::lock};

const WORK_DIR: &str = ".update";
const STAGE_DIR: &str = "stage";
const BACKUP_DIR: &str = "backup";
const ARCHIVE_ROOT: &str = "volcren/";
const FILES: [(&str, bool); 4] = [
    ("loader", true),
    ("injector", true),
    ("injector.exe", false),
    ("agent.dll", false),
];
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

static RELAUNCH: Mutex<Option<PathBuf>> = Mutex::new(None);
static STAGE: Mutex<()> = Mutex::new(());

pub struct Prepared {
    install: PathBuf,
    exe: PathBuf,
}

pub fn kind(exe: &Path) -> Kind {
    let Some(install) = exe.parent() else {
        return Kind::Manual;
    };
    let probe = install.join(WORK_DIR);
    if fs::create_dir_all(&probe).is_ok() {
        let _ = fs::remove_dir(&probe);
        Kind::Auto { admin: false }
    } else {
        Kind::Manual
    }
}

pub fn cleanup() {
    if let Ok(install) = own_exe().and_then(|exe| own_folder(&exe)) {
        let _ = fs::remove_dir_all(install.join(WORK_DIR));
    }
}

pub fn prepare(
    api: &Api,
    release: &Release,
    progress: &Progress,
    repaint: &dyn Fn(),
) -> Result<Prepared, String> {
    let exe = own_exe()?;
    let install = own_folder(&exe)?;
    let file = release_file(release)?;
    let work = install.join(WORK_DIR);
    let archive = download(api, file, &work, progress, repaint)?;
    let _stage = lock(&STAGE);
    progress.check()?;
    let stage = work.join(STAGE_DIR);
    let _ = fs::remove_dir_all(&stage);
    unpack(&archive, &stage)?;
    Ok(Prepared { install, exe })
}

pub fn apply(prepared: &Prepared) -> Result<(), String> {
    swap(&prepared.install)?;
    *lock(&RELAUNCH) = Some(prepared.exe.clone());
    Ok(())
}

pub fn relaunch_after_exit() {
    let exe = lock(&RELAUNCH).take();
    if let Some(exe) = exe
        && let Err(error) = Command::new(&exe).args(env::args_os().skip(1)).spawn()
    {
        eprintln!("Couldn't start {}: {error}", exe.display());
    }
}

fn unpack(archive: &Path, stage: &Path) -> Result<(), String> {
    let failed = |e: io::Error| format!("Couldn't unpack the update: {e}");
    let source = fs::File::open(archive).map_err(failed)?;
    fs::create_dir_all(stage).map_err(failed)?;
    let mut tar = tar::Archive::new(GzDecoder::new(source));
    for entry in tar.entries().map_err(failed)? {
        let mut entry = entry.map_err(failed)?;
        let path = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        if entry.header().entry_type().is_dir() && path.trim_end_matches('/') == "volcren" {
            continue;
        }
        let known = path
            .strip_prefix(ARCHIVE_ROOT)
            .and_then(|name| FILES.iter().find(|(file, _)| *file == name))
            .filter(|_| entry.header().entry_type().is_file());
        let Some((name, executable)) = known else {
            return Err(format!(
                "The update has a file that can't be used: {path:?}"
            ));
        };
        let mode = if *executable { 0o755 } else { 0o644 };
        let mut output = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(stage.join(name))
            .map_err(failed)?;
        let copied =
            io::copy(&mut (&mut entry).take(MAX_FILE_BYTES + 1), &mut output).map_err(failed)?;
        if copied > MAX_FILE_BYTES {
            return Err(format!("{name} in the update is too large."));
        }
    }
    for (name, _) in FILES {
        let size = fs::metadata(stage.join(name)).map_or(0, |m| m.len());
        if size == 0 {
            return Err(format!("The update is missing {name}."));
        }
    }
    Ok(())
}

fn swap(install: &Path) -> Result<(), String> {
    let work = install.join(WORK_DIR);
    let (stage, backup) = (work.join(STAGE_DIR), work.join(BACKUP_DIR));
    let _ = fs::remove_dir_all(&backup);
    fs::create_dir_all(&backup).map_err(|e| format!("Couldn't install the update: {e}"))?;
    for (index, (name, _)) in FILES.iter().enumerate() {
        let (from, to, old) = (stage.join(name), install.join(name), backup.join(name));
        let step = (|| {
            if to.exists() {
                fs::hard_link(&to, &old)?;
            }
            fs::rename(&from, &to)
        })();
        if let Err(error) = step {
            for (done, _) in &FILES[..index] {
                let (to, old) = (install.join(done), backup.join(done));
                let _ = if old.exists() {
                    fs::rename(&old, &to)
                } else {
                    fs::remove_file(&to)
                };
            }
            return Err(format!(
                "Couldn't install the update, the old version stays: {error}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, thread::sleep, time::Duration};

    use flate2::{Compression, write::GzEncoder};

    use super::{super::tests::*, *};
    use crate::{
        fake_api::{FakeServer, Reply},
        session::sha256_hex,
        update::{File, Files},
    };

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o600);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_path("placeholder").unwrap();
            let name = &mut header.as_old_mut().name;
            name.fill(0);
            name[..path.len()].copy_from_slice(path.as_bytes());
            header.set_cksum();
            tar.append(&header, *contents).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn release_files(loader: &[u8]) -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("volcren/loader", loader.to_vec()),
            ("volcren/injector", b"new injector".to_vec()),
            ("volcren/injector.exe", b"new injector.exe".to_vec()),
            ("volcren/agent.dll", b"new agent".to_vec()),
        ]
    }

    fn full_archive(loader: &[u8]) -> Vec<u8> {
        let files = release_files(loader);
        let entries: Vec<(&str, &[u8])> = files.iter().map(|(n, c)| (*n, c.as_slice())).collect();
        archive(&entries)
    }

    fn unpack_bytes(name: &str, bytes: &[u8]) -> (PathBuf, Result<(), String>) {
        let dir = temp_dir(name);
        let path = dir.join("a.tar.gz");
        fs::write(&path, bytes).unwrap();
        let stage = dir.join("stage");
        let result = unpack(&path, &stage);
        (stage, result)
    }

    #[test]
    fn unpacks_the_release_without_its_folder_and_marks_programs_executable() {
        let (stage, result) = unpack_bytes("good", &full_archive(b"new loader"));
        result.unwrap();
        assert_eq!(fs::read(stage.join("loader")).unwrap(), b"new loader");
        let mode =
            |name: &str| fs::metadata(stage.join(name)).unwrap().permissions().mode() & 0o111;
        assert_ne!(mode("loader"), 0);
        assert_ne!(mode("injector"), 0);
        assert_eq!(mode("agent.dll"), 0);
    }

    #[test]
    fn rejects_paths_that_leave_the_folder_or_files_it_does_not_expect() {
        for (case, path) in [
            ("up", "volcren/../../evil"),
            ("absolute", "/etc/evil"),
            ("no-root", "loader"),
            ("nested", "volcren/lib/loader"),
            ("unknown", "volcren/evil"),
        ] {
            let bytes = archive(&[(path, b"x")]);
            let (stage, result) = unpack_bytes(&format!("reject-{case}"), &bytes);
            assert!(result.is_err(), "{path}");
            assert_eq!(fs::read_dir(stage).unwrap().count(), 0, "{path}");
        }
    }

    #[test]
    fn a_link_is_rejected() {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        tar.append_link(&mut header, "volcren/loader", "/usr/bin/evil")
            .unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        assert!(unpack_bytes("link", &bytes).1.is_err());
    }

    #[test]
    fn an_incomplete_release_is_rejected() {
        let bytes = archive(&[("volcren/loader", b"x"), ("volcren/agent.dll", b"")]);
        assert!(
            unpack_bytes("incomplete", &bytes)
                .1
                .unwrap_err()
                .contains("missing")
        );
    }

    fn fake_install(name: &str) -> PathBuf {
        let install = temp_dir(name);
        for (file, _) in FILES {
            fs::write(install.join(file), format!("old {file}")).unwrap();
        }
        install
    }

    #[test]
    fn swap_replaces_every_file() {
        let install = fake_install("swap");
        let stage = install.join(WORK_DIR).join(STAGE_DIR);
        fs::create_dir_all(&stage).unwrap();
        for (file, _) in FILES {
            fs::write(stage.join(file), format!("new {file}")).unwrap();
        }
        swap(&install).unwrap();
        for (file, _) in FILES {
            assert_eq!(
                fs::read_to_string(install.join(file)).unwrap(),
                format!("new {file}")
            );
        }
    }

    #[test]
    fn a_failed_swap_puts_the_old_files_back() {
        let install = fake_install("rollback");
        let stage = install.join(WORK_DIR).join(STAGE_DIR);
        fs::create_dir_all(&stage).unwrap();
        for (file, _) in &FILES[..3] {
            fs::write(stage.join(file), format!("new {file}")).unwrap();
        }
        assert!(swap(&install).unwrap_err().contains("old version stays"));
        for (file, _) in FILES {
            assert_eq!(
                fs::read_to_string(install.join(file)).unwrap(),
                format!("old {file}")
            );
        }
    }

    #[test]
    fn the_whole_update_on_a_scratch_install() {
        let install = fake_install("flow");
        let marker = install.join("relaunched");
        let new_loader = format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display());
        let bytes = full_archive(new_loader.as_bytes());
        let release = Release {
            version: "2026.11.05-abc1234".into(),
            build: 1,
            files: Files {
                windows: None,
                linux: Some(File {
                    path: "2026.11.05-abc1234/a.tar.gz".into(),
                    sha256: sha256_hex(&bytes),
                }),
            },
        };
        let server = FakeServer::start(move |_, _| Reply::bytes(bytes.clone()));
        let api = api(&server.url);

        let work = install.join(WORK_DIR);
        let file = release.file().unwrap();
        let archive = download(&api, file, &work, &Progress::default(), &|| {}).unwrap();
        unpack(&archive, &work.join(STAGE_DIR)).unwrap();
        for (name, _) in FILES {
            assert_eq!(
                fs::read_to_string(install.join(name)).unwrap(),
                format!("old {name}"),
                "preparing changes nothing"
            );
        }

        let exe = install.join("loader");
        apply(&Prepared {
            install: install.clone(),
            exe: exe.clone(),
        })
        .unwrap();
        assert_eq!(fs::read(install.join("agent.dll")).unwrap(), b"new agent");
        assert_eq!(fs::read_to_string(&exe).unwrap(), new_loader);

        relaunch_after_exit();
        for _ in 0..100 {
            if marker.exists() {
                return;
            }
            sleep(Duration::from_millis(50));
        }
        panic!("the new loader didn't start");
    }

    #[test]
    fn a_folder_that_cant_be_written_gets_the_manual_update() {
        let install = temp_dir("read-only");
        fs::set_permissions(&install, fs::Permissions::from_mode(0o555)).unwrap();
        let result = kind(&install.join("loader"));
        fs::set_permissions(&install, fs::Permissions::from_mode(0o755)).unwrap();
        if unsafe { libc::geteuid() } != 0 {
            assert_eq!(result, Kind::Manual);
        }
        assert_eq!(kind(&install.join("loader")), Kind::Auto { admin: false });
    }
}
