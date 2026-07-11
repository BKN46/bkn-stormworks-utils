#![allow(non_snake_case)]

use std::{
    ffi::{c_char, c_void},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, HINSTANCE},
    System::LibraryLoader::{
        DisableThreadLibraryCalls, GetModuleFileNameW, GetProcAddress, LoadLibraryW,
    },
};

#[link(name = "kernel32")]
extern "system" {
    fn CreateThread(
        lpThreadAttributes: *const c_void,
        dwStackSize: usize,
        lpStartAddress: Option<unsafe extern "system" fn(*mut c_void) -> u32>,
        lpParameter: *mut c_void,
        dwCreationFlags: u32,
        lpThreadId: *mut u32,
    ) -> HANDLE;
}

static BOOTSTRAP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static BOOTSTRAP_SUCCEEDED: AtomicBool = AtomicBool::new(false);
static BOOTSTRAP_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static OPENAL_REAL_MODULE: AtomicUsize = AtomicUsize::new(0);
static PROXY_MODULE: AtomicUsize = AtomicUsize::new(0);
static LOGS_PREPARED: AtomicBool = AtomicBool::new(false);

type BootstrapReplaceDll = unsafe extern "system" fn(*mut u16) -> u32;

type ALenum = i32;
type ALint = i32;
type ALuint = u32;
type ALsizei = i32;
type ALfloat = f32;
type ALCboolean = i8;
type ALCdevice = c_void;
type ALCcontext = c_void;
type ALCenum = i32;
type ALCint = i32;
type ALCsizei = i32;
type ALCchar = c_char;

#[no_mangle]
pub extern "system" fn DllMain(module: HINSTANCE, reason: u32, _: *mut c_void) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        PROXY_MODULE.store(module as usize, Ordering::SeqCst);
        BOOTSTRAP_IN_PROGRESS.store(false, Ordering::SeqCst);
        BOOTSTRAP_SUCCEEDED.store(false, Ordering::SeqCst);
        BOOTSTRAP_ATTEMPTS.store(0, Ordering::SeqCst);
        LOGS_PREPARED.store(false, Ordering::SeqCst);
        unsafe {
            let _ = DisableThreadLibraryCalls(module);
        }
        start_video_get_bootstrap();
    }
    1
}

fn start_video_get_bootstrap() {
    const MAX_BOOTSTRAP_ATTEMPTS: usize = 3;
    if BOOTSTRAP_SUCCEEDED.load(Ordering::SeqCst)
        || BOOTSTRAP_ATTEMPTS.load(Ordering::SeqCst) >= MAX_BOOTSTRAP_ATTEMPTS
    {
        return;
    }
    if BOOTSTRAP_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        BOOTSTRAP_ATTEMPTS.fetch_add(1, Ordering::SeqCst);
        unsafe {
            let handle = CreateThread(
                std::ptr::null(),
                0,
                Some(bootstrap_thread_entry),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
            );
            if handle.is_null() {
                BOOTSTRAP_IN_PROGRESS.store(false, Ordering::SeqCst);
                if let Some(root) = dll_directory() {
                    append_proxy_log(&root, "bootstrap CreateThread failed");
                }
            } else {
                let _ = CloseHandle(handle);
            }
        }
    }
}

unsafe extern "system" fn bootstrap_thread_entry(_: *mut c_void) -> u32 {
    if bootstrap_video_get() {
        BOOTSTRAP_SUCCEEDED.store(true, Ordering::SeqCst);
    }
    BOOTSTRAP_IN_PROGRESS.store(false, Ordering::SeqCst);
    0
}

