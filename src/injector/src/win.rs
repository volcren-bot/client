use std::{
    ffi::c_void,
    collections::hash_map::DefaultHasher,
    fs, io,
    hash::{Hash, Hasher},
    iter::once,
    mem::{size_of, transmute},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{copy_nonoverlapping, null, null_mut, write_bytes, NonNull},
    thread::sleep,
    sync::{Arc, Mutex, OnceLock, atomic::{Ordering, fence}},
    time::{Duration, Instant},
};

use windows_sys::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_BAD_LENGTH, GetLastError, HANDLE, HWND, INVALID_HANDLE_VALUE,
            LPARAM, POINT, WAIT_ABANDONED, WAIT_OBJECT_0,
        },
        Graphics::Gdi::ClientToScreen,
        System::{
            Diagnostics::{
                Debug::{ReadProcessMemory, WriteProcessMemory},
                ToolHelp::{
                    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                    TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
                },
            },
            LibraryLoader::{GetModuleHandleW, GetProcAddress},
            Memory::{
                FILE_MAP_READ, FILE_MAP_WRITE, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
                CreateFileMappingW, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile, VirtualAllocEx,
                VirtualFreeEx,
            },
            Threading::{
                CreateMutexW, CreateRemoteThread, EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE, ReleaseMutex, IsWow64Process, OpenEventW, OpenProcess, PROCESS_CREATE_THREAD,
                PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
                QueryFullProcessImageNameW, SetEvent, WaitForSingleObject,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetCursorPos, GetWindowTextW, GetWindowThreadProcessId,
        },
    },
    core::BOOL,
    s, w,
};

use ipc::{
    HEADER_SIZE, Header, IMAGE_DATA_SIZE, INITIAL_CURSOR_SIZE, Incompatible, overlay_mode, overlay_offset, shared_memory_name,
    frame_event_name, initial_cursor_name, unload_event_name, work_event_name, worker_mutex_name,
    read::short_string_text,
    state::{
        item_name_status, loc_status, menu_status, model_status, model_target, project_status, cache_kind, cache_status, CacheReply, CacheRequest, GameValRequest, ItemNameRequest, LocsReply, ModelRequest, NamesReply, worker_kind, worker_status, WorkerRequest, ProjectRequest, CACHE_BATCH, CACHE_FILE_MISSING, CACHE_FILE_NO_ROOM, ITEM_NAME_BATCH, PROJECT_BATCH,
        LAYOUT_SIZE, Layout, LayoutMemory, LayoutView, ReadError, Shared, TotalXpRequest, game_state::LOGGED_IN, name_status, part, state_memory_name,
        total_xp_status,
    },
};

use crate::{
    worker,
    CONNECT_TIMEOUT, Client, Connection, DisplayName, GameState, TotalXp, LoadReport, MODULE_PREFIX, Overlay, Rgba,
    UNLOAD_TIMEOUT, Widget, dll_version,
    projection::{self, Failure, Terrain, Via},
    widgets::{Memory, SubInterface},
    state,
    world::{self, Item, Npc, Player},
    Position,
    world_select::WorldSelect,
};

const LOAD_LIBRARY_TIMEOUT_MS: u32 = 10_000;

struct Handle(HANDLE);

// SAFETY: a kernel handle can be used and closed from any thread.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

impl Handle {
    fn new(handle: HANDLE) -> Option<Self> {
        (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(Self(handle))
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(once(0)).collect()
}

fn from_wide(buffer: &[u16]) -> String {
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

struct Module {
    name: String,
    path: String,
    base: usize,
}

fn modules(pid: u32) -> Result<Vec<Module>, String> {
    let mut attempts = 0;
    let snapshot = loop {
        let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
        if let Some(handle) = Handle::new(handle) {
            break handle;
        }
        attempts += 1;
        if unsafe { GetLastError() } != ERROR_BAD_LENGTH || attempts == 5 {
            return Err(format!("Couldn't list the modules of {pid}: {}", io::Error::last_os_error()));
        }
        sleep(Duration::from_millis(50));
    };

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };
    let mut modules = Vec::new();
    let mut ok = unsafe { Module32FirstW(snapshot.0, &mut entry) };
    while ok != 0 {
        modules.push(Module {
            name: from_wide(&entry.szModule),
            path: from_wide(&entry.szExePath),
            base: entry.modBaseAddr as usize,
        });
        ok = unsafe { Module32NextW(snapshot.0, &mut entry) };
    }
    Ok(modules)
}

fn agent_module(modules: &[Module]) -> Option<&Module> {
    modules
        .iter()
        .find(|m| m.name.to_ascii_lowercase().starts_with(MODULE_PREFIX))
}

fn process_name(pid: u32) -> Option<String> {
    let process = Handle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) })?;
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len) } == 0 {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned())
}

fn is_64bit(process: HANDLE) -> bool {
    let mut wow64: BOOL = 0;
    let ok = unsafe { IsWow64Process(process, &mut wow64) };
    ok != 0 && wow64 == 0
}

pub(crate) fn class_is(hwnd: HWND, name: &str) -> bool {
    let mut class = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
    from_wide(&class[..len.max(0) as usize]) == name
}

pub fn list_clients() -> Vec<Client> {
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let found = unsafe { &mut *(lparam as *mut Vec<(u32, String)>) };

        if !class_is(hwnd, "JagWindow") {
            return 1;
        }

        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if found.iter().any(|(p, _)| *p == pid) {
            return 1;
        }

        let mut title = [0u16; 256];
        let len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
        found.push((pid, from_wide(&title[..len.max(0) as usize])));
        1
    }

    let mut windows: Vec<(u32, String)> = Vec::new();
    unsafe { EnumWindows(Some(callback), &mut windows as *mut _ as LPARAM) };

    windows
        .into_iter()
        .map(|(pid, title)| {
            let process = Handle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
            Client {
                pid,
                exe: process_name(pid).unwrap_or_default(),
                title,
                is_64bit: process.is_some_and(|p| is_64bit(p.0)),
                loaded: modules(pid)
                    .ok()
                    .and_then(|m| agent_module(&m).map(|m| m.path.clone())),
                version: connection(pid).map(|c| c.version),
            }
        })
        .collect()
}

fn remote_load_library(remote_kernel32: usize) -> Result<unsafe extern "system" fn(*mut c_void) -> u32, String> {
    let local_kernel32 = unsafe { GetModuleHandleW(w!("kernel32.dll")) };
    let function = unsafe { GetProcAddress(local_kernel32, s!("LoadLibraryW")) }
        .ok_or("LoadLibraryW not found in kernel32.dll")?;
    let address = remote_kernel32 + (function as usize - local_kernel32 as usize);
    Ok(unsafe { transmute::<usize, unsafe extern "system" fn(*mut c_void) -> u32>(address) })
}

fn versioned_copy(dll: &Path, bytes: &[u8], version: &str) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("agent");
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let stem = dll.file_stem().unwrap_or_default().to_string_lossy();
    let copy = dir.join(format!("{stem}-{version}.dll"));
    if copy.is_file() {
        return Ok(copy);
    }

    let partial = dir.join(format!("{stem}-{version}.{}.tmp", std::process::id()));
    fs::write(&partial, bytes).map_err(|e| format!("Cannot write {}: {e}", partial.display()))?;
    if let Err(error) = fs::rename(&partial, &copy) {
        let _ = fs::remove_file(&partial);
        if !copy.is_file() {
            return Err(format!("Cannot write {}: {error}", copy.display()));
        }
    }
    Ok(copy)
}

fn remove_old_copies(dll: &Path) {
    let (Some(dir), Some(name)) = (dll.parent(), dll.file_name()) else { return };
    let Some(stem) = name.to_str().and_then(|n| n.rsplit_once('-')).map(|(stem, _)| format!("{stem}-")) else {
        return;
    };
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.starts_with(&stem) && file.ends_with(".dll") && entry.file_name() != name {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Leaves the cursor where the agent will find it while it builds its header; keep the handle alive until it connects.
fn seed_cursor(pid: u32, cursor: &crate::Cursor) -> Option<Handle> {
    let name = wide(initial_cursor_name(pid));
    let mapping = Handle::new(unsafe {
        CreateFileMappingW(INVALID_HANDLE_VALUE, null(), PAGE_READWRITE, 0, INITIAL_CURSOR_SIZE as u32, name.as_ptr())
    })?;
    let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, INITIAL_CURSOR_SIZE) };
    if view.Value.is_null() {
        return None;
    }
    let [r, g, b] = cursor.color;
    let bytes = [cursor.style.value(), r, g, b, cursor.size];
    unsafe {
        (view.Value as *mut u8).copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
        UnmapViewOfFile(view);
    }
    Some(mapping)
}

pub fn load(pid: u32, dll: &Path, cursor: Option<&crate::Cursor>) -> Result<LoadReport, String> {
    forget(pid);
    let dll = std::path::absolute(dll).map_err(|e| format!("Invalid DLL path: {e}"))?;
    let bytes = fs::read(&dll).map_err(|e| format!("Cannot read {}: {e}", dll.display()))?;
    let version = dll_version(&bytes);
    let dll = versioned_copy(&dll, &bytes, &version)?;
    let dll_name = dll.file_name().unwrap_or_default().to_string_lossy().to_ascii_lowercase();

    let process = Handle::new(unsafe {
        OpenProcess(
            PROCESS_CREATE_THREAD
                | PROCESS_QUERY_INFORMATION
                | PROCESS_VM_OPERATION
                | PROCESS_VM_WRITE
                | PROCESS_VM_READ,
            0,
            pid,
        )
    })
    .ok_or_else(|| format!("Couldn't open process {pid}: {}", io::Error::last_os_error()))?;

    if !is_64bit(process.0) {
        return Err("Only 64-bit clients are supported.".into());
    }

    let is_loaded = |modules: &[Module]| modules.iter().any(|m| m.name.to_ascii_lowercase() == dll_name);

    let mut before = modules(pid)?;
    let mut replaced = None;
    if agent_module(&before).is_some() {
        let current = connection(pid);
        if is_loaded(&before) || current.as_ref().is_some_and(|c| c.version == version) {
            if let Some(cursor) = cursor {
                let _ = set_cursor(pid, cursor);
            }
            return Ok(LoadReport { already_loaded: true, replaced: None, connection: current });
        }
        let old = current.map(|c| c.version).unwrap_or_else(|| "unknown".into());
        unload(pid).map_err(|e| format!("Cannot replace agent {old}: {e}"))?;
        replaced = Some(old);
        before = modules(pid)?;
    }

    let _cursor = cursor.and_then(|c| seed_cursor(pid, c));

    let kernel32 = before
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case("kernel32.dll"))
        .ok_or("kernel32.dll isn't loaded in the client.")?;
    let load_library = remote_load_library(kernel32.base)?;

    let path = wide(&dll);
    let size = path.len() * size_of::<u16>();
    let remote = unsafe { VirtualAllocEx(process.0, null(), size, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
    if remote.is_null() {
        return Err(format!("VirtualAllocEx failed: {}", io::Error::last_os_error()));
    }
    let free = || unsafe { VirtualFreeEx(process.0, remote, 0, MEM_RELEASE) };

    if unsafe { WriteProcessMemory(process.0, remote, path.as_ptr().cast(), size, null_mut()) } == 0 {
        let error = io::Error::last_os_error();
        free();
        return Err(format!("WriteProcessMemory failed: {error}"));
    }

    let Some(thread) = Handle::new(unsafe {
        CreateRemoteThread(process.0, null(), 0, Some(load_library), remote, 0, null_mut())
    }) else {
        let error = io::Error::last_os_error();
        free();
        return Err(format!("CreateRemoteThread failed: {error}"));
    };

    if unsafe { WaitForSingleObject(thread.0, LOAD_LIBRARY_TIMEOUT_MS) } != WAIT_OBJECT_0 {
        return Err("Timed out waiting for LoadLibraryW in the client.".into());
    }
    free();

    if !is_loaded(&modules(pid).unwrap_or_default()) {
        return Err("LoadLibraryW failed in the client. Check that the DLL is 64-bit and its dependencies are available.".into());
    }

    remove_old_copies(&dll);
    Ok(LoadReport {
        already_loaded: false,
        replaced,
        connection: wait_for_connection(pid, &version, CONNECT_TIMEOUT),
    })
}

pub fn unload(pid: u32) -> Result<(), String> {
    forget(pid);
    if agent_module(&modules(pid)?).is_none() {
        return Ok(());
    }
    let name = wide(unload_event_name(pid));
    let event = Handle::new(unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) }).ok_or(
        "it's a build from before hot reload, which can't unload. Restart the client once.",
    )?;
    if unsafe { SetEvent(event.0) } == 0 {
        return Err(format!("SetEvent failed: {}", io::Error::last_os_error()));
    }

    let start = Instant::now();
    while start.elapsed() < UNLOAD_TIMEOUT {
        sleep(Duration::from_millis(100));
        if agent_module(&modules(pid)?).is_none() {
            return Ok(());
        }
    }
    Err("it didn't unload, the client console says why.".into())
}

fn wait_for_connection(pid: u32, version: &str, timeout: Duration) -> Option<Connection> {
    let start = Instant::now();
    let mut latest = None;
    loop {
        let written = |c: &Connection| c.protocol != 0 && (c.version == version || !c.is_compatible());
        if let Some(connection) = connection(pid).filter(written) {
            if connection.offsets_found.is_some() || !connection.is_compatible() {
                return Some(connection);
            }
            latest = Some(connection);
        }
        if start.elapsed() >= timeout {
            return latest;
        }
        sleep(Duration::from_millis(100));
    }
}

fn read_header(pid: u32) -> Option<Result<Header, Incompatible>> {
    let name = wide(shared_memory_name(pid));
    let mapping = Handle::new(unsafe { OpenFileMappingW(FILE_MAP_READ, 0, name.as_ptr()) })?;
    let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_READ, 0, 0, HEADER_SIZE) };
    if view.Value.is_null() {
        return None;
    }

    let mut header = [0u8; HEADER_SIZE];
    unsafe {
        copy_nonoverlapping(view.Value as *const u8, header.as_mut_ptr(), HEADER_SIZE);
        UnmapViewOfFile(view);
    }
    Some(Header::from_bytes(&header))
}

