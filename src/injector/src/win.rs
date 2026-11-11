use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet, hash_map::DefaultHasher},
    ffi::c_void,
    fmt, fs,
    hash::{Hash, Hasher},
    io,
    iter::once,
    mem::{self, size_of, transmute},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    ptr::{NonNull, copy_nonoverlapping, null, null_mut, write_bytes},
    sync::{
        Arc, LazyLock, Mutex, MutexGuard, OnceLock,
        atomic::{Ordering, fence},
    },
    thread::sleep,
    time::{Duration, Instant},
};

use windows_sys::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_ALREADY_EXISTS, ERROR_BAD_LENGTH, GetLastError, HANDLE, HWND,
            INVALID_HANDLE_VALUE, LPARAM, POINT, WAIT_ABANDONED, WAIT_OBJECT_0,
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
                CreateFileMappingW, FILE_MAP, FILE_MAP_READ, FILE_MAP_WRITE, MEM_COMMIT,
                MEM_RELEASE, MEM_RESERVE, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
                OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile, VirtualAllocEx, VirtualFreeEx,
            },
            Threading::{
                CreateEventW, CreateMutexW, CreateRemoteThread, EVENT_MODIFY_STATE, IsWow64Process,
                OpenEventW, OpenProcess, PROCESS_CREATE_THREAD, PROCESS_NAME_WIN32,
                PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION,
                PROCESS_VM_READ, PROCESS_VM_WRITE, QueryFullProcessImageNameW, ReleaseMutex,
                SYNCHRONIZATION_ACCESS_RIGHTS, SYNCHRONIZATION_SYNCHRONIZE, SetEvent,
                WaitForMultipleObjects, WaitForSingleObject,
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
    HEADER_SIZE, Header, IMAGE_DATA_SIZE, INITIAL_CURSOR_SIZE, Incompatible, frame_event_name,
    initial_cursor_name,
    item_log::{self, item_log_memory_name, item_wake_event_name},
    overlay_mode, overlay_offset,
    packets::{self as packet_log, LOG_SIZE, Log, direction, packets_memory_name},
    read::short_string_text,
    request_mutex_name,
    script_runs::{
        self as script_log, arg_kind, phase as script_runs_phase, script_runs_memory_name,
    },
    shared_memory_name,
    state::{
        CACHE_BATCH, CACHE_FILE_MISSING, CACHE_FILE_NO_ROOM, CacheReply, CacheRequest,
        GameValRequest, ITEM_ICON_HEIGHT, ITEM_ICON_SHOW_COUNT, ITEM_ICON_WIDTH, ITEM_NAME_BATCH,
        ItemNameRequest, LAYOUT_SIZE, Layout, LayoutView, LocsReply, ModelRequest, NamesReply,
        PROJECT_BATCH, ProjectRequest, ReadError, Shared, TotalXpRequest, WorkerRequest,
        WorldLayout, cache_kind, cache_status, game_state::LOGGED_IN, item_name_status, loc_status,
        menu_status, model_status, model_target, name_status, part, project_status,
        state_memory_name, total_xp_status, worker_kind, worker_status,
    },
    unload_event_name,
    varc_log::{self, varc_log_memory_name},
    work_event_name, worker_mutex_name, worker_reply_event_name,
};

use crate::{
    Client, Connection, DisplayName, GameState, Image, ItemIconRequest, LoadReport, Overlay,
    Position, Rgba, TotalXp, Widget, dll_version,
    memory::Memory,
    projection::{self, Terrain, Via},
    state,
    widgets::SubInterface,
    worker,
    world::{self, Item, Npc, Player},
    world_select::WorldSelect,
};

const MODULE_PREFIX: &str = "agent";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(15);
const LOAD_LIBRARY_TIMEOUT_MS: u32 = 10_000;

struct Handle(HANDLE);

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

fn open_event(name: String, access: SYNCHRONIZATION_ACCESS_RIGHTS) -> Option<Handle> {
    let name = wide(name);
    Handle::new(unsafe { OpenEventW(access, 0, name.as_ptr()) })
}

fn new_event(name: String) -> Option<Handle> {
    let name = wide(name);
    let handle = unsafe { CreateEventW(null(), 0, 0, name.as_ptr()) };
    let taken = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    Handle::new(handle).filter(|_| !taken)
}

fn create_mutex(name: String) -> Option<Handle> {
    let name = wide(name);
    Handle::new(unsafe { CreateMutexW(null(), 0, name.as_ptr()) })
}

struct SlotGuard<'a>(&'a Handle);

impl<'a> SlotGuard<'a> {
    fn lock(mutex: &'a Handle, timeout: Duration) -> Option<Self> {
        matches!(
            unsafe { WaitForSingleObject(mutex.0, wait_ms(timeout)) },
            WAIT_OBJECT_0 | WAIT_ABANDONED
        )
        .then_some(Self(mutex))
    }
}

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        unsafe { ReleaseMutex(self.0.0) };
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn wait_ms(timeout: Duration) -> u32 {
    timeout.as_millis().min(u32::MAX as u128 - 1) as u32
}

fn open_mapping(name: String, access: FILE_MAP) -> Option<Handle> {
    let name = wide(name);
    Handle::new(unsafe { OpenFileMappingW(access, 0, name.as_ptr()) })
}

struct View {
    address: NonNull<c_void>,
    _mapping: Handle,
}

unsafe impl Send for View {}
unsafe impl Sync for View {}

impl View {
    fn new(mapping: Handle, access: FILE_MAP, size: usize) -> io::Result<Self> {
        let view = unsafe { MapViewOfFile(mapping.0, access, 0, 0, size) };
        let address = NonNull::new(view.Value).ok_or_else(io::Error::last_os_error)?;
        Ok(Self {
            address,
            _mapping: mapping,
        })
    }

    fn ptr<T>(&self) -> NonNull<T> {
        self.address.cast()
    }
}

impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.address.as_ptr(),
            })
        };
    }
}

struct Module {
    name: String,
    path: String,
    base: usize,
}

fn modules(pid: u32) -> Result<Vec<Module>, String> {
    let mut attempts = 0;
    let snapshot = loop {
        let handle =
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
        if let Some(handle) = Handle::new(handle) {
            break handle;
        }
        attempts += 1;
        if unsafe { GetLastError() } != ERROR_BAD_LENGTH || attempts == 5 {
            return Err(format!(
                "Couldn't list the modules of {pid}: {}",
                io::Error::last_os_error()
            ));
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

fn foreign_bot(modules: &[Module]) -> Option<&'static str> {
    let taken = modules.iter().any(|m| {
        let name = m.name.to_ascii_lowercase();
        let path = m.path.to_ascii_lowercase().replace('/', "\\");
        (name.starts_with("dc_") && name.ends_with(".dll"))
            || (name.ends_with(".exe") && path.contains("\\.dc\\"))
    });
    taken.then_some("Detuks")
}

fn process_name(process: &Handle) -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    if unsafe {
        QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut len)
    } == 0
    {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..len as usize]);
    Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
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

fn each_jag_window(mut visit: impl FnMut(HWND, u32) -> bool) {
    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let visit = unsafe { &mut *(lparam as *mut &mut dyn FnMut(HWND, u32) -> bool) };
        if !class_is(hwnd, "JagWindow") {
            return 1;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        visit(hwnd, pid).into()
    }

    let mut visit: &mut dyn FnMut(HWND, u32) -> bool = &mut visit;
    unsafe { EnumWindows(Some(callback), &mut visit as *mut _ as LPARAM) };
}

pub fn list_clients() -> Vec<Client> {
    let mut windows: Vec<(u32, String)> = Vec::new();
    each_jag_window(|hwnd, pid| {
        if !windows.iter().any(|(p, _)| *p == pid) {
            let mut title = [0u16; 256];
            let len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
            windows.push((pid, from_wide(&title[..len.max(0) as usize])));
        }
        true
    });

    windows
        .into_iter()
        .map(|(pid, title)| {
            let process =
                Handle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
            let modules = modules(pid).ok();
            Client {
                pid,
                exe: process.as_ref().and_then(process_name).unwrap_or_default(),
                title,
                is_64bit: process.is_some_and(|p| is_64bit(p.0)),
                loaded: modules
                    .as_deref()
                    .and_then(|m| agent_module(m).map(|m| m.path.clone())),
                version: connection(pid).map(|c| c.version),
                foreign_bot: modules.as_deref().and_then(foreign_bot).map(String::from),
            }
        })
        .collect()
}