fn bootstrap_video_get() -> bool {
    std::thread::sleep(Duration::from_millis(250));
    let Some(root) = dll_directory() else {
        return false;
    };
    prepare_fresh_logs(&root);
    append_proxy_log(
        &root,
        &format!(
            "bootstrap start attempt={} root={}",
            BOOTSTRAP_ATTEMPTS.load(Ordering::SeqCst),
            root.display()
        ),
    );
    let paths = proxy_paths(&root);
    let video_get = paths.video_get_dll;
    let context = paths.runtime_context;
    if !video_get.is_file() {
        append_proxy_log(
            &root,
            &format!("bootstrap missing dll={}", video_get.display()),
        );
        return false;
    }
    if !context.is_file() {
        append_proxy_log(
            &root,
            &format!("bootstrap missing context={}", context.display()),
        );
        return false;
    }
    let mut dll_wide = wide_null(&video_get);
    let mut context_wide = wide_null(&context);
    unsafe {
        let module = LoadLibraryW(dll_wide.as_mut_ptr());
        if module.is_null() {
            append_proxy_log(
                &root,
                &format!(
                    "bootstrap LoadLibraryW failed dll={} gle={}",
                    video_get.display(),
                    GetLastError()
                ),
            );
            return false;
        }
        let name = b"stormworks_video_get_bootstrap_replace_dll\0";
        let proc = GetProcAddress(module, name.as_ptr());
        let Some(proc) = proc else {
            append_proxy_log(
                &root,
                &format!(
                    "bootstrap GetProcAddress failed symbol=stormworks_video_get_bootstrap_replace_dll gle={}",
                    GetLastError()
                ),
            );
            return false;
        };
        let bootstrap: BootstrapReplaceDll = std::mem::transmute(proc);
        let result = bootstrap(context_wide.as_mut_ptr());
        append_proxy_log(&root, &format!("bootstrap returned result={result}"));
        result != 0
    }
}

fn openal_real_module() -> *mut c_void {
    let cached = OPENAL_REAL_MODULE.load(Ordering::SeqCst);
    if cached != 0 {
        return cached as *mut c_void;
    }
    let Some(root) = dll_directory() else {
        return std::ptr::null_mut();
    };
    let mut path = wide_null(&proxy_paths(&root).real_openal);
    let module = unsafe { LoadLibraryW(path.as_mut_ptr()) };
    if module.is_null() {
        return std::ptr::null_mut();
    }
    let value = module as usize;
    match OPENAL_REAL_MODULE.compare_exchange(0, value, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => module,
        Err(existing) => existing as *mut c_void,
    }
}

fn cached_proc(cache: &AtomicUsize, name: &'static [u8]) -> usize {
    let cached = cache.load(Ordering::SeqCst);
    if cached != 0 {
        return cached;
    }
    let module = openal_real_module();
    if module.is_null() {
        return 0;
    }
    let Some(proc) = (unsafe { GetProcAddress(module, name.as_ptr()) }) else {
        return 0;
    };
    let value = proc as usize;
    match cache.compare_exchange(0, value, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) => value,
        Err(existing) => existing,
    }
}

fn dll_directory() -> Option<PathBuf> {
    let module = PROXY_MODULE.load(Ordering::SeqCst);
    if module != 0 {
        if let Some(path) = module_file_path(module as HINSTANCE) {
            return directory_from_file_path(path);
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(directory_from_file_path)
}

fn module_file_path(module: HINSTANCE) -> Option<PathBuf> {
    const INITIAL_CAPACITY: usize = 260;
    let mut capacity = INITIAL_CAPACITY;
    loop {
        let mut buffer = vec![0u16; capacity];
        let len = unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) };
        if len == 0 {
            return None;
        }
        let len = len as usize;
        if len + 1 < buffer.len() {
            buffer.truncate(len);
            return Some(PathBuf::from(String::from_utf16_lossy(&buffer)));
        }
        capacity *= 2;
        if capacity > 32768 {
            return None;
        }
    }
}