pub fn connection(pid: u32) -> Option<Connection> {
    Some(Connection::from_header(read_header(pid)?))
}

pub(crate) fn canvas_offset(pid: u32) -> (i32, i32) {
    match read_header(pid) {
        Some(Ok(header)) => (header.offset_x, header.offset_y_top_down()),
        _ => (0, 0),
    }
}

struct ProcessMemory(Arc<Handle>);

impl Memory for ProcessMemory {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        let mut read = 0;
        let ok = unsafe {
            ReadProcessMemory(self.0.0, address as *const c_void, buffer.as_mut_ptr().cast(), buffer.len(), &mut read)
        };
        ok != 0 && read == buffer.len()
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        let mut written = 0;
        let ok = unsafe {
            WriteProcessMemory(self.0.0, address as *const c_void, bytes.as_ptr().cast(), bytes.len(), &mut written)
        };
        ok != 0 && written == bytes.len()
    }
}

fn reader(pid: u32, needs: u64, what: &str) -> Result<(Layout, ProcessMemory), String> {
    let session = session(pid)?;
    let layout = read_layout(pid, &session)?;
    if !layout.has(needs) {
        return Err(format!("agent in client {pid} didn't find {what}"));
    }
    Ok((layout, ProcessMemory(session.process.clone())))
}

pub fn game_state(pid: u32) -> Result<GameState, String> {
    if let Some(state) = ask_worker(pid, worker_kind::GAME_STATE, [0; 4]) {
        return Ok(state);
    }
    let (layout, memory) = reader(pid, 0, "")?;
    Ok(worker::game_state(&memory, &layout))
}

pub fn varps(pid: u32) -> Result<Option<Vec<i32>>, String> {
    let (layout, memory) = reader(pid, part::VARPS, "the varps")?;
    Ok(state::varps(&memory, &layout))
}

pub fn varp_value(pid: u32, id: usize) -> Result<Option<i32>, String> {
    let (layout, memory) = reader(pid, part::VARPS, "the varps")?;
    Ok(state::varp(&memory, &layout, id))
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
// How often a request checks for the agent's answer; it answers within a frame, so a longer pause dominates the cost.
const REQUEST_POLL: Duration = Duration::from_millis(1);

/// A client's process and state block, kept per pid for `SESSION_TTL` so an agent reload is picked up soon.
struct Session {
    process: Arc<Handle>,
    view: StateView,
    born: Instant,
    // The agent's work event, opened on first use (`None` when the agent has none).
    work: OnceLock<Option<Handle>>,
    // The mutex guarding the worker's request slot, opened on first use.
    slot: OnceLock<Option<Handle>>,
    // The agent's frame event, opened on first use (`None` when it has none).
    frame: OnceLock<Option<Handle>>,
}

// SAFETY: the process handle works from any thread and the view is read through sequence-locked helpers, one request at a time per script.
unsafe impl Send for Session {}
unsafe impl Sync for Session {}

const SESSION_TTL: Duration = Duration::from_millis(500);

static SESSIONS: Mutex<Vec<(u32, Arc<Session>)>> = Mutex::new(Vec::new());

fn session(pid: u32) -> Result<Arc<Session>, String> {
    let mut sessions = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    sessions.retain(|(_, s)| s.born.elapsed() < SESSION_TTL);
    if let Some((_, session)) = sessions.iter().find(|(p, _)| *p == pid) {
        return Ok(session.clone());
    }
    let view = StateView::open(pid, true)?;
    let process = Handle::new(unsafe { OpenProcess(PROCESS_VM_READ, 0, pid) })
        .ok_or_else(|| format!("Cannot open client {pid} to read it"))?;
    let session = Arc::new(Session { process: Arc::new(process), view, born: Instant::now(), work: OnceLock::new(), slot: OnceLock::new(), frame: OnceLock::new() });
    sessions.push((pid, session.clone()));
    Ok(session)
}

/// Drops `pid`'s session, for after something went wrong or the agent was loaded or unloaded.
fn forget(pid: u32) {
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| *p != pid);
}

/// A client's shared state block mapped into this process, unmapped on drop; its handles borrow it.
struct StateView {
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    block: NonNull<LayoutMemory>,
    writable: bool,
    _mapping: Handle,
}

impl StateView {
    fn open(pid: u32, writable: bool) -> Result<Self, String> {
        let access = if writable { FILE_MAP_READ | FILE_MAP_WRITE } else { FILE_MAP_READ };
        let name = wide(state_memory_name(pid));
        let mapping = Handle::new(unsafe { OpenFileMappingW(access, 0, name.as_ptr()) })
            .ok_or_else(|| format!("No game state in client {pid}: agent isn't loaded or is too old"))?;
        let view = unsafe { MapViewOfFile(mapping.0, access, 0, 0, LAYOUT_SIZE) };
        let Some(block) = NonNull::new(view.Value.cast::<LayoutMemory>()) else {
            return Err(format!("Cannot map the game state of client {pid}"));
        };
        Ok(Self { view, block, writable, _mapping: mapping })
    }

    /// For views opened writable.
    fn shared(&self) -> Shared<'_> {
        assert!(self.writable, "the view was opened read-only");
        // SAFETY: the agent set up the writable LAYOUT_SIZE block, mapped until `self` drops.
        unsafe { Shared::attach(self.block) }
    }

    fn layout(&self) -> LayoutView<'_> {
        // SAFETY: as in `shared`, for reading.
        unsafe { LayoutView::attach(self.block) }
    }
}

impl Drop for StateView {
    fn drop(&mut self) {
        unsafe { UnmapViewOfFile(self.view) };
    }
}

