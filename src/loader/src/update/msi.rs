//! The Windows update: Windows Installer replaces the files, run by a copy of the loader outside the install.

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{self, Command},
};

use windows_sys::{
    Win32::{
        Foundation::{CloseHandle, ERROR_SUCCESS, WAIT_OBJECT_0},
        System::{
            ApplicationInstallationAndServicing::{
                INSTALLPROPERTY_ASSIGNMENTTYPE, INSTALLPROPERTY_INSTALLLOCATION,
                MsiEnumRelatedProductsW, MsiGetProductInfoW,
            },
            Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        },
    },
    core::PCWSTR,
};

use super::{
    FAILED_ARG, Kind, Progress, Release, download,
    msi_command::{Scope, command_line, failure},
};
use crate::api::Api;

/// `UpgradeCode` in installer/volcren.wxs.
const UPGRADE_CODE: &str = "{DF7C7129-38EF-4E32-B9EE-F6FCFA75ED84}";
const FINISH_ARG: &str = "--finish-update";
const HELPER: &str = "finish-update.exe";
const LOG: &str = "install.log";
const WAIT_FOR_EXIT_MS: u32 = 60_000;
const PRODUCT_CODE_LEN: usize = 39;
const PROPERTY_LEN: usize = 1024;

pub struct Prepared {
    msi: PathBuf,
    scope: Scope,
    exe: PathBuf,
    dir: PathBuf,
}

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}

fn product_property(product: &[u16], property: PCWSTR) -> Option<String> {
    let mut value = vec![0u16; PROPERTY_LEN];
    let mut len = value.len() as u32;
    // SAFETY: `product` is null terminated and `len` is the length of `value`.
    let status =
        unsafe { MsiGetProductInfoW(product.as_ptr(), property, value.as_mut_ptr(), &mut len) };
    (status == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&value[..len as usize]))
}

fn same_folder(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

/// How the installed product holding `folder` was installed. A PC can have one of each mode.
fn installed_scope(folder: &Path) -> Option<Scope> {
    let folder = folder.to_str()?;
    let code = wide(OsStr::new(UPGRADE_CODE));
    (0..).find_map(|index| {
        let mut product = [0u16; PRODUCT_CODE_LEN];
        // SAFETY: `code` is null terminated and `product` holds a product code and its null.
        let status =
            unsafe { MsiEnumRelatedProductsW(code.as_ptr(), 0, index, product.as_mut_ptr()) };
        if status != ERROR_SUCCESS {
            return Some(None);
        }
        let location = product_property(&product, INSTALLPROPERTY_INSTALLLOCATION)?;
        if !same_folder(&location, folder) {
            return None;
        }
        Some(
            match product_property(&product, INSTALLPROPERTY_ASSIGNMENTTYPE)?.as_str() {
                "0" => Some(Scope::PerUser),
                "1" => Some(Scope::PerMachine),
                _ => None,
            },
        )
    })?
}

pub fn kind(exe: &Path) -> Kind {
    match exe.parent().and_then(installed_scope) {
        Some(Scope::PerUser) => Kind::Auto { admin: false },
        Some(Scope::PerMachine) => Kind::Auto { admin: true },
        None => Kind::Manual,
    }
}

fn work_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("com", "volcren", "Volcren")
        .map(|dirs| dirs.data_dir().join("update"))
}

/// Removes what an earlier update left, except the installer's log, which a failure message points at.
pub fn cleanup() {
    let Some(dir) = work_dir() else { return };
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_name() != LOG {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Downloads and checks the installer. Nothing in the install changes.
pub fn prepare(
    api: &Api,
    release: &Release,
    progress: &Progress,
    repaint: &dyn Fn(),
) -> Result<Prepared, String> {
    let exe = env::current_exe().map_err(|e| format!("Can't find the loader's own path: {e}"))?;
    let folder = exe.parent().ok_or("The loader's own path has no folder.")?;
    let scope = installed_scope(folder)
        .ok_or("This copy of the loader wasn't installed with the installer.")?;
    let file = release
        .file()
        .ok_or("This release has no build for this system.")?;
    let dir = work_dir().ok_or("Couldn't find the data folder.")?;
    let msi = download(api, file, &dir, progress, repaint)?;
    Ok(Prepared {
        msi,
        scope,
        exe,
        dir,
    })
}

/// Starts the copy of this loader that installs the update once this one closes.
pub fn apply(prepared: &Prepared) -> Result<(), String> {
    let helper = prepared.dir.join(HELPER);
    fs::copy(&prepared.exe, &helper).map_err(|e| format!("Couldn't start the update: {e}"))?;
    Command::new(helper)
        .arg(FINISH_ARG)
        .arg(&prepared.msi)
        .arg(process::id().to_string())
        .arg(&prepared.exe)
        .arg(prepared.scope.word())
        .arg(prepared.dir.join(LOG))
        .spawn()
        .map(drop)
        .map_err(|e| format!("Couldn't start the update: {e}"))
}

fn wait_for_exit(pid: u32) -> bool {
    // SAFETY: the handle is checked and closed here, and a pid that's gone gives null.
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return true;
        }
        let exited = WaitForSingleObject(process, WAIT_FOR_EXIT_MS) == WAIT_OBJECT_0;
        CloseHandle(process);
        exited
    }
}

/// With `--finish-update`, this process is the copy `apply` started: it installs, starts the loader and returns true.
pub fn finish_if_asked() -> bool {
    let mut args = env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(FINISH_ARG)) {
        return false;
    }
    let args: Vec<OsString> = args.collect();
    if let [msi, pid, exe, scope, log] = args.as_slice() {
        finish(msi, pid, Path::new(exe), scope, log);
    }
    true
}

fn finish(msi: &OsStr, pid: &OsStr, exe: &Path, scope: &OsStr, log: &OsStr) {
    let pid = pid.to_str().and_then(|pid| pid.parse().ok());
    let scope = scope.to_str().and_then(Scope::parse);
    let (Some(pid), Some(scope), Some(folder), Some(msi), Some(log)) = (
        pid,
        scope,
        exe.parent().and_then(Path::to_str),
        msi.to_str(),
        log.to_str(),
    ) else {
        return;
    };
    if !wait_for_exit(pid) {
        return;
    }
    let system = env::var_os("SystemRoot").map_or_else(
        || PathBuf::from(r"C:\Windows\System32"),
        |root| PathBuf::from(root).join("System32"),
    );
    let status = Command::new(system.join("msiexec.exe"))
        .raw_arg(command_line(msi, log, folder, scope))
        .status();
    let message = failure(status.ok().and_then(|status| status.code()), log);
    let mut loader = Command::new(exe);
    if let Some(message) = message {
        loader.arg(FAILED_ARG).arg(message);
    }
    let _ = loader.spawn();
}