fn directory_from_file_path(path: PathBuf) -> Option<PathBuf> {
    path.parent().map(PathBuf::from)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxyPaths {
    real_openal: PathBuf,
    video_get_dll: PathBuf,
    runtime_context: PathBuf,
    proxy_log: PathBuf,
}

fn proxy_paths(root: &Path) -> ProxyPaths {
    let video_get = root.join("video_get");
    ProxyPaths {
        real_openal: root.join("OpenAL64_real.dll"),
        video_get_dll: video_get.join("StormworksVideoGet.dll"),
        runtime_context: video_get.join("runtime-context.json"),
        proxy_log: video_get.join("logs").join("openal_proxy.log"),
    }
}

fn wide_null(path: &PathBuf) -> Vec<u16> {
    path.as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect()
}

fn append_proxy_log(root: &Path, line: &str) {
    let path = proxy_paths(root).proxy_log;
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{line}");
    }
}

fn prepare_fresh_logs(root: &Path) {
    if LOGS_PREPARED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let log_dir = proxy_paths(root).proxy_log.parent().map(PathBuf::from);
    let Some(log_dir) = log_dir else {
        return;
    };
    let _ = fs::create_dir_all(&log_dir);
    for name in [
        "openal_proxy.log",
        "video_get.log",
        "video_get_runtime_snapshot.json",
        "video_get_runtime_snapshots.jsonl",
        "video_get_runtime_heartbeat.json",
    ] {
        let _ = fs::remove_file(log_dir.join(name));
    }
    for name in ["load_events", "frame_previews", "archive"] {
        let _ = fs::remove_dir_all(log_dir.join(name));
    }
    append_proxy_log(root, "fresh log start previous logs cleared");
}

macro_rules! forward_void {
    ($name:ident($($arg:ident: $ty:ty),* $(,)?)) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name($($arg: $ty),*) {
            start_video_get_bootstrap();
            static PROC: AtomicUsize = AtomicUsize::new(0);
            let proc = cached_proc(&PROC, concat!(stringify!($name), "\0").as_bytes());
            if proc == 0 {
                return;
            }
            let function: unsafe extern "C" fn($($ty),*) = std::mem::transmute(proc);
            function($($arg),*);
        }
    };
}

macro_rules! forward_ret {
    ($name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty, $default:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name($($arg: $ty),*) -> $ret {
            start_video_get_bootstrap();
            static PROC: AtomicUsize = AtomicUsize::new(0);
            let proc = cached_proc(&PROC, concat!(stringify!($name), "\0").as_bytes());
            if proc == 0 {
                return $default;
            }
            let function: unsafe extern "C" fn($($ty),*) -> $ret = std::mem::transmute(proc);
            function($($arg),*)
        }
    };
}