fn ask<S>(
    pid: u32,
    message: u32,
    write: impl FnOnce(Shared<'_>) -> u32,
    status: impl Fn(Shared<'_>) -> Option<(u32, S)>,
) -> Result<S, String> {
    let session = session(pid)?;
    let shared = session.view.shared();
    let sequence = write(shared);
    crate::input::post_request(pid, message).and_then(|()| {
        let start = Instant::now();
        loop {
            match status(shared) {
                Some((answered, status)) if answered == sequence => break Ok(status),
                _ if start.elapsed() >= REQUEST_TIMEOUT => {
                    forget(pid);
                    break Err("agent didn't answer: its window hook may be off".to_owned());
                }
                _ => sleep(REQUEST_POLL),
            }
        }
    })
}

pub fn set_display_name(pid: u32, name: &str) -> Result<DisplayName, String> {
    let (layout, memory) = reader(pid, 0, "")?;
    let request = state::name_request(&memory, &layout, name)?;
    let status = ask(
        pid,
        ipc::input::NAME_MESSAGE,
        |shared| shared.write_name_request(&request),
        |shared| shared.read_name_status(100).map(|s| (s.sequence, s)),
    )?;
    let real = short_string_text(&status.real)
        .filter(|real| !real.is_empty())
        .map(|real| String::from_utf8_lossy(real).replace('\u{a0}', " "));
    match status.status {
        name_status::KEPT => Ok(DisplayName::Kept { real }),
        name_status::WAITING => Ok(DisplayName::Waiting),
        name_status::OFF => Ok(DisplayName::Off),
        name_status::LONG_NAME => Err("the client keeps this player's name as a long string, which can't be replaced safely".into()),
        name_status::OTHER_THREAD => Err("the client runs its game on another thread than its window's, so the name isn't replaced".into()),
        name_status::NOT_FOUND => Err("agent didn't find the client's appearance decoder, so it can't keep the name".into()),
        other => Err(format!("unknown display name status {other}")),
    }
}

pub fn set_total_xp(pid: u32, offset: Option<i64>) -> Result<TotalXp, String> {
    let (layout, memory) = reader(pid, part::SKILLS, "the skills")?;
    let real = state::total_xp(&memory, &layout);
    let request = TotalXpRequest { enabled: offset.is_some_and(|o| o != 0) as u32, offset: offset.unwrap_or(0), ..Default::default() };
    let status = ask(
        pid,
        ipc::input::TOTAL_XP_MESSAGE,
        |shared| shared.write_total_xp_request(&request),
        |shared| shared.read_total_xp_status(100).map(|s| (s.sequence, s.status)),
    )?;
    match status {
        total_xp_status::ON => Ok(TotalXp::Fake { offset: request.offset, real }),
        total_xp_status::OFF => Ok(TotalXp::Off),
        total_xp_status::NOT_FOUND => Err("agent didn't find the client's component invalidation, so it can't fake the total XP".into()),
        total_xp_status::OTHER_THREAD => Err("the client runs its game on another thread than its window's, so the total XP isn't faked".into()),
        other => Err(format!("unknown total XP status {other}")),
    }
}

pub fn item_names(pid: u32, ids: &[i32]) -> Result<Vec<Option<String>>, String> {
    names(pid, ids, ipc::input::ITEM_NAMES_MESSAGE, |shared, batch| {
        let mut request = ItemNameRequest { count: batch.len() as u32, ..Default::default() };
        request.ids[..batch.len()].copy_from_slice(batch);
        shared.write_item_name_request(&request)
    }, |shared| shared.read_item_name_status(100))
}

pub fn game_val_names(pid: u32, kind: u32, ids: &[i32]) -> Result<Vec<Option<String>>, String> {
    names(pid, ids, ipc::input::GAME_VALS_MESSAGE, |shared, batch| {
        let mut request = GameValRequest { kind, count: batch.len() as u32, ..Default::default() };
        request.ids[..batch.len()].copy_from_slice(batch);
        shared.write_game_val_request(&request)
    }, |shared| shared.read_game_val_status(100))
}

fn names(
    pid: u32,
    ids: &[i32],
    message: u32,
    write: impl Fn(Shared<'_>, &[i32]) -> u32,
    read: impl Fn(Shared<'_>) -> Option<NamesReply>,
) -> Result<Vec<Option<String>>, String> {
    reader(pid, 0, "")?;
    let mut names = Vec::with_capacity(ids.len());
    for batch in ids.chunks(ITEM_NAME_BATCH) {
        let status = ask(pid, message, |shared| write(shared, batch), |shared| read(shared).map(|s| (s.sequence, s)))?;
        match status.status {
            item_name_status::OK => names.extend((0..status.count as usize).map(|i| {
                status.name(i).filter(|name| !name.is_empty()).map(|name| String::from_utf8_lossy(name).replace('\u{a0}', " "))
            })),
            item_name_status::NOT_FOUND => return Err("agent didn't find what it needs in the client to look these up".into()),
            item_name_status::OTHER_THREAD => return Err("the client runs its game on another thread than its window's, so nothing can be looked up".into()),
            item_name_status::TOO_MANY => return Err("too many ids in one request".into()),
            other => return Err(format!("unknown name status {other}")),
        }
    }
    Ok(names)
}

static TERRAIN: Mutex<Option<(u32, Terrain)>> = Mutex::new(None);

fn project_locally(pid: u32, points: &[projection::Point], shift: Option<[i32; 2]>) -> Result<Vec<[f32; 2]>, Failure> {
    let (layout, memory) = reader(pid, 0, "").map_err(Failure::Unavailable)?;
    let mut cache = TERRAIN.lock().unwrap_or_else(|e| e.into_inner());
    let mut terrain = cache.take().filter(|(p, _)| *p == pid).map(|(_, terrain)| terrain);
    let result = projection::project(&memory, &layout, &mut terrain, points, shift, || decorations(pid));
    *cache = terrain.map(|terrain| (pid, terrain));
    result
}

pub fn project(pid: u32, points: &[projection::Point], via: Via) -> Result<Vec<[f32; 2]>, String> {
    if via != Via::Client {
        match project_locally(pid, points, None) {
            Ok(screen) => return Ok(screen),
            Err(Failure::Unavailable(why)) if via == Via::Local => return Err(format!("local projection: {why}")),
            Err(Failure::Unavailable(_)) => {}
        }
    }
    let plain: Vec<[i32; 3]> = points.iter().map(|p| [p.x, p.y, p.z]).collect();
    project_in_client(pid, &plain)
}

// `project` with the camera moved by `shift` first; only the local projection can move the camera.
pub fn project_moved(pid: u32, points: &[projection::Point], shift: [i32; 2]) -> Result<Vec<[f32; 2]>, String> {
    match project_locally(pid, points, Some(shift)) {
        Ok(screen) => Ok(screen),
        Err(Failure::Unavailable(why)) => Err(format!("local projection: {why}")),
    }
}

fn decorations(pid: u32) -> Vec<projection::Decoration> {
    let Ok(reply) = locs_reply(pid) else { return Vec::new() };
    reply.locs[..(reply.count as usize).min(reply.locs.len())]
        .iter()
        .filter(|l| l.kind == projection::GROUND_DECORATION && l.decoration_height != 0)
        .map(|l| ((reply.plane, l.x as i32, l.y as i32), l.decoration_height as i32))
        .collect()
}

fn project_in_client(pid: u32, points: &[[i32; 3]]) -> Result<Vec<[f32; 2]>, String> {
    reader(pid, 0, "")?;
    let mut screen = Vec::with_capacity(points.len());
    for batch in points.chunks(PROJECT_BATCH) {
        let mut request = ProjectRequest { count: batch.len() as u32, ..Default::default() };
        request.points[..batch.len()].copy_from_slice(batch);
        let reply = ask(
            pid,
            ipc::input::PROJECT_MESSAGE,
            |shared| shared.write_project_request(&request),
            |shared| shared.read_project_reply(100).map(|r| (r.sequence, r)),
        )?;
        match reply.status {
            project_status::OK => screen.extend_from_slice(&reply.points[..reply.count as usize]),
            project_status::NOT_IN_GAME => return Err("not logged in".into()),
            project_status::NOT_FOUND => return Err("agent didn't find the client's projection".into()),
            project_status::OTHER_THREAD => return Err("the client runs its game on another thread than its window's".into()),
            project_status::TOO_MANY => return Err("too many points in one request".into()),
            other => return Err(format!("unknown projection status {other}")),
        }
    }
    Ok(screen)
}

fn model(pid: u32, request: ModelRequest) -> Result<Option<crate::EntityModel>, String> {
    reader(pid, 0, "")?;
    let reply = ask(
        pid,
        ipc::input::MODEL_MESSAGE,
        |shared| shared.write_model_request(&request),
        |shared| shared.read_model_reply(100).map(|r| (r.sequence, r)),
    )?;
    match reply.status {
        model_status::OK => Ok(Some(crate::EntityModel {
            bounding_box: reply.bounding_box != 0,
            orientation: reply.orientation,
            vertex_count: reply.vertex_count,
            corners: reply.corners[..(reply.corner_count as usize).min(reply.corners.len())].to_vec(),
            hull: reply.hull[..(reply.hull_count as usize).min(reply.hull.len())].to_vec(),
            face_count: reply.face_count,
            faces: reply.faces[..(reply.face_count as usize).min(reply.faces.len())].to_vec(),
        })),
        model_status::NO_NPC | model_status::NO_MODEL => Ok(None),
        model_status::NOT_IN_GAME => Err("not logged in".into()),
        model_status::NOT_FOUND => Err("agent didn't find the client's models".into()),
        model_status::OTHER_THREAD => Err("the client runs its game on another thread than its window's".into()),
        other => Err(format!("unknown model status {other}")),
    }
}

pub fn npc_model(pid: u32, npc_index: i32) -> Result<Option<crate::EntityModel>, String> {
    model(pid, ModelRequest { index: npc_index, target: model_target::NPC, ..Default::default() })
}

pub fn player_model(pid: u32, player_index: i32) -> Result<Option<crate::EntityModel>, String> {
    model(pid, ModelRequest { index: player_index, target: model_target::PLAYER, ..Default::default() })
}

pub fn loc_model(pid: u32, kind: u8, scene_x: i32, scene_y: i32, id: i32) -> Result<Option<crate::EntityModel>, String> {
    model(pid, ModelRequest { target: model_target::LOC, loc_kind: kind as u32, loc_x: scene_x, loc_y: scene_y, loc_id: id, ..Default::default() })
}

fn locs_reply(pid: u32) -> Result<Box<LocsReply>, String> {
    reader(pid, 0, "")?;
    let reply = ask(
        pid,
        ipc::input::LOCS_MESSAGE,
        |shared| shared.write_locs_request(),
        |shared| {
            let mut reply: Box<LocsReply> = unsafe { Box::new_zeroed().assume_init() };
            shared.read_locs_reply_into(&mut reply, 100).then_some((reply.sequence, reply))
        },
    )?;
    match reply.status {
        loc_status::OK => {}
        loc_status::NOT_IN_GAME => return Err("not logged in".into()),
        loc_status::NOT_FOUND => return Err("agent didn't find the client's scene".into()),
        loc_status::OTHER_THREAD => return Err("the client runs its game on another thread than its window's".into()),
        other => return Err(format!("unknown locs status {other}")),
    }
    Ok(reply)
}

// How long `scene_tiles` reuses what it asked the agent for: locs and ground items change at most per tick.
const TICK_CACHE_TTL: Duration = Duration::from_millis(600);

type TickCache<T> = Mutex<Option<(u32, Instant, T)>>;

static LOCS_CACHE: TickCache<Arc<LocsReply>> = Mutex::new(None);
static GROUND_ITEMS_CACHE: TickCache<Arc<Vec<crate::GroundItem>>> = Mutex::new(None);

fn cached<T: Clone>(cache: &TickCache<T>, pid: u32, fetch: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached, at, value)) = cache.as_ref()
        && *cached == pid
        && at.elapsed() < TICK_CACHE_TTL
    {
        return Ok(value.clone());
    }
    let value = fetch()?;
    *cache = Some((pid, Instant::now(), value.clone()));
    Ok(value)
}

fn cached_locs_reply(pid: u32) -> Result<Arc<LocsReply>, String> {
    cached(&LOCS_CACHE, pid, || locs_reply(pid).map(Arc::from))
}

pub fn locs(pid: u32) -> Result<Vec<crate::Loc>, String> {
    let reply = locs_reply(pid)?;
    let names: Vec<String> =
        reply.names[..(reply.name_count as usize).min(reply.names.len())].iter().map(|n| String::from_utf8_lossy(ipc::state::text(n)).replace('\u{a0}', " ")).collect();
    let known = |v: u8| (v != 0xff).then_some(v);
    Ok(reply.locs[..(reply.count as usize).min(reply.locs.len())]
        .iter()
        .map(|l| crate::Loc {
            id: l.id,
            name: names.get(l.name as usize).cloned(),
            kind: l.kind,
            x: reply.base_x + l.x as i32,
            y: reply.base_y + l.y as i32,
            plane: reply.plane,
            scene_x: l.x as i32,
            scene_y: l.y as i32,
            size_x: l.size_x,
            size_y: l.size_y,
            shape: known(l.shape),
            rotation: known(l.rotation),
            clickable: l.flags & ipc::state::LOC_CLICKABLE != 0,
        })
        .collect())
}

fn cache_request(pid: u32, request: &CacheRequest) -> Result<Box<CacheReply>, String> {
    let reply = ask(
        pid,
        ipc::input::CACHE_MESSAGE,
        |shared| shared.write_cache_request(request),
        |shared| {
            let mut reply: Box<CacheReply> = unsafe { Box::new_zeroed().assume_init() };
            shared.read_cache_reply_into(&mut reply, 100).then_some((reply.sequence, reply))
        },
    )?;
    match reply.status {
        cache_status::OK => Ok(reply),
        cache_status::NOT_FOUND => Err("agent didn't find the client's cache reader".into()),
        cache_status::NO_INDEX => Err(format!("the client has no cache index {}", request.index)),
        cache_status::NO_ARCHIVE => Err(format!("cache index {} has no archive {}", request.index, request.archive)),
        cache_status::OTHER_THREAD => Err("the client runs its game on another thread than its window's".into()),
        cache_status::TOO_MANY => Err("too many files in one request".into()),
        other => Err(format!("unknown cache status {other}")),
    }
}

fn cache_ids(pid: u32, kind: u32, index: u32, archive: i32) -> Result<crate::cache::CacheIds, String> {
    reader(pid, 0, "")?;
    let reply = cache_request(pid, &CacheRequest { kind, index, archive, ..Default::default() })?;
    let ids = reply.data[..reply.used as usize].as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect();
    Ok(crate::cache::CacheIds { ids, fingerprint: reply.fingerprint })
}

pub fn cache_archives(pid: u32, index: u32) -> Result<crate::cache::CacheIds, String> {
    cache_ids(pid, cache_kind::ARCHIVES, index, 0)
}

pub fn cache_archive_files(pid: u32, index: u32, archive: i32) -> Result<crate::cache::CacheIds, String> {
    cache_ids(pid, cache_kind::ARCHIVE_FILES, index, archive)
}

pub fn cache_files(pid: u32, index: u32, files: &[[i32; 2]]) -> Result<Vec<Option<Vec<u8>>>, String> {
    cache_files_batched(pid, index, files, CACHE_BATCH)
}

fn cache_files_batched(pid: u32, index: u32, files: &[[i32; 2]], batch: usize) -> Result<Vec<Option<Vec<u8>>>, String> {
    reader(pid, 0, "")?;
    let mut out: Vec<Option<Vec<u8>>> = vec![None; files.len()];
    let mut next = 0;
    while next < files.len() {
        let batch = &files[next..(next + batch.clamp(1, CACHE_BATCH)).min(files.len())];
        let mut request = CacheRequest { kind: cache_kind::FILES, index, count: batch.len() as u32, ..Default::default() };
        request.files[..batch.len()].copy_from_slice(batch);
        let reply = cache_request(pid, &request)?;
        let mut done = 0;
        for (i, file) in reply.files[..batch.len()].iter().enumerate() {
            match file.length {
                CACHE_FILE_NO_ROOM => break,
                CACHE_FILE_MISSING => {}
                length => {
                    let start = file.offset as usize;
                    out[next + i] = reply.data.get(start..start + length as usize).map(<[u8]>::to_vec);
                }
            }
            done = i + 1;
        }
        if done == 0 {
            let [archive, file] = batch[0];
            return Err(format!("cache file {index}/{archive}/{file} is too big to be sent"));
        }
        next += done;
    }
    Ok(out)
}

fn config_files(pid: u32, archive: i32, ids: &[i32]) -> Result<Vec<Option<Vec<u8>>>, String> {
    let files: Vec<[i32; 2]> = ids.iter().map(|&id| [archive, id]).collect();
    cache_files(pid, crate::cache::CONFIG_INDEX, &files)
}

pub fn enum_def(pid: u32, id: i32) -> Result<Option<crate::config::EnumDef>, String> {
    let Some(data) = config_files(pid, crate::config::ENUM_ARCHIVE, &[id])?.pop().flatten() else { return Ok(None) };
    crate::config::decode_enum(&data).map(Some).ok_or_else(|| format!("enum {id} has an opcode this reader doesn't know"))
}

pub fn item_params(pid: u32, ids: &[i32]) -> Result<Vec<Option<std::collections::BTreeMap<u32, crate::config::Value>>>, String> {
    let files = config_files(pid, crate::config::OBJ_ARCHIVE, ids)?;
    ids.iter()
        .zip(files)
        .map(|(id, data)| match data {
            None => Ok(None),
            Some(data) => crate::config::decode_obj_params(&data).map(Some).ok_or_else(|| format!("item {id} has an opcode this reader doesn't know")),
        })
        .collect()
}

// Every varbit stored in `varp`: (id, definition, gameval name), by lowest bit.
pub fn varbits_of(pid: u32, varp: u16) -> Result<Vec<(i32, crate::config::VarbitDef, Option<String>)>, String> {
    let ids: Vec<i32> = cache_archive_files(pid, crate::cache::CONFIG_INDEX, crate::config::VARBIT_ARCHIVE)?.ids.iter().map(|&id| id as i32).collect();
    let mut found = Vec::new();
    for (&id, data) in ids.iter().zip(config_files(pid, crate::config::VARBIT_ARCHIVE, &ids)?) {
        if let Some(def) = data.as_deref().and_then(crate::config::decode_varbit).filter(|d| d.varp == varp) {
            found.push((id, def));
        }
    }
    found.sort_by_key(|(_, d)| d.lsb);
    let names = game_val_names(pid, ipc::state::game_val::VARBIT, &found.iter().map(|(id, _)| *id).collect::<Vec<_>>())?;
    Ok(found.into_iter().zip(names).map(|((id, def), name)| (id, def, name)).collect())
}

// Varbit definitions only change with game updates, so they're read from the cache once per process.
type VarbitCache = (u32, std::collections::HashMap<i32, Option<crate::config::VarbitDef>>);
static VARBITS: Mutex<Option<VarbitCache>> = Mutex::new(None);

pub fn varbit_def(pid: u32, id: i32) -> Result<Option<crate::config::VarbitDef>, String> {
    {
        let cached = VARBITS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(def) = cached.as_ref().filter(|(p, _)| *p == pid).and_then(|(_, defs)| defs.get(&id)) {
            return Ok(*def);
        }
    }
    let def = match config_files(pid, crate::config::VARBIT_ARCHIVE, &[id])?.pop().flatten() {
        None => None,
        Some(data) => Some(crate::config::decode_varbit(&data).ok_or_else(|| format!("varbit {id} has an opcode this reader doesn't know"))?),
    };
    let mut cached = VARBITS.lock().unwrap_or_else(|e| e.into_inner());
    if cached.as_ref().is_none_or(|(p, _)| *p != pid) {
        *cached = Some((pid, std::collections::HashMap::new()));
    }
    if let Some((_, defs)) = cached.as_mut() {
        defs.insert(id, def);
    }
    Ok(def)
}

pub fn varbit(pid: u32, id: i32) -> Result<Option<i32>, String> {
    let Some(def) = varbit_def(pid, id)? else { return Err(format!("the cache has no varbit {id}")) };
    Ok(varp_value(pid, def.varp as usize)?.map(|varp| def.value(varp)))
}

type LocTexts = (u32, std::collections::HashMap<u32, Option<(crate::collision::LocDef, crate::collision::LocText)>>);
// Loc definitions only change with game updates, so each is read from the cache once per process.
static LOC_TEXTS: Mutex<Option<LocTexts>> = Mutex::new(None);

const CACHE_LOCS_MAX_SQUARES: i32 = 64;

/// The locs the cache places on the map inside `area` (`[x1, y1, x2, y2]`, both corners included), loaded or not, with their names and options.
pub fn cache_locs(pid: u32, area: [i32; 4], filter: &crate::CacheLocFilter) -> Result<Vec<crate::CacheLoc>, String> {
    use crate::{cache::*, collision::*};
    let [x1, y1, x2, y2] = area;
    if x1 > x2 || y1 > y2 || x1 < 0 || y1 < 0 {
        return Err(format!("{x1}, {y1} to {x2}, {y2} isn't an area"));
    }
    let squares = ((x2 >> 6) - (x1 >> 6) + 1) * ((y2 >> 6) - (y1 >> 6) + 1);
    if squares > CACHE_LOCS_MAX_SQUARES {
        return Err(format!("the area covers {squares} map squares, at most {CACHE_LOCS_MAX_SQUARES}"));
    }
    let mut regions = Vec::new();
    for rx in (x1 >> 6)..=(x2 >> 6) {
        for ry in (y1 >> 6)..=(y2 >> 6) {
            regions.push((rx, ry));
        }
    }
    let files: Vec<[i32; 2]> = regions.iter().map(|&(rx, ry)| [region_id(rx, ry) as i32, 1]).collect();
    let mut found = Vec::new();
    for (data, &(rx, ry)) in cache_files(pid, MAPS_INDEX, &files)?.into_iter().zip(&regions) {
        let Some(data) = data else { continue };
        let placements = decode_locs(&data).ok_or_else(|| format!("map square {}'s locs don't decode", region_id(rx, ry)))?;
        for p in placements {
            let (x, y, plane) = (rx * 64 + p.x as i32, ry * 64 + p.y as i32, p.plane as i32);
            if x >= x1 && x <= x2 && y >= y1 && y <= y2 && filter.plane.is_none_or(|want| want == plane) {
                found.push((p, x, y));
            }
        }
    }

    let mut ids: Vec<u32> = found.iter().map(|(p, _, _)| p.id).collect();
    ids.sort_unstable();
    ids.dedup();
    let missing: Vec<u32> = {
        let cached = LOC_TEXTS.lock().unwrap_or_else(|e| e.into_inner());
        let known = cached.as_ref().filter(|(p, _)| *p == pid).map(|(_, defs)| defs);
        ids.iter().copied().filter(|id| !known.is_some_and(|defs| defs.contains_key(id))).collect()
    };
    if !missing.is_empty() {
        let files: Vec<[i32; 2]> = missing.iter().map(|&id| [LOC_ARCHIVE, id as i32]).collect();
        let defs = cache_files(pid, CONFIG_INDEX, &files)?;
        let mut cached = LOC_TEXTS.lock().unwrap_or_else(|e| e.into_inner());
        if cached.as_ref().is_none_or(|(p, _)| *p != pid) {
            *cached = Some((pid, std::collections::HashMap::new()));
        }
        if let Some((_, known)) = cached.as_mut() {
            for (id, data) in missing.iter().zip(defs) {
                known.insert(*id, data.as_deref().and_then(decode_loc_text));
            }
        }
    }

    let lower = |list: &Option<Vec<String>>| list.as_ref().map(|l| l.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>());
    let (names, actions) = (lower(&filter.names), lower(&filter.actions));
    let cached = LOC_TEXTS.lock().unwrap_or_else(|e| e.into_inner());
    let known = cached.as_ref().filter(|(p, _)| *p == pid).map(|(_, defs)| defs);
    let mut out = Vec::new();
    for (p, x, y) in found {
        let Some((def, text)) = known.and_then(|defs| defs.get(&p.id)).and_then(|d| d.as_ref()) else { continue };
        let name = text.name.to_lowercase();
        if names.as_ref().is_some_and(|names| !names.iter().any(|n| name.contains(n.as_str()))) {
            continue;
        }
        if actions.as_ref().is_some_and(|wanted| !text.actions.iter().any(|a| wanted.contains(&a.to_lowercase()))) {
            continue;
        }
        let (size_x, size_y) = if p.rotation & 1 == 1 { (def.size_y, def.size_x) } else { (def.size_x, def.size_y) };
        out.push(crate::CacheLoc {
            id: p.id as i32,
            name: text.name.clone(),
            x,
            y,
            plane: p.plane as i32,
            size_x,
            size_y,
            shape: p.shape,
            rotation: p.rotation,
            actions: text.actions.iter().filter(|a| !a.is_empty()).cloned().collect(),
        });
    }
    out.sort_by_key(|l| (l.plane, l.x, l.y, l.id));
    Ok(out)
}

pub fn world_collision(pid: u32, regions: &[u32]) -> Result<crate::collision::WorldCollision, String> {
    use crate::{cache::*, collision::*};
    let files: Vec<[i32; 2]> = regions.iter().flat_map(|&r| [[r as i32, 0], [r as i32, 1]]).collect();
    let data = cache_files(pid, MAPS_INDEX, &files)?;
    let mut squares = Vec::new();
    for (pair, &region) in data.as_chunks::<2>().0.iter().zip(regions) {
        let (Some(terrain), Some(locs)) = (&pair[0], &pair[1]) else { continue };
        let settings = decode_terrain(terrain).ok_or_else(|| format!("map square {region}'s terrain doesn't decode"))?;
        let locs = decode_locs(locs).ok_or_else(|| format!("map square {region}'s locs don't decode"))?;
        squares.push((region, settings, locs));
    }
    let mut ids: Vec<u32> = squares.iter().flat_map(|(_, _, locs)| locs.iter().map(|l| l.id)).collect();
    ids.sort_unstable();
    ids.dedup();
    let files: Vec<[i32; 2]> = ids.iter().map(|&id| [LOC_ARCHIVE, id as i32]).collect();
    let defs: std::collections::HashMap<u32, LocDef> = ids
        .iter()
        .zip(cache_files(pid, CONFIG_INDEX, &files)?)
        .filter_map(|(&id, data)| Some((id, decode_loc(&data?)?)))
        .collect();
    let mut world = WorldCollision::default();
    for (region, _, _) in &squares {
        world.add_square(*region);
    }
    for (region, settings, locs) in &squares {
        world.add_terrain_and_locs(*region, settings, locs, &|id| defs.get(&id).copied());
    }
    Ok(world)
}

const WORLD_SQUARE_BATCH: usize = 16;
const WORLD_FORMAT: u64 = 2;

// The key it was built for, and the process whose cache that key was read from: the cache only changes with a game update, so later calls for that process skip reading its indexes.
static WORLD: Mutex<Option<(u64, u32, std::sync::Arc<crate::collision::WorldCollision<crate::collision::Movement>>)>> = Mutex::new(None);

fn world_cache_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir).join("injector")
}