fn remote_load_library(
    remote_kernel32: usize,
) -> Result<unsafe extern "system" fn(*mut c_void) -> u32, String> {
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
    let (Some(dir), Some(name)) = (dll.parent(), dll.file_name()) else {
        return;
    };
    let Some(stem) = name
        .to_str()
        .and_then(|n| n.rsplit_once('-'))
        .map(|(stem, _)| format!("{stem}-"))
    else {
        return;
    };
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.starts_with(&stem) && file.ends_with(".dll") && entry.file_name() != name {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn seed_cursor(pid: u32, cursor: &crate::Cursor) -> Option<View> {
    let name = wide(initial_cursor_name(pid));
    let mapping = Handle::new(unsafe {
        CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            null(),
            PAGE_READWRITE,
            0,
            INITIAL_CURSOR_SIZE as u32,
            name.as_ptr(),
        )
    })?;
    let view = View::new(mapping, FILE_MAP_WRITE, INITIAL_CURSOR_SIZE).ok()?;
    let [r, g, b] = cursor.color;
    let bytes = [cursor.style.value(), r, g, b, cursor.size];
    unsafe {
        view.ptr::<u8>()
            .as_ptr()
            .copy_from_nonoverlapping(bytes.as_ptr(), bytes.len())
    };
    Some(view)
}

pub fn load(pid: u32, dll: &Path, cursor: Option<&crate::Cursor>) -> Result<LoadReport, String> {
    forget(pid);
    let dll = std::path::absolute(dll).map_err(|e| format!("Invalid DLL path: {e}"))?;
    let bytes = fs::read(&dll).map_err(|e| format!("Cannot read {}: {e}", dll.display()))?;
    let version = dll_version(&bytes);
    let dll = versioned_copy(&dll, &bytes, &version)?;
    let dll_name = dll
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();

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
    .ok_or_else(|| {
        format!(
            "Couldn't open process {pid}: {}",
            io::Error::last_os_error()
        )
    })?;

    if !is_64bit(process.0) {
        return Err("Only 64-bit clients are supported.".into());
    }

    let is_loaded = |modules: &[Module]| {
        modules
            .iter()
            .any(|m| m.name.to_ascii_lowercase() == dll_name)
    };

    let mut before = modules(pid)?;
    if let Some(bot) = foreign_bot(&before) {
        return Err(format!(
            "Client {pid} is already controlled by {bot}, refusing to inject it."
        ));
    }
    let mut replaced = None;
    if agent_module(&before).is_some() {
        let current = fresh_connection(pid);
        if is_loaded(&before) || current.as_ref().is_some_and(|c| c.version == version) {
            if let Some(cursor) = cursor {
                let _ = set_cursor(pid, cursor);
            }
            return Ok(LoadReport {
                already_loaded: true,
                replaced: None,
                connection: current,
            });
        }
        let old = current
            .map(|c| c.version)
            .unwrap_or_else(|| "unknown".into());
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
    let remote = unsafe {
        VirtualAllocEx(
            process.0,
            null(),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote.is_null() {
        return Err(format!(
            "VirtualAllocEx failed: {}",
            io::Error::last_os_error()
        ));
    }
    let free = || unsafe { VirtualFreeEx(process.0, remote, 0, MEM_RELEASE) };

    if unsafe { WriteProcessMemory(process.0, remote, path.as_ptr().cast(), size, null_mut()) } == 0
    {
        let error = io::Error::last_os_error();
        free();
        return Err(format!("WriteProcessMemory failed: {error}"));
    }

    let Some(thread) = Handle::new(unsafe {
        CreateRemoteThread(
            process.0,
            null(),
            0,
            Some(load_library),
            remote,
            0,
            null_mut(),
        )
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
    let event = open_event(unload_event_name(pid), EVENT_MODIFY_STATE).ok_or(
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
        let written =
            |c: &Connection| c.protocol != 0 && (c.version == version || !c.is_compatible());
        if let Some(connection) = fresh_connection(pid).filter(written) {
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

fn header_in(view: &View) -> Result<Header, Incompatible> {
    let mut header = [0u8; HEADER_SIZE];
    unsafe { copy_nonoverlapping(view.ptr::<u8>().as_ptr(), header.as_mut_ptr(), HEADER_SIZE) };
    Header::from_bytes(&header)
}

fn fresh_header(pid: u32) -> Option<Result<Header, Incompatible>> {
    let mapping = open_mapping(shared_memory_name(pid), FILE_MAP_READ)?;
    Some(header_in(
        &View::new(mapping, FILE_MAP_READ, HEADER_SIZE).ok()?,
    ))
}

fn read_header(pid: u32) -> Option<Result<Header, Incompatible>> {
    session(pid)
        .ok()
        .and_then(|session| session.shared(pid, |_| String::new()).ok().map(header_in))
        .or_else(|| fresh_header(pid))
}

fn fresh_connection(pid: u32) -> Option<Connection> {
    Some(Connection::from_header(fresh_header(pid)?))
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
            ReadProcessMemory(
                self.0.0,
                address as *const c_void,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut read,
            )
        };
        ok != 0 && read == buffer.len()
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        let mut written = 0;
        let ok = unsafe {
            WriteProcessMemory(
                self.0.0,
                address as *const c_void,
                bytes.as_ptr().cast(),
                bytes.len(),
                &mut written,
            )
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
const REQUEST_POLL: Duration = Duration::from_millis(1);

struct Session {
    process: Arc<Handle>,
    view: StateView,
    born: Instant,
    work: OnceLock<Option<Handle>>,
    slot: OnceLock<Option<Handle>>,
    reply: OnceLock<Option<Handle>>,
    frame: OnceLock<Option<Handle>>,
    shared: OnceLock<View>,
    requests: Mutex<HashMap<u32, Arc<Handle>>>,
}

impl Session {
    fn shared(
        &self,
        pid: u32,
        unmapped: impl FnOnce(io::Error) -> String,
    ) -> Result<&View, String> {
        if let Some(view) = self.shared.get() {
            return Ok(view);
        }
        let view = shared_view(pid, overlay_offset(ipc::OVERLAY_SLOTS), unmapped)?;
        Ok(self.shared.get_or_init(|| view))
    }

    fn request_mutex(&self, pid: u32, message: u32) -> Option<Arc<Handle>> {
        let mut requests = lock(&self.requests);
        if let Some(mutex) = requests.get(&message) {
            return Some(mutex.clone());
        }
        let mutex = Arc::new(create_mutex(request_mutex_name(pid, message))?);
        requests.insert(message, mutex.clone());
        Some(mutex)
    }
}

const SESSION_TTL: Duration = Duration::from_millis(500);

static SESSIONS: Mutex<Vec<(u32, Arc<Session>)>> = Mutex::new(Vec::new());

fn session(pid: u32) -> Result<Arc<Session>, String> {
    let mut sessions = lock(&SESSIONS);
    sessions.retain(|(_, s)| s.born.elapsed() < SESSION_TTL);
    if let Some((_, session)) = sessions.iter().find(|(p, _)| *p == pid) {
        return Ok(session.clone());
    }
    let view = StateView::open(pid)?;
    let process = Handle::new(unsafe { OpenProcess(PROCESS_VM_READ, 0, pid) })
        .ok_or_else(|| format!("Cannot open client {pid} to read it"))?;
    let session = Arc::new(Session {
        process: Arc::new(process),
        view,
        born: Instant::now(),
        work: OnceLock::new(),
        slot: OnceLock::new(),
        reply: OnceLock::new(),
        frame: OnceLock::new(),
        shared: OnceLock::new(),
        requests: Mutex::new(HashMap::new()),
    });
    sessions.push((pid, session.clone()));
    Ok(session)
}

fn forget(pid: u32) {
    lock(&SESSIONS).retain(|(p, _)| *p != pid);
}

struct StateView(View);

impl StateView {
    fn open(pid: u32) -> Result<Self, String> {
        let access = FILE_MAP_READ | FILE_MAP_WRITE;
        let mapping = open_mapping(state_memory_name(pid), access).ok_or_else(|| {
            format!("No game state in client {pid}: agent isn't loaded or is too old")
        })?;
        View::new(mapping, access, LAYOUT_SIZE)
            .map(Self)
            .map_err(|_| format!("Cannot map the game state of client {pid}"))
    }

    fn shared(&self) -> Shared<'_> {
        unsafe { Shared::attach(self.0.ptr()) }
    }

    fn layout(&self) -> LayoutView<'_> {
        unsafe { LayoutView::attach(self.0.ptr()) }
    }
}

fn ask<S>(
    pid: u32,
    message: u32,
    write: impl FnOnce(Shared<'_>) -> u32,
    mut status: impl FnMut(Shared<'_>) -> Option<(u32, S)>,
) -> Result<S, String> {
    let session = session(pid)?;
    let mutex = session
        .request_mutex(pid, message)
        .ok_or("cannot take the agent's request slot")?;
    let Some(_slot) = SlotGuard::lock(&mutex, REQUEST_TIMEOUT) else {
        return Err("the agent is busy with another request".to_owned());
    };
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

const OTHER_THREAD: &str = "the client runs its game on another thread than its window's";

fn game_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace('\u{a0}', " ")
}

fn first<T>(items: &[T], count: u32) -> &[T] {
    &items[..(count as usize).min(items.len())]
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
        .map(game_text);
    match status.status {
        name_status::KEPT => Ok(DisplayName::Kept { real }),
        name_status::WAITING => Ok(DisplayName::Waiting),
        name_status::OFF => Ok(DisplayName::Off),
        name_status::LONG_NAME => Err(
            "the client keeps this player's name as a long string, which can't be replaced safely"
                .into(),
        ),
        name_status::OTHER_THREAD => Err(format!("{OTHER_THREAD}, so the name isn't replaced")),
        name_status::NOT_FOUND => Err(
            "agent didn't find the client's appearance decoder, so it can't keep the name".into(),
        ),
        other => Err(format!("unknown display name status {other}")),
    }
}

pub fn set_total_xp(pid: u32, offset: Option<i64>) -> Result<TotalXp, String> {
    let (layout, memory) = reader(pid, part::SKILLS, "the skills")?;
    let real = state::total_xp(&memory, &layout);
    let request = TotalXpRequest {
        enabled: offset.is_some_and(|o| o != 0) as u32,
        offset: offset.unwrap_or(0),
        ..Default::default()
    };
    let status = ask(
        pid,
        ipc::input::TOTAL_XP_MESSAGE,
        |shared| shared.write_total_xp_request(&request),
        |shared| {
            shared
                .read_total_xp_status(100)
                .map(|s| (s.sequence, s.status))
        },
    )?;
    match status {
        total_xp_status::ON => Ok(TotalXp::Fake {
            offset: request.offset,
            real,
        }),
        total_xp_status::OFF => Ok(TotalXp::Off),
        total_xp_status::NOT_FOUND => Err(
            "agent didn't find the client's component invalidation, so it can't fake the total XP"
                .into(),
        ),
        total_xp_status::OTHER_THREAD => {
            Err(format!("{OTHER_THREAD}, so the total XP isn't faked"))
        }
        other => Err(format!("unknown total XP status {other}")),
    }
}

pub fn item_names(pid: u32, ids: &[i32]) -> Result<Vec<Option<String>>, String> {
    names(
        pid,
        ids,
        ipc::input::ITEM_NAMES_MESSAGE,
        |shared, batch| {
            let mut request = ItemNameRequest {
                count: batch.len() as u32,
                ..Default::default()
            };
            request.ids[..batch.len()].copy_from_slice(batch);
            shared.write_item_name_request(&request)
        },
        |shared| shared.read_item_name_status(100),
    )
}

pub fn game_val_names(pid: u32, kind: u32, ids: &[i32]) -> Result<Vec<Option<String>>, String> {
    names(
        pid,
        ids,
        ipc::input::GAME_VALS_MESSAGE,
        |shared, batch| {
            let mut request = GameValRequest {
                kind,
                count: batch.len() as u32,
                ..Default::default()
            };
            request.ids[..batch.len()].copy_from_slice(batch);
            shared.write_game_val_request(&request)
        },
        |shared| shared.read_game_val_status(100),
    )
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
        let status = ask(
            pid,
            message,
            |shared| write(shared, batch),
            |shared| read(shared).map(|s| (s.sequence, s)),
        )?;
        match status.status {
            item_name_status::OK => names.extend((0..status.count as usize).map(|i| {
                status
                    .name(i)
                    .filter(|name| !name.is_empty())
                    .map(game_text)
            })),
            item_name_status::NOT_FOUND => {
                return Err(
                    "agent didn't find what it needs in the client to look these up".into(),
                );
            }
            item_name_status::OTHER_THREAD => {
                return Err(format!("{OTHER_THREAD}, so nothing can be looked up"));
            }
            item_name_status::TOO_MANY => return Err("too many ids in one request".into()),
            other => return Err(format!("unknown name status {other}")),
        }
    }
    Ok(names)
}

static TERRAIN: Mutex<Option<(u32, Terrain)>> = Mutex::new(None);

fn project_locally(
    pid: u32,
    points: &[projection::Point],
    shift: Option<[i32; 3]>,
    clip: bool,
) -> Result<Vec<[f32; 2]>, String> {
    let (layout, memory) = reader(pid, 0, "")?;
    kept_terrain(pid, |terrain| {
        projection::project(&memory, &layout, terrain, points, shift, clip, || {
            decorations(pid)
        })
    })
}

fn kept_terrain<T>(pid: u32, use_terrain: impl FnOnce(&mut Option<Terrain>) -> T) -> T {
    let mut cache = lock(&TERRAIN);
    let mut terrain = cache
        .take()
        .filter(|(p, _)| *p == pid)
        .map(|(_, terrain)| terrain);
    let result = use_terrain(&mut terrain);
    *cache = terrain.map(|terrain| (pid, terrain));
    result
}

pub fn project(pid: u32, points: &[projection::Point], via: Via) -> Result<Vec<[f32; 2]>, String> {
    if via != Via::Client {
        match project_locally(pid, points, None, false) {
            Ok(screen) => return Ok(screen),
            Err(why) if via == Via::Local => return Err(format!("local projection: {why}")),
            Err(_) => {}
        }
    }
    let plain: Vec<[i32; 3]> = points.iter().map(|p| [p.x, p.y, p.z]).collect();
    project_in_client(pid, &plain)
}

fn project_clipped(pid: u32, points: &[projection::Point]) -> Result<Vec<[f32; 2]>, String> {
    project_locally(pid, points, None, true).or_else(|_| project(pid, points, Via::Client))
}

pub fn project_moved(
    pid: u32,
    points: &[projection::Point],
    shift: [i32; 3],
) -> Result<Vec<[f32; 2]>, String> {
    project_locally(pid, points, Some(shift), false)
        .map_err(|why| format!("local projection: {why}"))
}

fn decorations(pid: u32) -> Vec<projection::Decoration> {
    let Ok(reply) = locs_reply(pid, ipc::state::locs_request::PLAYER_PLANE) else {
        return Vec::new();
    };
    first(&reply.locs, reply.count)
        .iter()
        .filter(|l| l.kind == projection::GROUND_DECORATION && l.decoration_height != 0)
        .map(|l| {
            (
                (reply.plane, l.x as i32, l.y as i32),
                l.decoration_height as i32,
            )
        })
        .collect()
}

fn project_in_client(pid: u32, points: &[[i32; 3]]) -> Result<Vec<[f32; 2]>, String> {
    reader(pid, 0, "")?;
    let mut screen = Vec::with_capacity(points.len());
    for batch in points.chunks(PROJECT_BATCH) {
        let mut request = ProjectRequest {
            count: batch.len() as u32,
            ..Default::default()
        };
        request.points[..batch.len()].copy_from_slice(batch);
        let reply = ask(
            pid,
            ipc::input::PROJECT_MESSAGE,
            |shared| shared.write_project_request(&request),
            |shared| shared.read_project_reply(100).map(|r| (r.sequence, r)),
        )?;
        match reply.status {
            project_status::OK if reply.count as usize == batch.len() => {
                screen.extend_from_slice(&reply.points[..batch.len()]);
            }
            project_status::OK => {
                return Err(format!(
                    "agent projected {} of {} points",
                    reply.count,
                    batch.len()
                ));
            }
            project_status::NOT_IN_GAME => return Err("not logged in".into()),
            project_status::NOT_FOUND => {
                return Err("agent didn't find the client's projection".into());
            }
            project_status::OTHER_THREAD => {
                return Err(OTHER_THREAD.into());
            }
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
            corners: first(&reply.corners, reply.corner_count).to_vec(),
            hull: first(&reply.hull, reply.hull_count).to_vec(),
            face_count: reply.face_count,
            faces: first(&reply.faces, reply.face_count).to_vec(),
            frame_count: reply.frame_count,
        })),
        model_status::NO_NPC | model_status::NO_MODEL => Ok(None),
        model_status::BAD_ANIMATION => Err(format!(
            "animation {} has no frame {} (or is skeletal, which isn't supported)",
            request.animation, request.frame
        )),
        model_status::NOT_IN_GAME => Err("not logged in".into()),
        model_status::NOT_FOUND => Err("agent didn't find the client's models".into()),
        model_status::OTHER_THREAD => Err(OTHER_THREAD.into()),
        other => Err(format!("unknown model status {other}")),
    }
}

pub fn npc_model(pid: u32, npc_index: i32) -> Result<Option<crate::EntityModel>, String> {
    model(
        pid,
        ModelRequest {
            index: npc_index,
            target: model_target::NPC,
            ..Default::default()
        },
    )
}

pub fn npc_type_model(
    pid: u32,
    id: i32,
    animation: i32,
    frame: i32,
    fine: (i32, i32),
    orientation: i32,
) -> Result<Option<crate::EntityModel>, String> {
    model(
        pid,
        ModelRequest {
            target: model_target::NPC_TYPE,
            type_id: id,
            animation,
            frame,
            fine_x: fine.0,
            fine_y: fine.1,
            orientation,
            ..Default::default()
        },
    )
}

pub fn player_model(pid: u32, player_index: i32) -> Result<Option<crate::EntityModel>, String> {
    model(
        pid,
        ModelRequest {
            index: player_index,
            target: model_target::PLAYER,
            ..Default::default()
        },
    )
}

pub fn loc_model(
    pid: u32,
    kind: u8,
    scene_x: i32,
    scene_y: i32,
    id: i32,
) -> Result<Option<crate::EntityModel>, String> {
    model(
        pid,
        ModelRequest {
            target: model_target::LOC,
            loc_kind: kind as u32,
            loc_x: scene_x,
            loc_y: scene_y,
            loc_id: id,
            ..Default::default()
        },
    )
}

fn locs_reply(pid: u32, request: i32) -> Result<Box<LocsReply>, String> {
    reader(pid, 0, "")?;
    let mut reply: Box<LocsReply> = unsafe { Box::new_zeroed().assume_init() };
    ask(
        pid,
        ipc::input::LOCS_MESSAGE,
        |shared| shared.write_locs_request(&request),
        |shared| {
            shared
                .read_locs_reply_into(&mut reply, 100)
                .then_some((reply.sequence, ()))
        },
    )?;
    match reply.status {
        loc_status::OK => {}
        loc_status::NOT_IN_GAME => return Err("not logged in".into()),
        loc_status::NOT_FOUND => return Err("agent didn't find the client's scene".into()),
        loc_status::OTHER_THREAD => {
            return Err(OTHER_THREAD.into());
        }
        other => return Err(format!("unknown locs status {other}")),
    }
    Ok(reply)
}

const TICK_CACHE_TTL: Duration = Duration::from_millis(600);

type TickCache<T> = Mutex<Option<(u32, Instant, T)>>;

static LOCS_CACHE: TickCache<Arc<LocsReply>> = Mutex::new(None);
static GROUND_ITEMS_CACHE: TickCache<Arc<Vec<crate::GroundItem>>> = Mutex::new(None);

fn cached<T: Clone>(
    cache: &TickCache<T>,
    pid: u32,
    fetch: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut cache = lock(cache);
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
    cached(&LOCS_CACHE, pid, || {
        locs_reply(pid, ipc::state::locs_request::PLAYER_PLANE).map(Arc::from)
    })
}

pub fn locs(pid: u32, plane: Option<i32>) -> Result<Vec<crate::Loc>, String> {
    let request = match plane {
        Some(plane @ 0..=3) => plane,
        Some(plane) => return Err(format!("there is no plane {plane}")),
        None => ipc::state::locs_request::PLAYER_PLANE,
    };
    Ok(decode_locs_reply(&*locs_reply(pid, request)?))
}

fn clickable_locs(pid: u32) -> Result<Vec<crate::Loc>, String> {
    use ipc::state::locs_request::*;
    Ok(decode_locs_reply(&*locs_reply(
        pid,
        ALL_PLANES | CLICKABLE_ONLY,
    )?))
}

fn decode_locs_reply(reply: &LocsReply) -> Vec<crate::Loc> {
    use ipc::state::{LOC_CLICKABLE, LOC_PLANE_MASK, LOC_PLANE_SHIFT};
    let names: Vec<String> = first(&reply.names, reply.name_count)
        .iter()
        .map(|n| game_text(ipc::state::text(n)))
        .collect();
    let known = |v: u8| (v != 0xff).then_some(v);
    first(&reply.locs, reply.count)
        .iter()
        .map(|l| crate::Loc {
            id: l.id,
            name: names.get(l.name as usize).cloned(),
            kind: l.kind,
            x: reply.base_x + l.x as i32,
            y: reply.base_y + l.y as i32,
            plane: ((l.flags >> LOC_PLANE_SHIFT) & LOC_PLANE_MASK) as i32,
            scene_x: l.x as i32,
            scene_y: l.y as i32,
            size_x: l.size_x,
            size_y: l.size_y,
            shape: known(l.shape),
            rotation: known(l.rotation),
            clickable: l.flags & LOC_CLICKABLE != 0,
        })
        .collect()
}

fn cache_request(pid: u32, request: &CacheRequest) -> Result<Box<CacheReply>, String> {
    let reply = ask(
        pid,
        ipc::input::CACHE_MESSAGE,
        |shared| shared.write_cache_request(request),
        |shared| {
            let mut reply: Box<CacheReply> = unsafe { Box::new_zeroed().assume_init() };
            shared
                .read_cache_reply_into(&mut reply, 100)
                .then_some((reply.sequence, reply))
        },
    )?;
    match reply.status {
        cache_status::OK => Ok(reply),
        cache_status::NOT_FOUND => Err("agent didn't find the client's cache reader".into()),
        cache_status::NO_INDEX => Err(format!("the client has no cache index {}", request.index)),
        cache_status::NO_ARCHIVE => Err(format!(
            "cache index {} has no archive {}",
            request.index, request.archive
        )),
        cache_status::OTHER_THREAD => Err(OTHER_THREAD.into()),
        cache_status::TOO_MANY => Err("too many files in one request".into()),
        other => Err(format!("unknown cache status {other}")),
    }
}

fn cache_ids(
    pid: u32,
    kind: u32,
    index: u32,
    archive: i32,
) -> Result<crate::cache::CacheIds, String> {
    reader(pid, 0, "")?;
    let reply = cache_request(
        pid,
        &CacheRequest {
            kind,
            index,
            archive,
            ..Default::default()
        },
    )?;
    let ids = first(&reply.data, reply.used)
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    Ok(crate::cache::CacheIds {
        ids,
        fingerprint: reply.fingerprint,
    })
}

pub fn cache_archives(pid: u32, index: u32) -> Result<crate::cache::CacheIds, String> {
    cache_ids(pid, cache_kind::ARCHIVES, index, 0)
}

pub fn cache_archive_files(
    pid: u32,
    index: u32,
    archive: i32,
) -> Result<crate::cache::CacheIds, String> {
    cache_ids(pid, cache_kind::ARCHIVE_FILES, index, archive)
}

pub fn cache_files(
    pid: u32,
    index: u32,
    files: &[[i32; 2]],
) -> Result<Vec<Option<Vec<u8>>>, String> {
    cache_files_batched(pid, index, files, CACHE_BATCH)
}

fn cache_files_batched(
    pid: u32,
    index: u32,
    files: &[[i32; 2]],
    batch: usize,
) -> Result<Vec<Option<Vec<u8>>>, String> {
    cache_batches(
        pid,
        files,
        batch.clamp(1, CACHE_BATCH),
        |batch| {
            let mut request = CacheRequest {
                kind: cache_kind::FILES,
                index,
                count: batch.len() as u32,
                ..Default::default()
            };
            request.files[..batch.len()].copy_from_slice(batch);
            request
        },
        <[u8]>::to_vec,
        |&[archive, file]| format!("cache file {index}/{archive}/{file} is too big to be sent"),
    )
}

fn cache_batches<T, R>(
    pid: u32,
    items: &[T],
    batch: usize,
    request: impl Fn(&[T]) -> CacheRequest,
    decode: impl Fn(&[u8]) -> R,
    too_big: impl FnOnce(&T) -> String,
) -> Result<Vec<Option<R>>, String> {
    reader(pid, 0, "")?;
    let mut out: Vec<Option<R>> = items.iter().map(|_| None).collect();
    let mut next = 0;
    while next < items.len() {
        let batch = &items[next..(next + batch).min(items.len())];
        let reply = cache_request(pid, &request(batch))?;
        let mut done = 0;
        for (i, file) in reply.files[..batch.len()].iter().enumerate() {
            match file.length {
                CACHE_FILE_NO_ROOM => break,
                CACHE_FILE_MISSING => {}
                length => {
                    let start = file.offset as usize;
                    out[next + i] = reply.data.get(start..start + length as usize).map(&decode);
                }
            }
            done = i + 1;
        }
        if done == 0 {
            return Err(too_big(&batch[0]));
        }
        next += done;
    }
    Ok(out)
}

fn config_ids(pid: u32, archive: i32) -> Result<Vec<i32>, String> {
    Ok(
        cache_archive_files(pid, crate::cache::CONFIG_INDEX, archive)?
            .ids
            .iter()
            .map(|&id| id as i32)
            .collect(),
    )
}

fn config_files(pid: u32, archive: i32, ids: &[i32]) -> Result<Vec<Option<Vec<u8>>>, String> {
    let files: Vec<[i32; 2]> = ids.iter().map(|&id| [archive, id]).collect();
    cache_files(pid, crate::cache::CONFIG_INDEX, &files)
}

pub fn enum_def(pid: u32, id: i32) -> Result<Option<crate::config::EnumDef>, String> {
    let Some(data) = config_files(pid, crate::config::ENUM_ARCHIVE, &[id])?
        .pop()
        .flatten()
    else {
        return Ok(None);
    };
    crate::config::decode_enum(&data)
        .map(Some)
        .ok_or_else(|| format!("enum {id} has an opcode this reader doesn't know"))
}

fn item_defs<T>(
    pid: u32,
    ids: &[i32],
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Result<Vec<Option<T>>, String> {
    let files = config_files(pid, crate::config::OBJ_ARCHIVE, ids)?;
    ids.iter()
        .zip(files)
        .map(|(id, data)| match data {
            None => Ok(None),
            Some(data) => decode(&data)
                .map(Some)
                .ok_or_else(|| format!("item {id} has an opcode this reader doesn't know")),
        })
        .collect()
}

pub fn item_params(
    pid: u32,
    ids: &[i32],
) -> Result<Vec<Option<BTreeMap<u32, crate::config::Value>>>, String> {
    item_defs(pid, ids, crate::config::decode_obj_params)
}

pub fn item_icons(pid: u32, icons: &[ItemIconRequest]) -> Result<Vec<Option<Image>>, String> {
    cache_batches(
        pid,
        icons,
        CACHE_BATCH,
        |batch| {
            let mut request = CacheRequest {
                kind: cache_kind::ITEM_ICONS,
                count: batch.len() as u32,
                ..Default::default()
            };
            for (slot, icon) in request.files.iter_mut().zip(batch) {
                let flags = if icon.show_count {
                    ITEM_ICON_SHOW_COUNT
                } else {
                    0
                };
                *slot = [icon.id | flags, icon.quantity];
            }
            request
        },
        icon_from_argb,
        |_| "the agent had no room for an item icon".into(),
    )
}

fn icon_from_argb(words: &[u8]) -> Image {
    let mut rgba = Vec::with_capacity(words.len());
    for word in words.as_chunks::<4>().0 {
        let argb = u32::from_le_bytes(*word);
        if argb == 0 {
            rgba.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            rgba.extend_from_slice(&[
                (argb >> 16) as u8,
                (argb >> 8) as u8,
                argb as u8,
                (argb >> 24) as u8,
            ]);
        }
    }
    Image::from_rgba(ITEM_ICON_WIDTH, ITEM_ICON_HEIGHT, &rgba)
}

pub fn sprites(pid: u32, ids: &[i32]) -> Result<Vec<Option<Image>>, String> {
    let files: Vec<[i32; 2]> = ids.iter().map(|&id| [id, 0]).collect();
    let archives = cache_files(pid, crate::font::SPRITES_INDEX, &files)?;
    Ok(archives
        .into_iter()
        .map(|data| {
            let frames = crate::font::decode_sprites(&data?).ok()?;
            let frame = frames.into_iter().next()?;
            Some(Image::from_rgba(
                u32::try_from(frame.width).ok()?,
                u32::try_from(frame.height).ok()?,
                &frame.rgba,
            ))
        })
        .collect())
}

pub fn item_stackable(pid: u32, ids: &[i32]) -> Result<Vec<Option<bool>>, String> {
    item_defs(pid, ids, |data| {
        crate::config::decode_obj(data).map(|def| def.stackable)
    })
}

pub fn item_families(pid: u32, ids: &[i32]) -> Result<Vec<crate::config::ItemFamily>, String> {
    let decode = |data: Option<Vec<u8>>| {
        data.and_then(|data| crate::config::decode_obj(&data))
            .filter(|def| !def.copy)
            .and_then(|def| Some((def.name?, crate::config::bonuses_of(&def.params))))
    };
    let named: Vec<Option<(String, crate::config::Bonuses)>> =
        config_files(pid, crate::config::OBJ_ARCHIVE, ids)?
            .into_iter()
            .map(decode)
            .collect();
    let wanted: Vec<(i32, Option<&str>, crate::config::Bonuses)> = ids
        .iter()
        .copied()
        .zip(named.iter())
        .map(|(id, def)| match def {
            Some((name, bonuses)) => (id, Some(name.as_str()), *bonuses),
            None => (id, None, crate::config::Bonuses::default()),
        })
        .collect();
    let all = config_ids(pid, crate::config::OBJ_ARCHIVE)?;
    let every: Vec<(i32, String, crate::config::Bonuses)> = all
        .iter()
        .zip(config_files(pid, crate::config::OBJ_ARCHIVE, &all)?)
        .filter_map(|(&id, data)| {
            let (name, bonuses) = decode(data)?;
            Some((id, name, bonuses))
        })
        .collect();
    Ok(crate::config::item_families(
        &wanted,
        every.iter().map(|(id, name, bonuses)| (*id, name.as_str(), *bonuses)),
    ))
}

pub fn varbits_of(
    pid: u32,
    varp: u16,
) -> Result<Vec<(i32, crate::config::VarbitDef, Option<String>)>, String> {
    let ids = config_ids(pid, crate::config::VARBIT_ARCHIVE)?;
    let mut found = Vec::new();
    for (&id, data) in ids
        .iter()
        .zip(config_files(pid, crate::config::VARBIT_ARCHIVE, &ids)?)
    {
        if let Some(def) = data
            .as_deref()
            .and_then(crate::config::decode_varbit)
            .filter(|d| d.varp == varp)
        {
            found.push((id, def));
        }
    }
    found.sort_by_key(|(_, d)| d.lsb);
    let names = game_val_names(
        pid,
        ipc::state::game_val::VARBIT,
        &found.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
    )?;
    Ok(found
        .into_iter()
        .zip(names)
        .map(|((id, def), name)| (id, def, name))
        .collect())
}

type PerClient<K, V> = Option<(u32, HashMap<K, V>)>;

fn of_client<K, V>(cache: &PerClient<K, V>, pid: u32) -> Option<&HashMap<K, V>> {
    cache
        .as_ref()
        .filter(|(p, _)| *p == pid)
        .map(|(_, map)| map)
}

fn of_client_mut<K, V>(cache: &mut PerClient<K, V>, pid: u32) -> &mut HashMap<K, V> {
    if cache.as_ref().is_some_and(|(p, _)| *p != pid) {
        *cache = None;
    }
    &mut cache.get_or_insert_with(|| (pid, HashMap::new())).1
}

static VARBITS: Mutex<PerClient<i32, Option<crate::config::VarbitDef>>> = Mutex::new(None);

fn varbit_def(pid: u32, id: i32) -> Result<Option<crate::config::VarbitDef>, String> {
    if let Some(def) = of_client(&lock(&VARBITS), pid).and_then(|defs| defs.get(&id)) {
        return Ok(*def);
    }
    let def = match config_files(pid, crate::config::VARBIT_ARCHIVE, &[id])?
        .pop()
        .flatten()
    {
        None => None,
        Some(data) => Some(
            crate::config::decode_varbit(&data)
                .ok_or_else(|| format!("varbit {id} has an opcode this reader doesn't know"))?,
        ),
    };
    of_client_mut(&mut lock(&VARBITS), pid).insert(id, def);
    Ok(def)
}

fn known_varbit(pid: u32, id: i32) -> Result<crate::config::VarbitDef, String> {
    varbit_def(pid, id)?.ok_or_else(|| format!("the cache has no varbit {id}"))
}

pub fn varbit(pid: u32, id: i32) -> Result<Option<i32>, String> {
    let def = known_varbit(pid, id)?;
    Ok(varp_value(pid, def.varp as usize)?.map(|varp| def.value(varp)))
}

type ShownLoc = (crate::collision::LocDef, crate::collision::LocText);

static LOC_TEXTS: Mutex<PerClient<u32, Option<ShownLoc>>> = Mutex::new(None);

const CACHE_LOCS_MAX_SQUARES: i32 = 64;

fn read_loc_texts(pid: u32, ids: &[u32]) -> Result<(), String> {
    use crate::{cache::*, collision::*};
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let missing: Vec<u32> = {
        let cached = lock(&LOC_TEXTS);
        let known = of_client(&cached, pid);
        ids.iter()
            .copied()
            .filter(|id| !known.is_some_and(|defs| defs.contains_key(id)))
            .collect()
    };
    if missing.is_empty() {
        return Ok(());
    }
    let files: Vec<[i32; 2]> = missing.iter().map(|&id| [LOC_ARCHIVE, id as i32]).collect();
    let defs = cache_files(pid, CONFIG_INDEX, &files)?;
    let mut cached = lock(&LOC_TEXTS);
    let known = of_client_mut(&mut cached, pid);
    for (id, data) in missing.iter().zip(defs) {
        known.insert(*id, data.as_deref().and_then(decode_loc_text));
    }
    Ok(())
}

fn shown_locs(pid: u32, ids: &[u32]) -> Result<HashMap<u32, ShownLoc>, String> {
    let lookup = |ids: &[u32]| -> Result<Vec<Option<ShownLoc>>, String> {
        read_loc_texts(pid, ids)?;
        let cached = lock(&LOC_TEXTS);
        let known = of_client(&cached, pid);
        Ok(ids
            .iter()
            .map(|id| known.and_then(|defs| defs.get(id)).cloned().flatten())
            .collect())
    };
    let mut shown = HashMap::new();
    let mut transforming = Vec::new();
    for (&id, def) in ids.iter().zip(lookup(ids)?) {
        let Some(def) = def else { continue };
        match &def.1.transforms {
            None => {
                shown.insert(id, def);
            }
            Some(t) => transforming.push((id, t.clone())),
        }
    }
    let mut transformed: Vec<(u32, u32)> = Vec::new();
    if !transforming.is_empty() {
        let varps = varps(pid)?;
        let varp = |index: usize| varps.as_ref().and_then(|v| v.get(index).copied());
        for (id, t) in transforming {
            let value = match (t.varbit, t.varp) {
                (Some(bit), _) => {
                    let def = known_varbit(pid, bit as i32)?;
                    varp(def.varp as usize).map(|v| def.value(v))
                }
                (None, Some(index)) => varp(index as usize),
                (None, None) => None,
            };
            if let Some(to) = t.pick(value) {
                transformed.push((id, to));
            }
        }
    }
    let targets: Vec<u32> = transformed.iter().map(|&(_, to)| to).collect();
    for ((id, _), def) in transformed.into_iter().zip(lookup(&targets)?) {
        if let Some(def) = def {
            shown.insert(id, def);
        }
    }
    Ok(shown)
}

fn cache_placements(
    pid: u32,
    area: [i32; 4],
    plane_wanted: Option<i32>,
) -> Result<Vec<(crate::collision::Placement, i32, i32)>, String> {
    use crate::{cache::*, collision::*};
    let [x1, y1, x2, y2] = area;
    if x1 > x2 || y1 > y2 || x1 < 0 || y1 < 0 {
        return Err(format!("{x1}, {y1} to {x2}, {y2} isn't an area"));
    }
    let squares = i64::from((x2 >> 6) - (x1 >> 6) + 1) * i64::from((y2 >> 6) - (y1 >> 6) + 1);
    if squares > i64::from(CACHE_LOCS_MAX_SQUARES) {
        return Err(format!(
            "the area covers {squares} map squares, at most {CACHE_LOCS_MAX_SQUARES}"
        ));
    }
    let mut regions = Vec::new();
    for rx in (x1 >> 6)..=(x2 >> 6) {
        for ry in (y1 >> 6)..=(y2 >> 6) {
            regions.push((rx, ry));
        }
    }
    let files: Vec<[i32; 2]> = regions
        .iter()
        .map(|&(rx, ry)| [region_id(rx, ry) as i32, 1])
        .collect();
    let mut found = Vec::new();
    for (data, &(rx, ry)) in cache_files(pid, MAPS_INDEX, &files)?
        .into_iter()
        .zip(&regions)
    {
        let Some(data) = data else { continue };
        let placements = decode_locs(&data)
            .ok_or_else(|| format!("map square {}'s locs don't decode", region_id(rx, ry)))?;
        for p in placements {
            let (x, y, plane) = (rx * 64 + p.x as i32, ry * 64 + p.y as i32, p.plane as i32);
            if x >= x1
                && x <= x2
                && y >= y1
                && y <= y2
                && plane_wanted.is_none_or(|want| want == plane)
            {
                found.push((p, x, y));
            }
        }
    }
    Ok(found)
}

pub fn loc_shown(pid: u32, ids: &[u32]) -> Result<Vec<crate::LocView>, String> {
    let shown = shown_locs(pid, ids)?;
    Ok(ids
        .iter()
        .map(|id| {
            shown
                .get(id)
                .map(|(_, text)| crate::LocView {
                    name: text.name.clone(),
                    actions: shown_actions(text),
                })
                .unwrap_or_default()
        })
        .collect())
}

fn shown_actions(text: &crate::collision::LocText) -> Vec<String> {
    text.actions
        .iter()
        .filter(|a| !a.is_empty())
        .cloned()
        .collect()
}

pub fn cache_locs(
    pid: u32,
    area: [i32; 4],
    filter: &crate::CacheLocFilter,
) -> Result<Vec<crate::CacheLoc>, String> {
    let found = cache_placements(pid, area, filter.plane)?;
    let ids: Vec<u32> = found.iter().map(|(p, _, _)| p.id).collect();
    read_loc_texts(pid, &ids)?;

    let lower = |list: &Option<Vec<String>>| {
        list.as_ref()
            .map(|l| l.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>())
    };
    let (names, actions) = (lower(&filter.names), lower(&filter.actions));
    let cached = lock(&LOC_TEXTS);
    let known = of_client(&cached, pid);
    let mut out = Vec::new();
    for (p, x, y) in found {
        let Some((def, text)) = known
            .and_then(|defs| defs.get(&p.id))
            .and_then(|d| d.as_ref())
        else {
            continue;
        };
        let name = text.name.to_lowercase();
        if names
            .as_ref()
            .is_some_and(|names| !names.iter().any(|n| name.contains(n.as_str())))
        {
            continue;
        }
        if actions.as_ref().is_some_and(|wanted| {
            !text
                .actions
                .iter()
                .any(|a| wanted.contains(&a.to_lowercase()))
        }) {
            continue;
        }
        let (size_x, size_y) = crate::collision::turned(def.size_x, def.size_y, p.rotation);
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
            actions: shown_actions(text),
        });
    }
    out.sort_by_key(|l| (l.plane, l.x, l.y, l.id));
    Ok(out)
}

fn cache_climbables(
    pid: u32,
    area: [i32; 4],
    request: &crate::nav::Request,
    flags: crate::nav::Flags,
    loaded: &dyn Fn(i32, i32) -> bool,
) -> Result<Vec<crate::nav::Climbable>, String> {
    let found: Vec<_> = cache_placements(pid, area, None)?
        .into_iter()
        .filter(|&(_, x, y)| !loaded(x, y))
        .collect();
    let ids: Vec<u32> = found.iter().map(|(p, _, _)| p.id).collect();
    read_loc_texts(pid, &ids)?;
    let cached = lock(&LOC_TEXTS);
    let known = of_client(&cached, pid);
    let lookup = |id: i32| {
        known
            .and_then(|defs| defs.get(&(id as u32)))
            .and_then(|d| d.as_ref())
    };
    let locs: Vec<crate::Loc> = found
        .iter()
        .filter_map(|&(p, x, y)| {
            let (def, _) = lookup(p.id as i32)?;
            let (size_x, size_y) = crate::collision::turned(def.size_x, def.size_y, p.rotation);
            let kind = match p.shape {
                0..=3 => 0,
                4..=8 => 1,
                9..=21 => 2,
                _ => 3,
            };
            Some(crate::Loc {
                id: p.id as i32,
                name: None,
                kind,
                x,
                y,
                plane: p.plane as i32,
                scene_x: 0,
                scene_y: 0,
                size_x,
                size_y,
                shape: Some(p.shape),
                rotation: Some(p.rotation),
                clickable: true,
            })
        })
        .collect();
    Ok(crate::nav::scene_locs(flags, &locs, &lookup, request).1)
}

fn world_collision(pid: u32, regions: &[u32]) -> Result<crate::collision::WorldCollision, String> {
    use crate::{cache::*, collision::*};
    let data = cache_files(pid, MAPS_INDEX, &map_square_files(regions))?;
    let mut squares = Vec::new();
    for (pair, &region) in data.as_chunks::<2>().0.iter().zip(regions) {
        let (Some(terrain), Some(locs)) = (&pair[0], &pair[1]) else {
            continue;
        };
        let settings = decode_terrain(terrain)
            .ok_or_else(|| format!("map square {region}'s terrain doesn't decode"))?;
        let locs =
            decode_locs(locs).ok_or_else(|| format!("map square {region}'s locs don't decode"))?;
        squares.push((region, settings, locs));
    }
    let mut ids: Vec<u32> = squares
        .iter()
        .flat_map(|(_, _, locs)| locs.iter().map(|l| l.id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let files: Vec<[i32; 2]> = ids.iter().map(|&id| [LOC_ARCHIVE, id as i32]).collect();
    let defs: HashMap<u32, LocDef> = ids
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

fn map_square_files(regions: &[u32]) -> Vec<[i32; 2]> {
    regions
        .iter()
        .flat_map(|&r| [[r as i32, 0], [r as i32, 1]])
        .collect()
}

const WORLD_SQUARE_BATCH: usize = 16;
const WORLD_FORMAT: u64 = 8;

type WorldMovement = Arc<crate::collision::WorldCollision<crate::collision::Movement>>;
static WORLD: Mutex<Option<(u64, u32, WorldMovement)>> = Mutex::new(None);

fn world_cache_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("injector")
}

fn world_movement(pid: u32) -> Result<WorldMovement, String> {
    use crate::{cache::*, collision::*};
    if let Some((_, _, world)) = lock(&WORLD).as_ref().filter(|(_, p, _)| *p == pid) {
        return Ok(world.clone());
    }
    let maps = cache_archives(pid, MAPS_INDEX)?;
    let loc_ids = cache_archive_files(pid, CONFIG_INDEX, LOC_ARCHIVE)?;
    let key = maps.fingerprint ^ loc_ids.fingerprint.rotate_left(17) ^ WORLD_FORMAT.rotate_left(42);
    let mut cached = lock(&WORLD);
    if let Some((_, _, world)) = cached.as_ref().filter(|(k, _, _)| *k == key) {
        let world = world.clone();
        *cached = Some((key, pid, world.clone()));
        return Ok(world);
    }
    let path = world_cache_dir().join(format!("collision-{key:016x}.bin"));
    if let Some(world) = fs::read(&path)
        .ok()
        .and_then(|bytes| WorldCollision::from_bytes(&bytes, key))
    {
        let world = Arc::new(world);
        *cached = Some((key, pid, world.clone()));
        return Ok(world);
    }

    let started = Instant::now();
    eprintln!(
        "building world collision from the client's cache ({} map squares, {} loc definitions)",
        maps.ids.len(),
        loc_ids.ids.len()
    );
    let files: Vec<[i32; 2]> = loc_ids
        .ids
        .iter()
        .map(|&id| [LOC_ARCHIVE, id as i32])
        .collect();
    let mut defs = vec![None; loc_ids.ids.iter().max().map_or(0, |&m| m as usize + 1)];
    let mut undecoded = Vec::new();
    for (&id, data) in loc_ids
        .ids
        .iter()
        .zip(cache_files(pid, CONFIG_INDEX, &files)?)
    {
        match data.as_deref().map(decode_loc) {
            Some(Some(def)) => defs[id as usize] = Some(def),
            Some(None) => undecoded.push(id),
            None => {}
        }
    }
    if !undecoded.is_empty() {
        eprintln!(
            "{} loc definitions didn't decode (left out): {:?}",
            undecoded.len(),
            &undecoded[..undecoded.len().min(20)]
        );
    }
    let def = |id: u32| defs.get(id as usize).copied().flatten();

    let mut world = WorldCollision::<Movement>::default();
    for &region in &maps.ids {
        world.add_square(region);
    }
    for (done, regions) in maps.ids.chunks(WORLD_SQUARE_BATCH).enumerate() {
        let files = map_square_files(regions);
        let data = cache_files_batched(pid, MAPS_INDEX, &files, files.len())?;
        for (pair, &region) in data.as_chunks::<2>().0.iter().zip(regions) {
            let Some(settings) = pair[0].as_deref().and_then(decode_terrain) else {
                world.remove_square(region);
                continue;
            };
            let locs = pair[1]
                .as_deref()
                .map_or(Some(Vec::new()), decode_locs)
                .unwrap_or_else(|| {
                    eprintln!("map square {region}'s locs don't decode");
                    Vec::new()
                });
            world.add_terrain_and_locs(region, &settings, &locs, &def);
        }
        if done % 20 == 0 {
            eprintln!(
                "  {}/{} map squares",
                (done + 1) * WORLD_SQUARE_BATCH,
                maps.ids.len()
            );
        }
    }
    world.shrink();
    let bytes = world.to_bytes(key);
    if let Err(e) = fs::create_dir_all(world_cache_dir()).and_then(|()| fs::write(&path, &bytes)) {
        eprintln!(
            "couldn't keep the world collision in {}: {e}",
            path.display()
        );
    }
    eprintln!(
        "world collision built in {:.1} s ({} KB on disk)",
        started.elapsed().as_secs_f32(),
        bytes.len() / 1024
    );
    let world = Arc::new(world);
    *cached = Some((key, pid, world.clone()));
    Ok(world)
}

static WORLD_MAP: Mutex<Option<KeptWorldMap>> = Mutex::new(None);
type KeptWorldMap = ((WorldMovement, u64), Arc<crate::landmass::WorldMap>);

const LANDMASS_RADIUS: i32 = 3;

pub fn landmasses(pid: u32, points: &[[i32; 3]]) -> Result<Vec<u32>, String> {
    let map = world_map(pid)?;
    Ok(points
        .iter()
        .map(|&[x, y, plane]| map.masses.near(x, y, plane, LANDMASS_RADIUS))
        .collect())
}

fn world_map(pid: u32) -> Result<Arc<crate::landmass::WorldMap>, String> {
    let world = world_movement(pid)?;
    let version = crate::transports::version();
    let mut kept = lock(&WORLD_MAP);
    if let Some((_, map)) = kept
        .as_ref()
        .filter(|((w, v), _)| Arc::ptr_eq(w, &world) && *v == version)
    {
        return Ok(map.clone());
    }
    let started = Instant::now();
    let doors: Vec<(crate::nav::Point, crate::nav::Point)> =
        crate::transports::with_climbs(|all| {
            all.iter()
                .filter(|t| crate::transports::is_gated_door(t))
                .filter_map(transport_ends)
                .collect()
        });
    let mut closed = crate::nav::Closed::default();
    for &(o, d) in &doors {
        closed.insert((o, (d.0 - o.0, d.1 - o.1)));
        closed.insert((d, (o.0 - d.0, o.1 - d.1)));
    }
    let both_ways: HashSet<_> = doors.iter().copied().collect();
    let exits: Vec<_> = doors
        .iter()
        .filter(|&&(o, d)| !both_ways.contains(&(d, o)))
        .map(|&(o, d)| (d, o))
        .collect();
    let masses = crate::landmass::Landmasses::build(&world, &closed);
    let rows: Vec<_> = crate::transports::with_climbs(|all| {
        all.iter()
            .filter_map(|t| {
                let (o, d) = transport_ends(t)?;
                let key = crate::transports::gated(t).then(|| crate::transports::climb_key(t));
                Some((o, d, key, crate::transports::touches_wilderness(t)))
            })
            .collect()
    });
    let map = Arc::new(crate::landmass::WorldMap::new(
        masses,
        rows,
        exits,
        LANDMASS_RADIUS,
    ));
    eprintln!(
        "{} landmasses found in {:.1} s",
        map.masses.count,
        started.elapsed().as_secs_f32()
    );
    *kept = Some(((world, version), map.clone()));
    Ok(map)
}

fn transport_ends(
    t: &crate::transports::Transport,
) -> Option<(crate::nav::Point, crate::nav::Point)> {
    let (o, d) = (t.origin?, t.destination?);
    Some(((o.x, o.y, o.plane), (d.x, d.y, d.plane)))
}

fn walk_in_wilderness(from: crate::nav::Point, to: crate::nav::Point) -> bool {
    crate::transports::in_wilderness(from.0, from.1) || crate::transports::in_wilderness(to.0, to.1)
}

pub fn world_route(
    pid: u32,
    from: crate::nav::Point,
    to: crate::nav::Point,
    allow_climbs: &[String],
) -> Result<crate::landmass::Route, String> {
    let map = world_map(pid)?;
    let allowed: HashSet<&str> = allow_climbs.iter().map(String::as_str).collect();
    Ok(map.route(from, to, &allowed, walk_in_wilderness(from, to)))
}

pub fn find_path(
    pid: u32,
    plane: i32,
    from: [i32; 2],
    to: [i32; 2],
    max_tiles: Option<usize>,
) -> Result<Vec<[i32; 2]>, String> {
    use crate::pathfinding::{self, PathError};
    let world = world_movement(pid)?;
    match pathfinding::find_path(
        &world,
        plane,
        (from[0], from[1]),
        (to[0], to[1]),
        max_tiles.unwrap_or(PATH_MAX_TILES).min(PATH_MAX_TILES),
    ) {
        Ok(path) => Ok(path.into_iter().map(|(x, y)| [x, y]).collect()),
        Err(PathError::Blocked((x, y))) => {
            Err(format!("{x}, {y} (plane {plane}) can't be stood on"))
        }
        Err(PathError::NoPath) => Err(format!(
            "no walking path from {}, {} to {}, {} on plane {plane}",
            from[0], from[1], to[0], to[1]
        )),
        Err(PathError::TooFar(n)) => Err(format!("no walking path found within {n} tiles")),
    }
}

const PATH_MAX_TILES: usize = 4_000_000;

static SEEN_SCENES: Mutex<Vec<Arc<crate::scene_collision::SceneCollision>>> =
    Mutex::new(Vec::new());
const SEEN_SCENES_KEPT: usize = 4;

type RegionFlags = HashMap<u32, Arc<crate::collision::WorldCollision>>;

static REGION_FLAGS: LazyLock<Mutex<RegionFlags>> = LazyLock::new(Default::default);

fn whole_squares((x1, y1, x2, y2): (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    use crate::collision::REGION_SIZE;
    let square = |v: i32| v.div_euclid(REGION_SIZE) * REGION_SIZE;
    (
        square(x1),
        square(y1),
        square(x2) + REGION_SIZE - 1,
        square(y2) + REGION_SIZE - 1,
    )
}

fn region_flags(regions: &RegionFlags, plane: i32, x: i32, y: i32) -> Option<u32> {
    use crate::collision::{REGION_SIZE, region_id};
    let region = region_id(x.div_euclid(REGION_SIZE), y.div_euclid(REGION_SIZE));
    regions.get(&region)?.flags(plane, x, y)
}

const SCENE_EDGE: i32 = 4;

fn cache_regions(pid: u32, (x1, y1, x2, y2): (i32, i32, i32, i32)) -> Result<RegionFlags, String> {
    use crate::collision::{REGION_SIZE, region_id};
    let wanted: Vec<u32> = (x1.div_euclid(REGION_SIZE)..=x2.div_euclid(REGION_SIZE))
        .flat_map(|rx| {
            (y1.div_euclid(REGION_SIZE)..=y2.div_euclid(REGION_SIZE))
                .map(move |ry| region_id(rx, ry))
        })
        .collect();
    let mut cached = lock(&REGION_FLAGS);
    let missing: Vec<u32> = wanted
        .iter()
        .copied()
        .filter(|r| !cached.contains_key(r))
        .collect();
    if !missing.is_empty() {
        let world = Arc::new(world_collision(pid, &missing)?);
        cached.extend(missing.into_iter().map(|r| (r, world.clone())));
    }
    Ok(wanted.iter().map(|r| (*r, cached[r].clone())).collect())
}

pub fn scene_route(
    pid: u32,
    from: [i32; 2],
    to: [i32; 2],
    options: Option<crate::scene_graph::Options>,
) -> Result<crate::pathfinding::Route, String> {
    use crate::scene_collision::RING;
    const MAX_WINDOW: i32 = 400;
    let (layout, memory) = reader(pid, part::COLLISION, "the scene's collision maps")?;
    if state::current(&memory, &layout) != Some(LOGGED_IN) {
        return Err("not logged in".into());
    }
    let scene = crate::scene_collision::read(&memory, &layout)
        .ok_or("the scene's collision map can't be read")?;
    if scene.flags(from[0], from[1]).is_none() {
        return Err(format!(
            "{}, {} isn't in the loaded scene",
            from[0], from[1]
        ));
    }
    let (from, to) = ((from[0], from[1]), (to[0], to[1]));
    let seen: Vec<Arc<crate::scene_collision::SceneCollision>> = if options.is_some() {
        let mut seen = lock(&SEEN_SCENES);
        seen.retain(|s| (s.base_x, s.base_y, s.plane) != (scene.base_x, scene.base_y, scene.plane));
        seen.insert(0, Arc::new(scene.clone()));
        seen.truncate(SEEN_SCENES_KEPT);
        seen.iter()
            .skip(1)
            .filter(|s| s.plane == scene.plane)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let route = match options {
        None => crate::pathfinding::game_route(|x, y| scene.flags(x, y), from, to),
        Some(options) => {
            let (x1, y1, x2, y2) = scene.bounds();
            let bounds = if scene.flags(to.0, to.1).is_some_and(|f| f != RING) {
                (x1, y1, x2, y2)
            } else {
                whole_squares((
                    x1.min(to.0.saturating_sub(12)),
                    y1.min(to.1.saturating_sub(12)),
                    x2.max(to.0.saturating_add(12)),
                    y2.max(to.1.saturating_add(12)),
                ))
            };
            let span = |low: i32, high: i32| i64::from(high) - i64::from(low);
            if span(bounds.0, bounds.2) >= i64::from(MAX_WINDOW)
                || span(bounds.1, bounds.3) >= i64::from(MAX_WINDOW)
            {
                return Err(format!(
                    "{}, {} is too far from {}, {} for a route",
                    to.0, to.1, from.0, from.1
                ));
            }
            let regions = cache_regions(pid, bounds)?;
            let plane = scene.plane;
            let live = |s: &crate::scene_collision::SceneCollision, x: i32, y: i32| {
                let inner = |v: i32, base: i32| {
                    (base + SCENE_EDGE..base + SCENE_TILES - SCENE_EDGE).contains(&v)
                };
                (inner(x, s.base_x) && inner(y, s.base_y))
                    .then(|| s.flags(x, y))
                    .flatten()
                    .filter(|&f| f != RING)
            };
            let flags = |x: i32, y: i32| {
                live(&scene, x, y)
                    .or_else(|| seen.iter().find_map(|s| live(s, x, y)))
                    .or_else(|| region_flags(&regions, plane, x, y))
            };
            crate::scene_graph::route(&flags, bounds, from, to, &options)
        }
    };
    route.ok_or_else(|| {
        format!(
            "no route from {}, {} to {}, {} or near it",
            from.0, from.1, to.0, to.1
        )
    })
}

fn climb_rows(
    request: &crate::nav::Request,
    wilderness: bool,
) -> (Vec<crate::nav::ClimbRow>, Vec<String>, crate::nav::Closed) {
    use crate::transports::{climb_key, gated, is_gated_door, touches_wilderness, with_climbs};
    let allowed: HashSet<&str> = request.allow_climbs.iter().map(String::as_str).collect();
    let usable = |t: &crate::transports::Transport| {
        (!gated(t) || allowed.contains(climb_key(t).as_str()))
            && (wilderness || !touches_wilderness(t))
    };
    let closed = with_climbs(|all| {
        all.iter()
            .filter(|t| is_gated_door(t) && !usable(t))
            .filter_map(|t| {
                let (o, d) = transport_ends(t)?;
                Some((o, (d.0 - o.0, d.1 - o.1)))
            })
            .collect()
    });
    if !request.climbs {
        return (Vec::new(), Vec::new(), closed);
    }
    let mut options: Vec<String> = Vec::new();
    let portal_names: Vec<String> = request
        .portal_names
        .iter()
        .map(|p| p.to_lowercase())
        .collect();
    let rows = with_climbs(|all| {
        let mut rows = Vec::new();
        for t in all {
            let Some((origin, destination)) = transport_ends(t) else {
                continue;
            };
            if !usable(t) {
                continue;
            }
            let Some(option) = crate::transports::menu_option(&t.menu) else {
                continue;
            };
            let id = t
                .menu
                .split_whitespace()
                .last()
                .and_then(|w| w.parse().ok());
            if !options.iter().any(|o| o.eq_ignore_ascii_case(option)) {
                options.push(option.to_owned());
            }
            let portal = !portal_names.is_empty() && {
                let words: Vec<&str> = t.menu.split_whitespace().collect();
                let name = words
                    .get(1..words.len().saturating_sub(1))
                    .map_or(String::new(), |w| w.join(" "))
                    .to_lowercase();
                portal_names.iter().any(|p| name.contains(p))
            };
            rows.push(crate::nav::ClimbRow {
                origin,
                destination,
                option: option.to_owned(),
                id: id.unwrap_or(0),
                shortcut_ticks: (t.kind == crate::transports::SHORTCUT_KIND)
                    .then_some(if t.duration > 0 { t.duration } else { 4 }),
                portal,
            });
        }
        rows
    });
    (rows, options, closed)
}

pub fn nav_plan(
    pid: u32,
    from: crate::nav::Point,
    to: crate::nav::Point,
    request: &crate::nav::Request,
) -> Result<crate::nav::NavPlan, String> {
    use crate::{
        collision::flag,
        nav::{NavAction, NavPlan, NavTile, UNDERGROUND},
    };
    const MAX_WINDOW: i32 = 448;
    const MARGIN: i32 = 16;
    let (layout, memory) = reader(pid, part::COLLISION, "the scene's collision maps")?;
    if state::current(&memory, &layout) != Some(LOGGED_IN) {
        return Err("not logged in".into());
    }
    let live = crate::scene_collision::read_all(&memory, &layout)
        .ok_or("the scene's collision maps can't be read")?;
    let scene = live
        .iter()
        .flatten()
        .next()
        .ok_or("the scene has no collision maps")?;
    let (base_x, base_y) = (scene.base_x, scene.base_y);
    let instance = in_instance(pid).unwrap_or(true);
    let in_scene = |x: i32, y: i32, edge: i32| {
        (base_x + edge..base_x + SCENE_TILES - edge).contains(&x)
            && (base_y + edge..base_y + SCENE_TILES - edge).contains(&y)
    };
    if !in_scene(from.0, from.1, 0) {
        return Err(format!("{}, {} isn't in the loaded scene", from.0, from.1));
    }

    let shift = if base_y >= UNDERGROUND {
        UNDERGROUND
    } else {
        0
    };
    let across_to = |tile: crate::nav::Point| !instance && (tile.1 >= UNDERGROUND) != (shift != 0);
    let goal_across = across_to(to);
    let folded = |y: i32| if y >= UNDERGROUND { y - UNDERGROUND } else { y };
    let scene_box = (
        base_x,
        base_y - shift,
        base_x + SCENE_TILES - 1,
        base_y - shift + SCENE_TILES - 1,
    );
    let grown = |(x1, y1, x2, y2): (i32, i32, i32, i32), (x, y): (i32, i32)| {
        (
            x1.min(x.saturating_sub(MARGIN)),
            y1.min(y.saturating_sub(MARGIN)),
            x2.max(x.saturating_add(MARGIN)),
            y2.max(y.saturating_add(MARGIN)),
        )
    };
    let fits = |(x1, y1, x2, y2): (i32, i32, i32, i32)| {
        let wide = i64::from(MAX_WINDOW);
        i64::from(x2) - i64::from(x1) < wide && i64::from(y2) - i64::from(y1) < wide
    };
    const WIDEN: [i32; 3] = [48, 112, 224];
    let widen = |(x1, y1, x2, y2): (i32, i32, i32, i32), extra: i32| {
        let trim = |lo: i32, hi: i32| {
            let over = (hi - lo) - (MAX_WINDOW - 1);
            if over > 0 {
                (lo + over / 2, hi - (over - over / 2))
            } else {
                (lo, hi)
            }
        };
        let (x1, x2) = trim((x1 - extra).max(0), x2 + extra);
        let (y1, y2) = trim((y1 - extra).max(0), y2 + extra);
        (x1, y1, x2, y2)
    };
    let (rows, climb_options, closed) = climb_rows(request, walk_in_wilderness(from, to));
    let stop_along = |target: crate::nav::Point| -> Result<Option<crate::nav::Point>, String> {
        let world = world_movement(pid)?;
        let path = crate::pathfinding::find_path_with(
            &world,
            target.2,
            (from.0, from.1),
            (target.0, target.1),
            PATH_MAX_TILES,
            true,
            &closed,
        )
        .unwrap_or_default();
        Ok(path
            .iter()
            .rev()
            .find(|&&(x, y)| fits(grown(scene_box, (x, y - shift))))
            .map(|&(x, y)| (x, y, target.2)))
    };
    type Stop = (crate::nav::Point, (i32, i32, i32, i32));
    let through = |o: crate::nav::Point, d: crate::nav::Point| -> Result<Option<Stop>, String> {
        let at = |t: crate::nav::Point| (t.0, folded(t.1));
        let both = grown(grown(scene_box, at(o)), at(d));
        if fits(both) {
            return Ok(Some((d, both)));
        }
        if fits(grown(scene_box, at(o))) {
            return Ok(Some((o, grown(scene_box, at(o)))));
        }
        Ok(stop_along(o)?.map(|stop| (stop, grown(scene_box, (stop.0, folded(stop.1))))))
    };
    let route = |allow: &[String]| -> crate::landmass::Route {
        if instance {
            crate::landmass::Route::Unknown
        } else {
            world_route(pid, from, to, allow).unwrap_or(crate::landmass::Route::Unknown)
        }
    };
    let mut goal = to;
    let mut bounds = scene_box;
    if (goal_across || !in_scene(to.0, to.1, 0)) && !instance {
        if fits(grown(scene_box, (to.0, folded(to.1)))) {
            bounds = grown(scene_box, (to.0, folded(to.1)));
        } else if let crate::landmass::Route::Through(o, d) = route(&request.allow_climbs) {
            if let Some((stop, wide)) = through(o, d)? {
                goal = stop;
                bounds = wide;
            }
        } else if from.2 == to.2
            && !goal_across
            && let Some(stop) = stop_along(to)?
        {
            goal = stop;
            bounds = grown(scene_box, (stop.0, stop.1 - shift));
        }
    }
    let requested_climbs = request.allow_climbs.clone();
    let request = &crate::nav::Request {
        climb_options,
        ..request.clone()
    };
    let locs = clickable_locs(pid)?;
    let mut ids: Vec<u32> = locs.iter().map(|l| l.id as u32).collect();
    ids.sort_unstable();
    ids.dedup();
    let defs = shown_locs(pid, &ids)?;
    let lookup = |id: i32| defs.get(&(id as u32));

    let attempt = |across: bool,
                   bounds: (i32, i32, i32, i32),
                   goal: crate::nav::Point|
     -> Result<Option<crate::nav::Plan>, String> {
        let regions = if instance {
            RegionFlags::new()
        } else {
            let squares = |north: i32| {
                cache_regions(
                    pid,
                    whole_squares((bounds.0, bounds.1 + north, bounds.2, bounds.3 + north)),
                )
            };
            let mut regions = squares(shift)?;
            if across {
                regions.extend(squares(UNDERGROUND - shift)?);
            }
            regions
        };
        let cache = |x: i32, y: i32, plane: i32| {
            let flags = region_flags(&regions, plane, x, y)?;
            Some(flags & !flag::door_blocks(flags) & !flag::DOORS)
        };
        let edge = if regions.is_empty() { 0 } else { SCENE_EDGE };
        let flags = |x: i32, y: i32, plane: i32| -> Option<u32> {
            let scene = live.get(plane as usize).and_then(Option::as_ref);
            if in_scene(x, y, edge) {
                return scene.and_then(|s| s.flags(x, y));
            }
            cache(x, y, plane).or_else(|| {
                scene
                    .and_then(|s| s.flags(x, y))
                    .filter(|_| in_scene(x, y, 0))
            })
        };
        let (doors, mut climbables) = crate::nav::scene_locs(&flags, &locs, &lookup, request);
        if across && !instance {
            for north in [shift, UNDERGROUND - shift] {
                let area = [
                    bounds.0.max(0),
                    (bounds.1 + north).max(0),
                    bounds.2,
                    bounds.3 + north,
                ];
                let loaded = |x: i32, y: i32| north == shift && in_scene(x, y, edge);
                climbables.extend(cache_climbables(pid, area, request, &flags, &loaded)?);
            }
        }
        let (x1, y1, x2, y2) = bounds;
        let in_box: Vec<crate::nav::ClimbRow> = rows
            .iter()
            .filter(|r| (x1..=x2).contains(&r.origin.0) && (y1..=y2).contains(&folded(r.origin.1)))
            .cloned()
            .collect();
        let climbs = crate::nav::data_climbs(&flags, &climbables, &in_box, &request.door_options);
        Ok(crate::nav::plan(
            &flags,
            bounds,
            &doors,
            &climbs,
            from,
            goal,
            &request.options,
            &closed,
        ))
    };
    let in_box = |bounds: (i32, i32, i32, i32),
                  goal: crate::nav::Point|
     -> Result<Option<crate::nav::Plan>, String> {
        let goal_across = across_to(goal);
        let mut plan = attempt(goal_across, bounds, goal)?;
        if !goal_across && !instance && goal == to && plan.as_ref().is_none_or(|p| !p.reached) {
            plan = match (attempt(true, bounds, goal)?, plan) {
                (Some(through), first) if through.reached || first.is_none() => Some(through),
                (_, first) => first,
            };
        }
        Ok(plan)
    };
    let mut plan = in_box(bounds, goal)?;
    if !instance && plan.as_ref().is_none_or(|p| !p.reached) {
        let mut last = bounds;
        for extra in WIDEN {
            let wide = widen(bounds, extra);
            if wide == last {
                continue;
            }
            last = wide;
            plan = match (in_box(wide, goal)?, plan) {
                (Some(wider), first) if wider.reached || first.is_none() => Some(wider),
                (_, first) => first,
            };
            if plan.as_ref().is_some_and(|p| p.reached) {
                break;
            }
        }
    }
    if goal == to
        && plan.as_ref().is_none_or(|p| !p.reached)
        && let crate::landmass::Route::Through(o, d) = route(&requested_climbs)
        && let Some((stop, wide)) = through(o, d)?
    {
        let toward = in_box(wide, stop)?;
        if toward.as_ref().is_some_and(|p| p.reached) {
            plan = toward;
            goal = stop;
        }
    }
    let plan = plan.ok_or_else(|| {
        format!(
            "no way from {}, {} (plane {}) toward {}, {} (plane {})",
            from.0, from.1, from.2, to.0, to.1, to.2
        )
    })?;

    let tile = |(x, y, plane): crate::nav::Point| NavTile { x, y, plane };
    let actions = plan
        .actions
        .into_iter()
        .map(|a| {
            let (kind, to_plane, loc) = match a.what {
                crate::nav::Use::Door(loc) => ("door", loc.plane, loc),
                crate::nav::Use::Climb(loc, _)
                    if request
                        .door_options
                        .iter()
                        .any(|o| o.eq_ignore_ascii_case(&loc.option)) =>
                {
                    ("door", loc.plane, loc)
                }
                crate::nav::Use::Climb(loc, to_plane) => ("climb", to_plane, loc),
            };
            NavAction {
                at: a.at + 1,
                kind,
                to_plane,
                loc,
            }
        })
        .collect();
    Ok(NavPlan {
        tiles: plan.tiles.into_iter().map(tile).collect(),
        actions,
        reached: plan.reached && goal == to,
        goal: tile(goal),
    })
}

type SceneCollision = (i32, i32, i32, Vec<Vec<u32>>);

pub fn scene_collision(pid: u32) -> Result<SceneCollision, String> {
    let (layout, memory) = reader(pid, part::COLLISION, "the scene's collision maps")?;
    let scene = crate::scene_collision::read(&memory, &layout)
        .ok_or("the scene's collision map can't be read")?;
    let (x1, y1, x2, y2) = scene.bounds();
    let flags = (x1..=x2)
        .map(|x| {
            (y1..=y2)
                .map(|y| scene.flags(x, y).unwrap_or(u32::MAX))
                .collect()
        })
        .collect();
    Ok((x1, y1, scene.plane, flags))
}

pub fn player_plane(pid: u32) -> Result<i32, String> {
    let (layout, memory) = reader(pid, part::PROJECTION, "the scene")?;
    state::game_state(&memory, &layout)
        .position
        .and_then(|p| p.plane)
        .ok_or_else(|| "not logged in".into())
}

pub fn server_tile(pid: u32) -> Result<Option<crate::ServerTile>, String> {
    let (layout, memory) = reader(pid, part::PLAYER | part::PATH, "the player's path")?;
    Ok(state::server_tile(&memory, &layout))
}

pub fn map_regions(pid: u32) -> Result<Vec<[i32; 2]>, String> {
    let (layout, memory) = reader(pid, part::PLAYER, "the scene")?;
    ipc::read::map_regions(&memory, &layout)
        .ok_or_else(|| "the scene's map squares can't be found".into())
}

pub fn in_instance(pid: u32) -> Result<bool, String> {
    let (layout, memory) = reader(pid, part::PLAYER | part::SCENE_LOADER, "the scene loader")?;
    ipc::read::in_instance(&memory, &layout)
        .ok_or_else(|| "the client's instance flag can't be read".into())
}

pub fn collision_area(
    pid: u32,
    plane: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
) -> Result<Vec<Vec<Option<u32>>>, String> {
    use crate::collision::{REGION_SIZE, region_id};
    let mut regions = Vec::new();
    for rx in (x1 / REGION_SIZE - 1)..=(x2 / REGION_SIZE + 1) {
        for ry in (y1 / REGION_SIZE - 1)..=(y2 / REGION_SIZE + 1) {
            if rx >= 0 && ry >= 0 {
                regions.push(region_id(rx, ry));
            }
        }
    }
    let world = world_collision(pid, &regions)?;
    Ok((x1..=x2)
        .map(|x| (y1..=y2).map(|y| world.flags(plane, x, y)).collect())
        .collect())
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
        menu_status::OTHER_THREAD => Err(OTHER_THREAD.into()),
        other => Err(format!("unknown menu status {other}")),
    }
}

pub fn menu(pid: u32) -> Result<Vec<crate::MenuEntry>, String> {
    let reply = menu_reply(pid)?;
    Ok(first(&reply.entries, reply.count)
        .iter()
        .map(menu_entry)
        .collect())
}

fn is_open(reply: &ipc::state::MenuReply) -> Result<bool, String> {
    match reply.open {
        ipc::state::menu_open::CLOSED => Ok(false),
        ipc::state::menu_open::OPEN => Ok(true),
        _ => Err("agent doesn't know where the client keeps the menu's open flag".into()),
    }
}

fn menu_rows(g: &ipc::state::MenuGeometry) -> crate::MenuRect {
    crate::MenuRect {
        x: g.rows_x,
        y: g.rows_y,
        width: g.rows_width,
        height: g.rows_height,
    }
}

pub fn menu_open(pid: u32) -> Result<bool, String> {
    is_open(&menu_reply(pid)?)
}

pub fn menu_layout(pid: u32) -> Result<Option<crate::MenuLayout>, String> {
    let reply = menu_reply(pid)?;
    if !is_open(&reply)? {
        return Ok(None);
    }
    if reply.has_geometry == 0 {
        return Err("agent doesn't know how the client lays out the menu".into());
    }
    let g = reply.geometry;
    let submenus = first(&reply.submenus, reply.submenu_count)
        .iter()
        .map(|sub| {
            let s = sub.geometry;
            crate::MenuSubmenu {
                row: sub.row + 1,
                entries: first(&sub.entries, sub.count)
                    .iter()
                    .map(menu_entry)
                    .collect(),
                x: s.x,
                y: s.y,
                width: s.width,
                height: s.height,
                rows: menu_rows(&s),
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
        rows: menu_rows(&g),
        header_height: g.header_height,
        row_height: g.row_height,
        scroll: g.scroll,
        margin: g.margin,
        submenus,
    }))
}

pub fn menu_click(pid: u32) -> Result<Option<crate::MenuClick>, String> {
    let reply = menu_reply(pid)?;
    Ok((reply.has_click != 0).then(|| crate::MenuClick {
        entry: menu_entry(&reply.clicked),
        x: reply.click_x,
        y: reply.click_y,
        age: reply.click_age,
    }))
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
    let (layout, memory) = interfaces_reader(pid)?;
    Ok(worker::sub_interfaces(&memory, &layout))
}

pub fn widget(pid: u32, group: u16, child: u16) -> Result<Option<Widget>, String> {
    if let Some(widget) = ask_worker(pid, worker_kind::WIDGET, [group as i32, child as i32, 0, 0]) {
        return Ok(widget);
    }
    let (layout, memory) = interfaces_reader(pid)?;
    Ok(worker::widget(&memory, &layout, group, child))
}

pub fn widget_model(
    pid: u32,
    group: u16,
    child: u16,
) -> Result<Option<decode::widgets::WidgetModel>, String> {
    use decode::widgets::{MODEL_ID, MODEL_TYPE, WidgetModel};
    let (layout, memory) = interfaces_reader(pid)?;
    let layout = &layout.interfaces;
    let Some(client) = memory.ptr(layout.module_base + layout.client as u64) else {
        return Ok(None);
    };
    let Some(component) =
        ipc::read::component(&memory, layout, client, (group as i32) << 16 | child as i32)
    else {
        return Ok(None);
    };
    Ok(
        match (
            memory.i32(component + MODEL_TYPE),
            memory.i32(component + MODEL_ID),
        ) {
            (Some(kind), Some(id)) => Some(WidgetModel { kind, id }),
            _ => None,
        },
    )
}

pub fn widget_tree(pid: u32) -> Result<decode::widgets::WidgetTree, String> {
    let (layout, memory) = interfaces_reader(pid)?;
    worker::widget_tree(&memory, &layout)
        .ok_or_else(|| format!("Cannot read the interfaces of client {pid} (not logged in?)"))
}

pub fn widget_children(pid: u32, group: u16, child: u16) -> Result<Option<Vec<Widget>>, String> {
    if let Some(children) = ask_worker(
        pid,
        worker_kind::WIDGET_CHILDREN,
        [group as i32, child as i32, 0, 0],
    ) {
        return Ok(children);
    }
    let (layout, memory) = interfaces_reader(pid)?;
    Ok(worker::widget_children(&memory, &layout, group, child))
}

fn world_reader(pid: u32) -> Result<(Layout, ProcessMemory), String> {
    reader(pid, part::WORLD, "where items, NPCs and players are")
}

fn interfaces_reader(pid: u32) -> Result<(Layout, ProcessMemory), String> {
    reader(pid, part::INTERFACES, "the interfaces")
}

pub fn item_container(pid: u32, id: i32) -> Result<Option<Vec<Item>>, String> {
    if let Some(items) = ask_worker(pid, worker_kind::ITEM_CONTAINER, [id, 0, 0, 0]) {
        return Ok(items);
    }
    let (layout, memory) = world_reader(pid)?;
    Ok(worker::item_container(&memory, &layout, id))
}

pub fn item_containers(pid: u32) -> Result<Vec<(i32, Vec<Item>)>, String> {
    let (layout, memory) = world_reader(pid)?;
    worker::item_containers(&memory, &layout)
        .ok_or_else(|| format!("Cannot read the item containers of client {pid}"))
}

pub fn varc(pid: u32, id: i32) -> Result<Option<i32>, String> {
    if let Some(value) = ask_worker(pid, worker_kind::VARC, [id, 0, 0, 0]) {
        return Ok(value);
    }
    let (layout, memory) = world_reader(pid)?;
    Ok(worker::varc(&memory, &layout, id))
}

pub fn chat_messages(pid: u32) -> Result<Vec<crate::ChatMessage>, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let messages = match ask_worker(
        pid,
        worker_kind::CHAT,
        [now as i32, (now >> 32) as i32, 0, 0],
    ) {
        Some(messages) => messages,
        None => {
            let (layout, memory) = reader(pid, part::CHAT, "the chat history")?;
            worker::chat(&memory, &layout, now)
        }
    };
    messages.ok_or_else(|| format!("Cannot read the chat history of client {pid}"))
}

static ITEM_NAMES: LazyLock<Mutex<HashMap<i32, String>>> = LazyLock::new(Default::default);

pub fn ground_items(pid: u32) -> Result<Vec<crate::GroundItem>, String> {
    let mut items = match ask_worker(pid, worker_kind::GROUND_ITEMS, [0; 4]) {
        Some(items) => items,
        None => {
            let (layout, memory) =
                reader(pid, worker::GROUND_ITEM_PARTS, "where ground items are")?;
            worker::ground_items(&memory, &layout)
        }
    };
    let mut ids: Vec<i32> = items.iter().map(|i| i.id).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut known = lock(&ITEM_NAMES);
    let missing: Vec<i32> = ids
        .iter()
        .copied()
        .filter(|id| !known.contains_key(id))
        .collect();
    if !missing.is_empty() {
        drop(known);
        let found = item_names(pid, &missing)?;
        known = lock(&ITEM_NAMES);
        known.extend(
            missing
                .into_iter()
                .zip(found)
                .filter_map(|(id, name)| Some((id, name?))),
        );
    }
    for item in &mut items {
        item.name = known.get(&item.id).cloned();
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

const WORKER_TIMEOUT: Duration = Duration::from_millis(50);

thread_local! {
    static WORKER_BYTES: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn ask_worker<T: serde::de::DeserializeOwned>(pid: u32, kind: u32, args: [i32; 4]) -> Option<T> {
    static NO_WORKER: LazyLock<bool> =
        LazyLock::new(|| std::env::var_os("VOLCREN_NO_WORKER").is_some());
    if *NO_WORKER {
        return None;
    }
    let start = Instant::now();
    let session = session(pid).ok()?;
    let event = session
        .work
        .get_or_init(|| open_event(work_event_name(pid), EVENT_MODIFY_STATE))
        .as_ref()?;
    let mutex = session
        .slot
        .get_or_init(|| create_mutex(worker_mutex_name(pid)))
        .as_ref()?;
    let reply = session
        .reply
        .get_or_init(|| open_event(worker_reply_event_name(pid), SYNCHRONIZATION_SYNCHRONIZE))
        .as_ref()?;
    let _slot = SlotGuard::lock(mutex, WORKER_TIMEOUT)?;
    let shared = session.view.shared();
    let sequence = shared.write_worker_request(&WorkerRequest {
        kind,
        args,
        reserved: 0,
    });
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
            let left = WORKER_TIMEOUT.saturating_sub(start.elapsed());
            if left.is_zero() {
                return None;
            }
            unsafe { WaitForSingleObject(reply.0, left.as_millis().max(1) as u32) };
        };
        if status != worker_status::OK {
            return None;
        }
        decode::postcard::from_bytes(&bytes).ok()
    })
}

fn by_index<T>(
    pid: u32,
    indices: &[i32],
    read: impl Fn(&ProcessMemory, &WorldLayout, i32) -> Option<T>,
) -> Result<BTreeMap<i32, T>, String> {
    let (layout, memory) = world_reader(pid)?;
    Ok(indices
        .iter()
        .filter_map(|&i| Some((i, read(&memory, &layout.world, i)?)))
        .collect())
}

pub fn npc_sizes(pid: u32, indices: &[i32]) -> Result<BTreeMap<i32, i32>, String> {
    by_index(pid, indices, world::npc_size)
}

pub fn npc_hitsplats(
    pid: u32,
    indices: &[i32],
) -> Result<BTreeMap<i32, Vec<world::Hitsplat>>, String> {
    by_index(pid, indices, world::npc_hitsplats)
}

pub fn player_hitsplats(
    pid: u32,
    indices: &[i32],
) -> Result<BTreeMap<i32, Vec<world::Hitsplat>>, String> {
    by_index(pid, indices, world::player_hitsplats)
}

pub fn client_cycle(pid: u32) -> Result<i32, String> {
    let (layout, memory) = reader(pid, part::PROJECTILES, "the client cycle")?;
    memory
        .i32(layout.module_base + layout.projectiles.cycle as u64)
        .ok_or_else(|| "can't read the client cycle".into())
}

fn read_words(memory: &ProcessMemory, address: u64, count: usize) -> Option<Vec<i32>> {
    let mut bytes = vec![0u8; count.checked_mul(4)?];
    memory.read(address, &mut bytes).then(|| {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c))
            .collect()
    })
}

pub fn memory_words(pid: u32, address: u64, count: usize) -> Result<Vec<i32>, String> {
    let (_, memory) = world_reader(pid)?;
    read_words(&memory, address, count)
        .ok_or_else(|| format!("can't read {count} words at {address:#x}"))
}

pub fn npc_words(
    pid: u32,
    index: i32,
    count: usize,
    definition: bool,
) -> Result<(u64, Vec<i32>), String> {
    let (layout, memory) = world_reader(pid)?;
    world::npc_words(&memory, &layout.world, index, count, definition)
        .ok_or_else(|| format!("no NPC {index} in the scene"))
}

pub fn widget_words(
    pid: u32,
    group: u16,
    child: u16,
    index: i32,
    count: usize,
) -> Result<(u64, Vec<i32>), String> {
    let (layout, memory) = interfaces_reader(pid)?;
    let layout = &layout.interfaces;
    let client = memory
        .ptr(layout.module_base + layout.client as u64)
        .ok_or("no client")?;
    let mut component =
        ipc::read::component(&memory, layout, client, (group as i32) << 16 | child as i32)
            .ok_or_else(|| format!("no component {group}.{child}"))?;
    if index >= 0 {
        use ipc::state::interfaces::{CHILDREN_DATA, ENTRY_COMPONENT, ENTRY_SIZE};
        let entries = memory
            .ptr(component + layout.children as u64 + CHILDREN_DATA as u64)
            .ok_or("no dynamic children")?;
        component = memory
            .ptr(entries + index as u64 * ENTRY_SIZE as u64 + ENTRY_COMPONENT as u64)
            .ok_or_else(|| format!("{group}.{child} has no dynamic child {index}"))?;
    }
    let words = read_words(&memory, component, count)
        .ok_or_else(|| format!("can't read the component at {component:#x}"))?;
    Ok((component, words))
}

pub fn widget_dump(pid: u32, count: usize) -> Result<Vec<(i32, i32, Vec<i32>)>, String> {
    let (layout, memory) = interfaces_reader(pid)?;
    let root = decode::state::root_interface(&memory, &layout).ok_or("no root interface")?;
    Ok(
        decode::widgets::component_addresses(&memory, &layout.interfaces, root)
            .ok_or("can't list the components")?
            .into_iter()
            .filter_map(|(id, index, address)| {
                Some((id, index, read_words(&memory, address, count)?))
            })
            .collect(),
    )
}

pub fn watch_client(pid: u32, seconds: u64, bytes: usize) -> Result<Vec<String>, String> {
    let (layout, memory) = world_reader(pid)?;
    let client = ipc::read::client(&memory, &layout).ok_or("no client")?;
    let read = |buffer: &mut Vec<u8>| memory.read(client, buffer);
    let mut last = vec![0u8; bytes];
    if !read(&mut last) {
        return Err("can't read the client object".into());
    }
    let mut now = last.clone();
    let mut changes: BTreeMap<usize, (i32, i32, u32)> = Default::default();
    let word = |b: &[u8], i: usize| i32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        if !read(&mut now) {
            continue;
        }
        for i in (0..bytes.saturating_sub(3)).step_by(4) {
            let (old, new) = (word(&last, i), word(&now, i));
            if old != new {
                let entry = changes.entry(i).or_insert((old, new, 0));
                entry.1 = new;
                entry.2 += 1;
            }
        }
        mem::swap(&mut last, &mut now);
    }
    Ok(changes
        .into_iter()
        .map(|(i, (first, last, n))| format!("+{i:#x} first {first} last {last} changes {n}"))
        .collect())
}

pub fn world_entities(pid: u32) -> Result<Vec<world::WorldEntity>, String> {
    let (layout, memory) = world_reader(pid)?;
    world::world_entities(&memory, &layout.world).ok_or_else(|| "no world views".to_string())
}

pub fn world_debug(pid: u32) -> Result<Vec<String>, String> {
    let (layout, memory) = world_reader(pid)?;
    world::world_debug(&memory, &layout.world).ok_or_else(|| "no world view".to_string())
}

pub fn player_words(pid: u32, index: i32, count: usize) -> Result<(u64, Vec<i32>), String> {
    let (layout, memory) = world_reader(pid)?;
    world::player_words(&memory, &layout.world, index, count)
        .ok_or_else(|| format!("no player {index} in the scene"))
}

pub fn players(pid: u32) -> Result<Vec<Player>, String> {
    if let Some(players) = ask_worker(pid, worker_kind::PLAYERS, [0; 4]) {
        return Ok(players);
    }
    let (layout, memory) = world_reader(pid)?;
    Ok(worker::players(&memory, &layout))
}

pub fn projectiles(pid: u32) -> Result<Vec<crate::Projectile>, String> {
    let mut projectiles = match ask_worker(pid, worker_kind::PROJECTILES, [0; 4]) {
        Some(projectiles) => projectiles,
        None => {
            let (layout, memory) = reader(pid, worker::PROJECTILE_PARTS, "where projectiles are")?;
            worker::projectiles(&memory, &layout)
        }
    };
    if projectiles.iter().any(|p| p.moving) {
        with_terrain(pid, |terrain| {
            for p in projectiles.iter_mut().filter(|p| p.moving) {
                p.height = terrain.heightmap(p.local_x, p.local_y, p.source_plane) - p.z;
            }
        })?;
    }
    Ok(projectiles)
}

pub fn spot_anims(pid: u32) -> Result<Vec<crate::SpotAnim>, String> {
    if let Some(objects) = ask_worker(pid, worker_kind::SPOT_ANIMS, [0; 4]) {
        return Ok(objects);
    }
    let (layout, memory) = reader(pid, worker::SPOT_ANIM_PARTS, "where spot animations are")?;
    Ok(worker::spot_anims(&memory, &layout))
}

const RELOAD: &str = ": restart the client and load a matching agent";

struct LogKind<L> {
    what: &'static str,
    finds: &'static str,
    memory_name: fn(u32) -> String,
    size: usize,
    compatible: unsafe fn(*const L) -> bool,
}

const PACKET_LOG: LogKind<Log> = LogKind {
    what: "packet",
    finds: "the packet functions",
    memory_name: packets_memory_name,
    size: LOG_SIZE,
    compatible: packet_log::is_compatible,
};

const SCRIPT_LOG: LogKind<script_log::Log> = LogKind {
    what: "script",
    finds: "the script runner",
    memory_name: script_runs_memory_name,
    size: script_log::LOG_SIZE,
    compatible: script_log::is_compatible,
};

const VARC_LOG: LogKind<varc_log::Log> = LogKind {
    what: "varc",
    finds: "the varc setters",
    memory_name: varc_log_memory_name,
    size: varc_log::LOG_SIZE,
    compatible: varc_log::is_compatible,
};

const ITEM_LOG: LogKind<item_log::Log> = LogKind {
    what: "item",
    finds: "the item container writes",
    memory_name: item_log_memory_name,
    size: item_log::LOG_SIZE,
    compatible: item_log::is_compatible,
};

impl<L> LogKind<L> {
    fn open(&self, pid: u32, access: FILE_MAP, unmapped_hint: &str) -> Result<View, String> {
        let what = self.what;
        let mapping = open_mapping((self.memory_name)(pid), access).ok_or_else(|| {
            format!(
                "No {what} log in client {pid}: the agent isn't loaded, is too old, or didn't find {}",
                self.finds
            )
        })?;
        let view = View::new(mapping, access, self.size)
            .map_err(|_| format!("Cannot map the {what} log of client {pid}{unmapped_hint}"))?;
        if !unsafe { (self.compatible)(view.ptr::<L>().as_ptr()) } {
            return Err(format!(
                "The {what} log of client {pid} has another layout{RELOAD}"
            ));
        }
        Ok(view)
    }

    fn read<T>(&self, pid: u32, read: impl FnOnce(*const L) -> T) -> Result<T, String> {
        let view = self.open(pid, FILE_MAP_READ, "")?;
        Ok(read(view.ptr::<L>().as_ptr()))
    }
}

fn claim_slot(
    subscribe: impl FnOnce(&mut dyn FnMut(u32) -> bool) -> Option<u32>,
    event_name: impl Fn(u32) -> String,
) -> Option<(u32, Handle)> {
    let mut event = None;
    let slot = subscribe(&mut |slot| match new_event(event_name(slot)) {
        Some(handle) => {
            event = Some(handle);
            true
        }
        None => false,
    });
    slot.zip(event)
}

pub fn packets(pid: u32, since: u64, limit: usize) -> Result<crate::PacketPage, String> {
    let page = PACKET_LOG.read(pid, |log| unsafe {
        packet_log::read_since(log, since, limit)
    })?;
    Ok(crate::PacketPage {
        next: page.next,
        dropped: page.dropped,
        packets: page
            .entries
            .into_iter()
            .map(|entry| crate::Packet {
                sequence: entry.sequence,
                time_ms: entry.time_ms,
                sent: entry.direction == direction::SENT,
                opcode: entry.opcode,
                length: entry.length,
                payload: crate::hex_lower(&entry.payload),
            })
            .collect(),
    })
}

pub fn packet_layouts(pid: u32) -> Result<Option<Vec<crate::PacketLayout>>, String> {
    let layouts = PACKET_LOG.read(pid, |log| unsafe { packet_log::read_layouts(log) })?;
    Ok(layouts.map(|layouts| {
        layouts
            .into_iter()
            .map(|layout| crate::PacketLayout {
                opcode: layout.opcode,
                size: layout.size,
                fields: layout
                    .fields
                    .map(|fields| fields.iter().map(|field| field.spelling()).collect()),
            })
            .collect()
    }))
}

pub fn movement(pid: u32) -> Result<Option<crate::Movement>, String> {
    let Some(seen) = PACKET_LOG.read(pid, |log| unsafe { packet_log::movement(log) })? else {
        return Ok(None);
    };
    let (layout, memory) = reader(
        pid,
        part::PLAYER | part::PATH | part::TICK_COUNT,
        "the player's path and the tick counter",
    )?;
    if state::server_tile(&memory, &layout).is_none() {
        return Ok(None);
    }
    let tick = ipc::read::client(&memory, &layout)
        .and_then(|client| memory.i32(client + layout.tick_count as u64))
        .ok_or("the tick counter can't be read")?;
    let plane = |plane: i32| (plane >= 0).then_some(plane);
    Ok(Some(crate::Movement {
        x: seen.x,
        y: seen.y,
        plane: plane(seen.plane),
        from_x: seen.from_x,
        from_y: seen.from_y,
        from_plane: plane(seen.from_plane),
        moving: seen.moving(tick),
        moved_tick: (seen.moved_tick != packet_log::NO_TICK).then_some(seen.moved_tick),
        tick,
    }))
}

pub struct PacketWatcher {
    view: View,
    event: Handle,
    slot: u32,
    filter: packet_log::Filter,
    next: u64,
}

impl PacketWatcher {
    pub fn new(pid: u32) -> Result<Self, String> {
        let view = PACKET_LOG.open(pid, FILE_MAP_READ | FILE_MAP_WRITE, RELOAD)?;
        let log = view.ptr::<Log>().as_ptr();
        let (slot, event) = claim_slot(
            |create| unsafe { packet_log::subscribe(log, create) },
            |slot| packet_log::packet_wake_event_name(pid, slot),
        )
        .ok_or_else(|| format!("Too many readers are waiting for packets in client {pid}"))?;
        let next = unsafe { packet_log::written(log) };
        Ok(Self {
            view,
            event,
            slot,
            filter: [0; packet_log::FILTER_WORDS],
            next,
        })
    }

    fn log(&self) -> *const Log {
        self.view.ptr::<Log>().as_ptr()
    }

    pub fn set_filter(&mut self, filter: packet_log::Filter) {
        self.filter = filter;
        unsafe { packet_log::set_filter(self.log(), self.slot, &filter) };
    }

    pub fn written(&self) -> u64 {
        unsafe { packet_log::written(self.log()) }
    }

    fn pending(&self) -> bool {
        self.written() > self.next
    }

    pub fn read(&mut self, limit: usize) -> packet_log::Page {
        let mut page = unsafe { packet_log::read_since(self.log(), self.next, limit) };
        self.next = page.next;
        page.entries
            .retain(|entry| packet_log::filter_has(&self.filter, entry.direction, entry.opcode));
        page
    }
}

impl Drop for PacketWatcher {
    fn drop(&mut self) {
        unsafe { packet_log::unsubscribe(self.log(), self.slot) };
    }
}

pub fn wait_watchers(
    items: Option<&ItemWatcher>,
    packets: Option<&PacketWatcher>,
    timeout: Duration,
) -> bool {
    if items.is_some_and(|items| items.pending())
        || packets.is_some_and(|packets| packets.pending())
    {
        return true;
    }
    let events: Vec<HANDLE> = items
        .map(|items| items.event.0)
        .into_iter()
        .chain(packets.map(|packets| packets.event.0))
        .collect();
    if events.is_empty() {
        sleep(timeout);
        return false;
    }
    let woke = unsafe {
        WaitForMultipleObjects(events.len() as u32, events.as_ptr(), 0, wait_ms(timeout))
    };
    woke < WAIT_OBJECT_0 + events.len() as u32
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
            Err(protocol_mismatch(pid, protocol))
        }
        Err(ReadError::Busy) => Err(format!(
            "The game state layout of client {pid} kept changing while reading it"
        )),
    }
}

fn protocol_mismatch(pid: u32, protocol: impl fmt::Display) -> String {
    format!(
        "Client {pid} runs agent protocol {protocol}, this is {}{RELOAD}",
        ipc::PROTOCOL_VERSION
    )
}

fn no_shared_memory(pid: u32) -> String {
    format!("No agent shared memory in client {pid}")
}

fn current_connection(pid: u32) -> Result<Connection, String> {
    let connection = connection(pid).ok_or_else(|| no_shared_memory(pid))?;
    if connection.protocol != ipc::PROTOCOL_VERSION {
        return Err(protocol_mismatch(pid, connection.protocol));
    }
    Ok(connection)
}

fn shared_view(
    pid: u32,
    size: usize,
    unmapped: impl FnOnce(io::Error) -> String,
) -> Result<View, String> {
    let mapping = open_mapping(shared_memory_name(pid), FILE_MAP_WRITE)
        .ok_or_else(|| no_shared_memory(pid))?;
    View::new(mapping, FILE_MAP_WRITE, size).map_err(unmapped)
}

pub fn screenshot(pid: u32, path: &Path) -> Result<(i32, i32), String> {
    let connection = connection(pid).ok_or_else(|| no_shared_memory(pid))?;
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

    let view = shared_view(pid, HEADER_SIZE + size, |_| {
        format!("Cannot map the shared memory of client {pid}")
    })?;
    let base = view.ptr::<u8>().as_ptr();
    let capture = unsafe { base.add(mem::offset_of!(Header, capture_enabled)) };
    let was_capturing = unsafe { *capture } != 0;
    if !was_capturing {
        unsafe { capture.write_volatile(1) };
        sleep(Duration::from_millis(200));
    }
    let pixels = unsafe { std::slice::from_raw_parts(base.add(HEADER_SIZE), size) }.to_vec();
    if !was_capturing {
        unsafe { capture.write_volatile(0) };
    }
    drop(view);

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
    current_connection(pid)?;
    let view = shared_view(pid, HEADER_SIZE, |_| {
        format!("Cannot map the shared memory of client {pid}")
    })?;
    let base = view.ptr::<u8>().as_ptr();
    unsafe {
        base.add(mem::offset_of!(Header, cursor_color))
            .copy_from_nonoverlapping(cursor.color.as_ptr(), 3);
        base.add(mem::offset_of!(Header, cursor_size))
            .write_volatile(cursor.size);
        base.add(mem::offset_of!(Header, cursor_style))
            .write_volatile(cursor.style.value());
    }
    Ok(())
}

pub fn draw(pid: u32, overlay: &Overlay) -> Result<(), String> {
    draw_batch(pid, std::slice::from_ref(overlay))
}

pub fn draw_batch(pid: u32, overlays: &[Overlay]) -> Result<(), String> {
    let Some(last) = overlays.last() else {
        return Ok(());
    };
    let mode = if matches!(last, Overlay::Clear) {
        overlay_mode::OFF
    } else {
        overlay_mode::ON_SERIAL
    };
    let clears = matches!(overlays.first(), Some(Overlay::Clear));
    with_overlay(
        pid,
        mode,
        !clears,
        None,
        |overlay_ptr, width, height| unsafe {
            for overlay in overlays {
                match *overlay {
                    Overlay::Clear => {
                        write_bytes(overlay_ptr, 0, width as usize * height as usize * 4)
                    }
                    Overlay::Rect {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                    } => draw_rect(overlay_ptr, width, height, [x1, y1, x2, y2], color),
                    Overlay::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        color,
                    } => draw_line(overlay_ptr, width, height, [x1, y1, x2, y2], color),
                }
            }
        },
    )
}

pub(crate) fn paint_overlay(
    pid: u32,
    paint: impl FnOnce([i32; 2], &mut dyn FnMut(i32, i32, Rgba)),
) -> Result<(), String> {
    with_overlay(
        pid,
        overlay_mode::ON_SERIAL,
        true,
        None,
        |buf, width, height| {
            paint([width, height], &mut |x, y, color| unsafe {
                put_pixel(buf, width, height, x, y, color)
            })
        },
    )
}

pub fn draw_lines(pid: u32, lines: &[[i32; 4]], color: Rgba) -> Result<(), String> {
    with_overlay(
        pid,
        overlay_mode::ON_SERIAL,
        true,
        None,
        |overlay_ptr, width, height| {
            for &[x1, y1, x2, y2] in lines {
                unsafe { draw_line(overlay_ptr, width, height, [x1, y1, x2, y2], color) };
            }
        },
    )
}

const TILE_SIZE: i32 = 128;
const TILE_HALF: i32 = TILE_SIZE / 2;
const TILE_CORNERS: [[i32; 2]; 4] = [
    [-TILE_HALF, -TILE_HALF],
    [TILE_HALF, -TILE_HALF],
    [TILE_HALF, TILE_HALF],
    [-TILE_HALF, TILE_HALF],
];
const SCENE_TILES: i32 = 104;
const CHUNK_TILES: i32 = 8;
const MAP_SQUARE_TILES: i32 = 64;
const MAP_SQUARE_RANGE: i32 = 16;
const LOS_RANGE: i32 = 10;
const HOVER_RANGE: i32 = 16;
const LOADING_INSET: i32 = 16;
const GRID_RADIUS: i32 = 32;

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba {
    Rgba { r, g, b, a }
}

const STROKE_WIDTH: i32 = 2;
const SCREEN_LIMIT: f32 = 100_000.0;
const LABEL_SHADOW: Rgba = rgba(0, 0, 0, 255);
const TEXT_COLOR: Rgba = rgba(255, 255, 255, 255);
const LABEL_SCALE: i32 = 2;
const NPC_LABEL_DEFAULT_HEIGHT: i32 = 200;
const NPC_LABEL_OFFSET: i32 = 40;
const TEXT_LEFT: i32 = 8;
const UP_TEXT_HEIGHT: f32 = 20.0;
const TEXT_MARGIN: i32 = 10;
const TEXT_PADDING: i32 = 4;
const TEXT_BACKGROUND: Rgba = rgba(64, 64, 64, 150);
const LAYERS: [&str; 22] = [
    "players",
    "world-entities",
    "interacting",
    "npcs",
    "projectiles",
    "spot-anims",
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

const PLAYER_TILE_COLOR: Rgba = rgba(41, 98, 255, 255);
const LOCAL_PLAYER_TILE_COLOR: Rgba = rgba(0, 184, 212, 255);
const NPC_TILE_COLOR: Rgba = rgba(255, 255, 0, 255);
const WORLD_ENTITY_COLOR: Rgba = rgba(0, 0, 255, 255);
const INTERACTING_COLOR: Rgba = rgba(0, 255, 255, 255);
const INTERACTING_SHIFT: f32 = 16.0;
const ARROW_SCALE: f32 = 2.0;
const OBJECT_TILE_COLOR: Rgba = rgba(0, 255, 0, 255);
const GROUND_ITEM_COLOR: Rgba = rgba(221, 44, 0, 255);
const GROUND_OBJECT_COLOR: Rgba = rgba(170, 0, 255, 255);
const WALL_COLOR: Rgba = rgba(158, 158, 158, 255);
const DECORATION_COLOR: Rgba = rgba(98, 0, 234, 255);
const TILE_FLAG_COLOR: Rgba = rgba(221, 44, 0, 255);
const CHUNK_COLOR: Rgba = rgba(0, 0, 255, 255);
const PROJECTILE_COLOR: Rgba = rgba(255, 0, 0, 255);
const PROJECTILE_PATH_COLOR: Rgba = rgba(255, 0, 0, 140);
const SPOT_ANIM_COLOR: Rgba = rgba(255, 0, 255, 255);
const MAP_SQUARE_COLOR: Rgba = rgba(0, 255, 0, 255);
const LOCAL_VALID_MOVEMENT_COLOR: Rgba = rgba(141, 220, 26, 255);
const LINE_OF_SIGHT_COLOR: Rgba = rgba(204, 42, 219, 255);
const HOVER_COLOR: Rgba = rgba(0, 200, 83, 255);
const GRID_COLOR: Rgba = rgba(255, 255, 255, 90);

static LABEL_FONT: Mutex<Option<Arc<crate::font::Font>>> = Mutex::new(None);

fn label_font(pid: u32) -> Result<Arc<crate::font::Font>, String> {
    let mut cached = lock(&LABEL_FONT);
    if let Some(font) = cached.as_ref() {
        return Ok(font.clone());
    }
    let archive = |index: u32| -> Result<Vec<u8>, String> {
        cache_files(pid, index, &[[crate::font::P12_FULL, 0]])?
            .pop()
            .flatten()
            .ok_or_else(|| format!("the client has no p12_full in index {index}"))
    };
    let font = Arc::new(crate::font::Font::decode(
        &archive(crate::font::FONTS_INDEX)?,
        &archive(crate::font::SPRITES_INDEX)?,
    )?);
    *cached = Some(font.clone());
    Ok(font)
}

#[derive(Default)]
struct Plan {
    points: Vec<projection::Point>,
    paths: Vec<Stroke>,
    labels: Vec<Label>,
    texts: Vec<Text>,
    arrows: Vec<Arrow>,
    rects: Vec<Rect>,
    backdrops: Vec<[i32; 4]>,
}

struct Rect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: Rgba,
}

fn widget_layer(layer: &str) -> Option<(i32, i32, i32, i32, Rgba)> {
    let mut parts = layer.strip_prefix("widget:")?.split(',');
    let mut number = || parts.next()?.parse::<i32>().ok();
    let (x, y, width, height) = (number()?, number()?, number()?, number()?);
    let hex = parts.next().filter(|hex| hex.len() == 6)?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    parts.next().is_none().then_some((
        x,
        y,
        width,
        height,
        rgba((value >> 16) as u8, (value >> 8) as u8, value as u8, 255),
    ))
}

struct Stroke {
    points: Vec<usize>,
    color: Rgba,
    closed: bool,
    marker: bool,
}

struct Arrow {
    from: usize,
    to: usize,
    color: Rgba,
}

struct Label {
    anchor: usize,
    text: String,
    color: Rgba,
}

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
        self.paths.push(Stroke {
            points,
            color,
            closed,
            marker,
        });
    }

    fn arrow(&mut self, from: projection::Point, to: projection::Point, color: Rgba) {
        let (from, to) = (self.point(from), self.point(to));
        self.arrows.push(Arrow { from, to, color });
    }

    fn label(&mut self, anchor: projection::Point, text: String, color: Rgba) {
        let anchor = self.point(anchor);
        self.labels.push(Label {
            anchor,
            text,
            color,
        });
    }

    fn text(&mut self, x: i32, y: i32, text: String, color: Rgba) {
        self.texts.push(Text { x, y, text, color });
    }

    fn tile(&mut self, tile: [i32; 2], label: String, color: Rgba) {
        self.path(&tile_corners(tile), color, true, true);
        self.label(tile_centre(tile), label, color);
    }
}