forward_void!(alAuxiliaryEffectSloti(effectslot: ALuint, param: ALenum, value: ALint));
forward_void!(alBufferData(buffer: ALuint, format: ALenum, data: *const c_void, size: ALsizei, freq: ALsizei));
forward_ret!(alcCloseDevice(device: *mut ALCdevice) -> ALCboolean, 0);
forward_ret!(alcCreateContext(device: *mut ALCdevice, attrlist: *const ALCint) -> *mut ALCcontext, std::ptr::null_mut());
forward_void!(alcDestroyContext(context: *mut ALCcontext));
forward_void!(alcGetIntegerv(device: *mut ALCdevice, param: ALCenum, size: ALCsizei, values: *mut ALCint));
forward_ret!(alcMakeContextCurrent(context: *mut ALCcontext) -> ALCboolean, 0);
forward_ret!(alcOpenDevice(devicename: *const ALCchar) -> *mut ALCdevice, std::ptr::null_mut());
forward_void!(alDeleteBuffers(n: ALsizei, buffers: *const ALuint));
forward_void!(alDeleteSources(n: ALsizei, sources: *const ALuint));
forward_void!(alDistanceModel(distance_model: ALenum));
forward_void!(alEffectf(effect: ALuint, param: ALenum, value: ALfloat));
forward_void!(alEffecti(effect: ALuint, param: ALenum, value: ALint));
forward_void!(alFilterf(filter: ALuint, param: ALenum, value: ALfloat));
forward_void!(alFilteri(filter: ALuint, param: ALenum, value: ALint));
forward_void!(alGenAuxiliaryEffectSlots(n: ALsizei, effectslots: *mut ALuint));
forward_void!(alGenBuffers(n: ALsizei, buffers: *mut ALuint));
forward_void!(alGenEffects(n: ALsizei, effects: *mut ALuint));
forward_void!(alGenFilters(n: ALsizei, filters: *mut ALuint));
forward_void!(alGenSources(n: ALsizei, sources: *mut ALuint));
forward_ret!(alGetError() -> ALenum, 0);
forward_void!(alGetSourcei(source: ALuint, param: ALenum, value: *mut ALint));
forward_void!(alListener3f(param: ALenum, value1: ALfloat, value2: ALfloat, value3: ALfloat));
forward_void!(alListenerfv(param: ALenum, values: *const ALfloat));
forward_void!(alSource3f(source: ALuint, param: ALenum, value1: ALfloat, value2: ALfloat, value3: ALfloat));
forward_void!(alSource3i(source: ALuint, param: ALenum, value1: ALint, value2: ALint, value3: ALint));
forward_void!(alSourcef(source: ALuint, param: ALenum, value: ALfloat));
forward_void!(alSourcei(source: ALuint, param: ALenum, value: ALint));
forward_void!(alSourcePlay(source: ALuint));
forward_void!(alSourceQueueBuffers(source: ALuint, nb: ALsizei, buffers: *const ALuint));
forward_void!(alSourceStop(source: ALuint));
forward_void!(alSourceUnqueueBuffers(source: ALuint, nb: ALsizei, buffers: *mut ALuint));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_paths_are_rooted_at_stormworks_exe_directory() {
        let root = PathBuf::from(r"D:\SteamLibrary\steamapps\common\Stormworks");
        let paths = proxy_paths(&root);
        assert_eq!(paths.real_openal, root.join("OpenAL64_real.dll"));
        assert_eq!(
            paths.video_get_dll,
            root.join("video_get").join("StormworksVideoGet.dll")
        );
        assert_eq!(
            paths.runtime_context,
            root.join("video_get").join("runtime-context.json")
        );
        assert_eq!(
            paths.proxy_log,
            root.join("video_get").join("logs").join("openal_proxy.log")
        );
    }

    #[test]
    fn proxy_directory_uses_proxy_dll_parent() {
        let path = PathBuf::from(r"D:\SteamLibrary\steamapps\common\Stormworks\OpenAL64.dll");
        assert_eq!(
            directory_from_file_path(path),
            Some(PathBuf::from(
                r"D:\SteamLibrary\steamapps\common\Stormworks"
            ))
        );
    }

    #[test]
    fn prepare_fresh_logs_clears_existing_files_once() {
        let root = std::env::temp_dir().join(format!(
            "stormworks_openal_proxy_log_clear_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let log_dir = root.join("video_get").join("logs");
        fs::create_dir_all(log_dir.join("load_events")).unwrap();
        fs::write(log_dir.join("openal_proxy.log"), "old proxy").unwrap();
        fs::write(log_dir.join("video_get.log"), "old plugin").unwrap();
        fs::write(
            log_dir.join("video_get_runtime_snapshot.json"),
            "{\"old\":true}",
        )
        .unwrap();
        fs::write(
            log_dir
                .join("load_events")
                .join("video_get_load_events.jsonl"),
            "{\"event\":\"old\"}",
        )
        .unwrap();

        LOGS_PREPARED.store(false, Ordering::SeqCst);
        prepare_fresh_logs(&root);
        prepare_fresh_logs(&root);

        assert!(!log_dir.join("archive").exists());
        assert!(log_dir.join("openal_proxy.log").is_file());
        assert_eq!(
            fs::read_to_string(log_dir.join("openal_proxy.log")).unwrap(),
            "fresh log start previous logs cleared\n"
        );
        assert!(!log_dir.join("video_get.log").exists());
        assert!(!log_dir.join("video_get_runtime_snapshot.json").exists());
        assert!(!log_dir.join("load_events").exists());

        let _ = fs::remove_dir_all(root);
        LOGS_PREPARED.store(false, Ordering::SeqCst);
    }
}