pub fn world_movement(pid: u32) -> Result<std::sync::Arc<crate::collision::WorldCollision<crate::collision::Movement>>, String> {
    use crate::{cache::*, collision::*};
    if let Some((_, _, world)) = WORLD.lock().unwrap_or_else(|e| e.into_inner()).as_ref().filter(|(_, p, _)| *p == pid) {
        return Ok(world.clone());
    }
    let maps = cache_archives(pid, MAPS_INDEX)?;
    let loc_ids = cache_archive_files(pid, CONFIG_INDEX, LOC_ARCHIVE)?;
    let key = maps.fingerprint ^ loc_ids.fingerprint.rotate_left(17) ^ WORLD_FORMAT.rotate_left(42);
    let mut cached = WORLD.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, _, world)) = cached.as_ref().filter(|(k, _, _)| *k == key) {
        let world = world.clone();
        *cached = Some((key, pid, world.clone()));
        return Ok(world);
    }
    let path = world_cache_dir().join(format!("collision-{key:016x}.bin"));
    if let Some(world) = std::fs::read(&path).ok().and_then(|bytes| WorldCollision::from_bytes(&bytes, key)) {
        let world = std::sync::Arc::new(world);
        *cached = Some((key, pid, world.clone()));
        return Ok(world);
    }

    let started = Instant::now();
    eprintln!("building world collision from the client's cache ({} map squares, {} loc definitions)", maps.ids.len(), loc_ids.ids.len());
    let files: Vec<[i32; 2]> = loc_ids.ids.iter().map(|&id| [LOC_ARCHIVE, id as i32]).collect();
    let mut defs = vec![None; loc_ids.ids.iter().max().map_or(0, |&m| m as usize + 1)];
    let mut undecoded = Vec::new();
    for (&id, data) in loc_ids.ids.iter().zip(cache_files(pid, CONFIG_INDEX, &files)?) {
        match data.as_deref().map(decode_loc) {
            Some(Some(def)) => defs[id as usize] = Some(def),
            Some(None) => undecoded.push(id),
            None => {}
        }
    }
    if !undecoded.is_empty() {
        eprintln!("{} loc definitions didn't decode (left out): {:?}", undecoded.len(), &undecoded[..undecoded.len().min(20)]);
    }
    let def = |id: u32| defs.get(id as usize).copied().flatten();

    let mut world = WorldCollision::<Movement>::default();
    for &region in &maps.ids {
        world.add_square(region);
    }
    for (done, regions) in maps.ids.chunks(WORLD_SQUARE_BATCH).enumerate() {
        let files: Vec<[i32; 2]> = regions.iter().flat_map(|&r| [[r as i32, 0], [r as i32, 1]]).collect();
        let data = cache_files_batched(pid, MAPS_INDEX, &files, files.len())?;
        for (pair, &region) in data.as_chunks::<2>().0.iter().zip(regions) {
            let Some(settings) = pair[0].as_deref().and_then(decode_terrain) else {
                // Not a map square (the last archive holds 3 byte placeholders).
                world.remove_square(region);
                continue;
            };
            let locs = pair[1].as_deref().map_or(Some(Vec::new()), decode_locs).unwrap_or_else(|| {
                eprintln!("map square {region}'s locs don't decode");
                Vec::new()
            });
            world.add_terrain_and_locs(region, &settings, &locs, &def);
        }
        if done % 20 == 0 {
            eprintln!("  {}/{} map squares", (done + 1) * WORLD_SQUARE_BATCH, maps.ids.len());
        }
    }
    world.shrink();
    let bytes = world.to_bytes(key);
    if let Err(e) = std::fs::create_dir_all(world_cache_dir()).and_then(|()| std::fs::write(&path, &bytes)) {
        eprintln!("couldn't keep the world collision in {}: {e}", path.display());
    }
    eprintln!("world collision built in {:.1} s ({} KB on disk)", started.elapsed().as_secs_f32(), bytes.len() / 1024);
    let world = std::sync::Arc::new(world);
    *cached = Some((key, pid, world.clone()));
    Ok(world)
}

/// `max_tiles` caps how many tiles the search looks at (default `PATH_MAX_TILES`): a target that can't be reached would otherwise search every tile that can.
pub fn find_path(pid: u32, plane: i32, from: [i32; 2], to: [i32; 2], max_tiles: Option<usize>) -> Result<Vec<[i32; 2]>, String> {
    use crate::pathfinding::{self, PathError};
    let world = world_movement(pid)?;
    match pathfinding::find_path(&world, plane, (from[0], from[1]), (to[0], to[1]), max_tiles.unwrap_or(PATH_MAX_TILES).min(PATH_MAX_TILES)) {
        Ok(path) => Ok(path.into_iter().map(|(x, y)| [x, y]).collect()),
        Err(PathError::Blocked((x, y))) => Err(format!("{x}, {y} (plane {plane}) can't be stood on")),
        Err(PathError::NoPath) => Err(format!("no walking path from {}, {} to {}, {} on plane {plane}", from[0], from[1], to[0], to[1])),
        Err(PathError::TooFar(n)) => Err(format!("no walking path found within {n} tiles")),
    }
}

const PATH_MAX_TILES: usize = 4_000_000;

// The cache's decoded collision of the map squares a route past the scene needs, plus the scenes seen lately (newest first) as a better guess than the cache.
static SEEN_SCENES: Mutex<Vec<Arc<crate::scene_collision::SceneCollision>>> = Mutex::new(Vec::new());
const SEEN_SCENES_KEPT: usize = 4;

static REGION_FLAGS: Mutex<Vec<(u32, Arc<crate::collision::WorldCollision>)>> = Mutex::new(Vec::new());

fn cache_regions(pid: u32, (x1, y1, x2, y2): (i32, i32, i32, i32)) -> Result<Vec<(u32, Arc<crate::collision::WorldCollision>)>, String> {
    use crate::collision::{region_id, REGION_SIZE};
    let wanted: Vec<u32> = (x1.div_euclid(REGION_SIZE)..=x2.div_euclid(REGION_SIZE))
        .flat_map(|rx| (y1.div_euclid(REGION_SIZE)..=y2.div_euclid(REGION_SIZE)).map(move |ry| region_id(rx, ry)))
        .collect();
    let mut cached = REGION_FLAGS.lock().unwrap_or_else(|e| e.into_inner());
    let missing: Vec<u32> = wanted.iter().copied().filter(|r| !cached.iter().any(|(c, _)| c == r)).collect();
    if !missing.is_empty() {
        let world = Arc::new(world_collision(pid, &missing)?);
        cached.extend(missing.into_iter().map(|r| (r, world.clone())));
    }
    Ok(cached.iter().filter(|(r, _)| wanted.contains(r)).cloned().collect())
}

// The route the game walks for a click on `to` over the client's collision map; with `options` it is randomized over a lattice (see `scene_graph`).
pub fn scene_route(pid: u32, from: [i32; 2], to: [i32; 2], options: Option<crate::scene_graph::Options>) -> Result<crate::pathfinding::Route, String> {
    use crate::collision::{region_id, REGION_SIZE};
    // a tile of the scene's edge ring is all ones
    const RING: u32 = 0xFF_FFFF;
    // the largest box a lattice is built over
    const MAX_WINDOW: i32 = 400;
    // a scene is this many tiles wide, and this many of its outermost tiles aren't trusted
    const SCENE_SIZE: i32 = 104;
    const SCENE_EDGE: i32 = 4;
    let (layout, memory) = reader(pid, part::COLLISION, "the scene's collision maps")?;
    if state::current(&memory, &layout) != Some(LOGGED_IN) {
        return Err("not logged in".into());
    }
    let scene = crate::scene_collision::read(&memory, &layout).ok_or("the scene's collision map can't be read")?;
    if scene.flags(from[0], from[1]).is_none() {
        return Err(format!("{}, {} isn't in the loaded scene", from[0], from[1]));
    }
    let (from, to) = ((from[0], from[1]), (to[0], to[1]));
    let seen: Vec<Arc<crate::scene_collision::SceneCollision>> = if options.is_some() {
        let mut seen = SEEN_SCENES.lock().unwrap_or_else(|e| e.into_inner());
        seen.retain(|s| (s.base_x, s.base_y, s.plane) != (scene.base_x, scene.base_y, scene.plane));
        seen.insert(0, Arc::new(scene.clone()));
        seen.truncate(SEEN_SCENES_KEPT);
        seen.iter().skip(1).filter(|s| s.plane == scene.plane).cloned().collect()
    } else {
        Vec::new()
    };
    let route = match options {
        None => crate::pathfinding::game_route(|x, y| scene.flags(x, y), from, to),
        Some(options) => {
            // the lattice is over the scene when `to` is in it, else over the map squares around both ends, so it is stable for nearby targets
            let (x1, y1, x2, y2) = scene.bounds();
            let bounds = if scene.flags(to.0, to.1).is_some_and(|f| f != RING) {
                (x1, y1, x2, y2)
            } else {
                let (x1, y1, x2, y2) = (x1.min(to.0 - 12), y1.min(to.1 - 12), x2.max(to.0 + 12), y2.max(to.1 + 12));
                let square = |v: i32| v.div_euclid(REGION_SIZE) * REGION_SIZE;
                (square(x1), square(y1), square(x2) + REGION_SIZE - 1, square(y2) + REGION_SIZE - 1)
            };
            if bounds.2 - bounds.0 >= MAX_WINDOW || bounds.3 - bounds.1 >= MAX_WINDOW {
                return Err(format!("{}, {} is too far from {}, {} for a route", to.0, to.1, from.0, from.1));
            }
            let regions = cache_regions(pid, bounds)?;
            let plane = scene.plane;
            // The client's own tiles first (this scene's, then recent ones) where complete: a scene's outermost tiles miss the ground beyond.
            let live = |s: &crate::scene_collision::SceneCollision, x: i32, y: i32| {
                let inner = |v: i32, base: i32| (base + SCENE_EDGE..base + SCENE_SIZE - SCENE_EDGE).contains(&v);
                (inner(x, s.base_x) && inner(y, s.base_y)).then(|| s.flags(x, y)).flatten().filter(|&f| f != RING)
            };
            let flags = |x: i32, y: i32| {
                live(&scene, x, y).or_else(|| seen.iter().find_map(|s| live(s, x, y))).or_else(|| {
                    let region = region_id(x.div_euclid(REGION_SIZE), y.div_euclid(REGION_SIZE));
                    regions.iter().find(|(r, _)| *r == region).and_then(|(_, world)| world.flags(plane, x, y))
                })
            };
            crate::scene_graph::route(&flags, bounds, from, to, &options)
        }
    };
    route.ok_or_else(|| format!("no route from {}, {} to {}, {} or near it", from.0, from.1, to.0, to.1))
}