fn footprint(tx: i32, ty: i32, size_x: i32, size_y: i32) -> [projection::Point; 4] {
    let (x, y, w, h) = (
        tx * TILE_SIZE,
        ty * TILE_SIZE,
        size_x * TILE_SIZE,
        size_y * TILE_SIZE,
    );
    let corner = |x, y, tile| projection::Point {
        tile: Some(tile),
        ..projection::Point::new(x, y, 0)
    };
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

fn tile_centre([tx, ty]: [i32; 2]) -> projection::Point {
    projection::Point::new(tx * TILE_SIZE + TILE_HALF, ty * TILE_SIZE + TILE_HALF, 0)
}

fn boundary(tx: i32, ty: i32) -> projection::Point {
    projection::Point {
        heightmap: true,
        ..projection::Point::new(tx * TILE_SIZE, ty * TILE_SIZE, 0)
    }
}

fn in_scene([tx, ty]: [i32; 2]) -> bool {
    (0..SCENE_TILES).contains(&tx) && (0..SCENE_TILES).contains(&ty)
}

fn scene_tile(position: &Position) -> [i32; 2] {
    [position.local_x >> 7, position.local_y >> 7]
}

fn scene_origin(position: &Position) -> (i32, i32) {
    let [tx, ty] = scene_tile(position);
    (position.x - tx, position.y - ty)
}

fn label_height(height: i32) -> i32 {
    if height > 0 {
        height
    } else {
        NPC_LABEL_DEFAULT_HEIGHT
    }
}

fn actor_marker(plan: &mut Plan, local: (i32, i32), height: i32, label: String, color: Rgba) {
    let (local_x, local_y) = local;
    let tile = Some([local_x >> 7, local_y >> 7]);
    let lift = label_height(height) + NPC_LABEL_OFFSET;
    let corners = TILE_CORNERS.map(|[dx, dy]| projection::Point {
        tile,
        ..projection::Point::new(local_x + dx, local_y + dy, 0)
    });
    plan.path(&corners, color, true, true);
    plan.label(
        projection::Point::new(local_x, local_y, -lift),
        label,
        color,
    );
}

fn npc_label(npc: &Npc) -> String {
    format!(
        "{} (ID:{}) (A: {}) (P: {}) (G: {})",
        crate::strip_tags(&npc.name),
        npc.id,
        npc.animation,
        npc.pose,
        npc.graphic
    )
}

fn projectile_label(p: &crate::Projectile) -> String {
    if p.moving {
        format!("(ID: {}) ({})", p.id, p.end_cycle - p.cycle)
    } else {
        format!("(ID: {}) (in {})", p.id, p.start_cycle - p.cycle)
    }
}

fn player_label(player: &Player) -> String {
    format!(
        "{} (A: {}) (P: {}) (G: {})",
        player.name, player.animation, player.pose, player.graphic
    )
}

fn loc_marker(plan: &mut Plan, loc: &ipc::state::LocInfo, color: Rgba) {
    let (tx, ty) = (loc.x as i32, loc.y as i32);
    let (sx, sy) = (loc.size_x.max(1) as i32, loc.size_y.max(1) as i32);
    plan.path(&footprint(tx, ty, sx, sy), color, true, true);
    let centre = projection::Point::new(
        tx * TILE_SIZE + sx * TILE_HALF,
        ty * TILE_SIZE + sy * TILE_HALF,
        0,
    );
    plan.label(centre, format!("ID: {}", loc.id), color);
}

fn grid_tiles([cx, cy]: [i32; 2]) -> Vec<[i32; 2]> {
    let mut tiles = Vec::new();
    for tx in (cx - GRID_RADIUS).max(0)..=(cx + GRID_RADIUS).min(SCENE_TILES - 1) {
        for ty in (cy - GRID_RADIUS).max(0)..=(cy + GRID_RADIUS).min(SCENE_TILES - 1) {
            tiles.push([tx, ty]);
        }
    }
    tiles
}

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

fn client_window(pid: u32) -> Option<HWND> {
    let mut found = None;
    each_jag_window(|hwnd, owner| {
        if owner == pid {
            found = Some(hwnd);
        }
        found.is_none()
    });
    found
}

fn cursor_canvas(pid: u32) -> Option<[f32; 2]> {
    let hwnd = client_window(pid)?;
    let mut cursor = POINT { x: 0, y: 0 };
    let mut origin = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut cursor) } == 0
        || unsafe { ClientToScreen(hwnd, &mut origin) } == 0
    {
        return None;
    }
    Some([(cursor.x - origin.x) as f32, (cursor.y - origin.y) as f32])
}

