use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        process::CommandExt,
    },
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
            SystemInformation::GetSystemDirectoryW,
            Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        },
    },
    core::PCWSTR,
};

use super::{
    FAILED_ARG, Kind, Progress, Release, download,
    msi_command::{Scope, command_line, failure},
    own_exe, own_folder, release_file,
};
use crate::api::Api;

const UPGRADE_CODE: &str = "{DF7C7129-38EF-4E32-B9EE-F6FCFA75ED84}";
const FINISH_ARG: &str = "--finish-update";
const HELPER: &str = "finish-update.exe";
const LOG: &str = "install.log";
const WAIT_FOR_EXIT_MS: u32 = 60_000;
const PRODUCT_CODE_LEN: usize = 39;
const PROPERTY_LEN: usize = 1024;
const SYSTEM_FOLDER_LEN: usize = 512;

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
    let status =
        unsafe { MsiGetProductInfoW(product.as_ptr(), property, value.as_mut_ptr(), &mut len) };
    (status == ERROR_SUCCESS).then(|| String::from_utf16_lossy(&value[..len as usize]))
}

fn same_folder(a: &str, b: &str) -> bool {
    a.trim_end_matches('\\')
        .eq_ignore_ascii_case(b.trim_end_matches('\\'))
}

fn installed_scope(folder: &Path) -> Option<Scope> {
    let folder = folder.to_str()?;
    let code = wide(OsStr::new(UPGRADE_CODE));
    (0..).find_map(|index| {
        let mut product = [0u16; PRODUCT_CODE_LEN];
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
    crate::paths::project_dirs().map(|dirs| dirs.data_dir().join("update"))
}

pub fn cleanup() {
    let Some(dir) = work_dir() else { return };
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_name() != LOG {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn prepare(
    api: &Api,
    release: &Release,
    progress: &Progress,
    repaint: &dyn Fn(),
) -> Result<Prepared, String> {
    let exe = own_exe()?;
    let folder = own_folder(&exe)?;
    let scope = installed_scope(&folder)
        .ok_or("This copy of the loader wasn't installed with the installer.")?;
    let file = release_file(release)?;
    let dir = work_dir().ok_or("Couldn't find the data folder.")?;
    let msi = download(api, file, &dir, progress, repaint)?;
    Ok(Prepared {
        msi,
        scope,
        exe,
        dir,
    })
}

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

fn system_folder() -> PathBuf {
    let mut buffer = vec![0u16; SYSTEM_FOLDER_LEN];
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() {
        return PathBuf::from(r"C:\Windows\System32");
    }
    PathBuf::from(OsString::from_wide(&buffer[..len]))
}

fn wait_for_exit(pid: u32) -> bool {
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
    let status = Command::new(system_folder().join("msiexec.exe"))
        .raw_arg(command_line(msi, log, folder, scope))
        .status();
    let message = failure(status.ok().and_then(|status| status.code()), log);
    let mut loader = Command::new(exe);
    if let Some(message) = message {
        loader.arg(FAILED_ARG).arg(message);
    }
    let _ = loader.spawn();
}