// The client's collision flags of the scene: `flags[x][y]` is world tile (x1 + x, y1 + y), edge ring included.
pub type SceneCollision = (i32, i32, i32, Vec<Vec<u32>>);

pub fn scene_collision(pid: u32) -> Result<SceneCollision, String> {
    let (layout, memory) = reader(pid, part::COLLISION, "the scene's collision maps")?;
    let scene = crate::scene_collision::read(&memory, &layout).ok_or("the scene's collision map can't be read")?;
    let (x1, y1, x2, y2) = scene.bounds();
    let flags = (x1..=x2).map(|x| (y1..=y2).map(|y| scene.flags(x, y).unwrap_or(u32::MAX)).collect()).collect();
    Ok((x1, y1, scene.plane, flags))
}

pub fn player_plane(pid: u32) -> Result<i32, String> {
    let (layout, memory) = reader(pid, part::PROJECTION, "the scene")?;
    state::game_state(&memory, &layout).position.and_then(|p| p.plane).ok_or_else(|| "not logged in".into())
}

pub fn map_regions(pid: u32) -> Result<Vec<[i32; 2]>, String> {
    let (layout, memory) = reader(pid, part::PLAYER, "the scene")?;
    ipc::read::map_regions(&memory, &layout).ok_or_else(|| "the scene's map squares can't be found".into())
}

pub fn in_instance(pid: u32) -> Result<bool, String> {
    let (layout, memory) = reader(pid, part::PLAYER | part::SCENE_LOADER, "the scene loader")?;
    ipc::read::in_instance(&memory, &layout).ok_or_else(|| "the client's instance flag can't be read".into())
}

pub fn collision_area(pid: u32, plane: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> Result<Vec<Vec<Option<u32>>>, String> {
    use crate::collision::{region_id, REGION_SIZE};
    let mut regions = Vec::new();
    for rx in (x1 / REGION_SIZE - 1)..=(x2 / REGION_SIZE + 1) {
        for ry in (y1 / REGION_SIZE - 1)..=(y2 / REGION_SIZE + 1) {
            if rx >= 0 && ry >= 0 {
                regions.push(region_id(rx, ry));
            }
        }
    }
    let world = world_collision(pid, &regions)?;
    Ok((x1..=x2).map(|x| (y1..=y2).map(|y| world.flags(plane, x, y)).collect()).collect())
}

fn menu_entry(e: &ipc::state::MenuEntryInfo) -> crate::MenuEntry {
    let raw_target = String::from_utf8_lossy(ipc::state::text(&e.target)).into_owned();
    crate::MenuEntry {
        option: crate::strip_tags(&String::from_utf8_lossy(ipc::state::text(&e.option))),
        target: crate::strip_tags(&raw_target),
        raw_target,
        id: e.id,
        index: e.index,
        action: e.action,
        priority: e.priority,
        kind: e.kind,
        submenu: e.submenu != 0,
    }
}

fn menu_reply(pid: u32) -> Result<ipc::state::MenuReply, String> {
    reader(pid, 0, "")?;
    let reply = ask(
        pid,
        ipc::input::MENU_MESSAGE,
        |shared| shared.write_menu_request(),
        |shared| shared.read_menu_reply(100).map(|r| (r.sequence, r)),
    )?;
    match reply.status {
        menu_status::OK => Ok(reply),
        menu_status::NOT_FOUND => Err("agent didn't find the client's menu".into()),
        menu_status::OTHER_THREAD => Err("the client runs its game on another thread than its window's".into()),
        other => Err(format!("unknown menu status {other}")),
    }
}

pub fn menu(pid: u32) -> Result<Vec<crate::MenuEntry>, String> {
    let reply = menu_reply(pid)?;
    Ok(reply.entries[..reply.count as usize].iter().map(menu_entry).collect())
}

/// Whether the client's choose-option menu is open (the agent reads the flag from the menu object).
pub fn menu_open(pid: u32) -> Result<bool, String> {
    let reply = menu_reply(pid)?;
    match reply.open {
        ipc::state::menu_open::CLOSED => Ok(false),
        ipc::state::menu_open::OPEN => Ok(true),
        _ => Err("agent doesn't know where the client keeps the menu's open flag".into()),
    }
}

/// The open menu as the client lays it out, or `None` while it is closed.
pub fn menu_layout(pid: u32) -> Result<Option<crate::MenuLayout>, String> {
    let reply = menu_reply(pid)?;
    match reply.open {
        ipc::state::menu_open::CLOSED => return Ok(None),
        ipc::state::menu_open::OPEN => {}
        _ => return Err("agent doesn't know where the client keeps the menu's open flag".into()),
    }
    if reply.has_geometry == 0 {
        return Err("agent doesn't know how the client lays out the menu".into());
    }
    let g = reply.geometry;
    let count = (reply.submenu_count as usize).min(reply.submenus.len());
    let submenus = reply.submenus[..count]
        .iter()
        .map(|sub| {
            let s = sub.geometry;
            crate::MenuSubmenu {
                row: sub.row + 1,
                entries: sub.entries[..(sub.count as usize).min(sub.entries.len())].iter().map(menu_entry).collect(),
                x: s.x,
                y: s.y,
                width: s.width,
                height: s.height,
                rows: crate::MenuRect { x: s.rows_x, y: s.rows_y, width: s.rows_width, height: s.rows_height },
                header_height: s.header_height,
                row_height: s.row_height,
                scroll: s.scroll,
                margin: s.margin,
            }
        })
        .collect();
    Ok(Some(crate::MenuLayout {
        x: g.x,
        y: g.y,
        width: g.width,
        height: g.height,
        rows: crate::MenuRect { x: g.rows_x, y: g.rows_y, width: g.rows_width, height: g.rows_height },
        header_height: g.header_height,
        row_height: g.row_height,
        scroll: g.scroll,
        margin: g.margin,
        submenus,
    }))
}

pub fn menu_click(pid: u32) -> Result<Option<crate::MenuClick>, String> {
    let reply = menu_reply(pid)?;
    Ok((reply.has_click != 0).then(|| crate::MenuClick { entry: menu_entry(&reply.clicked), x: reply.click_x, y: reply.click_y, age: reply.click_age }))
}

pub fn selected_widget(pid: u32) -> Result<Option<crate::SelectedWidget>, String> {
    let reply = menu_reply(pid)?;
    if reply.has_selection == 0 {
        return Err("agent doesn't know where the client keeps the selected component".into());
    }
    if reply.selected == 0 {
        return Ok(None);
    }
    let e = &reply.selection;
    let raw_name = String::from_utf8_lossy(ipc::state::text(&e.target)).into_owned();
    Ok(Some(crate::SelectedWidget {
        verb: crate::strip_tags(&String::from_utf8_lossy(ipc::state::text(&e.option))),
        name: crate::strip_tags(&raw_name),
        raw_name,
        id: e.id,
        group: e.id >> 16,
        child: e.id & 0xffff,
        index: e.index,
        item: e.action,
        targets: e.kind,
    }))
}

pub fn sub_interfaces(pid: u32) -> Result<Option<Vec<SubInterface>>, String> {
    if let Some(interfaces) = ask_worker(pid, worker_kind::SUB_INTERFACES, [0; 4]) {
        return Ok(interfaces);
    }
    let (layout, memory) = reader(pid, part::INTERFACES, "the interfaces")?;
    Ok(worker::sub_interfaces(&memory, &layout))
}

pub fn widget(pid: u32, group: u16, child: u16) -> Result<Option<Widget>, String> {
    if let Some(widget) = ask_worker(pid, worker_kind::WIDGET, [group as i32, child as i32, 0, 0]) {
        return Ok(widget);
    }
    let (layout, memory) = reader(pid, part::INTERFACES, "the interfaces")?;
    Ok(worker::widget(&memory, &layout, group, child))
}

pub fn widget_children(pid: u32, group: u16, child: u16) -> Result<Option<Vec<Widget>>, String> {
    if let Some(children) = ask_worker(pid, worker_kind::WIDGET_CHILDREN, [group as i32, child as i32, 0, 0]) {
        return Ok(children);
    }
    let (layout, memory) = reader(pid, part::INTERFACES, "the interfaces")?;
    Ok(worker::widget_children(&memory, &layout, group, child))
}

fn world_reader(pid: u32) -> Result<(Layout, ProcessMemory), String> {
    reader(pid, part::WORLD, "where items, NPCs and players are")
}

pub fn item_container(pid: u32, id: i32) -> Result<Option<Vec<Item>>, String> {
    if let Some(items) = ask_worker(pid, worker_kind::ITEM_CONTAINER, [id, 0, 0, 0]) {
        return Ok(items);
    }
    let (layout, memory) = world_reader(pid)?;
    Ok(worker::item_container(&memory, &layout, id))
}

pub fn varc(pid: u32, id: i32) -> Result<Option<i32>, String> {
    if let Some(value) = ask_worker(pid, worker_kind::VARC, [id, 0, 0, 0]) {
        return Ok(value);
    }
    let (layout, memory) = world_reader(pid)?;
    Ok(worker::varc(&memory, &layout, id))
}

pub fn chat_messages(pid: u32) -> Result<Vec<crate::ChatMessage>, String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
    let messages = match ask_worker(pid, worker_kind::CHAT, [now as i32, (now >> 32) as i32, 0, 0]) {
        Some(messages) => messages,
        None => {
            let (layout, memory) = reader(pid, part::CHAT, "the chat history")?;
            worker::chat(&memory, &layout, now)
        }
    };
    messages.ok_or_else(|| format!("Cannot read the chat history of client {pid}"))
}

// Item names don't change while the client runs, so each found name is looked up once and kept.
static ITEM_NAMES: Mutex<Vec<(i32, String)>> = Mutex::new(Vec::new());

pub fn ground_items(pid: u32) -> Result<Vec<crate::GroundItem>, String> {
    let mut items = match ask_worker(pid, worker_kind::GROUND_ITEMS, [0; 4]) {
        Some(items) => items,
        None => {
            let (layout, memory) = reader(pid, worker::GROUND_ITEM_PARTS, "where ground items are")?;
            worker::ground_items(&memory, &layout)
        }
    };
    let mut ids: Vec<i32> = items.iter().map(|i| i.id).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut known = ITEM_NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let missing: Vec<i32> = ids.iter().copied().filter(|id| !known.iter().any(|(k, _)| k == id)).collect();
    if !missing.is_empty() {
        drop(known);
        let found = item_names(pid, &missing)?;
        known = ITEM_NAMES.lock().unwrap_or_else(|e| e.into_inner());
        for (id, name) in missing.into_iter().zip(found) {
            if let Some(name) = name {
                known.push((id, name));
            }
        }
    }
    for item in &mut items {
        item.name = known.iter().find(|(id, _)| *id == item.id).map(|(_, name)| name.clone());
    }
    Ok(items)
}

pub fn npcs(pid: u32) -> Result<Vec<Npc>, String> {
    if let Some(npcs) = ask_worker(pid, worker_kind::NPCS, [0; 4]) {
        return Ok(npcs);
    }
    let (layout, memory) = world_reader(pid)?;
    Ok(worker::npcs(&memory, &layout))
}

// How long to yield while the worker answers, then sleep between checks, and how long a request may take before the caller reads the memory itself.
const WORKER_SPIN: Duration = Duration::from_micros(500);
const WORKER_TIMEOUT: Duration = Duration::from_millis(50);

thread_local! {
    static WORKER_BYTES: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
}

// Holds the worker's slot (a mutex shared with every injector process on this client) until dropped.
struct SlotGuard(HANDLE);

impl Drop for SlotGuard {
    fn drop(&mut self) {
        unsafe { ReleaseMutex(self.0) };
    }
}