type HoveredTile = ([i32; 2], [f32; 2]);

fn hovered_tile(pid: u32, centre: [i32; 2]) -> Result<Option<HoveredTile>, String> {
    let Some(cursor) = cursor_canvas(pid) else {
        return Ok(None);
    };
    let [cx, cy] = centre;
    let tiles: Vec<[i32; 2]> = (cx - HOVER_RANGE..=cx + HOVER_RANGE)
        .flat_map(|tx| (cy - HOVER_RANGE..=cy + HOVER_RANGE).map(move |ty| [tx, ty]))
        .filter(|&tile| in_scene(tile))
        .collect();
    let points: Vec<projection::Point> =
        tiles.iter().flat_map(|&tile| tile_corners(tile)).collect();
    if points.is_empty() {
        return Ok(None);
    }
    let screen = project(pid, &points, Via::Auto)?;
    Ok(tiles
        .iter()
        .zip(screen.as_chunks::<4>().0.iter())
        .find(|(_, corners)| inside(*corners, cursor))
        .map(|(&tile, _)| (tile, cursor)))
}

fn with_terrain<T>(pid: u32, read: impl FnOnce(&Terrain) -> T) -> Result<T, String> {
    let (layout, memory) = reader(pid, 0, "")?;
    kept_terrain(pid, |terrain| {
        projection::ensure_terrain(&memory, &layout, terrain, || decorations(pid))
            .map(|terrain| read(terrain))
    })
}