// Asks the agent's worker to read inside the client in one request; `None` when it can't answer, and the caller reads the memory itself.
fn ask_worker<T: serde::de::DeserializeOwned>(pid: u32, kind: u32, args: [i32; 4]) -> Option<T> {
    // VOLCREN_NO_WORKER forces the injector's own reads, to compare the two paths.
    if std::env::var_os("VOLCREN_NO_WORKER").is_some() {
        return None;
    }
    let start = Instant::now();
    let session = session(pid).ok()?;
    let event = session
        .work
        .get_or_init(|| {
            let name = wide(work_event_name(pid));
            Handle::new(unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) })
        })
        .as_ref()?;
    let mutex = session
        .slot
        .get_or_init(|| {
            let name = wide(worker_mutex_name(pid));
            Handle::new(unsafe { CreateMutexW(null(), 0, name.as_ptr()) })
        })
        .as_ref()?;
    let wait = WORKER_TIMEOUT.as_millis() as u32;
    // An abandoned mutex (its owner died with it) is ours all the same; the slot is rewritten below.
    if !matches!(unsafe { WaitForSingleObject(mutex.0, wait) }, WAIT_OBJECT_0 | WAIT_ABANDONED) {
        return None;
    }
    let _slot = SlotGuard(mutex.0);

    let shared = session.view.shared();
    let sequence = shared.write_worker_request(&WorkerRequest { kind, args, reserved: 0 });
    if unsafe { SetEvent(event.0) } == 0 {
        return None;
    }
    WORKER_BYTES.with(|bytes| {
        let mut bytes = bytes.borrow_mut();
        let status = loop {
            if shared.worker_reply_answers() == Some(sequence)
                && let Some(status) = shared.read_worker_reply(sequence, &mut bytes)
            {
                break status;
            }
            let waited = start.elapsed();
            if waited >= WORKER_TIMEOUT {
                return None;
            }
            if waited < WORKER_SPIN {
                std::thread::yield_now();
            } else {
                sleep(Duration::from_micros(100));
            }
        };
        if status != worker_status::OK {
            return None;
        }
        decode::postcard::from_bytes(&bytes).ok()
    })
}

// Debug: the words of an NPC's actor, to find fields (see `world::npc_words`).
pub fn npc_words(pid: u32, index: i32, count: usize) -> Result<(u64, Vec<i32>), String> {
    let (layout, memory) = world_reader(pid)?;
    world::npc_words(&memory, &layout.world, index, count).ok_or_else(|| format!("no NPC {index} in the scene"))
}

pub fn players(pid: u32) -> Result<Vec<Player>, String> {
    if let Some(players) = ask_worker(pid, worker_kind::PLAYERS, [0; 4]) {
        return Ok(players);
    }
    let (layout, memory) = world_reader(pid)?;
    Ok(worker::players(&memory, &layout))
}

pub fn world_select(pid: u32) -> Result<Option<WorldSelect>, String> {
    if let Some(world) = ask_worker(pid, worker_kind::WORLD_SELECT, [0; 4]) {
        return Ok(world);
    }
    let (layout, memory) = reader(pid, part::WORLD_SELECT, "the world switcher")?;
    Ok(worker::world_select(&memory, &layout))
}

fn read_layout(pid: u32, session: &Session) -> Result<Layout, String> {
    match session.view.layout().read(1000) {
        Ok(layout) => Ok(layout),
        Err(ReadError::Incompatible(protocol)) => {
            forget(pid);
            Err(format!(
                "Client {pid} runs agent protocol {protocol}, this is {}: restart the client and load a matching agent",
                ipc::PROTOCOL_VERSION
            ))
        }
        Err(ReadError::Busy) => Err(format!("The game state layout of client {pid} kept changing while reading it")),
    }
}

pub fn screenshot(pid: u32, path: &Path) -> Result<(i32, i32), String> {
    let connection = connection(pid).ok_or_else(|| format!("No agent shared memory in client {pid}"))?;
    if connection.protocol != ipc::PROTOCOL_VERSION {
        return Err(format!(
            "Client {pid} runs agent protocol {}, this is {}: reload agent",
            connection.protocol,
            ipc::PROTOCOL_VERSION
        ));
    }
    let (width, height) = (connection.width, connection.height);
    let size = width.max(0) as usize * height.max(0) as usize * 4;
    if size == 0 || size > IMAGE_DATA_SIZE {
        return Err(format!("Client {pid} hasn't captured a frame yet"));
    }

    let name = wide(shared_memory_name(pid));
    let mapping = Handle::new(unsafe { OpenFileMappingW(FILE_MAP_WRITE, 0, name.as_ptr()) })
        .ok_or_else(|| format!("No agent shared memory in client {pid}"))?;
    let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, HEADER_SIZE + size) };
    if view.Value.is_null() {
        return Err(format!("Cannot map the shared memory of client {pid}"));
    }
    let base = view.Value as *mut u8;
    // The agent captures frames only while a reader asks; turn capture on long enough to get a few frames past the PBO's lag.
    let capture = unsafe { base.add(core::mem::offset_of!(Header, capture_enabled)) };
    let was_capturing = unsafe { *capture } != 0;
    if !was_capturing {
        unsafe { capture.write_volatile(1) };
        sleep(Duration::from_millis(200));
    }
    let pixels = unsafe { std::slice::from_raw_parts(base.add(HEADER_SIZE), size) }.to_vec();
    unsafe {
        if !was_capturing {
            capture.write_volatile(0);
        }
        UnmapViewOfFile(view);
    }

    let mut bmp = Vec::with_capacity(54 + size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&((54 + size) as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&54u32.to_le_bytes());
    bmp.extend_from_slice(&40u32.to_le_bytes());
    bmp.extend_from_slice(&width.to_le_bytes());
    bmp.extend_from_slice(&(-height).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes());
    bmp.extend_from_slice(&32u16.to_le_bytes());
    bmp.extend_from_slice(&[0; 24]);
    bmp.extend_from_slice(&pixels);
    fs::write(path, bmp).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok((width, height))
}

pub fn set_cursor(pid: u32, cursor: &crate::Cursor) -> Result<(), String> {
    let connection = connection(pid).ok_or_else(|| format!("No agent shared memory in client {pid}"))?;
    if connection.protocol != ipc::PROTOCOL_VERSION {
        return Err(format!(
            "Client {pid} runs agent protocol {}, this is {}: restart the client and load a matching agent",
            connection.protocol,
            ipc::PROTOCOL_VERSION
        ));
    }
    let name = wide(shared_memory_name(pid));
    let mapping = Handle::new(unsafe { OpenFileMappingW(FILE_MAP_WRITE, 0, name.as_ptr()) })
        .ok_or_else(|| format!("No agent shared memory in client {pid}"))?;
    let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, HEADER_SIZE) };
    if view.Value.is_null() {
        return Err(format!("Cannot map the shared memory of client {pid}"));
    }
    let base = view.Value as *mut u8;
    unsafe {
        base.add(core::mem::offset_of!(Header, cursor_color)).copy_from_nonoverlapping(cursor.color.as_ptr(), 3);
        base.add(core::mem::offset_of!(Header, cursor_size)).write_volatile(cursor.size);
        base.add(core::mem::offset_of!(Header, cursor_style)).write_volatile(cursor.style.value());
        UnmapViewOfFile(view);
    }
    Ok(())
}

pub fn draw(pid: u32, overlay: &Overlay) -> Result<(), String> {
    draw_batch(pid, std::slice::from_ref(overlay))
}

// Paints every shape in order as one overlay update the agent never sees half drawn; an empty batch changes nothing.
pub fn draw_batch(pid: u32, overlays: &[Overlay]) -> Result<(), String> {
    let Some(last) = overlays.last() else { return Ok(()) };
    let mode = if matches!(last, Overlay::Clear) { overlay_mode::OFF } else { overlay_mode::ON_SERIAL };
    let clears = matches!(overlays.first(), Some(Overlay::Clear));
    with_overlay(pid, mode, !clears, None, |overlay_ptr, width, height| unsafe {
        for overlay in overlays {
            match *overlay {
                Overlay::Clear => write_bytes(overlay_ptr, 0, width as usize * height as usize * 4),
                Overlay::Rect { x1, y1, x2, y2, color } => draw_rect(overlay_ptr, width, height, [x1, y1, x2, y2], color),
                Overlay::Line { x1, y1, x2, y2, color } => draw_line(overlay_ptr, width, height, [x1, y1, x2, y2], color),
            }
        }
    })
}

pub fn draw_lines(pid: u32, lines: &[[i32; 4]], color: Rgba) -> Result<(), String> {
    with_overlay(pid, overlay_mode::ON_SERIAL, true, None, |overlay_ptr, width, height| {
        for &[x1, y1, x2, y2] in lines {
            unsafe { draw_line(overlay_ptr, width, height, [x1, y1, x2, y2], color) };
        }
    })
}

// Scene tiles: devtools overlays on the ground. `scene_tiles` clears the overlay, then draws the layers named as the loader's checkboxes.
const TILE_SIZE: i32 = 128;
const TILE_HALF: i32 = TILE_SIZE / 2;
const TILE_CORNERS: [[i32; 2]; 4] = [[-TILE_HALF, -TILE_HALF], [TILE_HALF, -TILE_HALF], [TILE_HALF, TILE_HALF], [-TILE_HALF, TILE_HALF]];
const SCENE_TILES: i32 = 104;
// RuneLite's `Constants.CHUNK_SIZE` and the map square size.
const CHUNK_TILES: i32 = 8;
const MAP_SQUARE_TILES: i32 = 64;
// How far from the player the map square lines, line of sight and hover reach; the hover covers the view, about 16 tiles each side.
const MAP_SQUARE_RANGE: i32 = 16;
const LOS_RANGE: i32 = 10;
const HOVER_RANGE: i32 = 16;
// The loading lines sit this many tiles in from the scene's edge.
const LOADING_INSET: i32 = 16;
// The grid (the Grid layer): every tile within `GRID_RADIUS` of the player.
const GRID_RADIUS: i32 = 32;

// Corners further out than this are behind the camera or otherwise meaningless, so they're not drawn.
const SCREEN_LIMIT: f32 = 100_000.0;
const LABEL_SHADOW: Rgba = Rgba { r: 0, g: 0, b: 0, a: 255 };
const TEXT_COLOR: Rgba = Rgba { r: 255, g: 255, b: 255, a: 255 };
// The p12 glyphs are drawn at twice their size.
const LABEL_SCALE: i32 = 2;
// Used for an actor whose model height can't be read; labels sit this far above, like the client's NPC overlay.
const NPC_LABEL_DEFAULT_HEIGHT: i32 = 200;
const NPC_LABEL_OFFSET: i32 = 40;
// Text panels start this far in from the canvas's top left.
const TEXT_LEFT: i32 = 8;
const TEXT_TOP: i32 = 8;
const LAYERS: [&str; 18] = [
    "players",
    "npcs",
    "objects",
    "grid",
    "ground-items",
    "ground-objects",
    "walls",
    "decorations",
    "tile-flags",
    "zone-borders",
    "map-squares",
    "loading-lines",
    "line-of-sight",
    "valid-movement",
    "location",
    "camera",
    "movement-flags",
    "tile-location",
];

// RuneLite's devtools colours.
const PLAYER_TILE_COLOR: Rgba = Rgba { r: 41, g: 98, b: 255, a: 255 };
const LOCAL_PLAYER_TILE_COLOR: Rgba = Rgba { r: 0, g: 184, b: 212, a: 255 };
const NPC_TILE_COLOR: Rgba = Rgba { r: 255, g: 255, b: 0, a: 255 };
const OBJECT_TILE_COLOR: Rgba = Rgba { r: 0, g: 255, b: 0, a: 255 };
const GROUND_ITEM_COLOR: Rgba = Rgba { r: 221, g: 44, b: 0, a: 255 };
const GROUND_OBJECT_COLOR: Rgba = Rgba { r: 170, g: 0, b: 255, a: 255 };
const WALL_COLOR: Rgba = Rgba { r: 158, g: 158, b: 158, a: 255 };
const DECORATION_COLOR: Rgba = Rgba { r: 98, g: 0, b: 234, a: 255 };
const TILE_FLAG_COLOR: Rgba = Rgba { r: 221, g: 44, b: 0, a: 255 };
const CHUNK_COLOR: Rgba = Rgba { r: 0, g: 0, b: 255, a: 255 };
const MAP_SQUARE_COLOR: Rgba = Rgba { r: 0, g: 255, b: 0, a: 255 };
const LOCAL_VALID_MOVEMENT_COLOR: Rgba = Rgba { r: 141, g: 220, b: 26, a: 255 };
const LINE_OF_SIGHT_COLOR: Rgba = Rgba { r: 204, g: 42, b: 219, a: 255 };
const HOVER_COLOR: Rgba = Rgba { r: 0, g: 200, b: 83, a: 255 };
const GRID_COLOR: Rgba = Rgba { r: 255, g: 255, b: 255, a: 90 };

// The label font is decoded once and kept, since it doesn't change while the cache revision does not.
static LABEL_FONT: Mutex<Option<Arc<crate::font::Font>>> = Mutex::new(None);

fn label_font(pid: u32) -> Result<Arc<crate::font::Font>, String> {
    let mut cached = LABEL_FONT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(font) = cached.as_ref() {
        return Ok(font.clone());
    }
    let archive = |index: u32| -> Result<Vec<u8>, String> {
        cache_files(pid, index, &[[crate::font::P12_FULL, 0]])?
            .pop()
            .flatten()
            .ok_or_else(|| format!("the client has no p12_full in index {index}"))
    };
    let font = Arc::new(crate::font::Font::decode(&archive(crate::font::FONTS_INDEX)?, &archive(crate::font::SPRITES_INDEX)?)?);
    *cached = Some(font.clone());
    Ok(font)
}

// What one `scene_tiles` call draws: points are projected together, layers refer to them by index.
#[derive(Default)]
struct Plan {
    points: Vec<projection::Point>,
    paths: Vec<Stroke>,
    labels: Vec<Label>,
    texts: Vec<Text>,
}

// A line through the points, or around them when `closed`; a `marker` outline counts towards the `scene_tiles` result.
struct Stroke {
    points: Vec<usize>,
    color: Rgba,
    closed: bool,
    marker: bool,
}

// A label drawn centred above its anchor point.
struct Label {
    anchor: usize,
    text: String,
    color: Rgba,
}

// A line of text at a canvas position.
struct Text {
    x: i32,
    y: i32,
    text: String,
    color: Rgba,
}

impl Plan {
    fn point(&mut self, point: projection::Point) -> usize {
        self.points.push(point);
        self.points.len() - 1
    }

    fn path(&mut self, points: &[projection::Point], color: Rgba, closed: bool, marker: bool) {
        let points = points.iter().map(|&p| self.point(p)).collect();
        self.paths.push(Stroke { points, color, closed, marker });
    }

    fn label(&mut self, anchor: projection::Point, text: String, color: Rgba) {
        let anchor = self.point(anchor);
        self.labels.push(Label { anchor, text, color });
    }

    fn text(&mut self, x: i32, y: i32, text: String, color: Rgba) {
        self.texts.push(Text { x, y, text, color });
    }

    // A tile's outline, with its label above the tile.
    fn tile(&mut self, tile: [i32; 2], label: String, color: Rgba) {
        self.path(&tile_corners(tile), color, true, true);
        self.label(tile_centre(tile), label, color);
    }
}

// The corners of a `size_x` by `size_y` footprint from tile `(tx, ty)` in scene coordinates; far corners lie on the tiles beyond, so they name it.
fn footprint(tx: i32, ty: i32, size_x: i32, size_y: i32) -> [projection::Point; 4] {
    let (x, y, w, h) = (tx * TILE_SIZE, ty * TILE_SIZE, size_x * TILE_SIZE, size_y * TILE_SIZE);
    let corner = |x, y, tile| projection::Point { x, y, z: 0, tile: Some(tile) };
    [
        corner(x, y, [tx, ty]),
        corner(x + w, y, [tx + size_x - 1, ty]),
        corner(x + w, y + h, [tx + size_x - 1, ty + size_y - 1]),
        corner(x, y + h, [tx, ty + size_y - 1]),
    ]
}

fn tile_corners([tx, ty]: [i32; 2]) -> [projection::Point; 4] {
    footprint(tx, ty, 1, 1)
}

// The ground at the middle of a tile, where its labels go.
fn tile_centre([tx, ty]: [i32; 2]) -> projection::Point {
    projection::Point::new(tx * TILE_SIZE + TILE_HALF, ty * TILE_SIZE + TILE_HALF, 0)
}

// A point on the scene's grid lines, which names no tile: the ground is the tile beyond it, as the client draws it.
fn boundary(tx: i32, ty: i32) -> projection::Point {
    projection::Point::new(tx * TILE_SIZE, ty * TILE_SIZE, 0)
}

fn in_scene([tx, ty]: [i32; 2]) -> bool {
    (0..SCENE_TILES).contains(&tx) && (0..SCENE_TILES).contains(&ty)
}

// The world coordinates of the scene's tile (0, 0), from the player's position.
fn scene_origin(position: &Position) -> (i32, i32) {
    (position.x - (position.local_x >> 7), position.y - (position.local_y >> 7))
}

// An actor's outline on its tile with its label above the model; `height` is -1 when unknown.
fn actor_marker(plan: &mut Plan, local: (i32, i32), height: i32, label: String, color: Rgba) {
    let (local_x, local_y) = local;
    let height = if height > 0 { height } else { NPC_LABEL_DEFAULT_HEIGHT };
    let tile = Some([local_x >> 7, local_y >> 7]);
    // Heights point down in projection coordinates, so lifting a point is a negative offset.
    let lift = height + NPC_LABEL_OFFSET;
    let corners = TILE_CORNERS.map(|[dx, dy]| projection::Point { x: local_x + dx, y: local_y + dy, z: 0, tile });
    plan.path(&corners, color, true, true);
    plan.label(projection::Point::new(local_x, local_y, -lift), label, color);
}

fn npc_label(npc: &Npc) -> String {
    format!("{} (ID:{}) (A: {}) (P: {}) (G: {})", crate::strip_tags(&npc.name), npc.id, npc.animation, npc.pose, npc.graphic)
}

// Like RuneLite's player overlay: the name, then the animation, pose and graphic.
fn player_label(player: &Player) -> String {
    format!("{} (A: {}) (P: {}) (G: {})", player.name, player.animation, player.pose, player.graphic)
}

// A game object's footprint, and its label at the ground (RuneLite draws game objects at zOffset 0).
fn loc_marker(plan: &mut Plan, loc: &ipc::state::LocInfo, color: Rgba) {
    let (tx, ty) = (loc.x as i32, loc.y as i32);
    let (sx, sy) = (loc.size_x.max(1) as i32, loc.size_y.max(1) as i32);
    plan.path(&footprint(tx, ty, sx, sy), color, true, true);
    let centre = projection::Point::new(tx * TILE_SIZE + sx * TILE_HALF, ty * TILE_SIZE + sy * TILE_HALF, 0);
    plan.label(centre, format!("ID: {}", loc.id), color);
}

// The grid (the Grid layer): every tile within `GRID_RADIUS` of the centre, in the scene.
fn grid_tiles([cx, cy]: [i32; 2]) -> Vec<[i32; 2]> {
    let mut tiles = Vec::new();
    for tx in (cx - GRID_RADIUS).max(0)..=(cx + GRID_RADIUS).min(SCENE_TILES - 1) {
        for ty in (cy - GRID_RADIUS).max(0)..=(cy + GRID_RADIUS).min(SCENE_TILES - 1) {
            tiles.push([tx, ty]);
        }
    }
    tiles
}

// Whether a point lies inside a convex quadrilateral (its corners in order).
fn inside(corners: &[[f32; 2]], [px, py]: [f32; 2]) -> bool {
    let (mut positive, mut negative) = (false, false);
    for i in 0..corners.len() {
        let ([ax, ay], [bx, by]) = (corners[i], corners[(i + 1) % corners.len()]);
        let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
        positive |= cross > 0.0;
        negative |= cross < 0.0;
    }
    !(positive && negative)
}

// The client window of a client process: its `JagWindow`.
fn client_window(pid: u32) -> Option<HWND> {
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let (pid, found) = unsafe { &mut *(lparam as *mut (u32, Option<HWND>)) };
        if !class_is(hwnd, "JagWindow") {
            return 1;
        }
        let mut owner = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut owner) };
        if owner == *pid {
            *found = Some(hwnd);
            return 0;
        }
        1
    }

    let mut search: (u32, Option<HWND>) = (pid, None);
    unsafe { EnumWindows(Some(callback), &mut search as *mut _ as LPARAM) };
    search.1
}

// The real mouse position on the client's canvas, not the cursor the injector moves for input.
fn cursor_canvas(pid: u32) -> Option<[f32; 2]> {
    let hwnd = client_window(pid)?;
    let mut cursor = POINT { x: 0, y: 0 };
    let mut origin = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut cursor) } == 0 || unsafe { ClientToScreen(hwnd, &mut origin) } == 0 {
        return None;
    }
    Some([(cursor.x - origin.x) as f32, (cursor.y - origin.y) as f32])
}

// The scene tile under the mouse and its position, among tiles within `HOVER_RANGE` of the player.
type HoveredTile = ([i32; 2], [f32; 2]);

fn hovered_tile(pid: u32, centre: [i32; 2]) -> Result<Option<HoveredTile>, String> {
    let Some(cursor) = cursor_canvas(pid) else { return Ok(None) };
    let [cx, cy] = centre;
    let tiles: Vec<[i32; 2]> = (cx - HOVER_RANGE..=cx + HOVER_RANGE)
        .flat_map(|tx| (cy - HOVER_RANGE..=cy + HOVER_RANGE).map(move |ty| [tx, ty]))
        .filter(|&tile| in_scene(tile))
        .collect();
    let points: Vec<projection::Point> = tiles.iter().flat_map(|&tile| tile_corners(tile)).collect();
    if points.is_empty() {
        return Ok(None);
    }
    let screen = project(pid, &points, Via::Auto)?;
    Ok(tiles.iter().zip(screen.as_chunks::<4>().0.iter()).find(|(_, corners)| inside(*corners, cursor)).map(|(&tile, _)| (tile, cursor)))
}

// Runs `read` on the scene's terrain (its tile settings), reusing the terrain that projection keeps.
fn with_terrain<T>(pid: u32, read: impl FnOnce(&Terrain) -> T) -> Result<T, String> {
    let (layout, memory) = reader(pid, 0, "")?;
    let mut cache = TERRAIN.lock().unwrap_or_else(|e| e.into_inner());
    let mut terrain = cache.take().filter(|(p, _)| *p == pid).map(|(_, terrain)| terrain);
    let result = projection::ensure_terrain(&memory, &layout, &mut terrain, || decorations(pid));
    let value = match (result, terrain.as_ref()) {
        (Ok(_), Some(terrain)) => Ok(read(terrain)),
        (Ok(_), None) => Err("the scene's terrain isn't known".into()),
        (Err(Failure::Unavailable(why)), _) => Err(why),
    };
    *cache = terrain.map(|terrain| (pid, terrain));
    value
}

// How long `scene_tiles_next_frame` waits for a frame, so a caller looping on it can check whether to stop.
const FRAME_WAIT: Duration = Duration::from_millis(100);
// The longest single wait on the frame event: also notices a frame whose event another waiter took.
const FRAME_SLICE: Duration = Duration::from_millis(20);

fn frame_serial(pid: u32) -> Result<u32, String> {
    match read_header(pid) {
        Some(Ok(header)) => Ok(header.frame_serial),
        Some(Err(Incompatible { protocol, .. })) => Err(format!(
            "Client {pid} runs agent protocol {protocol}, this is {}: restart the client and load a matching agent",
            ipc::PROTOCOL_VERSION
        )),
        None => Err(format!("No agent shared memory in client {pid}")),
    }
}

// Waits up to `timeout` for client `pid` to present a frame other than `after` (a `Header::frame_serial`); `None` if none came.
fn next_frame(pid: u32, after: u32, timeout: Duration) -> Result<Option<u32>, String> {
    let session = session(pid)?;
    let event = session
        .frame
        .get_or_init(|| {
            let name = wide(frame_event_name(pid));
            Handle::new(unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) })
        })
        .as_ref();
    let deadline = Instant::now() + timeout;
    loop {
        let frame = frame_serial(pid)?;
        if frame != after {
            return Ok(Some(frame));
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(None);
        }
        match event {
            Some(event) => {
                unsafe { WaitForSingleObject(event.0, left.min(FRAME_SLICE).as_millis().max(1) as u32) };
            }
            None => sleep(left.min(Duration::from_millis(1))),
        }
    }
}

/// Draws `layers` as `scene_tiles` does, once client `pid` has presented a frame after `after`, so a loop of these paints
/// once per frame. Returns the frame it painted after, to pass as the next `after`, or `None` (nothing painted) when no
/// frame came within `FRAME_WAIT` (the client is minimized or loading).
pub fn scene_tiles_next_frame(pid: u32, layers: &[&str], after: u32) -> Result<Option<u32>, String> {
    let Some(frame) = next_frame(pid, after, FRAME_WAIT)? else { return Ok(None) };
    scene_tiles(pid, layers)?;
    Ok(Some(frame))
}