const FRAME_WAIT: Duration = Duration::from_millis(100);
const FRAME_SLICE: Duration = Duration::from_millis(20);

fn frame_serial(pid: u32) -> Result<u32, String> {
    match read_header(pid) {
        Some(Ok(header)) => Ok(header.frame_serial),
        Some(Err(Incompatible { protocol, .. })) => Err(protocol_mismatch(pid, protocol)),
        None => Err(no_shared_memory(pid)),
    }
}

fn next_frame(pid: u32, after: u32, timeout: Duration) -> Result<Option<u32>, String> {
    let session = session(pid)?;
    let event = session
        .frame
        .get_or_init(|| open_event(frame_event_name(pid), SYNCHRONIZATION_SYNCHRONIZE))
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
                unsafe {
                    WaitForSingleObject(event.0, left.min(FRAME_SLICE).as_millis().max(1) as u32)
                };
            }
            None => sleep(left.min(Duration::from_millis(1))),
        }
    }
}

pub fn scene_tiles_next_frame(
    pid: u32,
    layers: &[&str],
    after: u32,
) -> Result<Option<u32>, String> {
    let Some(frame) = next_frame(pid, after, FRAME_WAIT)? else {
        return Ok(None);
    };
    scene_tiles(pid, layers)?;
    Ok(Some(frame))
}