// Draws the layers named in `layers` (see `LAYERS`); returns how many tile outlines were drawn in full on the canvas.
pub fn scene_tiles(pid: u32, layers: &[&str]) -> Result<usize, String> {
    if let Some(unknown) = layers.iter().find(|layer| !LAYERS.contains(layer)) {
        return Err(format!("unknown tile layer {unknown:?}"));
    }
    let on = |name: &str| layers.contains(&name);
    let font = label_font(pid)?;
    let line_height = font.max_height * LABEL_SCALE + 2;
    let state = game_state(pid)?;
    let plane_text = |position: &Position| position.plane.map_or("?".to_string(), |plane| plane.to_string());
    let here = || state.position.ok_or("the player's position isn't known (not logged in?)");

    let mut plan = Plan::default();

    if on("grid") {
        let here = here()?;
        for tile in grid_tiles([here.local_x >> 7, here.local_y >> 7]) {
            plan.path(&tile_corners(tile), GRID_COLOR, true, false);
        }
    }

    if on("npcs") {
        for npc in npcs(pid)? {
            let color = NPC_TILE_COLOR;
            actor_marker(&mut plan, (npc.local_x, npc.local_y), npc.height, npc_label(&npc), color);
        }
    }

    if on("players") {
        // The local player is the one standing where the game state says the player is.
        let me = state.position.map(|p| (p.local_x, p.local_y));
        for player in players(pid)? {
            let local = (player.local_x, player.local_y);
            let color = if Some(local) == me { LOCAL_PLAYER_TILE_COLOR } else { PLAYER_TILE_COLOR };
            actor_marker(&mut plan, local, player.height, player_label(&player), color);
        }
    }

    if on("objects") || on("ground-objects") || on("walls") || on("decorations") {
        let reply = cached_locs_reply(pid)?;
        for loc in &reply.locs[..(reply.count as usize).min(reply.locs.len())] {
            let color = match loc.kind {
                projection::GAME_OBJECT if on("objects") => OBJECT_TILE_COLOR,
                projection::GROUND_DECORATION if on("ground-objects") => GROUND_OBJECT_COLOR,
                projection::WALL if on("walls") => WALL_COLOR,
                projection::WALL_DECORATION if on("decorations") => DECORATION_COLOR,
                _ => continue,
            };
            loc_marker(&mut plan, loc, color);
        }
    }

    if on("ground-items") {
        let here = here()?;
        for item in cached(&GROUND_ITEMS_CACHE, pid, || ground_items(pid).map(Arc::new))?.iter().filter(|item| Some(item.plane) == here.plane) {
            plan.tile([item.scene_x, item.scene_y], format!("ID: {} Qty:{}", item.id, item.quantity), GROUND_ITEM_COLOR);
        }
    }

    if on("tile-flags") {
        let flagged = with_terrain(pid, |terrain| {
            let [size_x, size_y] = terrain.size();
            let plane = terrain.key().plane;
            let mut flagged = Vec::new();
            for x in 0..size_x {
                for y in 0..size_y {
                    let bridge = terrain.setting(1, x, y).is_some_and(|s| s & crate::devtools::TILE_FLAG_BRIDGE != 0);
                    let text = crate::devtools::tile_flag_text(bridge, terrain.setting(plane, x, y).unwrap_or(0));
                    if !text.is_empty() {
                        flagged.push(([x, y], text));
                    }
                }
            }
            flagged
        })?;
        for (tile, text) in flagged {
            plan.label(tile_centre(tile), text, TILE_FLAG_COLOR);
        }
    }

    if on("zone-borders") {
        // The chunk grid across the whole scene, as RuneLite's zone borders.
        let lines: Vec<i32> = (CHUNK_TILES..=SCENE_TILES - CHUNK_TILES).step_by(CHUNK_TILES as usize).collect();
        for &x in &lines {
            let points: Vec<_> = lines.iter().map(|&y| boundary(x, y)).collect();
            plan.path(&points, CHUNK_COLOR, false, false);
        }
        for &y in &lines {
            let points: Vec<_> = lines.iter().map(|&x| boundary(x, y)).collect();
            plan.path(&points, CHUNK_COLOR, false, false);
        }
    }

    if on("map-squares") {
        let here = here()?;
        let (origin_x, origin_y) = scene_origin(&here);
        let (near_x, near_y) = (here.x, here.y);
        // The map square edges within reach of the player, as RuneLite's map squares.
        for x in (near_x - MAP_SQUARE_RANGE..=near_x + MAP_SQUARE_RANGE).filter(|x| x.rem_euclid(MAP_SQUARE_TILES) == 0) {
            let points: Vec<_> = (near_y - MAP_SQUARE_RANGE..=near_y + MAP_SQUARE_RANGE).map(|y| boundary(x - origin_x, y - origin_y)).collect();
            plan.path(&points, MAP_SQUARE_COLOR, false, false);
        }
        for y in (near_y - MAP_SQUARE_RANGE..=near_y + MAP_SQUARE_RANGE).filter(|y| y.rem_euclid(MAP_SQUARE_TILES) == 0) {
            let points: Vec<_> = (near_x - MAP_SQUARE_RANGE..=near_x + MAP_SQUARE_RANGE).map(|x| boundary(x - origin_x, y - origin_y)).collect();
            plan.path(&points, MAP_SQUARE_COLOR, false, false);
        }
    }

    if on("loading-lines") {
        let (near, far) = (LOADING_INSET, SCENE_TILES - LOADING_INSET);
        let square = [boundary(near, near), boundary(near, far), boundary(far, far), boundary(far, near)];
        plan.path(&square, CHUNK_COLOR, true, false);
    }

    if on("line-of-sight") {
        let here = here()?;
        let (origin_x, origin_y) = scene_origin(&here);
        let (x1, y1, _, flags) = scene_collision(pid)?;
        let flag_at = |x: i32, y: i32| flags.get((x - x1) as usize).and_then(|column| column.get((y - y1) as usize)).copied().unwrap_or(0);
        let from = (here.x, here.y);
        for x in here.x - LOS_RANGE..=here.x + LOS_RANGE {
            for y in here.y - LOS_RANGE..=here.y + LOS_RANGE {
                let tile = [x - origin_x, y - origin_y];
                if (x, y) == from || !in_scene(tile) || !crate::devtools::line_of_sight(flag_at, from, (x, y)) {
                    continue;
                }
                plan.path(&tile_corners(tile), LINE_OF_SIGHT_COLOR, true, false);
            }
        }
    }

    if on("valid-movement") {
        let here = here()?;
        let plane = here.plane.ok_or("the player's plane isn't known")?;
        let world = world_movement(pid)?;
        let (origin_x, origin_y) = scene_origin(&here);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let tile = [here.x + dx - origin_x, here.y + dy - origin_y];
                if (dx, dy) != (0, 0) && in_scene(tile) && crate::pathfinding::can_step(&world, plane, here.x, here.y, dx, dy) {
                    plan.path(&tile_corners(tile), LOCAL_VALID_MOVEMENT_COLOR, true, false);
                }
            }
        }
    }

    // The text panels at the top left, one under another.
    let mut panel_y = TEXT_TOP;
    let mut panel = |plan: &mut Plan, lines: Vec<String>| {
        for line in lines {
            plan.text(TEXT_LEFT, panel_y, line, TEXT_COLOR);
            panel_y += line_height;
        }
    };
    if on("location") {
        let here = here()?;
        panel(
            &mut plan,
            vec![
                format!("Local {}, {}", here.local_x, here.local_y),
                format!("World {}, {}, {}", here.x, here.y, plane_text(&here)),
                format!("Scene {}, {}", here.local_x >> 7, here.local_y >> 7),
            ],
        );
    }
    if on("camera") {
        let camera = state.camera.ok_or("the camera isn't known")?;
        panel(
            &mut plan,
            vec![
                format!("Camera {}, {}, {}", camera.x, camera.y, camera.z),
                format!("Pitch {}", camera.pitch),
                format!("Yaw {}", camera.yaw),
                format!("Scale {}", camera.zoom.map_or("?".to_string(), |zoom| zoom.to_string())),
            ],
        );
    }

    if on("movement-flags") || on("tile-location") {
        let here = here()?;
        let (origin_x, origin_y) = scene_origin(&here);
        if let Some((tile, cursor)) = hovered_tile(pid, [here.local_x >> 7, here.local_y >> 7])? {
            plan.path(&tile_corners(tile), HOVER_COLOR, true, false);
            let (x, y) = (origin_x + tile[0], origin_y + tile[1]);
            let mut lines = Vec::new();
            if on("tile-location") {
                let settings = with_terrain(pid, |terrain| terrain.setting(terrain.key().plane, tile[0], tile[1]).unwrap_or(0))?;
                let (region, region_x, region_y) = crate::devtools::region(x, y);
                lines.push(format!("World location: {}, {}, {}", x, y, plane_text(&here)));
                lines.push(format!("Region ID: {region} location: {region_x}, {region_y}"));
                lines.push(format!("Flags: {settings}"));
            }
            if on("movement-flags") {
                let (x1, y1, _, flags) = scene_collision(pid)?;
                let data = flags.get((x - x1) as usize).and_then(|column| column.get((y - y1) as usize)).copied().unwrap_or(0);
                let names = crate::devtools::movement_flag_names(data);
                if names.is_empty() {
                    lines.push("No movement flags".to_string());
                }
                lines.extend(names.into_iter().map(str::to_string));
            }
            let (left, top) = (cursor[0] as i32 + 16, cursor[1] as i32 + 16);
            for (i, line) in lines.into_iter().enumerate() {
                plan.text(left, top + i as i32 * line_height, line, TEXT_COLOR);
            }
        }
    }

    let screen = if plan.points.is_empty() { Vec::new() } else { project(pid, &plan.points, Via::Auto)? };
    let on_screen = |[x, y]: &[f32; 2]| x.abs() < SCREEN_LIMIT && y.abs() < SCREEN_LIMIT;
    let drawn = plan.paths.iter().filter(|path| path.marker && path.points.iter().all(|&i| on_screen(&screen[i]))).count();

    // What ends up on the canvas, to skip repainting a frame identical to the last one.
    let mut hasher = DefaultHasher::new();
    for path in &plan.paths {
        (path.color, path.closed).hash(&mut hasher);
        for &i in &path.points {
            let [x, y] = screen[i];
            (on_screen(&[x, y]), x.round() as i32, y.round() as i32).hash(&mut hasher);
        }
    }
    for label in &plan.labels {
        let [x, y] = screen[label.anchor];
        (&label.text, label.color, x.round() as i32, y.round() as i32).hash(&mut hasher);
    }
    for text in &plan.texts {
        (text.x, text.y, &text.text, text.color).hash(&mut hasher);
    }

    with_overlay(pid, overlay_mode::ON_SERIAL, false, Some(hasher.finish()), |buf, width, height| unsafe {
        for path in &plan.paths {
            let points: Vec<[f32; 2]> = path.points.iter().map(|&i| screen[i]).collect();
            let edges = if path.closed { points.len() } else { points.len().saturating_sub(1) };
            for i in 0..edges {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                if on_screen(&a) && on_screen(&b) {
                    draw_line(buf, width, height, [a[0].round() as i32, a[1].round() as i32, b[0].round() as i32, b[1].round() as i32], path.color);
                }
            }
        }
        for label in &plan.labels {
            let [ax, ay] = screen[label.anchor];
            if !on_screen(&[ax, ay]) {
                continue;
            }
            let left = ax.round() as i32 - font.width(&label.text, LABEL_SCALE) / 2;
            let top = ay.round() as i32 - font.max_height * LABEL_SCALE;
            font.plot(&label.text, left + 1, top + 1, LABEL_SCALE, |x, y| put_pixel(buf, width, height, x, y, LABEL_SHADOW));
            font.plot(&label.text, left, top, LABEL_SCALE, |x, y| put_pixel(buf, width, height, x, y, label.color));
        }
        for text in &plan.texts {
            font.plot(&text.text, text.x + 1, text.y + 1, LABEL_SCALE, |x, y| put_pixel(buf, width, height, x, y, LABEL_SHADOW));
            font.plot(&text.text, text.x, text.y, LABEL_SCALE, |x, y| put_pixel(buf, width, height, x, y, text.color));
        }
    })?;
    Ok(drawn)
}

// Paints the overlay into the slot not shown and flips `overlay_serial`; `keep` starts from what is shown, `hash` and `mode` skip repeats, and `EVERY_FRAME` is painted in place.
fn with_overlay(pid: u32, mode: u8, keep: bool, hash: Option<u64>, paint: impl FnOnce(*mut u8, i32, i32)) -> Result<(), String> {
    let connection = connection(pid)
        .ok_or_else(|| format!("No agent shared memory in client {pid}"))?;
    if connection.protocol != ipc::PROTOCOL_VERSION {
        return Err(format!(
            "Client {pid} runs agent protocol {}, this is {}: restart the client and load a matching agent",
            connection.protocol,
            ipc::PROTOCOL_VERSION
        ));
    }
    let (width, height) = (connection.width, connection.height);
    if width <= 0 || height <= 0 {
        return Err(format!("Client {pid} hasn't rendered a frame yet"));
    }

    let name = wide(shared_memory_name(pid));
    let mapping = Handle::new(unsafe { OpenFileMappingW(FILE_MAP_WRITE, 0, name.as_ptr()) })
        .ok_or_else(|| format!("No agent shared memory in client {pid}"))?;
    let size = overlay_offset(ipc::OVERLAY_SLOTS);
    let view = unsafe { MapViewOfFile(mapping.0, FILE_MAP_WRITE, 0, 0, size) };
    if view.Value.is_null() {
        return Err(format!("Cannot map agent's shared memory in client {pid}: {}", io::Error::last_os_error()));
    }

    let base = view.Value as *mut u8;
    let bytes = width as usize * height as usize * 4;
    unsafe {
        let serial_at = base.add(core::mem::offset_of!(Header, overlay_serial)) as *mut u32;
        let hash_at = base.add(core::mem::offset_of!(Header, overlay_hash)) as *mut u64;
        let mode_at = base.add(core::mem::offset_of!(Header, overlay_mode));
        let current = *mode_at;

        if current == overlay_mode::EVERY_FRAME {
            paint(base.add(overlay_offset(0)), width, height);
            UnmapViewOfFile(view);
            return Ok(());
        }

        // Never 0, which is what an unknown overlay holds.
        let hash = hash.map(|h| (h ^ (width as u64) << 32 ^ height as u64) | 1);
        if hash.is_some() && hash_at.read_unaligned() == hash.unwrap_or(0) && current == mode {
            UnmapViewOfFile(view);
            return Ok(());
        }

        let serial = serial_at.read_unaligned();
        let shown = base.add(overlay_offset(serial as usize & 1));
        let back = base.add(overlay_offset((serial as usize & 1) ^ 1));
        if keep {
            copy_nonoverlapping(shown, back, bytes);
        } else {
            write_bytes(back, 0, bytes);
        }
        paint(back, width, height);
        fence(Ordering::Release);
        hash_at.write_unaligned(hash.unwrap_or(0));
        serial_at.write_unaligned(serial.wrapping_add(1));
        *mode_at = mode;
        UnmapViewOfFile(view);
    }
    Ok(())
}

unsafe fn draw_rect(buf: *mut u8, width: i32, height: i32, [x1, y1, x2, y2]: [i32; 4], color: Rgba) {
    let (left, right) = (x1.min(x2), x1.max(x2));
    let (top, bottom) = (y1.min(y2), y1.max(y2));
    for x in left..=right {
        unsafe {
            put_pixel(buf, width, height, x, top, color);
            put_pixel(buf, width, height, x, bottom, color);
        }
    }
    for y in top..=bottom {
        unsafe {
            put_pixel(buf, width, height, left, y, color);
            put_pixel(buf, width, height, right, y, color);
        }
    }
}

unsafe fn draw_line(buf: *mut u8, width: i32, height: i32, [x1, y1, x2, y2]: [i32; 4], color: Rgba) {
    let (dx, dy) = ((x2 - x1).abs(), -(y2 - y1).abs());
    let (sx, sy) = (if x1 < x2 { 1 } else { -1 }, if y1 < y2 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x1, y1, dx + dy);
    for _ in 0..=dx.max(-dy).min(1 << 15) {
        unsafe { put_pixel(buf, width, height, x, y, color) };
        if x == x2 && y == y2 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

unsafe fn put_pixel(buf: *mut u8, width: i32, height: i32, x: i32, y: i32, color: Rgba) {
    if x < 0 || y < 0 || x >= width || y >= height {
        return;
    }
    let offset = (y as usize * width as usize + x as usize) * 4;
    unsafe {
        let pixel = buf.add(offset);
        *pixel = color.b;
        *pixel.add(1) = color.g;
        *pixel.add(2) = color.r;
        *pixel.add(3) = color.a;
    }
}