pub fn scene_tiles(pid: u32, layers: &[&str]) -> Result<usize, String> {
    if let Some(unknown) = layers
        .iter()
        .find(|layer| !LAYERS.contains(layer) && widget_layer(layer).is_none())
    {
        return Err(format!("unknown tile layer {unknown:?}"));
    }
    let on = |name: &str| layers.contains(&name);
    let font = label_font(pid)?;
    let line_height = font.max_height * LABEL_SCALE + 2;
    let state = game_state(pid)?;
    let plane_text = |position: &Position| {
        position
            .plane
            .map_or("?".to_string(), |plane| plane.to_string())
    };
    let here = || {
        state
            .position
            .ok_or("the player's position isn't known (not logged in?)")
    };

    let mut plan = Plan::default();
    let (mut npcs, mut players, mut collision) = (Fetched(None), Fetched(None), Fetched(None));

    for (x, y, width, height, color) in layers.iter().filter_map(|layer| widget_layer(layer)) {
        plan.rects.push(Rect {
            x,
            y,
            width,
            height,
            color,
        });
    }

    if on("grid") {
        let here = here()?;
        for tile in grid_tiles(scene_tile(&here)) {
            plan.path(&tile_corners(tile), GRID_COLOR, true, false);
        }
    }

    if on("npcs") {
        for npc in npcs.get(|| crate::npcs(pid))? {
            actor_marker(
                &mut plan,
                (npc.local_x, npc.local_y),
                npc.height,
                npc_label(npc),
                NPC_TILE_COLOR,
            );
        }
    }

    if on("players") {
        let me = state.position.map(|p| (p.local_x, p.local_y));
        for player in players.get(|| crate::players(pid))? {
            let local = (player.local_x, player.local_y);
            let color = if Some(local) == me {
                LOCAL_PLAYER_TILE_COLOR
            } else {
                PLAYER_TILE_COLOR
            };
            actor_marker(&mut plan, local, player.height, player_label(player), color);
        }
    }

    if on("world-entities") {
        for entity in world_entities(pid)? {
            let label = format!("ID: {} Type: {}", entity.view_id, entity.config_id);
            plan.label(
                projection::Point::new(entity.local_x, entity.local_y, 0),
                label,
                WORLD_ENTITY_COLOR,
            );
        }
    }

    if on("interacting") {
        let (npcs, players) = (
            npcs.get(|| crate::npcs(pid))?,
            players.get(|| crate::players(pid))?,
        );
        let middle = |local_x, local_y, height: i32| {
            projection::Point::new(local_x, local_y, -label_height(height) / 2)
        };
        let npc_at = |index: i32| {
            npcs.iter()
                .find(|n| n.index == index)
                .map(|n| middle(n.local_x, n.local_y, n.height))
        };
        let player_at = |index: i32| {
            players
                .iter()
                .find(|p| p.index == index)
                .map(|p| middle(p.local_x, p.local_y, p.height))
        };
        let target = |npc: i32, player: i32| npc_at(npc).or_else(|| player_at(player));
        for npc in npcs {
            if let Some(to) = target(npc.interacting_npc, npc.interacting_player) {
                plan.arrow(
                    middle(npc.local_x, npc.local_y, npc.height),
                    to,
                    INTERACTING_COLOR,
                );
            }
        }
        for player in players {
            if let Some(to) = target(player.interacting_npc, player.interacting_player) {
                plan.arrow(
                    middle(player.local_x, player.local_y, player.height),
                    to,
                    INTERACTING_COLOR,
                );
            }
        }
    }

    if on("projectiles") {
        let here = here()?;
        let (origin_x, origin_y) = scene_origin(&here);
        let projectiles = projectiles(pid)?;
        let npcs: &[Npc] = if projectiles.iter().any(|p| p.target_npc >= 0) {
            npcs.get(|| crate::npcs(pid))?
        } else {
            &[]
        };
        let players: &[Player] = if projectiles.iter().any(|p| p.target_player >= 0) {
            players.get(|| crate::players(pid))?
        } else {
            &[]
        };
        for p in projectiles
            .iter()
            .filter(|p| Some(p.source_plane) == here.plane)
        {
            let at = projection::Point::new(p.local_x, p.local_y, -p.height);
            let actor = npcs
                .iter()
                .find(|n| n.index == p.target_npc)
                .map(|n| (n.local_x, n.local_y))
                .or_else(|| {
                    players
                        .iter()
                        .find(|pl| pl.index == p.target_player)
                        .map(|pl| (pl.local_x, pl.local_y))
                });
            let target = match actor {
                Some((x, y)) => projection::Point::new(x, y, -p.end_height),
                None => projection::Point {
                    z: -p.end_height,
                    ..tile_centre([p.target_x - origin_x, p.target_y - origin_y])
                },
            };
            plan.path(&[at, target], PROJECTILE_PATH_COLOR, false, false);
            plan.label(at, projectile_label(p), PROJECTILE_COLOR);
        }
    }

    if on("spot-anims") {
        let here = here()?;
        for object in spot_anims(pid)?
            .iter()
            .filter(|object| object.view_id == 0 && Some(object.plane) == here.plane)
        {
            let tile = [object.local_x >> 7, object.local_y >> 7];
            plan.path(&tile_corners(tile), SPOT_ANIM_COLOR, true, true);
            plan.label(
                tile_centre(tile),
                format!("(ID: {})", object.id),
                TEXT_COLOR,
            );
        }
    }

    if on("objects") || on("ground-objects") || on("walls") || on("decorations") {
        let reply = cached_locs_reply(pid)?;
        for loc in first(&reply.locs, reply.count) {
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
        for item in cached(&GROUND_ITEMS_CACHE, pid, || ground_items(pid).map(Arc::new))?
            .iter()
            .filter(|item| Some(item.plane) == here.plane)
        {
            plan.tile(
                [item.scene_x, item.scene_y],
                format!("ID: {} Qty:{}", item.id, item.quantity),
                GROUND_ITEM_COLOR,
            );
        }
    }

    if on("tile-flags") {
        let flagged = with_terrain(pid, |terrain| {
            let [size_x, size_y] = terrain.size();
            let plane = terrain.key().plane;
            let mut flagged = Vec::new();
            for x in 0..size_x {
                for y in 0..size_y {
                    let bridge = terrain
                        .setting(1, x, y)
                        .is_some_and(|s| s & crate::devtools::TILE_FLAG_BRIDGE != 0);
                    let text = crate::devtools::tile_flag_text(
                        bridge,
                        terrain.setting(plane, x, y).unwrap_or(0),
                    );
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
        let lines: Vec<i32> = (CHUNK_TILES..=SCENE_TILES - CHUNK_TILES)
            .step_by(CHUNK_TILES as usize)
            .collect();
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
        for x in (near_x - MAP_SQUARE_RANGE..=near_x + MAP_SQUARE_RANGE)
            .filter(|x| x.rem_euclid(MAP_SQUARE_TILES) == 0)
        {
            let points: Vec<_> = (near_y - MAP_SQUARE_RANGE..=near_y + MAP_SQUARE_RANGE)
                .map(|y| boundary(x - origin_x, y - origin_y))
                .collect();
            plan.path(&points, MAP_SQUARE_COLOR, false, false);
        }
        for y in (near_y - MAP_SQUARE_RANGE..=near_y + MAP_SQUARE_RANGE)
            .filter(|y| y.rem_euclid(MAP_SQUARE_TILES) == 0)
        {
            let points: Vec<_> = (near_x - MAP_SQUARE_RANGE..=near_x + MAP_SQUARE_RANGE)
                .map(|x| boundary(x - origin_x, y - origin_y))
                .collect();
            plan.path(&points, MAP_SQUARE_COLOR, false, false);
        }
    }

    if on("loading-lines") {
        let (near, far) = (LOADING_INSET, SCENE_TILES - LOADING_INSET);
        let square = [
            boundary(near, near),
            boundary(near, far),
            boundary(far, far),
            boundary(far, near),
        ];
        plan.path(&square, CHUNK_COLOR, true, false);
    }

    if on("line-of-sight") {
        let here = here()?;
        let (origin_x, origin_y) = scene_origin(&here);
        let collision = collision.get(|| crate::scene_collision(pid))?;
        let flag_at = |x: i32, y: i32| collision_at(collision, x, y);
        let from = (here.x, here.y);
        for x in here.x - LOS_RANGE..=here.x + LOS_RANGE {
            for y in here.y - LOS_RANGE..=here.y + LOS_RANGE {
                let tile = [x - origin_x, y - origin_y];
                if (x, y) == from
                    || !in_scene(tile)
                    || !crate::devtools::line_of_sight(flag_at, from, (x, y))
                {
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
                if (dx, dy) != (0, 0)
                    && in_scene(tile)
                    && crate::pathfinding::can_step(&world, plane, here.x, here.y, dx, dy)
                {
                    plan.path(&tile_corners(tile), LOCAL_VALID_MOVEMENT_COLOR, true, false);
                }
            }
        }
    }

    let panel_top = (UP_TEXT_HEIGHT * state.ui_scale.unwrap_or(1.0)).ceil() as i32 + TEXT_MARGIN;
    let mut panel_y = panel_top;
    let mut panel_width = 0;
    let mut panel = |plan: &mut Plan, lines: Vec<String>| {
        for line in lines {
            panel_width = panel_width.max(font.width(&line, LABEL_SCALE));
            plan.text(TEXT_LEFT, panel_y, line, TEXT_COLOR);
            panel_y += line_height;
        }
    };
    if on("location") {
        let here = here()?;
        let [scene_x, scene_y] = scene_tile(&here);
        panel(
            &mut plan,
            vec![
                format!("Local {}, {}", here.local_x, here.local_y),
                format!("World {}, {}, {}", here.x, here.y, plane_text(&here)),
                format!("Scene {scene_x}, {scene_y}"),
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
                format!(
                    "Scale {}",
                    camera.zoom.map_or("?".to_string(), |zoom| zoom.to_string())
                ),
            ],
        );
    }

    if on("movement-flags") || on("tile-location") {
        let here = here()?;
        let (origin_x, origin_y) = scene_origin(&here);
        if let Some((tile, cursor)) = hovered_tile(pid, scene_tile(&here))? {
            plan.path(&tile_corners(tile), HOVER_COLOR, true, false);
            let (x, y) = (origin_x + tile[0], origin_y + tile[1]);
            let mut lines = Vec::new();
            if on("tile-location") {
                let settings = with_terrain(pid, |terrain| {
                    terrain
                        .setting(terrain.key().plane, tile[0], tile[1])
                        .unwrap_or(0)
                })?;
                let (region, region_x, region_y) = crate::devtools::region(x, y);
                lines.push(format!(
                    "World location: {}, {}, {}",
                    x,
                    y,
                    plane_text(&here)
                ));
                lines.push(format!(
                    "Region ID: {region} location: {region_x}, {region_y}"
                ));
                lines.push(format!("Flags: {settings}"));
            }
            if on("movement-flags") {
                let names = crate::devtools::movement_flag_names(collision_at(
                    collision.get(|| crate::scene_collision(pid))?,
                    x,
                    y,
                ));
                if names.is_empty() {
                    lines.push("No movement flags".to_string());
                }
                lines.extend(names.into_iter().map(str::to_string));
            }
            let (left, top) = (cursor[0] as i32 + 16, cursor[1] as i32 + 16);
            let widest = lines
                .iter()
                .map(|line| font.width(line, LABEL_SCALE))
                .max()
                .unwrap_or(0);
            plan.backdrops.push([
                left - TEXT_PADDING,
                top - 1,
                widest + TEXT_PADDING * 2,
                lines.len() as i32 * line_height,
            ]);
            for (i, line) in lines.into_iter().enumerate() {
                plan.text(left, top + i as i32 * line_height, line, TEXT_COLOR);
            }
        }
    }

    if panel_y > panel_top {
        plan.backdrops.push([
            TEXT_LEFT - TEXT_PADDING,
            panel_top - 1,
            panel_width + TEXT_PADDING * 2,
            panel_y - panel_top,
        ]);
    }

    let screen = if plan.points.is_empty() {
        Vec::new()
    } else {
        project_clipped(pid, &plan.points)?
    };
    let on_screen = |[x, y]: &[f32; 2]| x.abs() < SCREEN_LIMIT && y.abs() < SCREEN_LIMIT;
    let drawn = plan
        .paths
        .iter()
        .filter(|path| path.marker && path.points.iter().all(|&i| on_screen(&screen[i])))
        .count();

    let mut hasher = DefaultHasher::new();
    for path in &plan.paths {
        (path.color, path.closed).hash(&mut hasher);
        for &i in &path.points {
            let [x, y] = screen[i];
            (on_screen(&[x, y]), x.round() as i32, y.round() as i32).hash(&mut hasher);
        }
    }
    for arrow in &plan.arrows {
        (
            arrow.color,
            screen[arrow.from].map(|v| v.round() as i32),
            screen[arrow.to].map(|v| v.round() as i32),
        )
            .hash(&mut hasher);
    }
    for label in &plan.labels {
        let [x, y] = screen[label.anchor];
        (&label.text, label.color, x.round() as i32, y.round() as i32).hash(&mut hasher);
    }
    for text in &plan.texts {
        (text.x, text.y, &text.text, text.color).hash(&mut hasher);
    }
    for backdrop in &plan.backdrops {
        backdrop.hash(&mut hasher);
    }
    for rect in &plan.rects {
        (rect.x, rect.y, rect.width, rect.height, rect.color).hash(&mut hasher);
    }

    with_overlay(
        pid,
        overlay_mode::ON_SERIAL,
        false,
        Some(hasher.finish()),
        |buf, width, height| unsafe {
            for path in &plan.paths {
                let count = path.points.len();
                let edges = if path.closed {
                    count
                } else {
                    count.saturating_sub(1)
                };
                for i in 0..edges {
                    let (a, b) = (screen[path.points[i]], screen[path.points[(i + 1) % count]]);
                    if on_screen(&a) && on_screen(&b) {
                        draw_stroke(
                            buf,
                            width,
                            height,
                            [
                                a[0].round() as i32,
                                a[1].round() as i32,
                                b[0].round() as i32,
                                b[1].round() as i32,
                            ],
                            path.color,
                        );
                    }
                }
            }
            for arrow in &plan.arrows {
                let [from, to] = [arrow.from, arrow.to].map(|i| screen[i]);
                if on_screen(&from) && on_screen(&to) {
                    draw_arrow(
                        buf,
                        width,
                        height,
                        [from[0], from[1] + INTERACTING_SHIFT],
                        [to[0], to[1] + INTERACTING_SHIFT],
                        arrow.color,
                    );
                }
            }
            for label in &plan.labels {
                let [ax, ay] = screen[label.anchor];
                if !on_screen(&[ax, ay]) {
                    continue;
                }
                let left = ax.round() as i32 - font.width(&label.text, LABEL_SCALE) / 2;
                let top = ay.round() as i32 - font.max_height * LABEL_SCALE;
                draw_text(
                    buf,
                    width,
                    height,
                    &font,
                    &label.text,
                    [left, top],
                    label.color,
                );
            }
            for rect in &plan.rects {
                let (left, top, right, bottom) =
                    (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
                for line in [
                    [left, top, right, top],
                    [right, top, right, bottom],
                    [right, bottom, left, bottom],
                    [left, bottom, left, top],
                ] {
                    draw_stroke(buf, width, height, line, rect.color);
                }
            }
            for &backdrop in &plan.backdrops {
                fill_rect(buf, width, height, backdrop, TEXT_BACKGROUND);
            }
            for text in &plan.texts {
                draw_text(
                    buf,
                    width,
                    height,
                    &font,
                    &text.text,
                    [text.x, text.y],
                    text.color,
                );
            }
        },
    )?;
    Ok(drawn)
}

struct Fetched<T>(Option<T>);

impl<T> Fetched<T> {
    fn get(&mut self, fetch: impl FnOnce() -> Result<T, String>) -> Result<&T, String> {
        match &mut self.0 {
            Some(value) => Ok(value),
            empty => Ok(empty.insert(fetch()?)),
        }
    }
}

fn collision_at((x1, y1, _, flags): &SceneCollision, x: i32, y: i32) -> u32 {
    flags
        .get((x - x1) as usize)
        .and_then(|column| column.get((y - y1) as usize))
        .copied()
        .unwrap_or(0)
}

unsafe fn draw_text(
    buf: *mut u8,
    width: i32,
    height: i32,
    font: &crate::font::Font,
    text: &str,
    [left, top]: [i32; 2],
    color: Rgba,
) {
    font.plot(text, left + 1, top + 1, LABEL_SCALE, |x, y| unsafe {
        put_pixel(buf, width, height, x, y, LABEL_SHADOW)
    });
    font.plot(text, left, top, LABEL_SCALE, |x, y| unsafe {
        put_pixel(buf, width, height, x, y, color)
    });
}

unsafe fn fill_rect(
    buf: *mut u8,
    width: i32,
    height: i32,
    [left, top, rect_width, rect_height]: [i32; 4],
    color: Rgba,
) {
    for y in top.max(0)..(top + rect_height).min(height) {
        for x in left.max(0)..(left + rect_width).min(width) {
            unsafe { put_pixel(buf, width, height, x, y, color) };
        }
    }
}

unsafe fn draw_stroke(
    buf: *mut u8,
    width: i32,
    height: i32,
    [x1, y1, x2, y2]: [i32; 4],
    color: Rgba,
) {
    for shift in 0..STROKE_WIDTH {
        unsafe {
            draw_line(buf, width, height, [x1 + shift, y1, x2 + shift, y2], color);
            draw_line(buf, width, height, [x1, y1 + shift, x2, y2 + shift], color);
        }
    }
}

fn with_overlay(
    pid: u32,
    mode: u8,
    keep: bool,
    hash: Option<u64>,
    paint: impl FnOnce(*mut u8, i32, i32),
) -> Result<(), String> {
    let connection = current_connection(pid)?;
    let (width, height) = (connection.width, connection.height);
    let bytes = width.max(0) as usize * height.max(0) as usize * 4;
    if width <= 0 || height <= 0 || bytes > IMAGE_DATA_SIZE {
        return Err(format!("Client {pid} hasn't rendered a frame yet"));
    }

    let unmapped = |e| format!("Cannot map agent's shared memory in client {pid}: {e}");
    let session = session(pid).ok();
    let owned;
    let view = match session.as_deref() {
        Some(session) => session.shared(pid, unmapped)?,
        None => {
            owned = shared_view(pid, overlay_offset(ipc::OVERLAY_SLOTS), unmapped)?;
            &owned
        }
    };
    let base = view.ptr::<u8>().as_ptr();
    unsafe {
        let serial_at = base.add(mem::offset_of!(Header, overlay_serial)) as *mut u32;
        let hash_at = base.add(mem::offset_of!(Header, overlay_hash)) as *mut u64;
        let mode_at = base.add(mem::offset_of!(Header, overlay_mode));
        let current = *mode_at;

        let hash = hash.map(|h| (h ^ (width as u64) << 32 ^ height as u64) | 1);
        if hash.is_some_and(|h| hash_at.read_unaligned() == h) && current == mode {
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
    }
    Ok(())
}

unsafe fn draw_rect(
    buf: *mut u8,
    width: i32,
    height: i32,
    [x1, y1, x2, y2]: [i32; 4],
    color: Rgba,
) {
    let (left, right) = (x1.min(x2), x1.max(x2));
    let (top, bottom) = (y1.min(y2), y1.max(y2));
    for x in left.max(0)..=right.min(width - 1) {
        unsafe {
            put_pixel(buf, width, height, x, top, color);
            put_pixel(buf, width, height, x, bottom, color);
        }
    }
    for y in top.max(0)..=bottom.min(height - 1) {
        unsafe {
            put_pixel(buf, width, height, left, y, color);
            put_pixel(buf, width, height, right, y, color);
        }
    }
}

unsafe fn draw_arrow(
    buf: *mut u8,
    width: i32,
    height: i32,
    from: [f32; 2],
    to: [f32; 2],
    color: Rgba,
) {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let length = dx.hypot(dy);
    let round = |[x, y]: [f32; 2]| [x.round() as i32, y.round() as i32];
    let [x1, y1] = round(from);
    let [x2, y2] = round(to);
    unsafe { draw_stroke(buf, width, height, [x1, y1, x2, y2], color) };
    if length < 1.0 {
        return;
    }
    let (ux, uy) = (dx / length * ARROW_SCALE, dy / length * ARROW_SCALE);
    let base = [to[0] - 5.0 * ux, to[1] - 5.0 * uy];
    let corners = [
        to,
        [base[0] - 3.0 * uy, base[1] + 3.0 * ux],
        [base[0] + 3.0 * uy, base[1] - 3.0 * ux],
    ];
    let edge = |a: [f32; 2], b: [f32; 2], p: [f32; 2]| {
        (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
    };
    let extent = |axis: usize| {
        corners.iter().fold((i32::MAX, i32::MIN), |(low, high), c| {
            (
                low.min(c[axis].floor() as i32),
                high.max(c[axis].ceil() as i32),
            )
        })
    };
    let ((low, high), (top, bottom)) = (extent(0), extent(1));
    for y in top..=bottom {
        for x in low..=high {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let sides = [
                edge(corners[0], corners[1], p),
                edge(corners[1], corners[2], p),
                edge(corners[2], corners[0], p),
            ];
            if sides.iter().all(|&s| s >= 0.0) || sides.iter().all(|&s| s <= 0.0) {
                unsafe { put_pixel(buf, width, height, x, y, color) };
            }
        }
    }
}

unsafe fn draw_line(
    buf: *mut u8,
    width: i32,
    height: i32,
    [x1, y1, x2, y2]: [i32; 4],
    color: Rgba,
) {
    let (dx, dy) = (
        (i64::from(x2) - i64::from(x1)).abs(),
        -(i64::from(y2) - i64::from(y1)).abs(),
    );
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
            x = x.wrapping_add(sx);
        }
        if e2 <= dx {
            err += dx;
            y = y.wrapping_add(sy);
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

pub fn script_runs(pid: u32, since: u64, limit: usize) -> Result<crate::ScriptPage, String> {
    let page = SCRIPT_LOG.read(pid, |log| unsafe {
        script_log::read_since(log, since, limit)
    })?;
    Ok(crate::ScriptPage {
        next: page.next,
        dropped: page.dropped,
        runs: page
            .entries
            .into_iter()
            .map(|entry| crate::ScriptRun {
                sequence: entry.sequence,
                time_us: entry.time_us,
                tick: entry.tick,
                end: entry.phase == script_runs_phase::END,
                event_type: entry.event_type,
                arg_count: entry.arg_count,
                depth: entry.depth,
                args: entry
                    .args
                    .into_iter()
                    .map(|(kind, value)| match kind {
                        arg_kind::INT => value.to_string(),
                        arg_kind::STRING => "\"…\"".into(),
                        _ => "?".into(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

pub fn varc_changes(pid: u32, since: u64, limit: usize) -> Result<crate::VarcPage, String> {
    let page = VARC_LOG.read(pid, |log| unsafe {
        varc_log::read_since(log, since, limit)
    })?;
    let text = |bytes: &[u8], length: u32| {
        let mut text = String::from_utf8_lossy(bytes).into_owned();
        if (bytes.len() as u32) < length {
            text.push('…');
        }
        text
    };
    Ok(crate::VarcPage {
        next: page.next,
        dropped: page.dropped,
        changes: page
            .entries
            .into_iter()
            .map(|entry| {
                let string = entry.kind == varc_log::kind::STRING;
                crate::VarcChange {
                    sequence: entry.sequence,
                    time_ms: entry.time_ms,
                    index: entry.index,
                    string,
                    old: entry.old_known.then(|| {
                        if string {
                            text(&entry.old_text, entry.old_length)
                        } else {
                            entry.old_int.to_string()
                        }
                    }),
                    new: if string {
                        text(&entry.new_text, entry.new_length)
                    } else {
                        entry.new_int.to_string()
                    },
                }
            })
            .collect(),
    })
}

pub fn item_changes(pid: u32, since: u64, limit: usize) -> Result<crate::ItemPage, String> {
    let page = ITEM_LOG.read(pid, |log| unsafe {
        if since == crate::ITEM_CHANGES_NOW {
            item_log::Page {
                next: item_log::written(log),
                ..Default::default()
            }
        } else {
            item_log::read_since(log, since, limit)
        }
    })?;
    Ok(item_page(page))
}

pub struct ItemWatcher {
    view: View,
    event: Handle,
    slot: u32,
    next: u64,
}

impl ItemWatcher {
    pub fn new(pid: u32) -> Result<Self, String> {
        let view = ITEM_LOG.open(pid, FILE_MAP_READ | FILE_MAP_WRITE, "")?;
        let log = view.ptr::<item_log::Log>().as_ptr();
        let (slot, event) = claim_slot(
            |create| unsafe { item_log::subscribe(log, create) },
            |slot| item_wake_event_name(pid, slot),
        )
        .ok_or_else(|| format!("Too many readers are waiting for item changes in client {pid}"))?;
        let next = unsafe { item_log::written(log) };
        Ok(Self {
            view,
            event,
            slot,
            next,
        })
    }

    fn log(&self) -> *const item_log::Log {
        self.view.ptr::<item_log::Log>().as_ptr()
    }

    fn pending(&self) -> bool {
        unsafe { item_log::written(self.log()) > self.next }
    }

    fn wait(&self, timeout: Duration) -> bool {
        self.pending()
            || unsafe { WaitForSingleObject(self.event.0, wait_ms(timeout)) } == WAIT_OBJECT_0
    }

    pub fn read(&mut self, limit: usize) -> crate::ItemPage {
        let page = unsafe { item_log::read_since(self.log(), self.next, limit) };
        self.next = page.next;
        item_page(page)
    }
}

impl Drop for ItemWatcher {
    fn drop(&mut self) {
        unsafe { item_log::unsubscribe(self.log(), self.slot) };
    }
}

fn item_page(page: item_log::Page) -> crate::ItemPage {
    crate::ItemPage {
        next: page.next,
        dropped: page.dropped,
        changes: page
            .entries
            .into_iter()
            .map(|entry| crate::ItemChange {
                sequence: entry.sequence,
                time_ms: entry.time_ms,
                tick: Some(entry.change.tick).filter(|&tick| tick != item_log::NO_TICK),
                container: entry.change.container,
                slot: entry.change.slot,
                old_known: entry.change.old_known,
                old_item: entry.change.old_item,
                old_quantity: entry.change.old_quantity,
                new_item: entry.change.new_item,
                new_quantity: entry.change.new_quantity,
            })
            .collect(),
    }
}

pub fn item_changes_wait(
    pid: u32,
    since: u64,
    limit: usize,
    wait: Duration,
) -> Result<crate::ItemPage, String> {
    let ready = item_changes(pid, since, 1)?;
    if since == crate::ITEM_CHANGES_NOW
        || !ready.changes.is_empty()
        || ready.dropped > 0
        || ready.next != since
        || wait.is_zero()
    {
        return item_changes(pid, since, limit);
    }
    let watcher = ItemWatcher::new(pid)?;
    if item_changes(pid, since, 1)?.changes.is_empty() {
        watcher.wait(wait);
    }
    item_changes(pid, since, limit)
}
