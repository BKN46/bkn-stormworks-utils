#![allow(non_snake_case)]
#![recursion_limit = "256"]

use serde::Serialize;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    ffi::{c_char, c_void, CStr, CString},
    fs,
    mem::size_of,
    path::PathBuf,
    ptr,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};
use stormworks_modkit_shared::{read_json, PluginRuntimeContext};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::GetLastError,
    System::{
        Diagnostics::Debug::FlushInstructionCache,
        Memory::{
            VirtualAlloc, VirtualFree, VirtualProtect, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        },
        Threading::{GetCurrentProcess, GetCurrentProcessId},
    },
};

#[cfg(windows)]
#[link(name = "opengl32")]
extern "system" {
    fn glBindTexture(target: u32, texture: u32);
    fn glFlush();
    fn glGetError() -> u32;
    fn glGetIntegerv(pname: u32, data: *mut i32);
    fn glGetTexImage(target: u32, level: i32, format: u32, ty: u32, pixels: *mut c_void);
    fn glGetTexLevelParameteriv(target: u32, level: i32, pname: u32, params: *mut i32);
    fn glIsTexture(texture: u32) -> u8;
    fn glReadBuffer(src: u32);
    fn glReadPixels(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: u32,
        ty: u32,
        data: *mut c_void,
    );
    fn wglGetProcAddress(name: *const c_char) -> *const c_void;
}

static FRAME_ID: AtomicU64 = AtomicU64::new(1);
static FRAME_PUMP_ACTIVE: AtomicBool = AtomicBool::new(false);
static VERBOSE_RUNTIME_DIAGNOSTICS: AtomicBool = AtomicBool::new(false);
static COMPONENT_LUA_REGISTRATION_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_UPLOAD_FRAME_LOGGED: AtomicBool = AtomicBool::new(false);
static VIDEO_INIT_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_UPLOAD_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_UPLOAD_CAPTURE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_UPLOAD_NO_SLOT_LOGGED_COUNT: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_UPLOAD_NO_MATCH_LOGGED_COUNT: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_UNMAPPED_NODE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_NO_SLOT_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_SOURCE_LAYOUT_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static VIDEO_LOGIC_EDGE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static VIDEO_NODE_REGISTRY_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static VIDEO_NODE_INIT_REGISTRY_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static SOURCE_TEXTURE_PROBE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static SOURCE_TEXTURE_CAPTURE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static MONITOR_RENDER_PROBE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static MONITOR_RENDER_CAPTURE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static MONITOR_RENDER_PBO_DRAIN_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static DYNAMIC_GL_BIND_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static RENDER_QUEUE_ALLOC_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static RENDER_QUEUE_SUBMIT_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_MONITOR_BIND_PROBE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_MONITOR_BIND_WITH_SLOTS_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_MONITOR_BIND_SLOT_PROBE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_MONITOR_BIND_CAPTURE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static MRQ_BRIDGE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_GL_BIND_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_GL_BIND_CAPTURE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_GL_BIND_UNIT_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_EXACT_READBACK_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_EXACT_READBACK_LAST_MS: AtomicU64 = AtomicU64::new(0);
static MONITOR_GL_BIND_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static RENDERER_VIDEO_PASS_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static PENDING_MONITOR_PROBE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static MONITOR_RENDER_HEAVY_PROBE_LAST_MS: AtomicU64 = AtomicU64::new(0);
static MONITOR_RENDER_HEAVY_PROBE_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
static MONITOR_INPUT_RELATION_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static MONITOR_BRIDGE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static RENDER_TARGET_TEXTURE_CREATE_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static RENDER_TARGET_TEXTURE_CREATE_WITH_SLOTS_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static LUA_PACKED_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);
static RUNTIME: OnceLock<Mutex<RuntimeState>> = OnceLock::new();
static LUA_ADAPTER: OnceLock<Mutex<LuaAdapterState>> = OnceLock::new();
static DETOURS: OnceLock<Mutex<DetourRegistry>> = OnceLock::new();
static GAME_LUA_COMPONENT_CONTEXTS: OnceLock<Mutex<BTreeMap<usize, usize>>> = OnceLock::new();
/// Exact type-6 vehicle wiring observed at c_vehicle_logic_slot_output_video vfuncs 6-8.
/// Keys are input-video slot objects and values are their connected output-video slot objects.
static VIDEO_INPUT_TO_OUTPUT_EDGES: Mutex<BTreeMap<usize, usize>> = Mutex::new(BTreeMap::new());
/// Component liveness: maps a component key to the last time it called any `video.*` Lua
/// callback. A live Lua component (its microcontroller is spawned and ticking) calls back
/// every frame, so recent activity is the reliable "alive" signal. This is used to drop
/// stale slots after a vehicle reload — the game creates a brand-new component context per
/// spawn and never notifies us when the old one dies, so registration state is not enough.
static GAME_LUA_COMPONENT_LAST_SEEN: OnceLock<Mutex<BTreeMap<String, Instant>>> = OnceLock::new();
#[cfg(windows)]
static DETOUR_SELF_TEST_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static GL_BIND_TEXTURE_IAT_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static GL_BIND_TEXTURE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static WGL_GET_PROC_ADDRESS_IAT_INSTALLED: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static WGL_GET_PROC_ADDRESS_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static GL_BIND_TEXTURE_UNIT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static GL_BIND_TEXTURES_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static GL_FRAMEBUFFER_TEXTURE_2D_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static GL_FRAMEBUFFER_TEXTURE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static GL_FRAMEBUFFER_TEXTURE_LAYER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static LUA_REGISTRATION_ORIGINAL_DIRECT: AtomicUsize = AtomicUsize::new(0);
static LUA_REGISTRATION_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static LUA_REGISTRATION_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static LUA_REGISTRATION_ORIGINAL_ARG3: AtomicUsize = AtomicUsize::new(0);
static LUA_REGISTRATION_ORIGINAL_ARG4: AtomicUsize = AtomicUsize::new(0);
static COMPONENT_LUA_INIT_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static GAME_LUA_CREATE_TABLE: AtomicUsize = AtomicUsize::new(0);
static GAME_LUA_PUSH_STRING: AtomicUsize = AtomicUsize::new(0);
static GAME_LUA_RAWSETI: AtomicUsize = AtomicUsize::new(0);
static GAME_LUA_REGISTER_TABLE: AtomicUsize = AtomicUsize::new(0);
static GAME_LUA_ARG_SLOT: AtomicUsize = AtomicUsize::new(0);
static COMPONENT_CONTEXT_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static COMPONENT_CONTEXT_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static COMPONENT_CONTEXT_ORIGINAL_ARG3: AtomicUsize = AtomicUsize::new(0);
static COMPONENT_CONTEXT_ORIGINAL_ARG4: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_ORIGINAL_ARG3: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_ORIGINAL_ARG4: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_NODE_UPDATE_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static INPUT_VIDEO_NODE_SELECT_ORIGINAL_ARG5: AtomicUsize = AtomicUsize::new(0);
static VIDEO_OUTPUT_SLOT_ADD_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static VIDEO_OUTPUT_SLOT_REMOVE_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static VIDEO_OUTPUT_SLOT_CLEAR_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_SOURCE_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_SOURCE_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_SOURCE_ORIGINAL_ARG3: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_SOURCE_ORIGINAL_ARG4: AtomicUsize = AtomicUsize::new(0);
static TEXTURE_UPLOAD_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static MONITOR_RENDER_QUEUE_ORIGINAL_ARG6: AtomicUsize = AtomicUsize::new(0);
static RENDER_QUEUE_ALLOC_ORIGINAL_ARG1: AtomicUsize = AtomicUsize::new(0);
static RENDER_QUEUE_SUBMIT_COPY_ORIGINAL_ARG2: AtomicUsize = AtomicUsize::new(0);
static RENDER_TARGET_TEXTURE_CREATE_ORIGINAL_ARG3: AtomicUsize = AtomicUsize::new(0);
static RENDERER_VIDEO_PASS_ORIGINAL_ARG8: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_MONITOR_BIND_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static ADDITIVE_MONITOR_VIDEO_BIND_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static LUA_COMPONENT_CONTEXT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    #[cfg(windows)]
    static ADDITIVE_MONITOR_GL_BIND_CONTEXT: RefCell<Vec<AdditiveMonitorGlBindFrame>> = const { RefCell::new(Vec::new()) };
    #[cfg(windows)]
    static MONITOR_RENDER_GL_BIND_CONTEXT: RefCell<Vec<MonitorRenderGlBindFrame>> = const { RefCell::new(Vec::new()) };
    #[cfg(windows)]
    static RENDER_QUEUE_ALLOC_CONTEXT: RefCell<Vec<RenderQueueAllocFrame>> = const { RefCell::new(Vec::new()) };
    #[cfg(windows)]
    static RENDERER_VIDEO_PASS_CONTEXT: RefCell<Vec<RendererVideoPassFrame>> = const { RefCell::new(Vec::new()) };
    #[cfg(windows)]
    static RENDER_TARGET_TEXTURE_CREATE_RECORDING: Cell<bool> = const { Cell::new(false) };
    // Latest scene_state (renderer_video_pass param_3) seen on THIS render thread. Set by the
    // lightweight renderer_video_pass hook WITHOUT pushing a probe context, so it does not
    // re-enable the per-glBindTexture GL-query path that caused the earlier lag. Read by the
    // additive_monitor bind hook to locate the monitor draw-list entry (and its camera source
    // object) that owns the texture being bound. Cleared to 0 when the pass returns.
    #[cfg(windows)]
    static RENDERER_VIDEO_PASS_SCENE_STATE: Cell<usize> = const { Cell::new(0) };
}

mod api;
mod hook_utils;
mod logging;
mod memory;
mod pe;
mod pixels;
mod preview;
mod time_utils;

use api::*;
use hook_utils::{
    panic_payload_message, run_hook_i32 as run_hook_i32_with_error,
    run_hook_void as run_hook_void_with_error,
};
#[cfg(test)]
use logging::append_jsonl;
use logging::{append_log, clear_plugin_log_outputs};
use memory::*;
use pe::{read_pe_image_base, verify_signature_bytes, ByteCheckSummary};
use pixels::*;

type GameLuaCreateTableFn = unsafe extern "C" fn(usize) -> usize;
type GameLuaPushStringFn = unsafe extern "C" fn(usize, *const c_char) -> usize;
type GameLuaRawSetIFn = unsafe extern "C" fn(usize, i32, i64);
type GameLuaRegisterTableFn =
    unsafe extern "C" fn(usize, *const usize, *const GameLuaFunctionPair, usize);
type GameLuaArgSlotFn = unsafe extern "C" fn(usize, i32) -> *mut u8;
#[cfg(windows)]
type GlGenBuffersFn = unsafe extern "system" fn(i32, *mut u32);
#[cfg(windows)]
type GlBindBufferFn = unsafe extern "system" fn(u32, u32);
#[cfg(windows)]
type GlBufferDataFn = unsafe extern "system" fn(u32, isize, *const c_void, u32);
#[cfg(windows)]
type GlMapBufferRangeFn = unsafe extern "system" fn(u32, isize, isize, u32) -> *mut c_void;
#[cfg(windows)]
type GlUnmapBufferFn = unsafe extern "system" fn(u32) -> u8;
#[cfg(windows)]
type GlFenceSyncFn = unsafe extern "system" fn(u32, u32) -> *mut c_void;
#[cfg(windows)]
type GlClientWaitSyncFn = unsafe extern "system" fn(*mut c_void, u32, u64) -> u32;
#[cfg(windows)]
type GlDeleteSyncFn = unsafe extern "system" fn(*mut c_void);
#[cfg(windows)]
type GlGenFramebuffersFn = unsafe extern "system" fn(i32, *mut u32);
#[cfg(windows)]
type GlBindFramebufferFn = unsafe extern "system" fn(u32, u32);
#[cfg(windows)]
type GlFramebufferTexture2DFn = unsafe extern "system" fn(u32, u32, u32, u32, i32);
#[cfg(windows)]
type GlCheckFramebufferStatusFn = unsafe extern "system" fn(u32) -> u32;
#[cfg(windows)]
type GlDeleteFramebuffersFn = unsafe extern "system" fn(i32, *const u32);
type GlActiveTextureFn = unsafe extern "system" fn(u32);
#[cfg(windows)]
type WglGetProcAddressFn = unsafe extern "system" fn(*const c_char) -> *const c_void;
#[cfg(windows)]
type GlBindTextureUnitFn = unsafe extern "system" fn(u32, u32);
#[cfg(windows)]
type GlBindTexturesFn = unsafe extern "system" fn(u32, i32, *const u32);
#[cfg(windows)]
type GlFramebufferTextureFn = unsafe extern "system" fn(u32, u32, u32, i32);
#[cfg(windows)]
type GlFramebufferTextureLayerFn = unsafe extern "system" fn(u32, u32, u32, i32, i32);

const GAME_LUA_FIRST_UPVALUE_INDEX: i32 = -1_001_001;
const LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA: usize = 0x550;
const MICROPROCESSOR_BRIDGE_VIDEO_INPUT_NODE_OFFSET: usize = 0x50;
const MICROPROCESSOR_BRIDGE_VIDEO_OUTPUT_NODE_OFFSET: usize = 0x2a8;
const INPUT_VIDEO_NODE_SELECTED_SOURCE_OFFSET: usize = 0x28;
const INPUT_VIDEO_NODE_RESOLVED_SOURCE_OFFSET: usize = 0x30;
const VTABLE_MICROPROCESSOR_LOGIC_NODE_INPUT_VIDEO: u64 = 0x140b8e7b8;
const VTABLE_MICROPROCESSOR_LOGIC_NODE_OUTPUT_VIDEO: u64 = 0x140af4f90;
const VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO: u64 = 0x140b7c6c8;
const VTABLE_VEHICLE_LOGIC_SLOT_OUTPUT_VIDEO: u64 = 0x140b72098;
const LOGIC_KIND_EXTERNAL_VIDEO_INPUT: u64 = 0x166;
const LOGIC_KIND_LUA_VIDEO_OUTPUT: u64 = 0x265;
const SOURCE_TEXTURE_SCAN_DIRECT_MIN_OFFSET: usize = 0x20;
const SOURCE_TEXTURE_SCAN_DIRECT_BYTES: usize = 0x100;
const SOURCE_TEXTURE_SCAN_POINTER_BYTES: usize = 0x100;
const SOURCE_TEXTURE_SCAN_POINTER_DEREF_BYTES: usize = 0x100;
const MAX_SOURCE_TEXTURE_CANDIDATES_PER_SOURCE: usize = 32;
const MAX_SOURCE_TEXTURE_READ_PIXELS: usize = 1024 * 1024;
const MONITOR_ACTIVE_OFFSET: usize = 0x4f0;
const MONITOR_WIDTH_OFFSET: usize = 0x4b8;
const MONITOR_HEIGHT_OFFSET: usize = 0x4bc;
const MONITOR_VIDEO_INPUT_SLOT_OBJECT_OFFSET: usize = 0x1a8;
const MONITOR_VIDEO_INPUT_SLOT_REF_OFFSET: usize = 0x1b8;
const MONITOR_VIDEO_INPUT_SLOT_OFFSET: usize = MONITOR_VIDEO_INPUT_SLOT_REF_OFFSET;
const MONITOR_RENDER_RESOURCE_A_OFFSET: usize = 0x4c8;
const MONITOR_RENDER_RESOURCE_B_OFFSET: usize = 0x4d8;
const RENDERER_COMMAND_MONITOR_OFFSET: usize = 0x00;
const RENDERER_COMMAND_RESOURCE_A_REF_OFFSET: usize = 0x148;
const RENDERER_COMMAND_RESOURCE_B_REF_OFFSET: usize = 0x150;
const RENDERER_COMMAND_MONITOR_WIDTH_OFFSET: usize = 0x16c;
const RENDERER_COMMAND_MONITOR_HEIGHT_OFFSET: usize = 0x170;
const RENDER_QUEUE_ITEM_SIZE: usize = 0x180;
const RENDER_CONTEXT_SUBMIT_QUEUE_OFFSET: usize = 0x5a0;
const RENDER_QUEUE_BUFFER_OFFSET: usize = 0x00;
const RENDER_QUEUE_CAPACITY_OFFSET: usize = 0x08;
const RENDER_QUEUE_START_OFFSET: usize = 0x0c;
const RENDER_QUEUE_COUNT_OFFSET: usize = 0x10;
const RENDER_QUEUE_SCAN_LIMIT: usize = 64;
const ADDITIVE_MONITOR_DRAW_ITEM_MONITOR_BACK_OFFSET: usize = 0x08;
const ADDITIVE_MONITOR_DRAW_ITEM_SCAN_BYTES: usize = 0x180;
const ADDITIVE_MONITOR_DRAW_ITEM_LAYOUT_BYTES: usize = 0x80;
const ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET: usize = 0x28;
const ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET: usize = 0x48;
const ADDITIVE_MONITOR_TEXTURE_NESTED_POINTER_OFFSET: usize = 0x08;
const ADDITIVE_BIND_ARGUMENT_LAYOUT_QWORDS: usize = 12;
const MONITOR_RESOURCE_SCAN_BYTES: usize = 0x120;
const GL_TEXTURE0: u32 = 0x84c0;
const GL_ACTIVE_TEXTURE: u32 = 0x84e0;
const GL_TEXTURE_BINDING_2D: u32 = 0x8069;
const GL_TEXTURE_2D: u32 = 0x0de1;
const GL_RGBA: u32 = 0x1908;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_TEXTURE_WIDTH: u32 = 0x1000;
const GL_TEXTURE_HEIGHT: u32 = 0x1001;
const GL_VIEWPORT: u32 = 0x0ba2;
const GL_SCISSOR_BOX: u32 = 0x0c10;
const GL_READ_FRAMEBUFFER: u32 = 0x8ca8;
const GL_READ_FRAMEBUFFER_BINDING: u32 = 0x8caa;
const GL_COLOR_ATTACHMENT0: u32 = 0x8ce0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8cd5;
const GL_COLOR_ATTACHMENT0_READ_BUFFER: u32 = GL_COLOR_ATTACHMENT0;
const GL_PIXEL_PACK_BUFFER: u32 = 0x88eb;
const GL_PIXEL_PACK_BUFFER_BINDING: u32 = 0x88ed;
const GL_STREAM_READ: u32 = 0x88e1;
const GL_MAP_READ_BIT: u32 = 0x0001;
const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;
const GL_SYNC_FLUSH_COMMANDS_BIT: u32 = 0x0000_0001;
const GL_ALREADY_SIGNALED: u32 = 0x911a;
const GL_TIMEOUT_EXPIRED: u32 = 0x911b;
const GL_CONDITION_SATISFIED: u32 = 0x911c;
const GL_WAIT_FAILED: u32 = 0x911d;
const GL_NO_ERROR: u32 = 0;
const MONITOR_PBO_RING: usize = 3;
const MONITOR_PBO_PENDING_MAX_AGE_MS: u64 = 5_000;
const MONITOR_RENDER_HEAVY_PROBE_MIN_INTERVAL_MS: u64 = 1_000;
const MONITOR_RENDER_HEAVY_PROBE_MAX_ATTEMPTS: usize = 12;
const MONITOR_RENDER_MAX_READBACK_CANDIDATES_PER_PROBE: usize = 3;
const MONITOR_RENDER_SUPERSAMPLE_SCALE: u32 = 2;
const ADDITIVE_EXACT_READBACK_MAX_ATTEMPTS: usize = 6;
const ADDITIVE_EXACT_READBACK_MIN_INTERVAL_MS: u64 = 1_000;
const RENDERER_PASS_TARGET_SCAN_BYTES: usize = 0x180;
const RENDERER_PASS_TARGET_NESTED_SCAN_BYTES: usize = 0x100;
const RENDERER_PASS_TARGET_NESTED_POINTER_LIMIT: usize = 16;
const GL_TEXTURE_BINDING_LIMIT: usize = 8192;
/// Build tag stamped into every captured diagnostic line so a live log unambiguously identifies
/// which DLL produced it. Bump this whenever the routing logic changes; if a supplied log does not
/// show the current tag, the player is running a stale DLL (a recurring source of confusion).
const VIDEO_GET_BUILD_TAG: &str = "video-logic-graph-2026-07-11a";

const MONITOR_GL_BIND_EVENT_LIMIT: usize = 64;
const RENDERER_VIDEO_PASS_EVENT_LIMIT: usize = 64;
const PENDING_MONITOR_RENDER_PROBE_LIMIT: usize = 32;
const PENDING_MONITOR_RENDER_PROBE_MAX_AGE_MS: u64 = 10_000;
const INPUT_VIDEO_SOURCE_LAYOUT_BYTES: usize = 0x80;
const VIDEO_NODE_RELATION_SCAN_BYTES: usize = 0x360;
const MONITOR_INPUT_REF_LAYOUT_BYTES: usize = 0x180;
const MONITOR_INPUT_REF_RELATION_SCAN_BYTES: usize = 0x80;
const MONITOR_INPUT_REF_NESTED_SCAN_BYTES: usize = 0x180;
const MONITOR_INPUT_REF_NESTED_POINTER_LIMIT: usize = 24;
#[cfg(windows)]
const STORMWORKS_IMAGE_BASE: u64 = 0x140000000;
#[cfg(windows)]
const STORMWORKS_WGL_GET_PROC_ADDRESS_IAT_VA: u64 = 0x140a92590;
#[cfg(windows)]
const STORMWORKS_GL_BIND_TEXTURE_IAT_VA: u64 = 0x140a92670;

#[repr(C)]
#[derive(Clone, Copy)]
struct GameLuaFunctionPair {
    name: *const c_char,
    function: Option<VideoGetLuaCFunction>,
}

unsafe impl Sync for GameLuaFunctionPair {}
unsafe impl Send for GameLuaFunctionPair {}

#[derive(Debug, Clone, Copy)]
struct GameLuaHelpers {
    create_table: GameLuaCreateTableFn,
    push_string: GameLuaPushStringFn,
    rawseti: GameLuaRawSetIFn,
    register_table: GameLuaRegisterTableFn,
    arg_slot: Option<GameLuaArgSlotFn>,
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct GlPboFunctions {
    gen_buffers: Option<GlGenBuffersFn>,
    bind_buffer: Option<GlBindBufferFn>,
    buffer_data: Option<GlBufferDataFn>,
    map_buffer_range: Option<GlMapBufferRangeFn>,
    unmap_buffer: Option<GlUnmapBufferFn>,
    fence_sync: Option<GlFenceSyncFn>,
    client_wait_sync: Option<GlClientWaitSyncFn>,
    delete_sync: Option<GlDeleteSyncFn>,
    gen_framebuffers: Option<GlGenFramebuffersFn>,
    bind_framebuffer: Option<GlBindFramebufferFn>,
    framebuffer_texture_2d: Option<GlFramebufferTexture2DFn>,
    check_framebuffer_status: Option<GlCheckFramebufferStatusFn>,
    delete_framebuffers: Option<GlDeleteFramebuffersFn>,
}

#[derive(Debug, Clone)]
struct RuntimeState {
    configured: bool,
    context: Option<PluginRuntimeContext>,
    config: VideoGetConfig,
    hook_runtime: HookRuntimeState,
    signatures_loaded: bool,
    signature_symbol_count: usize,
    signature_keys: Vec<String>,
    signature_symbols: serde_json::Value,
    signature_summary: serde_json::Value,
    byte_check_summary: ByteCheckSummary,
    hook_plan: Option<HookPlan>,
    hook_plan_path: Option<PathBuf>,
    slots: BTreeMap<SlotKey, SlotState>,
    latest_texture_upload_frame: Option<TextureUploadFrame>,
    gl_texture_bindings: BTreeMap<u64, GlTextureBinding>,
    video_node_sources: BTreeMap<u64, InputVideoNodeSourceHandles>,
    video_source_components: BTreeMap<u64, String>,
    monitor_pbo_readbacks: BTreeMap<u64, MonitorPboReadback>,
    monitor_gl_bind_events: Vec<MonitorGlBindEvent>,
    renderer_video_pass_events: Vec<RendererVideoPassEvent>,
    pending_monitor_render_probes: Vec<PendingMonitorRenderProbe>,
    last_error: Option<String>,
    log_path: Option<PathBuf>,
    load_event_path: Option<PathBuf>,
    runtime_snapshot_path: Option<PathBuf>,
    runtime_snapshot_jsonl_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct GlTextureBinding {
    handle: u32,
    owner_ptr: u64,
    texture_ptr: u64,
    width: u32,
    height: u32,
    last_seen: Instant,
}

#[derive(Debug, Clone)]
struct MonitorPboReadback {
    handle: u32,
    width: u32,
    height: u32,
    pbos: Vec<u32>,
    next_index: usize,
    pending: Vec<Option<MonitorPboPending>>,
}

#[derive(Debug, Clone)]
struct MonitorPboPending {
    candidate: MonitorRenderResourceCandidate,
    input_slot_handle: u64,
    input_handles: Vec<u64>,
    pbo: u32,
    width: u32,
    height: u32,
    byte_len: usize,
    sync: usize,
    submitted_at: Instant,
}

#[derive(Debug, Clone)]
struct MonitorPboReadyFrame {
    candidate: MonitorRenderResourceCandidate,
    input_slot_handle: u64,
    input_handles: Vec<u64>,
    width: u32,
    height: u32,
    rgb: Vec<[u8; 3]>,
}

#[derive(Debug, Clone)]
struct MonitorGlBindEvent {
    monitor: usize,
    render_context: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    arg6: u8,
    bind_index: u32,
    active_unit: u32,
    texture: u32,
    width: u32,
    height: u32,
    input_slot_object: u64,
    input_slot_ref: u64,
    input_effective_handle: u64,
    input_slot_relation: String,
    source_relation: String,
    slots: String,
    observed_at: Instant,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct MonitorRenderGlBindFrame {
    monitor: usize,
    render_context: usize,
    arg3: usize,
    arg4: usize,
    arg5: usize,
    arg6: u8,
    bind_index: u32,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
enum RenderQueueAllocKind {
    MonitorRender,
    SubmitCopy,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct RenderQueueAllocFrame {
    kind: RenderQueueAllocKind,
    owner: usize,
    source_item: usize,
    allocated_item: usize,
}

#[derive(Debug, Clone)]
struct RendererVideoPassEvent {
    renderer: usize,
    render_context: usize,
    scene_state: usize,
    command: usize,
    frame_a: usize,
    frame_b: usize,
    frame_c: usize,
    frame_a_texture: u32,
    frame_b_texture: u32,
    frame_c_texture: u32,
    render_target_primary: usize,
    render_target_secondary: usize,
    render_target_video: usize,
    queue_item: usize,
    queue_item_from: &'static str,
    queue_item_score: usize,
    queue_monitor: usize,
    queue_width: u32,
    queue_height: u32,
    queue_resource_a_ref: usize,
    queue_resource_b_ref: usize,
    queue_resource_a_value: usize,
    queue_resource_b_value: usize,
    queue_monitor_input_slot_object: u64,
    queue_monitor_input_slot_ref: u64,
    queue_monitor_effective_handle: u64,
    queue_monitor_input_relation: String,
    command_flags_0xc8: u32,
    command_flags_0xd8: u32,
    command_flags_0xdc: u32,
    object_relation: String,
    source_relation: String,
    slots: String,
    observed_at: Instant,
}

#[cfg(windows)]
#[derive(Debug, Clone)]
struct PendingMonitorRenderProbe {
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    input_handles: Vec<u64>,
    resource_a: usize,
    resource_b: usize,
    source: &'static str,
    observed_at: Instant,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct RendererVideoPassFrame {
    renderer: usize,
    render_context: usize,
    scene_state: usize,
    command: usize,
    frame_a: usize,
    frame_b: usize,
    frame_c: usize,
}

#[derive(Debug, Clone, Copy)]
struct RendererQueueItemProbe {
    base: usize,
    source: &'static str,
    score: usize,
    monitor: usize,
    width: u32,
    height: u32,
    resource_a_ref: usize,
    resource_b_ref: usize,
    resource_a_value: usize,
    resource_b_value: usize,
}

#[derive(Debug, Clone, Serialize)]
struct HookRuntimeState {
    install_attempted: bool,
    runtime_active: bool,
    detour_engine_ready: bool,
    installed_detour_count: usize,
    lua_registration_adapter: bool,
    lua_api_registered: bool,
    game_lua_callback_calls: u64,
    game_lua_last_callback: Option<String>,
    game_lua_last_component: Option<String>,
    mock_frame_pump_active: bool,
    real_lua_hook: bool,
    real_video_capture: bool,
    input_video_bridge_updates: u64,
    texture_source_bridge_frames: u64,
    texture_upload_bridge_frames: u64,
    texture_upload_skipped_bound_slots: u64,
    texture_upload_skipped_small_unbound_slots: u64,
    texture_upload_skipped_fps_slots: u64,
    texture_upload_auto_bound_slots: u64,
    monitor_render_attempts: u64,
    monitor_render_candidates: u64,
    monitor_render_blank_reads: u64,
    monitor_render_read_errors: u64,
    monitor_render_frames: u64,
    monitor_render_skipped_fps_slots: u64,
    additive_monitor_bind_attempts: u64,
    additive_monitor_bind_candidates: u64,
    additive_monitor_bind_blank_reads: u64,
    additive_monitor_bind_read_errors: u64,
    additive_monitor_bind_frames: u64,
    additive_monitor_bind_skipped_fps_slots: u64,
    source_texture_probe_attempts: u64,
    source_texture_probe_candidates: u64,
    source_texture_probe_read_errors: u64,
    source_texture_probe_blank_reads: u64,
    source_texture_probe_frames: u64,
    source_texture_probe_skipped_fps_slots: u64,
    installed_by_mode: Option<String>,
    last_install_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DetourStatus {
    engine_ready: bool,
    installed_count: usize,
    installed_labels: Vec<String>,
    trampoline_count: usize,
    trampoline_bytes_total: usize,
    last_error: Option<String>,
}

#[derive(Debug)]
struct DetourRegistry {
    installed: Vec<InstalledDetour>,
    last_error: Option<String>,
}

#[derive(Debug)]
struct InstalledDetour {
    label: String,
    target: *mut u8,
    original: Vec<u8>,
    trampoline: Option<AllocatedTrampoline>,
}

unsafe impl Send for InstalledDetour {}

#[derive(Debug, Clone)]
struct ReplacementResolution {
    name: String,
    address: Option<u64>,
    usable_for_patch: bool,
    note: String,
}

#[cfg(windows)]
#[derive(Debug)]
struct AllocatedTrampoline {
    ptr: *mut u8,
    len: usize,
}

#[cfg(windows)]
impl AllocatedTrampoline {
    fn as_ptr(&self) -> *const c_void {
        self.ptr as *const c_void
    }

    fn len(&self) -> usize {
        self.len
    }
}

#[cfg(windows)]
impl Drop for AllocatedTrampoline {
    fn drop(&mut self) {
        unsafe {
            let _ = VirtualFree(self.ptr as *mut c_void, 0, MEM_RELEASE);
        }
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
struct AllocatedTrampoline;

#[cfg(not(windows))]
impl AllocatedTrampoline {
    fn len(&self) -> usize {
        0
    }
}

unsafe impl Send for AllocatedTrampoline {}

#[derive(Debug, Clone)]
struct LuaAdapterState {
    api: Option<VideoGetLuaApiV1>,
    hook_api: Option<VideoGetLuaApiV1>,
    registrations: u64,
    hook_registrations: u64,
    hook_original_calls: u64,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct SlotKey {
    component: String,
    slot: u32,
}

#[derive(Debug, Clone, Serialize)]
struct SlotState {
    component: String,
    slot: u32,
    width: u32,
    height: u32,
    mode: String,
    frame_id: u64,
    ready: bool,
    connected: bool,
    input_source_handle: u64,
    input_candidate_source_handle: u64,
    input_selected_source_handle: u64,
    input_resolved_source_handle: u64,
    input_upstream_source_handle: u64,
    latest_frame: Option<FrameBuffer>,
    #[serde(skip_serializing)]
    texture_upload_handle: Option<u64>,
    #[serde(skip_serializing)]
    source_texture_handle: Option<u64>,
    #[serde(skip_serializing)]
    last_texture_upload_at: Option<Instant>,
}

#[derive(Debug, Clone, Serialize)]
struct FrameBuffer {
    frame_id: u64,
    width: u32,
    height: u32,
    source: String,
    rgb: Vec<[u8; 3]>,
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_abi_version() -> u32 {
    1
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_status() -> *mut c_char {
    let state = runtime_snapshot();
    json_result(Ok(serde_json::json!({
        "plugin": "video_get",
        "abi_version": 1,
        "backend": "synthetic",
        "configured": state.configured,
        "mode": state.context.as_ref().map(|context| context.mode.clone()),
        "game_build_label": state.context.as_ref().map(|context| context.game_build_label.clone()),
        "game_sha256": state.context.as_ref().map(|context| context.game_sha256.clone()),
        "config_limits": {
            "gray": {
                "max_width": state.config.limits.gray.max_width,
                "max_height": state.config.limits.gray.max_height
            },
            "rgb": {
                "max_width": state.config.limits.rgb.max_width,
                "max_height": state.config.limits.rgb.max_height
            },
            "max_active_slots": state.config.limits.max_active_slots
        },
        "signatures_loaded": state.signatures_loaded,
        "signature_symbol_count": state.signature_symbol_count,
        "signature_keys": state.signature_keys,
        "signature_summary": state.signature_summary,
        "observation_candidate_count": observation_candidate_count(&state.signature_symbols),
        "lua_dispatch": true,
        "lua_api_table": "video",
        "lua_dispatch_functions": lua_function_names(),
        "lua_registration_adapter": state.hook_runtime.lua_registration_adapter,
        "lua_api_registered": state.hook_runtime.lua_api_registered,
        "direct_hook_abi": true,
        "direct_hook_functions": direct_hook_function_names(),
        "hook_runtime": hook_runtime_status_value(&state.hook_runtime),
        "component_scoped_slots": true,
        "detours": detour_status_value(),
        "hook_plan_path": state.hook_plan_path.map(|path| path.display().to_string()),
        "hook_plan": state.hook_plan.as_ref().map(|plan| {
            let validation = validate_hook_plan(plan, &state.signature_symbols);
            hook_plan_summary_value(plan, &validation)
        }),
        "byte_checks": {
            "checked": state.byte_check_summary.checked,
            "verified": state.byte_check_summary.verified,
            "failed": state.byte_check_summary.failed,
            "failures": state.byte_check_summary.failures
        },
        "slots": state.slots.values().collect::<Vec<_>>(),
        "framed_slots": state.slots.values().filter(|slot| slot.latest_frame.is_some()).count(),
        "pushed_frame_slots": state.slots.values().filter(|slot| slot.latest_frame.as_ref().map(|frame| frame.source == "pushed_rgb").unwrap_or(false)).count(),
        "mock_frame_slots": state.slots.values().filter(|slot| slot.latest_frame.as_ref().map(|frame| frame.source == "mock_render").unwrap_or(false)).count(),
        "texture_source_frame_slots": state.slots.values().filter(|slot| slot.latest_frame.as_ref().map(|frame| frame.source == "texture_source").unwrap_or(false)).count(),
        "texture_upload_frame_slots": state.slots.values().filter(|slot| slot.latest_frame.as_ref().map(|frame| frame.source == "texture_upload").unwrap_or(false)).count(),
        "source_texture_frame_slots": state.slots.values().filter(|slot| slot.latest_frame.as_ref().map(|frame| frame.source == "source_texture").unwrap_or(false)).count(),
        "monitor_render_frame_slots": state.slots.values().filter(|slot| slot.latest_frame.as_ref().map(|frame| frame.source == "monitor_render").unwrap_or(false)).count(),
        "video_logic_edge_count": video_logic_edge_count(),
        "gl_texture_binding_count": state.gl_texture_bindings.len(),
        "monitor_pbo_readback_count": state.monitor_pbo_readbacks.len(),
        "gl_render_iat_hooks": gl_render_iat_status_value(),
        "gl_bind_texture_iat": gl_bind_texture_iat_status_value(),
        "log_path": state.log_path.map(|path| path.display().to_string()),
        "load_event_path": state.load_event_path.map(|path| path.display().to_string()),
        "runtime_snapshot_path": state.runtime_snapshot_path.map(|path| path.display().to_string()),
        "runtime_snapshot_jsonl_path": state.runtime_snapshot_jsonl_path.map(|path| path.display().to_string()),
        "last_error": state.last_error,
        "real_lua_hook": state.hook_runtime.real_lua_hook,
        "real_video_capture": state.hook_runtime.real_video_capture
    })))
}

fn hook_runtime_status_value(hook_runtime: &HookRuntimeState) -> serde_json::Value {
    serde_json::to_value(hook_runtime).unwrap_or_else(|error| {
        serde_json::json!({
            "error": format!("failed to serialize hook runtime: {error}")
        })
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_configure(context_json: *const c_char) -> *mut c_char {
    let result =
        configure_from_context_json(context_json).map(|summary| serde_json::json!(summary));
    json_result(result)
}

#[no_mangle]
pub extern "system" fn stormworks_video_get_configure_remote(context_path: *mut u16) -> u32 {
    match configure_from_context_path(context_path) {
        Ok(_) => 1,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_install_hooks() -> *mut c_char {
    json_result(install_hook_runtime(false))
}

#[no_mangle]
pub extern "system" fn stormworks_video_get_install_hooks_remote(_: *mut u16) -> u32 {
    match install_hook_runtime(true) {
        Ok(_) => 1,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn stormworks_video_get_bootstrap_replace_dll(context_path: *mut u16) -> u32 {
    match configure_from_context_path(context_path).and_then(|_| install_hook_runtime(false)) {
        Ok(_) => 1,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_hook_status() -> *mut c_char {
    json_result(Ok(hook_status_value()))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_hook_plan_dry_run() -> *mut c_char {
    json_result(Ok(hook_plan_dry_run_value()))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_detour_self_test() -> *mut c_char {
    json_result(run_detour_self_test())
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_unbound_review_stub() -> u32 {
    0
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_set_lua_hook_api(api: *const VideoGetLuaApiV1) -> i32 {
    match set_lua_hook_api(api) {
        Ok(()) => 1,
        Err(error) => {
            set_last_error(error);
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_enter_lua_component_context(
    component: *const c_char,
) -> u32 {
    match unsafe_cstr(component) {
        Some(component) => {
            let component = normalize_component(Some(&component));
            LUA_COMPONENT_CONTEXT.with(|stack| stack.borrow_mut().push(component));
            1
        }
        None => {
            set_last_error("missing component context".to_string());
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_leave_lua_component_context() -> u32 {
    LUA_COMPONENT_CONTEXT.with(|stack| {
        if stack.borrow_mut().pop().is_some() {
            1
        } else {
            set_last_error("component context stack is empty".to_string());
            0
        }
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_current_lua_component_context_write(
    out: *mut c_char,
    out_len: usize,
) -> usize {
    match current_lua_component_context() {
        Some(component) => write_c_string(&component, out, out_len).unwrap_or_else(|error| {
            set_last_error(error);
            0
        }),
        None => 0,
    }
}

fn run_hook_i32<F>(name: &'static str, action: F) -> i32
where
    F: FnOnce() -> Result<i32, String>,
{
    run_hook_i32_with_error(name, action, set_last_error)
}

fn run_hook_void<F>(name: &'static str, action: F)
where
    F: FnOnce() -> Result<(), String>,
{
    run_hook_void_with_error(name, action, set_last_error)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_register_lua_api_hook(lua_state: *mut c_void) -> i32 {
    run_hook_i32("register_lua_api_hook", || {
        register_lua_api_from_hook_chained(lua_state, LuaHookOriginalCall::Direct(lua_state))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_register_lua_api_hook_arg1(lua_state: *mut c_void) -> i32 {
    run_hook_i32("register_lua_api_hook_arg1", || {
        register_lua_api_from_hook_chained(lua_state, LuaHookOriginalCall::Arg1(lua_state))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_register_lua_api_hook_arg2(
    arg1: *mut c_void,
    lua_state: *mut c_void,
) -> i32 {
    run_hook_i32("register_lua_api_hook_arg2", || {
        register_lua_api_from_hook_chained(lua_state, LuaHookOriginalCall::Arg2(arg1, lua_state))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_register_lua_api_hook_arg3(
    arg1: *mut c_void,
    arg2: *mut c_void,
    lua_state: *mut c_void,
) -> i32 {
    run_hook_i32("register_lua_api_hook_arg3", || {
        register_lua_api_from_hook_chained(
            lua_state,
            LuaHookOriginalCall::Arg3(arg1, arg2, lua_state),
        )
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_register_lua_api_hook_arg4(
    arg1: *mut c_void,
    arg2: *mut c_void,
    arg3: *mut c_void,
    lua_state: *mut c_void,
) -> i32 {
    run_hook_i32("register_lua_api_hook_arg4", || {
        register_lua_api_from_hook_chained(
            lua_state,
            LuaHookOriginalCall::Arg4(arg1, arg2, arg3, lua_state),
        )
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_component_lua_init_hook_arg1(
    component_lua_init_context: *mut c_void,
) {
    run_hook_void("component_lua_init_hook_arg1", || {
        component_lua_init_from_hook_chained(component_lua_init_context)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_component_context_hook_arg1(component: *mut c_void) -> i32 {
    run_hook_i32("component_context_hook_arg1", || {
        component_context_from_hook_chained(ComponentContextHookOriginalCall::Arg1(component))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_component_context_hook_arg2(
    arg1: *mut c_void,
    component: *mut c_void,
) -> i32 {
    run_hook_i32("component_context_hook_arg2", || {
        component_context_from_hook_chained(ComponentContextHookOriginalCall::Arg2(arg1, component))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_component_context_hook_arg3(
    arg1: *mut c_void,
    arg2: *mut c_void,
    component: *mut c_void,
) -> i32 {
    run_hook_i32("component_context_hook_arg3", || {
        component_context_from_hook_chained(ComponentContextHookOriginalCall::Arg3(
            arg1, arg2, component,
        ))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_component_context_hook_arg4(
    arg1: *mut c_void,
    arg2: *mut c_void,
    arg3: *mut c_void,
    component: *mut c_void,
) -> i32 {
    run_hook_i32("component_context_hook_arg4", || {
        component_context_from_hook_chained(ComponentContextHookOriginalCall::Arg4(
            arg1, arg2, arg3, component,
        ))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_input_video_hook_arg1(source: *mut c_void) -> i32 {
    run_hook_i32("input_video_hook_arg1", || {
        input_video_from_hook_chained(InputVideoHookOriginalCall::Arg1(source))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_input_video_hook_arg2(
    arg1: *mut c_void,
    source: *mut c_void,
) -> i32 {
    run_hook_i32("input_video_hook_arg2", || {
        input_video_from_hook_chained(InputVideoHookOriginalCall::Arg2(arg1, source))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_input_video_hook_arg3(
    arg1: *mut c_void,
    arg2: *mut c_void,
    source: *mut c_void,
) -> i32 {
    run_hook_i32("input_video_hook_arg3", || {
        input_video_from_hook_chained(InputVideoHookOriginalCall::Arg3(arg1, arg2, source))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_input_video_hook_arg4(
    arg1: *mut c_void,
    arg2: *mut c_void,
    arg3: *mut c_void,
    source: *mut c_void,
) -> i32 {
    run_hook_i32("input_video_hook_arg4", || {
        input_video_from_hook_chained(InputVideoHookOriginalCall::Arg4(arg1, arg2, arg3, source))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_input_video_node_update_hook_arg2(
    node: *mut c_void,
    source: *mut c_void,
) {
    run_hook_void("input_video_node_update_hook_arg2", || {
        input_video_node_update_from_hook_chained(node, source)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_input_video_node_select_hook_arg5(
    node: *mut c_void,
    collection: *mut c_void,
    arg3: *mut c_void,
    arg4: *mut c_void,
    selected_index: *mut c_void,
) {
    run_hook_void("input_video_node_select_hook_arg5", || {
        input_video_node_select_from_hook_chained(node, collection, arg3, arg4, selected_index)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_video_output_slot_add_hook_arg2(
    output_slot: *mut c_void,
    input_slot: *mut c_void,
) {
    run_hook_void("video_output_slot_add_hook_arg2", || {
        video_output_slot_add_from_hook_chained(output_slot, input_slot)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_video_output_slot_remove_hook_arg2(
    output_slot: *mut c_void,
    input_slot: *mut c_void,
) {
    run_hook_void("video_output_slot_remove_hook_arg2", || {
        video_output_slot_remove_from_hook_chained(output_slot, input_slot)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_video_output_slot_clear_hook_arg1(output_slot: *mut c_void) {
    run_hook_void("video_output_slot_clear_hook_arg1", || {
        video_output_slot_clear_from_hook_chained(output_slot)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_texture_source_hook_arg1(source: *mut c_void) -> i32 {
    run_hook_i32("texture_source_hook_arg1", || {
        texture_source_from_hook_chained(TextureSourceHookOriginalCall::Arg1(source))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_texture_source_hook_arg2(
    arg1: *mut c_void,
    source: *mut c_void,
) -> i32 {
    run_hook_i32("texture_source_hook_arg2", || {
        texture_source_from_hook_chained(TextureSourceHookOriginalCall::Arg2(arg1, source))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_texture_source_hook_arg3(
    arg1: *mut c_void,
    arg2: *mut c_void,
    source: *mut c_void,
) -> i32 {
    run_hook_i32("texture_source_hook_arg3", || {
        texture_source_from_hook_chained(TextureSourceHookOriginalCall::Arg3(arg1, arg2, source))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_texture_source_hook_arg4(
    arg1: *mut c_void,
    arg2: *mut c_void,
    arg3: *mut c_void,
    source: *mut c_void,
) -> i32 {
    run_hook_i32("texture_source_hook_arg4", || {
        texture_source_from_hook_chained(TextureSourceHookOriginalCall::Arg4(
            arg1, arg2, arg3, source,
        ))
    })
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_texture_upload_hook_arg1(upload_context: *mut c_void) {
    run_hook_void("texture_upload_hook_arg1", || {
        texture_upload_from_hook_chained(upload_context)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_monitor_render_queue_hook_arg6(
    monitor: *mut c_void,
    render_context: *mut c_void,
    arg3: *mut c_void,
    arg4: *mut c_void,
    arg5: *mut c_void,
    arg6: u8,
) {
    run_hook_void("monitor_render_queue_hook_arg6", || {
        monitor_render_queue_from_hook_chained(monitor, render_context, arg3, arg4, arg5, arg6)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_render_queue_alloc_hook_arg1(
    queue: *mut c_void,
) -> *mut c_void {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_queue_alloc_from_hook_chained(queue)
    })) {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            set_last_error(error);
            ptr::null_mut()
        }
        Err(payload) => {
            set_last_error(format!(
                "hook panic: render_queue_alloc_hook_arg1: {}",
                panic_payload_message(payload.as_ref())
            ));
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_render_queue_submit_copy_hook_arg2(
    render_context: *mut c_void,
    source_item: *mut c_void,
) {
    run_hook_void("render_queue_submit_copy_hook_arg2", || {
        render_queue_submit_copy_from_hook_chained(render_context, source_item)
    });
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_render_target_texture_create_hook_arg3(
    texture_slot: *mut c_void,
    width: u32,
    height: u32,
) {
    run_hook_void("render_target_texture_create_hook_arg3", || {
        render_target_texture_create_from_hook_chained(texture_slot, width, height)
    });
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn stormworks_video_get_renderer_video_pass_hook_arg8(
    renderer: *mut c_void,
    render_context: *mut c_void,
    scene_state: *mut c_void,
    arg4: *mut c_void,
    command: *mut c_void,
    frame_a: *mut c_void,
    frame_b: *mut c_void,
    frame_c: *mut c_void,
) {
    run_hook_void("renderer_video_pass_hook_arg8", || {
        renderer_video_pass_from_hook_chained(
            renderer,
            render_context,
            scene_state,
            arg4,
            command,
            frame_a,
            frame_b,
            frame_c,
        )
    });
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn stormworks_video_get_additive_monitor_bind_hook(
    material: *mut c_void,
    draw_item: *mut c_void,
    texture_video: *mut c_void,
    texture_overlay: *mut c_void,
    arg5: *mut c_void,
    arg6: *mut c_void,
    arg7: *mut c_void,
    arg8: *mut c_void,
    arg9: u32,
    arg10: *mut c_void,
    arg11: u8,
    arg12: u32,
    arg13: *mut c_void,
    arg14: u32,
    arg15: u32,
    arg16: *mut c_void,
) {
    run_hook_void("additive_monitor_bind_hook", || {
        additive_monitor_bind_from_hook_chained(
            material,
            draw_item,
            texture_video,
            texture_overlay,
            arg5,
            arg6,
            arg7,
            arg8,
            arg9,
            arg10,
            arg11,
            arg12,
            arg13,
            arg14,
            arg15,
            arg16,
        )
    });
}

/// Replacement for `FUN_140688ec0`, the real `graphics/shaders/additive_monitor`
/// texture bind (register↔bind pair with `140688bf0`, called only from the
/// renderer video pass `1406d1960`). `140688e20` sets `texture_video` to sampler
/// unit 0; this function binds `glBindTexture(GL_TEXTURE_2D, *(u32*)(param_3 + 0x48))`
/// as that sampler. So `param_3` is the video texture wrapper and its GL id lives at
/// `+0x48`. This replaces the earlier `140677a10`/foam_particle attempt whose readback
/// path was hardcoded to sampler unit 3 and read unrelated asset textures.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn stormworks_video_get_additive_monitor_video_bind_hook_arg3(
    descriptor: *mut c_void,
    buffers: *mut c_void,
    video_texture_object: *mut c_void,
    overlay_texture_object: *mut c_void,
    arg5: *mut c_void,
    arg6: *mut c_void,
    arg7: *mut c_void,
    arg8: u64,
) {
    run_hook_void("additive_monitor_video_bind_hook", || {
        additive_monitor_video_bind_from_hook_chained(
            descriptor,
            buffers,
            video_texture_object,
            overlay_texture_object,
            arg5,
            arg6,
            arg7,
            arg8,
        )
    });
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn stormworks_video_get_gl_bind_texture_hook(target: u32, texture: u32) {
    call_original_gl_bind_texture(target, texture);
    if gl_bind_probe_context_active() {
        let _ = monitor_render_gl_texture_observation_after_original(
            "glBindTexture",
            None,
            target,
            texture,
        );
        let _ = additive_monitor_gl_bind_texture_after_original(target, texture);
    }
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn stormworks_video_get_wgl_get_proc_address_hook(
    name: *const c_char,
) -> *const c_void {
    let proc = call_original_wgl_get_proc_address(name);
    if name.is_null() {
        return proc;
    }
    let Ok(name_str) = (unsafe { CStr::from_ptr(name) }).to_str() else {
        return proc;
    };
    match name_str {
        "glBindTextureUnit" => {
            if valid_wgl_proc(proc) {
                store_original_dynamic_gl_proc(&GL_BIND_TEXTURE_UNIT_ORIGINAL, proc);
                stormworks_video_get_gl_bind_texture_unit_hook as *const c_void
            } else {
                proc
            }
        }
        "glBindTextures" => {
            if valid_wgl_proc(proc) {
                store_original_dynamic_gl_proc(&GL_BIND_TEXTURES_ORIGINAL, proc);
                stormworks_video_get_gl_bind_textures_hook as *const c_void
            } else {
                proc
            }
        }
        "glFramebufferTexture2D" => {
            if valid_wgl_proc(proc) {
                store_original_dynamic_gl_proc(&GL_FRAMEBUFFER_TEXTURE_2D_ORIGINAL, proc);
                stormworks_video_get_gl_framebuffer_texture_2d_hook as *const c_void
            } else {
                proc
            }
        }
        "glFramebufferTexture" | "glFramebufferTextureARB" => {
            if valid_wgl_proc(proc) {
                store_original_dynamic_gl_proc(&GL_FRAMEBUFFER_TEXTURE_ORIGINAL, proc);
                stormworks_video_get_gl_framebuffer_texture_hook as *const c_void
            } else {
                proc
            }
        }
        "glFramebufferTextureLayer"
        | "glFramebufferTextureLayerARB"
        | "glFramebufferTextureLayerEXT" => {
            if valid_wgl_proc(proc) {
                store_original_dynamic_gl_proc(&GL_FRAMEBUFFER_TEXTURE_LAYER_ORIGINAL, proc);
                stormworks_video_get_gl_framebuffer_texture_layer_hook as *const c_void
            } else {
                proc
            }
        }
        _ => proc,
    }
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn stormworks_video_get_gl_bind_texture_unit_hook(unit: u32, texture: u32) {
    call_original_gl_bind_texture_unit(unit, texture);
    if gl_bind_probe_context_active() {
        let _ = monitor_render_gl_texture_observation_after_original(
            "glBindTextureUnit",
            Some(unit),
            GL_TEXTURE_2D,
            texture,
        );
    }
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn stormworks_video_get_gl_bind_textures_hook(
    first: u32,
    count: i32,
    textures: *const u32,
) {
    call_original_gl_bind_textures(first, count, textures);
    if !gl_bind_probe_context_active() || textures.is_null() || count <= 0 {
        return;
    }
    let count = (count as usize).min(16);
    if !memory_range_is_readable(
        textures.cast::<c_void>(),
        count.saturating_mul(size_of::<u32>()),
    ) {
        return;
    }
    for index in 0..count {
        let texture = unsafe { *textures.add(index) };
        if texture == 0 {
            continue;
        }
        let unit = first.saturating_add(index as u32);
        let _ = monitor_render_gl_texture_observation_after_original(
            "glBindTextures",
            Some(unit),
            GL_TEXTURE_2D,
            texture,
        );
    }
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn stormworks_video_get_gl_framebuffer_texture_2d_hook(
    target: u32,
    attachment: u32,
    textarget: u32,
    texture: u32,
    level: i32,
) {
    call_original_gl_framebuffer_texture_2d(target, attachment, textarget, texture, level);
    if gl_bind_probe_context_active() {
        let _ = record_dynamic_gl_framebuffer_texture_observation(
            "glFramebufferTexture2D",
            target,
            attachment,
            textarget,
            texture,
            level,
            None,
        );
    }
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn stormworks_video_get_gl_framebuffer_texture_hook(
    target: u32,
    attachment: u32,
    texture: u32,
    level: i32,
) {
    call_original_gl_framebuffer_texture(target, attachment, texture, level);
    if gl_bind_probe_context_active() {
        let _ = record_dynamic_gl_framebuffer_texture_observation(
            "glFramebufferTexture",
            target,
            attachment,
            0,
            texture,
            level,
            None,
        );
    }
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn stormworks_video_get_gl_framebuffer_texture_layer_hook(
    target: u32,
    attachment: u32,
    texture: u32,
    level: i32,
    layer: i32,
) {
    call_original_gl_framebuffer_texture_layer(target, attachment, texture, level, layer);
    if gl_bind_probe_context_active() {
        let _ = record_dynamic_gl_framebuffer_texture_observation(
            "glFramebufferTextureLayer",
            target,
            attachment,
            0,
            texture,
            level,
            Some(layer),
        );
    }
}

#[cfg(test)]
extern "C" fn stormworks_video_get_test_noarg_detour_hook() -> i32 {
    42
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_register_lua_api(
    lua_state: *mut c_void,
    api: *const VideoGetLuaApiV1,
) -> i32 {
    match register_lua_api(lua_state, api) {
        Ok(count) => count,
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_observation_plan() -> *mut c_char {
    json_result(with_runtime(|state| {
        Ok(observation_plan_from_symbols(&state.signature_symbols))
    }))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_manifest() -> *mut c_char {
    json_result(Ok(lua_api_manifest()))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_call(call_json: *const c_char) -> *mut c_char {
    let result = parse_lua_call(call_json).and_then(dispatch_lua_call);
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_init_direct(
    component: *const c_char,
    slot: u32,
    width: u32,
    height: u32,
    mode: *const c_char,
) -> *mut c_char {
    let result = direct_lua_init(component, slot, width, height, mode);
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_is_connected_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let result = direct_lua_bool(component, slot, "isConnected");
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_is_ready_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let result = direct_lua_bool(component, slot, "isReady");
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_info_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let component = component_from_ptr(component);
    let result = frame_info_for_component_slot(&component, slot).map(|info| {
        lua_returns(
            &component,
            "getInfo",
            vec![
                info.get("frame_id")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                info.get("width")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                info.get("height")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                info.get("mode").cloned().unwrap_or(serde_json::Value::Null),
            ],
            Some(info),
        )
    });
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_get_size_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let component = component_from_ptr(component);
    let result = frame_size_for_component_slot(&component, slot).map(|(width, height)| {
        lua_returns(
            &component,
            "getSize",
            vec![serde_json::json!(width), serde_json::json!(height)],
            None,
        )
    });
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_get_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let component = component_from_ptr(component);
    let result = frame_for_component_slot_auto(&component, slot).map(|pixels| {
        lua_returns(
            &component,
            "get",
            vec![pixels, serde_json::Value::Null],
            None,
        )
    });
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_gray_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let result = direct_lua_matrix(component, slot, "gray", "getGray");
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_rgb_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let result = direct_lua_matrix(component, slot, "rgb", "getRGB");
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_packed_gray_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let result = direct_lua_packed(component, slot, "gray", "getPackedGray");
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_packed_rgb_direct(
    component: *const c_char,
    slot: u32,
) -> *mut c_char {
    let result = direct_lua_packed(component, slot, "rgb", "getPackedRGB");
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_push_rgb_slot_direct(
    component: *const c_char,
    slot: u32,
    width: u32,
    height: u32,
    rgb: *const u8,
    rgb_len: usize,
    connected: u32,
) -> *mut c_char {
    let result = push_rgb_frame_direct(component, slot, width, height, rgb, rgb_len, connected);
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_push_rgb_capture_request_direct(
    component_hash: u64,
    slot: u32,
    width: u32,
    height: u32,
    rgb: *const u8,
    rgb_len: usize,
    connected: u32,
) -> *mut c_char {
    let result = push_rgb_frame_for_capture_request(
        component_hash,
        slot,
        width,
        height,
        rgb,
        rgb_len,
        connected,
    );
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_bind_video_input_direct(
    component: *const c_char,
    slot: u32,
    connected: u32,
    input_source_handle: u64,
) -> *mut c_char {
    let result = bind_video_input_direct(component, slot, connected != 0, input_source_handle);
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_bind_video_input_capture_request_direct(
    component_hash: u64,
    slot: u32,
    connected: u32,
    input_source_handle: u64,
) -> *mut c_char {
    let result = bind_video_input_for_capture_request(
        component_hash,
        slot,
        connected != 0,
        input_source_handle,
    );
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_capture_request_count() -> u32 {
    runtime_snapshot().slots.len().min(u32::MAX as usize) as u32
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_capture_requests_write(
    out: *mut VideoGetCaptureRequestV1,
    max_count: usize,
) -> i32 {
    match write_capture_requests(out, max_count) {
        Ok(count) => count,
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_packed_gray_len(
    component: *const c_char,
    slot: u32,
) -> i32 {
    direct_packed_len(component, slot, "gray")
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_packed_rgb_len(
    component: *const c_char,
    slot: u32,
) -> i32 {
    direct_packed_len(component, slot, "rgb")
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_packed_gray_write(
    component: *const c_char,
    slot: u32,
    out: *mut u8,
    out_len: usize,
) -> i32 {
    direct_packed_write(component, slot, "gray", out, out_len)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_lua_packed_rgb_write(
    component: *const c_char,
    slot: u32,
    out: *mut u8,
    out_len: usize,
) -> i32 {
    direct_packed_write(component, slot, "rgb", out, out_len)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_init(config_json: *const c_char) -> *mut c_char {
    let result = parse_init(config_json).and_then(init_slot);
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_frame_info(slot: u32) -> *mut c_char {
    json_result(frame_info_for_slot(slot))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_gray(width: u32, height: u32) -> *mut c_char {
    let result = with_runtime(|state| {
        validate_frame_size(
            width,
            height,
            state.config.limits.gray.max_width,
            state.config.limits.gray.max_height,
            "gray",
        )?;
        let frame = synthetic_gray_matrix(width, height);
        FRAME_ID.fetch_add(1, Ordering::Relaxed);
        Ok(serde_json::to_value(frame).unwrap())
    });
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_rgb(width: u32, height: u32) -> *mut c_char {
    let result = with_runtime(|state| {
        validate_frame_size(
            width,
            height,
            state.config.limits.rgb.max_width,
            state.config.limits.rgb.max_height,
            "rgb",
        )?;
        let frame = synthetic_rgb_matrix(width, height);
        FRAME_ID.fetch_add(1, Ordering::Relaxed);
        Ok(serde_json::to_value(frame).unwrap())
    });
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_gray_slot(slot: u32) -> *mut c_char {
    json_result(frame_for_slot(slot, "gray"))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_rgb_slot(slot: u32) -> *mut c_char {
    json_result(frame_for_slot(slot, "rgb"))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_push_rgb_slot(frame_json: *const c_char) -> *mut c_char {
    let result = parse_frame_input(frame_json).and_then(push_rgb_frame);
    json_result(result)
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_is_connected(slot: u32) -> *mut c_char {
    json_result(with_slot(slot, |slot| {
        Ok(serde_json::json!(slot.connected))
    }))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_is_ready(slot: u32) -> *mut c_char {
    json_result(with_slot(slot, |slot| {
        Ok(serde_json::json!(is_slot_ready_for_lua(slot)))
    }))
}

#[no_mangle]
pub extern "C" fn stormworks_video_get_free_string(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(value);
    }
}

fn parse_init(config_json: *const c_char) -> Result<VideoInit, String> {
    let text = unsafe_cstr(config_json).ok_or_else(|| "missing config json".to_string())?;
    serde_json::from_str(&text).map_err(|error| format!("invalid config json: {error}"))
}

fn parse_frame_input(frame_json: *const c_char) -> Result<VideoFrameInput, String> {
    let text = unsafe_cstr(frame_json).ok_or_else(|| "missing frame json".to_string())?;
    serde_json::from_str(&text).map_err(|error| format!("invalid frame json: {error}"))
}

fn parse_lua_call(call_json: *const c_char) -> Result<LuaDispatchCall, String> {
    let text = unsafe_cstr(call_json).ok_or_else(|| "missing lua call json".to_string())?;
    serde_json::from_str(&text).map_err(|error| format!("invalid lua call json: {error}"))
}

fn init_slot(init: VideoInit) -> Result<serde_json::Value, String> {
    let mut state = request_runtime_state()?;
    validate_init(&init, &state)?;
    MONITOR_RENDER_HEAVY_PROBE_LAST_MS.store(0, Ordering::Relaxed);
    MONITOR_RENDER_HEAVY_PROBE_ATTEMPTS.store(0, Ordering::Relaxed);
    let component = normalize_component(init.component.as_deref());
    let key = slot_key(&component, init.slot);
    let initial_input_source_handles =
        lua_script_input_video_source_handles_for_component(&component).unwrap_or_default();
    let initial_input_source_handle = initial_input_source_handles.effective();
    let mut slot = SlotState {
        component: component.clone(),
        slot: init.slot,
        width: init.width,
        height: init.height,
        mode: init.mode.clone(),
        frame_id: FRAME_ID.load(Ordering::Relaxed),
        ready: false,
        connected: false,
        input_source_handle: 0,
        input_candidate_source_handle: 0,
        input_selected_source_handle: 0,
        input_resolved_source_handle: 0,
        input_upstream_source_handle: 0,
        latest_frame: None,
        texture_upload_handle: None,
        source_texture_handle: None,
        last_texture_upload_at: None,
    };
    if initial_input_source_handle != 0 {
        slot.frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
        slot.connected = true;
        slot.input_source_handle = initial_input_source_handle;
        slot.input_candidate_source_handle = initial_input_source_handles.candidate;
        slot.input_selected_source_handle = initial_input_source_handles.selected;
        slot.input_resolved_source_handle = initial_input_source_handles.resolved;
        slot.input_upstream_source_handle = initial_input_source_handles.upstream;
        state.hook_runtime.input_video_bridge_updates = state
            .hook_runtime
            .input_video_bridge_updates
            .saturating_add(1);
    }
    state.slots.insert(key, slot.clone());
    // Vehicle-reset / lifecycle hygiene: drop slots that belong to Lua component contexts
    // which are no longer registered (their vehicle/microcontroller was despawned or
    // reloaded). This keeps `video.init` from a freshly spawned vehicle from inheriting a
    // dead component's stale slot state, and bounds slot growth across reloads. The current
    // component's just-inserted slot is always kept.
    prune_dead_component_video_slots(&mut state, &component);
    let _ = apply_latest_texture_upload_frame_to_slot(&mut state, &component, init.slot);
    let slot = state
        .slots
        .get(&slot_key(&component, init.slot))
        .cloned()
        .unwrap_or(slot);
    log_runtime_diagnostic(
        &state,
        &format!(
            "video.init component={} slot={} request={}x{} mode={} initial_source={} candidate_source={} selected_source={} resolved_source={} upstream_source={} connected={} ready={} input_layout={} active_slots={} slots={}",
            component,
            slot.slot,
            slot.width,
            slot.height,
            slot.mode,
            format_hex_or_zero(slot.input_source_handle),
            format_hex_or_zero(slot.input_candidate_source_handle),
            format_hex_or_zero(slot.input_selected_source_handle),
            format_hex_or_zero(slot.input_resolved_source_handle),
            format_hex_or_zero(initial_input_source_handles.upstream),
            slot.connected,
            is_slot_ready_for_lua(&slot),
            lua_script_input_video_node_from_component(&component)
                .map(|node| format_input_video_source_layouts(node, initial_input_source_handles))
                .unwrap_or_else(|| "none".to_string()),
            state.slots.len(),
            describe_slots(&state)
        ),
        &VIDEO_INIT_DIAGNOSTIC_COUNT,
        16,
    );
    log_video_node_registry_diagnostic("video.init");
    set_runtime(state);
    Ok(serde_json::json!({
        "ok": true,
        "component": slot.component,
        "slot": slot.slot,
        "width": slot.width,
        "height": slot.height,
        "mode": slot.mode,
        "ready": is_slot_ready_for_lua(&slot),
        "connected": slot.connected,
        "input_source_handle": slot.input_source_handle,
        "input_candidate_source_handle": slot.input_candidate_source_handle,
        "input_selected_source_handle": slot.input_selected_source_handle,
        "input_resolved_source_handle": slot.input_resolved_source_handle,
        "input_upstream_source_handle": slot.input_upstream_source_handle,
        "frame_id": slot.frame_id
    }))
}

fn lua_api_manifest() -> serde_json::Value {
    serde_json::json!({
        "api_table": "video",
        "status": "native_dispatch_only",
        "real_lua_hook": false,
        "component_scoped": true,
        "description": "Native dispatch contract for the future Stormworks component Lua video table.",
        "direct_hook_abi": {
            "purpose": "Low-allocation entry points for the future injected Lua adapter and mock renderer.",
            "frame_ingest": "stormworks_video_get_push_rgb_slot_direct(component, slot, width, height, rgb_ptr, rgb_len, connected)",
            "capture_request_frame_ingest": "stormworks_video_get_push_rgb_capture_request_direct(component_hash, slot, width, height, rgb_ptr, rgb_len, connected)",
            "video_input_binding": "stormworks_video_get_bind_video_input_direct(component, slot, connected, input_source_handle)",
            "capture_request_video_input_binding": "stormworks_video_get_bind_video_input_capture_request_direct(component_hash, slot, connected, input_source_handle)",
            "capture_requests": [
                "stormworks_video_get_capture_request_count()",
                "stormworks_video_get_capture_requests_write(out_ptr, max_count)"
            ],
            "component_context": [
                "stormworks_video_get_enter_lua_component_context(component)",
                "stormworks_video_get_leave_lua_component_context()",
                "stormworks_video_get_current_lua_component_context_write(out_ptr, out_len)",
                "stormworks_video_get_component_context_hook_arg1(component)",
                "stormworks_video_get_component_context_hook_arg2(arg1, component)",
                "stormworks_video_get_component_context_hook_arg3(arg1, arg2, component)",
                "stormworks_video_get_component_context_hook_arg4(arg1, arg2, arg3, component)"
            ],
            "input_video": [
                "stormworks_video_get_input_video_hook_arg1(source)",
                "stormworks_video_get_input_video_hook_arg2(arg1, source)",
                "stormworks_video_get_input_video_hook_arg3(arg1, arg2, source)",
                "stormworks_video_get_input_video_hook_arg4(arg1, arg2, arg3, source)",
                "stormworks_video_get_input_video_node_update_hook_arg2(node, source_candidate)"
            ],
            "texture_source": [
                "stormworks_video_get_texture_source_hook_arg1(source)",
                "stormworks_video_get_texture_source_hook_arg2(arg1, source)",
                "stormworks_video_get_texture_source_hook_arg3(arg1, arg2, source)",
                "stormworks_video_get_texture_source_hook_arg4(arg1, arg2, arg3, source)"
            ],
            "packed_write": [
                "stormworks_video_get_lua_packed_gray_len(component, slot)",
                "stormworks_video_get_lua_packed_gray_write(component, slot, out_ptr, out_len)",
                "stormworks_video_get_lua_packed_rgb_len(component, slot)",
                "stormworks_video_get_lua_packed_rgb_write(component, slot, out_ptr, out_len)"
            ],
            "matrix_wrappers": [
                "stormworks_video_get_lua_get_direct(component, slot)",
                "stormworks_video_get_lua_gray_direct(component, slot)",
                "stormworks_video_get_lua_rgb_direct(component, slot)"
            ]
        },
        "functions": [
            {
                "name": "init",
                "lua": "ok, err = video.init(width, height, mode) or video.init(slot, width, height, mode)",
                "dispatch": {"function": "init", "args": ["[slot]", "width", "height", "[gray|rgb]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["ok", "err"]
            },
            {
                "name": "isConnected",
                "lua": "connected = video.isConnected(slot)",
                "dispatch": {"function": "isConnected", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["connected"]
            },
            {
                "name": "isReady",
                "lua": "ready = video.isReady(slot)",
                "dispatch": {"function": "isReady", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["ready"]
            },
            {
                "name": "getInfo",
                "lua": "frame_id, width, height, mode = video.getInfo(slot)",
                "dispatch": {"function": "getInfo", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["frame_id", "width", "height", "mode"]
            },
            {
                "name": "getSize",
                "lua": "width, height = video.getSize(slot)",
                "dispatch": {"function": "getSize", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["width", "height"]
            },
            {
                "name": "get",
                "lua": "pixels, err = video.get(slot)",
                "dispatch": {"function": "get", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["pixels", "err"],
                "mode": "uses the mode selected by video.init"
            },
            {
                "name": "getGray",
                "lua": "pixels, err = video.getGray(slot)",
                "dispatch": {"function": "getGray", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["pixels", "err"]
            },
            {
                "name": "getRGB",
                "lua": "pixels, err = video.getRGB(slot)",
                "dispatch": {"function": "getRGB", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["pixels", "err"]
            },
            {
                "name": "getPackedGray",
                "lua": "buffer, err = video.getPackedGray(slot)",
                "dispatch": {"function": "getPackedGray", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["buffer", "err"],
                "buffer": {"format": "u8-gray", "stride": 1, "order": "row-major, top-to-bottom, left-to-right"}
            },
            {
                "name": "getPackedRGB",
                "lua": "buffer, err = video.getPackedRGB(slot)",
                "dispatch": {"function": "getPackedRGB", "args": ["[slot]"]},
                "default_slot": DEFAULT_VIDEO_SLOT,
                "returns": ["buffer", "err"],
                "buffer": {"format": "u8-rgb", "stride": 3, "order": "row-major, top-to-bottom, left-to-right, RGB byte triples"}
            }
        ],
        "policy": {
            "starts_game": false,
            "attaches_to_process": false,
            "writes_target_memory": false,
            "registers_lua_table": false
        }
    })
}

fn lua_function_names() -> Vec<&'static str> {
    vec![
        "init",
        "isConnected",
        "isReady",
        "getInfo",
        "getSize",
        "get",
        "getGray",
        "getRGB",
        "getPackedGray",
        "getPackedRGB",
    ]
}

fn direct_hook_function_names() -> Vec<&'static str> {
    vec![
        "stormworks_video_get_lua_init_direct",
        "stormworks_video_get_lua_is_connected_direct",
        "stormworks_video_get_lua_is_ready_direct",
        "stormworks_video_get_lua_info_direct",
        "stormworks_video_get_lua_get_size_direct",
        "stormworks_video_get_lua_get_direct",
        "stormworks_video_get_lua_gray_direct",
        "stormworks_video_get_lua_rgb_direct",
        "stormworks_video_get_lua_packed_gray_direct",
        "stormworks_video_get_lua_packed_rgb_direct",
        "stormworks_video_get_push_rgb_slot_direct",
        "stormworks_video_get_push_rgb_capture_request_direct",
        "stormworks_video_get_bind_video_input_direct",
        "stormworks_video_get_bind_video_input_capture_request_direct",
        "stormworks_video_get_capture_request_count",
        "stormworks_video_get_capture_requests_write",
        "stormworks_video_get_enter_lua_component_context",
        "stormworks_video_get_leave_lua_component_context",
        "stormworks_video_get_current_lua_component_context_write",
        "stormworks_video_get_component_context_hook_arg1",
        "stormworks_video_get_component_context_hook_arg2",
        "stormworks_video_get_component_context_hook_arg3",
        "stormworks_video_get_component_context_hook_arg4",
        "stormworks_video_get_input_video_hook_arg1",
        "stormworks_video_get_input_video_hook_arg2",
        "stormworks_video_get_input_video_hook_arg3",
        "stormworks_video_get_input_video_hook_arg4",
        "stormworks_video_get_input_video_node_update_hook_arg2",
        "stormworks_video_get_input_video_node_select_hook_arg5",
        "stormworks_video_get_video_output_slot_add_hook_arg2",
        "stormworks_video_get_video_output_slot_remove_hook_arg2",
        "stormworks_video_get_video_output_slot_clear_hook_arg1",
        "stormworks_video_get_texture_source_hook_arg1",
        "stormworks_video_get_texture_source_hook_arg2",
        "stormworks_video_get_texture_source_hook_arg3",
        "stormworks_video_get_texture_source_hook_arg4",
        "stormworks_video_get_texture_upload_hook_arg1",
        "stormworks_video_get_monitor_render_queue_hook_arg6",
        "stormworks_video_get_render_queue_alloc_hook_arg1",
        "stormworks_video_get_render_queue_submit_copy_hook_arg2",
        "stormworks_video_get_render_target_texture_create_hook_arg3",
        "stormworks_video_get_renderer_video_pass_hook_arg8",
        "stormworks_video_get_additive_monitor_bind_hook",
        "stormworks_video_get_additive_monitor_video_bind_hook_arg3",
        "stormworks_video_get_lua_packed_gray_len",
        "stormworks_video_get_lua_packed_gray_write",
        "stormworks_video_get_lua_packed_rgb_len",
        "stormworks_video_get_lua_packed_rgb_write",
    ]
}

fn dispatch_lua_call(call: LuaDispatchCall) -> Result<serde_json::Value, String> {
    let function = normalize_lua_function(&call.function);
    let component = normalize_component(call.component.as_deref());
    match function.as_str() {
        "init" => {
            let (slot, width, height, mode) = lua_init_args(&call.args)?;
            let init = VideoInit {
                slot,
                width,
                height,
                mode,
                component: Some(component.clone()),
            };
            let native = init_slot(init)?;
            Ok(lua_returns(
                &component,
                &function,
                vec![serde_json::Value::Bool(true), serde_json::Value::Null],
                Some(native),
            ))
        }
        "isConnected" => {
            let slot = lua_slot_arg(&call.args)?;
            let connected = require_slot_for_component(&component, slot)?.connected;
            Ok(lua_returns(
                &component,
                &function,
                vec![serde_json::Value::Bool(connected)],
                None,
            ))
        }
        "isReady" => {
            let slot = lua_slot_arg(&call.args)?;
            let ready = is_slot_ready_for_lua(&require_slot_for_component(&component, slot)?);
            Ok(lua_returns(
                &component,
                &function,
                vec![serde_json::Value::Bool(ready)],
                None,
            ))
        }
        "getInfo" => {
            let slot = lua_slot_arg(&call.args)?;
            let info = frame_info_for_component_slot(&component, slot)?;
            Ok(lua_returns(
                &component,
                &function,
                vec![
                    info.get("frame_id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    info.get("width")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    info.get("height")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                    info.get("mode").cloned().unwrap_or(serde_json::Value::Null),
                ],
                Some(info),
            ))
        }
        "getSize" => {
            let slot = lua_slot_arg(&call.args)?;
            let (width, height) = frame_size_for_component_slot(&component, slot)?;
            Ok(lua_returns(
                &component,
                &function,
                vec![serde_json::json!(width), serde_json::json!(height)],
                None,
            ))
        }
        "get" => {
            let slot = lua_slot_arg(&call.args)?;
            let pixels = frame_for_component_slot_auto(&component, slot)?;
            Ok(lua_returns(
                &component,
                &function,
                vec![pixels, serde_json::Value::Null],
                None,
            ))
        }
        "getGray" => {
            let slot = lua_slot_arg(&call.args)?;
            let pixels = frame_for_component_slot(&component, slot, "gray")?;
            Ok(lua_returns(
                &component,
                &function,
                vec![pixels, serde_json::Value::Null],
                None,
            ))
        }
        "getRGB" => {
            let slot = lua_slot_arg(&call.args)?;
            let pixels = frame_for_component_slot(&component, slot, "rgb")?;
            Ok(lua_returns(
                &component,
                &function,
                vec![pixels, serde_json::Value::Null],
                None,
            ))
        }
        "getPackedGray" => {
            let slot = lua_slot_arg(&call.args)?;
            let buffer = packed_frame_for_component_slot(&component, slot, "gray")?;
            Ok(lua_returns(
                &component,
                &function,
                vec![buffer, serde_json::Value::Null],
                None,
            ))
        }
        "getPackedRGB" => {
            let slot = lua_slot_arg(&call.args)?;
            let buffer = packed_frame_for_component_slot(&component, slot, "rgb")?;
            Ok(lua_returns(
                &component,
                &function,
                vec![buffer, serde_json::Value::Null],
                None,
            ))
        }
        _ => Err(format!("unknown video lua function `{}`", call.function)),
    }
}

fn normalize_lua_function(function: &str) -> String {
    function
        .trim()
        .strip_prefix("video.")
        .unwrap_or_else(|| function.trim())
        .to_string()
}

fn lua_returns(
    component: &str,
    function: &str,
    returns: Vec<serde_json::Value>,
    native: Option<serde_json::Value>,
) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert("table".to_string(), serde_json::json!("video"));
    object.insert("component".to_string(), serde_json::json!(component));
    object.insert("function".to_string(), serde_json::json!(function));
    object.insert("returns".to_string(), serde_json::Value::Array(returns));
    if let Some(native) = native {
        object.insert("native".to_string(), native);
    }
    serde_json::Value::Object(object)
}

fn lua_arg_u32(args: &[serde_json::Value], index: usize, name: &str) -> Result<u32, String> {
    let value = args
        .get(index)
        .ok_or_else(|| format!("missing lua argument {name}"))?;
    let number = value
        .as_u64()
        .ok_or_else(|| format!("lua argument {name} must be an integer"))?;
    u32::try_from(number).map_err(|_| format!("lua argument {name} is out of range"))
}

fn lua_arg_string(args: &[serde_json::Value], index: usize, name: &str) -> Result<String, String> {
    args.get(index)
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("lua argument {name} must be a string"))
}

fn lua_slot_arg(args: &[serde_json::Value]) -> Result<u32, String> {
    match args.first() {
        Some(_) => lua_arg_u32(args, 0, "slot"),
        None => Ok(DEFAULT_VIDEO_SLOT),
    }
}

fn lua_arg_string_or_default(
    args: &[serde_json::Value],
    index: usize,
    default: &str,
) -> Result<String, String> {
    match args.get(index) {
        Some(value) => value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("lua argument mode must be a string")),
        None => Ok(default.to_string()),
    }
}

fn lua_init_args(args: &[serde_json::Value]) -> Result<(u32, u32, u32, String), String> {
    match args.len() {
        0 | 1 => Err("video.init requires width and height".to_string()),
        2 => Ok((
            DEFAULT_VIDEO_SLOT,
            lua_arg_u32(args, 0, "width")?,
            lua_arg_u32(args, 1, "height")?,
            "rgb".to_string(),
        )),
        3 if args.get(2).and_then(|value| value.as_str()).is_some() => Ok((
            DEFAULT_VIDEO_SLOT,
            lua_arg_u32(args, 0, "width")?,
            lua_arg_u32(args, 1, "height")?,
            lua_arg_string(args, 2, "mode")?,
        )),
        3 => Ok((
            lua_arg_u32(args, 0, "slot")?,
            lua_arg_u32(args, 1, "width")?,
            lua_arg_u32(args, 2, "height")?,
            "rgb".to_string(),
        )),
        _ => Ok((
            lua_arg_u32(args, 0, "slot")?,
            lua_arg_u32(args, 1, "width")?,
            lua_arg_u32(args, 2, "height")?,
            lua_arg_string_or_default(args, 3, "rgb")?,
        )),
    }
}

fn direct_lua_init(
    component: *const c_char,
    slot: u32,
    width: u32,
    height: u32,
    mode: *const c_char,
) -> Result<serde_json::Value, String> {
    let component = component_from_ptr(component);
    let mode = unsafe_cstr(mode).ok_or_else(|| "missing mode".to_string())?;
    let native = init_slot(VideoInit {
        slot,
        width,
        height,
        mode,
        component: Some(component.clone()),
    })?;
    Ok(lua_returns(
        &component,
        "init",
        vec![serde_json::Value::Bool(true), serde_json::Value::Null],
        Some(native),
    ))
}

fn direct_lua_bool(
    component: *const c_char,
    slot: u32,
    function: &str,
) -> Result<serde_json::Value, String> {
    let component = component_from_ptr(component);
    let slot = require_slot_for_component(&component, slot)?;
    let value = match function {
        "isConnected" => slot.connected,
        "isReady" => is_slot_ready_for_lua(&slot),
        _ => return Err("invalid direct bool function".to_string()),
    };
    Ok(lua_returns(
        &component,
        function,
        vec![serde_json::Value::Bool(value)],
        None,
    ))
}

fn direct_lua_matrix(
    component: *const c_char,
    slot: u32,
    requested_mode: &str,
    function: &str,
) -> Result<serde_json::Value, String> {
    let component = component_from_ptr(component);
    let pixels = frame_for_component_slot(&component, slot, requested_mode)?;
    Ok(lua_returns(
        &component,
        function,
        vec![pixels, serde_json::Value::Null],
        None,
    ))
}

fn direct_lua_packed(
    component: *const c_char,
    slot: u32,
    requested_mode: &str,
    function: &str,
) -> Result<serde_json::Value, String> {
    let component = component_from_ptr(component);
    let buffer = packed_frame_for_component_slot(&component, slot, requested_mode)?;
    Ok(lua_returns(
        &component,
        function,
        vec![buffer, serde_json::Value::Null],
        None,
    ))
}

fn push_rgb_frame_direct(
    component: *const c_char,
    slot: u32,
    width: u32,
    height: u32,
    rgb: *const u8,
    rgb_len: usize,
    connected: u32,
) -> Result<serde_json::Value, String> {
    push_rgb_frame_direct_with_source(
        component,
        slot,
        width,
        height,
        rgb,
        rgb_len,
        connected,
        "pushed_rgb",
    )
}

fn push_rgb_frame_direct_with_source(
    component: *const c_char,
    slot: u32,
    width: u32,
    height: u32,
    rgb: *const u8,
    rgb_len: usize,
    connected: u32,
    source: &str,
) -> Result<serde_json::Value, String> {
    if rgb.is_null() {
        return Err("missing rgb buffer".to_string());
    }
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| "frame byte size overflow".to_string())? as usize;
    if rgb_len != expected_len {
        return Err(format!(
            "rgb byte length {rgb_len} does not match {width}x{height}x3"
        ));
    }
    let bytes = unsafe { std::slice::from_raw_parts(rgb, rgb_len) };
    let rgb = bytes
        .chunks_exact(3)
        .map(|chunk| [chunk[0], chunk[1], chunk[2]])
        .collect::<Vec<_>>();
    push_rgb_frame_with_source(
        VideoFrameInput {
            slot,
            width,
            height,
            rgb,
            connected: Some(connected != 0),
            component: Some(component_from_ptr(component)),
        },
        source,
    )
}

fn push_rgb_frame_for_capture_request(
    component_hash: u64,
    slot: u32,
    width: u32,
    height: u32,
    rgb: *const u8,
    rgb_len: usize,
    connected: u32,
) -> Result<serde_json::Value, String> {
    push_rgb_frame_for_capture_request_with_source(
        component_hash,
        slot,
        width,
        height,
        rgb,
        rgb_len,
        connected,
        "pushed_rgb",
    )
}

fn push_rgb_frame_for_capture_request_with_source(
    component_hash: u64,
    slot: u32,
    width: u32,
    height: u32,
    rgb: *const u8,
    rgb_len: usize,
    connected: u32,
    source: &str,
) -> Result<serde_json::Value, String> {
    let component = component_for_capture_request(component_hash, slot)?;
    push_rgb_frame_direct_with_source(
        component.as_ptr(),
        slot,
        width,
        height,
        rgb,
        rgb_len,
        connected,
        source,
    )
}

fn component_for_capture_request(component_hash: u64, slot: u32) -> Result<CString, String> {
    if slot == 0 {
        return Err("invalid slot".to_string());
    }
    let state = request_runtime_state()?;
    let matches = state
        .slots
        .values()
        .filter(|candidate| {
            candidate.slot == slot && stable_component_hash(&candidate.component) == component_hash
        })
        .map(|candidate| candidate.component.clone())
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!(
            "capture request not initialized for component_hash=0x{component_hash:016x} slot={slot}"
        )),
        1 => CString::new(matches[0].clone()).map_err(|error| {
            format!(
                "component for component_hash=0x{component_hash:016x} contains nul byte: {error}"
            )
        }),
        _ => Err(format!(
            "component_hash collision for component_hash=0x{component_hash:016x} slot={slot}"
        )),
    }
}

fn bind_video_input_direct(
    component: *const c_char,
    slot: u32,
    connected: bool,
    input_source_handle: u64,
) -> Result<serde_json::Value, String> {
    if slot == 0 {
        return Err("invalid slot".to_string());
    }
    let component = component_from_ptr(component);
    let mut state = request_runtime_state()?;
    {
        let slot_state = state
            .slots
            .get_mut(&slot_key(&component, slot))
            .ok_or_else(|| "not initialized".to_string())?;
        let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
        slot_state.frame_id = frame_id;
        slot_state.connected = connected;
        slot_state.input_source_handle = if connected { input_source_handle } else { 0 };
        slot_state.input_candidate_source_handle = if connected { input_source_handle } else { 0 };
        slot_state.input_selected_source_handle = if connected { input_source_handle } else { 0 };
        slot_state.input_resolved_source_handle = if connected { input_source_handle } else { 0 };
        slot_state.input_upstream_source_handle = 0;
    }
    if connected {
        let _ = apply_latest_texture_upload_frame_to_slot(&mut state, &component, slot);
    }
    let slot_state = state
        .slots
        .get(&slot_key(&component, slot))
        .ok_or_else(|| "not initialized".to_string())?;
    log_runtime_diagnostic(
        &state,
        &format!(
            "video input direct bind component={} slot={} connected={} source={} candidate_source={} upstream_source={} ready={} slots={}",
            component,
            slot_state.slot,
            slot_state.connected,
            format_hex_or_zero(slot_state.input_source_handle),
            format_hex_or_zero(slot_state.input_candidate_source_handle),
            format_hex_or_zero(slot_state.input_upstream_source_handle),
            is_slot_ready_for_lua(slot_state),
            describe_slots(&state)
        ),
        &VIDEO_INIT_DIAGNOSTIC_COUNT,
        16,
    );
    let response = serde_json::json!({
        "component": slot_state.component,
        "slot": slot_state.slot,
        "frame_id": slot_state.frame_id,
        "width": slot_state.width,
        "height": slot_state.height,
        "mode": slot_state.mode,
        "ready": is_slot_ready_for_lua(slot_state),
        "connected": slot_state.connected,
        "input_source_handle": slot_state.input_source_handle,
        "input_candidate_source_handle": slot_state.input_candidate_source_handle,
        "input_selected_source_handle": slot_state.input_selected_source_handle,
        "input_resolved_source_handle": slot_state.input_resolved_source_handle,
        "input_upstream_source_handle": slot_state.input_upstream_source_handle
    });
    set_runtime(state);
    Ok(response)
}

fn bind_video_input_for_capture_request(
    component_hash: u64,
    slot: u32,
    connected: bool,
    input_source_handle: u64,
) -> Result<serde_json::Value, String> {
    let component = component_for_capture_request(component_hash, slot)?;
    bind_video_input_direct(component.as_ptr(), slot, connected, input_source_handle)
}

fn write_capture_requests(
    out: *mut VideoGetCaptureRequestV1,
    max_count: usize,
) -> Result<i32, String> {
    if out.is_null() && max_count > 0 {
        return Err("missing capture request output buffer".to_string());
    }
    let state = runtime_snapshot();
    let requests = state
        .slots
        .values()
        .map(capture_request_from_slot)
        .collect::<Vec<_>>();
    let count = requests.len().min(max_count);
    for (index, request) in requests.iter().take(count).enumerate() {
        unsafe {
            *out.add(index) = *request;
        }
    }
    i32::try_from(count).map_err(|_| "capture request count exceeds i32".to_string())
}

fn capture_request_from_slot(slot: &SlotState) -> VideoGetCaptureRequestV1 {
    VideoGetCaptureRequestV1 {
        size: size_of::<VideoGetCaptureRequestV1>() as u32,
        component_hash: stable_component_hash(&slot.component),
        slot: slot.slot,
        width: slot.width,
        height: slot.height,
        mode: capture_mode_code(&slot.mode),
        ready: if is_slot_ready_for_lua(slot) { 1 } else { 0 },
        connected: if slot.connected { 1 } else { 0 },
        frame_id: slot.frame_id,
        source: slot
            .latest_frame
            .as_ref()
            .filter(|frame| frame_source_is_enabled_for_lua(frame.source.as_str()))
            .map(|frame| capture_source_code(&frame.source))
            .unwrap_or(0),
        input_source_handle: slot.input_source_handle,
    }
}

fn stable_component_hash(component: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in component.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn capture_mode_code(mode: &str) -> u32 {
    match mode {
        "gray" => 1,
        "rgb" => 2,
        _ => 0,
    }
}

fn capture_source_code(source: &str) -> u32 {
    match source {
        "mock_render" => 1,
        "pushed_rgb" => 2,
        "texture_source" => 3,
        "texture_upload" => 4,
        "source_texture" => 5,
        "monitor_render" => 6,
        _ => 0,
    }
}

fn direct_packed_len(component: *const c_char, slot: u32, requested_mode: &str) -> i32 {
    match packed_frame_data_for_component_slot(&component_from_ptr(component), slot, requested_mode)
    {
        Ok(frame) => match i32::try_from(frame.bytes.len()) {
            Ok(length) => length,
            Err(_) => {
                set_last_error("packed buffer too large for i32 length".to_string());
                -1
            }
        },
        Err(error) => {
            set_last_error(error);
            -1
        }
    }
}

fn direct_packed_write(
    component: *const c_char,
    slot: u32,
    requested_mode: &str,
    out: *mut u8,
    out_len: usize,
) -> i32 {
    if out.is_null() {
        set_last_error("missing output buffer".to_string());
        return -1;
    }
    let component = component_from_ptr(component);
    let frame = match packed_frame_data_for_component_slot(&component, slot, requested_mode) {
        Ok(frame) => frame,
        Err(error) => {
            set_last_error(error);
            return -1;
        }
    };
    if out_len < frame.bytes.len() {
        set_last_error(format!(
            "output buffer too small: need {}, got {}",
            frame.bytes.len(),
            out_len
        ));
        return -2;
    }
    unsafe {
        ptr::copy_nonoverlapping(frame.bytes.as_ptr(), out, frame.bytes.len());
    }
    match i32::try_from(frame.bytes.len()) {
        Ok(written) => written,
        Err(_) => {
            set_last_error("packed buffer too large for i32 length".to_string());
            -1
        }
    }
}

fn push_rgb_frame(input: VideoFrameInput) -> Result<serde_json::Value, String> {
    push_rgb_frame_with_source(input, "pushed_rgb")
}

fn push_rgb_frame_with_source(
    input: VideoFrameInput,
    source: &str,
) -> Result<serde_json::Value, String> {
    if input.slot == 0 {
        return Err("invalid slot".to_string());
    }
    let component = normalize_component(input.component.as_deref());
    if input.width == 0 || input.height == 0 {
        return Err("width and height must be >= 1".to_string());
    }
    let expected_len = input
        .width
        .checked_mul(input.height)
        .ok_or_else(|| "frame size overflow".to_string())? as usize;
    if input.rgb.len() != expected_len {
        return Err(format!(
            "rgb length {} does not match {}x{}",
            input.rgb.len(),
            input.width,
            input.height
        ));
    }

    let mut state = request_runtime_state()?;
    let slot = state
        .slots
        .get_mut(&slot_key(&component, input.slot))
        .ok_or_else(|| "not initialized".to_string())?;
    if slot.width != input.width || slot.height != input.height {
        return Err(format!(
            "frame size {}x{} does not match slot {}x{}",
            input.width, input.height, slot.width, slot.height
        ));
    }

    let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
    slot.frame_id = frame_id;
    slot.ready = true;
    slot.connected = input.connected.unwrap_or(true);
    if !slot.connected {
        slot.input_source_handle = 0;
        slot.input_candidate_source_handle = 0;
        slot.input_selected_source_handle = 0;
        slot.input_resolved_source_handle = 0;
        slot.input_upstream_source_handle = 0;
    }
    slot.latest_frame = Some(FrameBuffer {
        frame_id,
        width: input.width,
        height: input.height,
        source: source.to_string(),
        rgb: input.rgb,
    });
    let response = serde_json::json!({
        "component": slot.component,
        "slot": slot.slot,
        "frame_id": slot.frame_id,
        "width": slot.width,
        "height": slot.height,
        "connected": slot.connected,
        "input_source_handle": slot.input_source_handle,
        "source": source
    });
    set_runtime(state);
    Ok(response)
}

fn validate_init(init: &VideoInit, state: &RuntimeState) -> Result<(), String> {
    if init.slot == 0 {
        return Err("slot must be >= 1".to_string());
    }
    if init.slot > state.config.limits.max_active_slots {
        return Err(format!(
            "slot {} exceeds max_active_slots {}",
            init.slot, state.config.limits.max_active_slots
        ));
    }
    if init.width == 0 || init.height == 0 {
        return Err("width and height must be >= 1".to_string());
    }
    match init.mode.as_str() {
        "gray" => validate_frame_size(
            init.width,
            init.height,
            state.config.limits.gray.max_width,
            state.config.limits.gray.max_height,
            "gray",
        ),
        "rgb" => validate_frame_size(
            init.width,
            init.height,
            state.config.limits.rgb.max_width,
            state.config.limits.rgb.max_height,
            "rgb",
        ),
        _ => Err("mode must be gray or rgb".to_string()),
    }
}

fn configure_from_context_json(context_json: *const c_char) -> Result<serde_json::Value, String> {
    let text =
        unsafe_cstr(context_json).ok_or_else(|| "missing runtime context json".to_string())?;
    let context: PluginRuntimeContext =
        serde_json::from_str(&text).map_err(|error| format!("invalid runtime context: {error}"))?;
    configure_runtime(context, None)
}

fn configure_from_context_path(context_path: *mut u16) -> Result<serde_json::Value, String> {
    if context_path.is_null() {
        return Err("missing runtime context path".to_string());
    }
    let path = unsafe { wide_ptr_to_path(context_path) };
    let context: PluginRuntimeContext =
        read_json(&path).map_err(|error| format!("loading runtime context: {error:#}"))?;
    configure_runtime(context, Some(path))
}

fn configure_runtime(
    mut context: PluginRuntimeContext,
    context_path: Option<PathBuf>,
) -> Result<serde_json::Value, String> {
    if context.mode == "replace_dll" && context.process_id.is_none() {
        context.process_id = current_process_id();
    }
    let config = match &context.config_path {
        Some(path) => read_json::<VideoGetConfig>(path)
            .map_err(|error| format!("loading video_get config: {error:#}"))?,
        None => default_video_get_config(),
    };
    validate_config(&config)?;

    let signatures = read_json::<SignatureFile>(&context.signatures_path)
        .map_err(|error| format!("loading signatures: {error:#}"))?;
    if !signatures
        .game_sha256
        .eq_ignore_ascii_case(&context.game_sha256)
    {
        return Err(format!(
            "signature hash mismatch: context={}, signatures={}",
            context.game_sha256, signatures.game_sha256
        ));
    }

    let signature_keys = signature_keys(&signatures.symbols);
    let signature_summary = signature_summary(&signatures.symbols);
    let byte_check_summary = verify_signature_bytes(&context, &signatures.symbols)?;
    let hook_plan = load_hook_plan(context.hook_plan_path.as_ref())?;
    let hook_plan_validation = validate_hook_plan(&hook_plan, &signatures.symbols);
    let log_path = context.log_dir.join("video_get.log");
    clear_plugin_log_outputs(&context.log_dir)?;
    VIDEO_INIT_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    TEXTURE_UPLOAD_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    TEXTURE_UPLOAD_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    SOURCE_TEXTURE_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    SOURCE_TEXTURE_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    MONITOR_RENDER_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    MONITOR_RENDER_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    MONITOR_RENDER_PBO_DRAIN_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    MONITOR_GL_BIND_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    MONITOR_INPUT_RELATION_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    MONITOR_BRIDGE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    VIDEO_NODE_REGISTRY_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    VIDEO_NODE_INIT_REGISTRY_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    RENDER_QUEUE_ALLOC_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    RENDER_QUEUE_SUBMIT_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    RENDERER_VIDEO_PASS_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    PENDING_MONITOR_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    MONITOR_RENDER_HEAVY_PROBE_LAST_MS.store(0, Ordering::Relaxed);
    MONITOR_RENDER_HEAVY_PROBE_ATTEMPTS.store(0, Ordering::Relaxed);
    RENDER_TARGET_TEXTURE_CREATE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    RENDER_TARGET_TEXTURE_CREATE_WITH_SLOTS_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_MONITOR_BIND_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_MONITOR_BIND_WITH_SLOTS_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_MONITOR_BIND_SLOT_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_MONITOR_BIND_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_GL_BIND_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_GL_BIND_UNIT_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_GL_BIND_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
    ADDITIVE_EXACT_READBACK_ATTEMPTS.store(0, Ordering::Relaxed);
    ADDITIVE_EXACT_READBACK_LAST_MS.store(0, Ordering::Relaxed);
    let state = RuntimeState {
        configured: true,
        context: Some(context.clone()),
        config,
        hook_runtime: default_hook_runtime_state(),
        signatures_loaded: true,
        signature_symbol_count: signature_keys.len(),
        signature_keys: signature_keys.clone(),
        signature_symbols: signatures.symbols.clone(),
        signature_summary: signature_summary.clone(),
        byte_check_summary: byte_check_summary.clone(),
        hook_plan: Some(hook_plan.clone()),
        hook_plan_path: context.hook_plan_path.clone(),
        slots: BTreeMap::new(),
        latest_texture_upload_frame: None,
        gl_texture_bindings: BTreeMap::new(),
        video_node_sources: BTreeMap::new(),
        video_source_components: BTreeMap::new(),
        monitor_pbo_readbacks: BTreeMap::new(),
        monitor_gl_bind_events: Vec::new(),
        renderer_video_pass_events: Vec::new(),
        pending_monitor_render_probes: Vec::new(),
        last_error: None,
        log_path: Some(log_path.clone()),
        load_event_path: None,
        runtime_snapshot_path: None,
        runtime_snapshot_jsonl_path: None,
    };
    set_runtime(state.clone());
    append_log(
        &log_path,
        &format!(
            "configured mode={} build={} signatures={}",
            context.mode,
            context.game_build_label,
            signature_keys.join(",")
        ),
    )?;
    Ok(serde_json::json!({
        "configured": true,
        "plugin": context.plugin_id,
        "mode": context.mode,
        "runtime_context_path": context_path,
        "game_build_label": context.game_build_label,
        "signatures_loaded": true,
        "signature_symbol_count": signature_keys.len(),
        "signature_summary": signature_summary,
        "byte_checks": {
            "checked": byte_check_summary.checked,
            "verified": byte_check_summary.verified,
            "failed": byte_check_summary.failed,
            "failures": byte_check_summary.failures
        },
        "hook_plan": hook_plan_summary_value(&hook_plan, &hook_plan_validation),
        "hook_plan_validation": hook_plan_validation,
        "hook_plan_path": context.hook_plan_path,
        "log_path": log_path.display().to_string(),
        "load_event_path": null,
        "runtime_snapshot_path": null,
        "runtime_snapshot_jsonl_path": null,
        "verbose_runtime_diagnostics": false
    }))
}

fn install_hook_runtime(remote_thread: bool) -> Result<serde_json::Value, String> {
    let mut state = request_runtime_state()?;
    let configured = state.config.clone();
    state.hook_runtime.install_attempted = true;
    state.hook_runtime.lua_registration_adapter = true;
    state.hook_runtime.detour_engine_ready = detour_engine_available();
    state.hook_runtime.installed_detour_count = detour_installed_count();
    state.hook_runtime.real_lua_hook = false;
    state.hook_runtime.real_video_capture = false;
    state.hook_runtime.installed_by_mode = state.context.as_ref().map(|context| {
        if remote_thread {
            format!("{}:remote", context.mode)
        } else {
            format!("{}:local", context.mode)
        }
    });
    let hook_plan = state.hook_plan.clone().unwrap_or_else(default_hook_plan);
    let hook_plan_validation = validate_hook_plan(&hook_plan, &state.signature_symbols);
    let install_dry_run = hook_install_dry_run(
        state.context.as_ref(),
        &hook_plan,
        &state.signature_symbols,
        &hook_plan_validation,
    );
    let target_patch_gate = evaluate_target_patch_gate(
        &configured,
        &hook_plan,
        &hook_plan_validation,
        &install_dry_run,
    );

    if !configured.hooking.auto_install {
        state.hook_runtime.runtime_active = false;
        state.hook_runtime.mock_frame_pump_active = false;
        state.hook_runtime.last_install_error = Some("hooking.auto_install=false".to_string());
        set_runtime(state);
        return Err("hooking.auto_install=false".to_string());
    }

    let missing = missing_required_hook_stages(&hook_plan, &state.signature_symbols);
    if configured.hooking.fail_closed && !missing.is_empty() {
        state.hook_runtime.runtime_active = false;
        state.hook_runtime.mock_frame_pump_active = false;
        state.hook_runtime.last_install_error = Some(format!(
            "missing required signature stages: {}",
            missing.join(", ")
        ));
        let result = serde_json::json!({
            "hook_runtime": state.hook_runtime,
            "installed": false,
            "fail_closed": true,
            "missing_required_stages": missing,
            "lua_registration_adapter": true,
            "message": "signature stages are incomplete; no target patch points were modified"
        });
        set_runtime(state);
        return Ok(result);
    }

    state.hook_runtime.runtime_active = true;
    state.hook_runtime.mock_frame_pump_active = configured.mock_render.enabled;
    state.hook_runtime.detour_engine_ready = detour_engine_available();
    state.hook_runtime.last_install_error = None;
    let mut target_patch_points_modified = false;
    let mut target_patch_install = serde_json::json!({
        "attempted": false,
        "installed_count": 0,
        "hooks": []
    });
    if target_patch_gate
        .get("can_patch")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        match install_hook_plan_detours(
            state.context.as_ref(),
            &hook_plan,
            &state.signature_symbols,
            &hook_plan_validation,
        ) {
            Ok(install) => {
                target_patch_points_modified = install
                    .get("installed_count")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(0)
                    > 0;
                target_patch_install = install;
                state.hook_runtime.real_lua_hook = target_patch_points_modified;
            }
            Err(error) => {
                state.hook_runtime.runtime_active = false;
                state.hook_runtime.mock_frame_pump_active = false;
                state.hook_runtime.last_install_error = Some(error.clone());
                state.hook_runtime.installed_detour_count = detour_installed_count();
                let result = serde_json::json!({
                    "hook_runtime": state.hook_runtime,
                    "installed": false,
                    "target_patch_points_modified": false,
                    "target_patch_gate": target_patch_gate,
                    "target_patch_install": {
                        "attempted": true,
                        "error": error
                    },
                    "detour_engine": detour_status_value(),
                    "lua_registration_adapter": true,
                    "message": "target patch gate opened but detour installation failed"
                });
                set_runtime(state);
                return Ok(result);
            }
        }
    }
    state.hook_runtime.installed_detour_count = detour_installed_count();
    let log_path = state.log_path.clone();
    let result = serde_json::json!({
        "hook_runtime": state.hook_runtime,
        "installed": true,
        "target_patch_points_modified": target_patch_points_modified,
        "target_patch_gate": target_patch_gate,
        "target_patch_install": target_patch_install,
        "detour_engine": detour_status_value(),
        "lua_registration_adapter": true,
        "mock_frame_pump": {
            "active": configured.mock_render.enabled,
            "max_fps": normalized_mock_fps(configured.mock_render.max_fps),
            "updates_initialized_slots": configured.mock_render.update_initialized_slots
        },
        "real_lua_hook": state.hook_runtime.real_lua_hook,
        "real_video_capture": false,
        "next_native_bridge": "The replace-DLL plan registers video through the FUN_1402e6050 component Lua initializer when its signature checks pass; real video-source readback still needs a verified render/input hook."
    });
    set_runtime(state);

    if configured.mock_render.enabled {
        ensure_mock_frame_pump_started();
    }
    if let Some(path) = log_path {
        let hook_summary = summarize_target_patch_install_hooks(&target_patch_install);
        let installed_count = target_patch_install
            .get("installed_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let _ = append_log(
            &path,
            &format!(
                "hook_runtime installed remote={} mock_fps={} target_patch_points_modified={} installed_count={} hooks={}",
                remote_thread,
                normalized_mock_fps(configured.mock_render.max_fps),
                target_patch_points_modified,
                installed_count,
                hook_summary
            ),
        );
    }
    Ok(result)
}

fn summarize_target_patch_install_hooks(install: &serde_json::Value) -> String {
    let Some(hooks) = install.get("hooks").and_then(|value| value.as_array()) else {
        return "none".to_string();
    };
    if hooks.is_empty() {
        return "none".to_string();
    }
    hooks
        .iter()
        .map(|hook| {
            let label = hook
                .get("label")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let stage = hook
                .get("stage")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let target = hook
                .get("target_va")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let runtime = hook
                .get("runtime_address")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let trampoline = hook
                .get("trampoline")
                .and_then(|value| value.as_str())
                .unwrap_or("none");
            format!("{label}:{stage}@{target}->{runtime}/trampoline={trampoline}")
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn hook_status_value() -> serde_json::Value {
    let state = runtime_snapshot();
    serde_json::json!({
        "configured": state.configured,
        "hook_runtime": state.hook_runtime,
        "lua_adapter": lua_adapter_status_value(),
        "detours": detour_status_value(),
        "active_slots": state.slots.len(),
        "mock_frame_pump_global_active": FRAME_PUMP_ACTIVE.load(Ordering::Relaxed)
    })
}

fn hook_plan_dry_run_value() -> serde_json::Value {
    let state = runtime_snapshot();
    let plan = state.hook_plan.unwrap_or_else(default_hook_plan);
    let validation = validate_hook_plan(&plan, &state.signature_symbols);
    let dry_run = hook_install_dry_run(
        state.context.as_ref(),
        &plan,
        &state.signature_symbols,
        &validation,
    );
    serde_json::json!({
        "configured": state.configured,
        "hook_plan_path": state.hook_plan_path.map(|path| path.display().to_string()),
        "summary": hook_plan_summary_value(&plan, &validation),
        "install_dry_run": dry_run
    })
}

fn register_lua_api(lua_state: *mut c_void, api: *const VideoGetLuaApiV1) -> Result<i32, String> {
    if lua_state.is_null() {
        return Err("missing lua_State".to_string());
    }
    if api.is_null() {
        return Err("missing Lua API table".to_string());
    }
    let api = unsafe { *api };
    validate_lua_api(&api)?;
    {
        let mut adapter = lua_adapter_cell()
            .lock()
            .map_err(|_| "lua adapter mutex poisoned".to_string())?;
        adapter.api = Some(api);
        adapter.registrations += 1;
        adapter.last_error = None;
    }

    unsafe {
        let lua_createtable = api.lua_createtable.unwrap();
        let lua_pushcclosure = api.lua_pushcclosure.unwrap();
        let lua_setglobal = api.lua_setglobal.unwrap();
        let lua_setfield = api.lua_setfield.unwrap();
        lua_createtable(lua_state, 0, lua_c_functions().len() as i32);
        for (name, function) in lua_c_functions() {
            let c_name = CString::new(name).map_err(|error| format!("lua name error: {error}"))?;
            lua_pushcclosure(lua_state, function, 0);
            lua_setfield(lua_state, -2, c_name.as_ptr());
        }
        let video_name = CString::new("video").unwrap();
        lua_setglobal(lua_state, video_name.as_ptr());
    }

    if let Ok(mut state) = runtime_cell().lock() {
        state.hook_runtime.lua_registration_adapter = true;
        state.hook_runtime.lua_api_registered = true;
        if verbose_runtime_diagnostics_enabled() {
            if let Some(path) = &state.log_path {
                let _ = append_log(
                    path,
                    &format!(
                        "lua api registered table=video functions={}",
                        lua_c_functions().len()
                    ),
                );
            }
        }
    }
    Ok(1)
}

fn set_lua_hook_api(api: *const VideoGetLuaApiV1) -> Result<(), String> {
    if api.is_null() {
        return Err("missing Lua hook API table".to_string());
    }
    let api = unsafe { *api };
    validate_lua_api(&api)?;
    let mut adapter = lua_adapter_cell()
        .lock()
        .map_err(|_| "lua adapter mutex poisoned".to_string())?;
    adapter.hook_api = Some(api);
    adapter.last_error = None;
    Ok(())
}

fn build_game_lua_helpers_from_hook_plan(
    context: Option<&PluginRuntimeContext>,
    game_lua: &HookPlanGameLua,
) -> Result<GameLuaHelpers, String> {
    Ok(GameLuaHelpers {
        create_table: resolve_lua_api_function::<GameLuaCreateTableFn>(
            context,
            "game_lua.create_table",
            game_lua.create_table.as_deref(),
        )?,
        push_string: resolve_lua_api_function::<GameLuaPushStringFn>(
            context,
            "game_lua.push_string",
            game_lua.push_string.as_deref(),
        )?,
        rawseti: resolve_lua_api_function::<GameLuaRawSetIFn>(
            context,
            "game_lua.rawseti",
            game_lua.rawseti.as_deref(),
        )?,
        register_table: resolve_lua_api_function::<GameLuaRegisterTableFn>(
            context,
            "game_lua.register_table",
            game_lua.register_table.as_deref(),
        )?,
        arg_slot: match game_lua.arg_slot.as_deref() {
            Some(value) if !value.trim().is_empty() => {
                Some(resolve_lua_api_function::<GameLuaArgSlotFn>(
                    context,
                    "game_lua.arg_slot",
                    Some(value),
                )?)
            }
            _ => None,
        },
    })
}

fn set_game_lua_helpers(helpers: GameLuaHelpers) -> Result<(), String> {
    GAME_LUA_CREATE_TABLE.store(helpers.create_table as usize, Ordering::SeqCst);
    GAME_LUA_PUSH_STRING.store(helpers.push_string as usize, Ordering::SeqCst);
    GAME_LUA_RAWSETI.store(helpers.rawseti as usize, Ordering::SeqCst);
    GAME_LUA_REGISTER_TABLE.store(helpers.register_table as usize, Ordering::SeqCst);
    GAME_LUA_ARG_SLOT.store(
        helpers
            .arg_slot
            .map(|arg_slot| arg_slot as usize)
            .unwrap_or(0),
        Ordering::SeqCst,
    );
    if let Ok(mut adapter) = lua_adapter_cell().lock() {
        adapter.last_error = None;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum LuaHookOriginalCall {
    Direct(*mut c_void),
    Arg1(*mut c_void),
    Arg2(*mut c_void, *mut c_void),
    Arg3(*mut c_void, *mut c_void, *mut c_void),
    Arg4(*mut c_void, *mut c_void, *mut c_void, *mut c_void),
}

fn register_lua_api_from_hook_chained(
    lua_state: *mut c_void,
    original_call: LuaHookOriginalCall,
) -> Result<i32, String> {
    let original_result = call_lua_registration_original(original_call)?;
    let registered_count = register_lua_api_from_hook(lua_state)?;
    if let Ok(mut adapter) = lua_adapter_cell().lock() {
        if original_result.is_some() {
            adapter.hook_original_calls += 1;
        }
    }
    Ok(original_result.unwrap_or(registered_count))
}

fn register_lua_api_from_hook(lua_state: *mut c_void) -> Result<i32, String> {
    let api = lua_adapter_cell()
        .lock()
        .map_err(|_| "lua adapter mutex poisoned".to_string())?
        .hook_api
        .ok_or_else(|| "lua hook API table is not configured".to_string())?;
    let count = register_lua_api(lua_state, &api)?;
    if let Ok(mut adapter) = lua_adapter_cell().lock() {
        adapter.hook_registrations += 1;
    }
    Ok(count)
}

fn call_lua_registration_original(
    original_call: LuaHookOriginalCall,
) -> Result<Option<i32>, String> {
    unsafe {
        match original_call {
            LuaHookOriginalCall::Direct(lua_state) => {
                let trampoline = LUA_REGISTRATION_ORIGINAL_DIRECT.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(None);
                }
                let original: extern "C" fn(*mut c_void) -> i32 = std::mem::transmute(trampoline);
                Ok(Some(original(lua_state)))
            }
            LuaHookOriginalCall::Arg1(lua_state) => {
                let trampoline = LUA_REGISTRATION_ORIGINAL_ARG1.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(None);
                }
                let original: extern "C" fn(*mut c_void) -> i32 = std::mem::transmute(trampoline);
                Ok(Some(original(lua_state)))
            }
            LuaHookOriginalCall::Arg2(arg1, lua_state) => {
                let trampoline = LUA_REGISTRATION_ORIGINAL_ARG2.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(None);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(Some(original(arg1, lua_state)))
            }
            LuaHookOriginalCall::Arg3(arg1, arg2, lua_state) => {
                let trampoline = LUA_REGISTRATION_ORIGINAL_ARG3.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(None);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(Some(original(arg1, arg2, lua_state)))
            }
            LuaHookOriginalCall::Arg4(arg1, arg2, arg3, lua_state) => {
                let trampoline = LUA_REGISTRATION_ORIGINAL_ARG4.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(None);
                }
                let original: extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) -> i32 = std::mem::transmute(trampoline);
                Ok(Some(original(arg1, arg2, arg3, lua_state)))
            }
        }
    }
}

fn set_lua_registration_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    match replacement {
        "stormworks_video_get_register_lua_api_hook" => {
            LUA_REGISTRATION_ORIGINAL_DIRECT.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_register_lua_api_hook_arg1" => {
            LUA_REGISTRATION_ORIGINAL_ARG1.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_register_lua_api_hook_arg2" => {
            LUA_REGISTRATION_ORIGINAL_ARG2.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_register_lua_api_hook_arg3" => {
            LUA_REGISTRATION_ORIGINAL_ARG3.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_register_lua_api_hook_arg4" => {
            LUA_REGISTRATION_ORIGINAL_ARG4.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_component_lua_init_hook_arg1" => {
            COMPONENT_LUA_INIT_ORIGINAL_ARG1.store(value, Ordering::SeqCst)
        }
        _ => {}
    }
}

fn component_lua_init_from_hook_chained(
    component_lua_init_context: *mut c_void,
) -> Result<(), String> {
    let original_called = call_component_lua_init_original_arg1(component_lua_init_context)?;
    if GAME_LUA_REGISTER_TABLE.load(Ordering::SeqCst) != 0 {
        register_game_lua_video_table(component_lua_init_context)?
    } else {
        let lua_state = component_lua_init_lua_owner(component_lua_init_context)?;
        register_lua_api_from_hook(lua_state)?
    };
    if let Ok(mut adapter) = lua_adapter_cell().lock() {
        if original_called {
            adapter.hook_original_calls += 1;
        }
    }
    Ok(())
}

fn register_game_lua_video_table(component_lua_init_context: *mut c_void) -> Result<i32, String> {
    if component_lua_init_context.is_null() {
        return Err("missing component Lua initialization context".to_string());
    }
    let lua_owner = component_lua_init_lua_owner(component_lua_init_context)? as usize;
    let helpers = game_lua_helpers()?;
    let table_name = game_lua_video_table_name();
    let table_name_ptr = table_name.as_ptr() as usize;
    unsafe {
        (helpers.register_table)(
            component_lua_init_context as usize,
            &table_name_ptr as *const usize,
            game_lua_function_pairs().as_ptr(),
            component_lua_init_context as usize,
        );
    }
    remember_game_lua_component_context(lua_owner, component_lua_init_context as usize);
    if let Ok(mut adapter) = lua_adapter_cell().lock() {
        adapter.registrations += 1;
        adapter.hook_registrations += 1;
        adapter.last_error = None;
    }
    if let Ok(mut state) = runtime_cell().lock() {
        state.hook_runtime.lua_registration_adapter = true;
        state.hook_runtime.lua_api_registered = true;
        if verbose_runtime_diagnostics_enabled()
            && COMPONENT_LUA_REGISTRATION_DIAGNOSTIC_COUNT.fetch_add(1, Ordering::Relaxed) < 16
        {
            if let Some(path) = &state.log_path {
                let _ = append_log(
                    path,
                    &format!(
                        "component_lua_init registered table=video context=0x{:x} functions={}",
                        component_lua_init_context as usize,
                        game_lua_c_functions().len()
                    ),
                );
            }
        }
    }
    Ok(1)
}

fn game_lua_helpers() -> Result<GameLuaHelpers, String> {
    let register_table = GAME_LUA_REGISTER_TABLE.load(Ordering::SeqCst);
    let create_table = GAME_LUA_CREATE_TABLE.load(Ordering::SeqCst);
    let push_string = GAME_LUA_PUSH_STRING.load(Ordering::SeqCst);
    let rawseti = GAME_LUA_RAWSETI.load(Ordering::SeqCst);
    let arg_slot = GAME_LUA_ARG_SLOT.load(Ordering::SeqCst);
    if register_table == 0 {
        return Err("game Lua register_table helper is not configured".to_string());
    }
    if create_table == 0 || push_string == 0 || rawseti == 0 {
        return Err("game Lua table/string helpers are not configured".to_string());
    }
    Ok(GameLuaHelpers {
        create_table: unsafe { std::mem::transmute(create_table) },
        push_string: unsafe { std::mem::transmute(push_string) },
        rawseti: unsafe { std::mem::transmute(rawseti) },
        register_table: unsafe { std::mem::transmute(register_table) },
        arg_slot: if arg_slot == 0 {
            None
        } else {
            Some(unsafe { std::mem::transmute(arg_slot) })
        },
    })
}

fn game_lua_video_table_name() -> &'static CString {
    static NAME: OnceLock<CString> = OnceLock::new();
    NAME.get_or_init(|| CString::new("video").unwrap())
}

fn game_lua_function_pairs() -> &'static [GameLuaFunctionPair] {
    static PAIRS: OnceLock<Vec<GameLuaFunctionPair>> = OnceLock::new();
    PAIRS.get_or_init(|| {
        let mut pairs = Vec::new();
        for (name, function) in game_lua_c_functions() {
            let name = CString::new(name).unwrap();
            let leaked_name = Box::leak(name.into_boxed_c_str());
            pairs.push(GameLuaFunctionPair {
                name: leaked_name.as_ptr(),
                function: Some(function),
            });
        }
        pairs.push(GameLuaFunctionPair {
            name: ptr::null(),
            function: None,
        });
        pairs
    })
}

fn game_lua_c_functions() -> [(&'static str, VideoGetLuaCFunction); 10] {
    [
        ("init", video_game_lua_init),
        ("isConnected", video_game_lua_is_connected),
        ("isReady", video_game_lua_is_ready),
        ("getInfo", video_game_lua_get_info),
        ("getSize", video_game_lua_get_size),
        ("get", video_game_lua_get),
        ("getGray", video_game_lua_get_gray),
        ("getRGB", video_game_lua_get_rgb),
        ("getPackedGray", video_game_lua_get_packed_gray),
        ("getPackedRGB", video_game_lua_get_packed_rgb),
    ]
}

fn component_lua_init_lua_owner(
    component_lua_init_context: *mut c_void,
) -> Result<*mut c_void, String> {
    if component_lua_init_context.is_null() {
        return Err("missing component Lua initialization context".to_string());
    }
    let lua_state = unsafe { *(component_lua_init_context.cast::<usize>().add(1)) as *mut c_void };
    if lua_state.is_null() {
        return Err("component Lua initialization context has null Lua owner at +0x8".to_string());
    }
    Ok(lua_state)
}

fn call_component_lua_init_original_arg1(
    component_lua_init_context: *mut c_void,
) -> Result<bool, String> {
    let trampoline = COMPONENT_LUA_INIT_ORIGINAL_ARG1.load(Ordering::SeqCst);
    if trampoline == 0 {
        return Ok(false);
    }
    let original: extern "C" fn(*mut c_void) = unsafe { std::mem::transmute(trampoline) };
    original(component_lua_init_context);
    Ok(true)
}

#[derive(Debug, Clone, Copy)]
enum ComponentContextHookOriginalCall {
    Arg1(*mut c_void),
    Arg2(*mut c_void, *mut c_void),
    Arg3(*mut c_void, *mut c_void, *mut c_void),
    Arg4(*mut c_void, *mut c_void, *mut c_void, *mut c_void),
}

fn component_context_from_hook_chained(
    original_call: ComponentContextHookOriginalCall,
) -> Result<i32, String> {
    let component = component_context_key_from_hook(&original_call);
    LUA_COMPONENT_CONTEXT.with(|stack| stack.borrow_mut().push(component));
    let result = call_component_context_original(original_call);
    let leave_ok = LUA_COMPONENT_CONTEXT.with(|stack| stack.borrow_mut().pop().is_some());
    if !leave_ok {
        return Err("component context stack is empty after hook call".to_string());
    }
    result
}

fn component_context_key_from_hook(original_call: &ComponentContextHookOriginalCall) -> String {
    let ptr = match *original_call {
        ComponentContextHookOriginalCall::Arg1(component)
        | ComponentContextHookOriginalCall::Arg2(_, component)
        | ComponentContextHookOriginalCall::Arg3(_, _, component)
        | ComponentContextHookOriginalCall::Arg4(_, _, _, component) => component,
    };
    format!("component_ptr:{:x}", ptr as usize)
}

fn call_component_context_original(
    original_call: ComponentContextHookOriginalCall,
) -> Result<i32, String> {
    unsafe {
        match original_call {
            ComponentContextHookOriginalCall::Arg1(component) => {
                let trampoline = COMPONENT_CONTEXT_ORIGINAL_ARG1.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void) -> i32 = std::mem::transmute(trampoline);
                Ok(original(component))
            }
            ComponentContextHookOriginalCall::Arg2(arg1, component) => {
                let trampoline = COMPONENT_CONTEXT_ORIGINAL_ARG2.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(original(arg1, component))
            }
            ComponentContextHookOriginalCall::Arg3(arg1, arg2, component) => {
                let trampoline = COMPONENT_CONTEXT_ORIGINAL_ARG3.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(original(arg1, arg2, component))
            }
            ComponentContextHookOriginalCall::Arg4(arg1, arg2, arg3, component) => {
                let trampoline = COMPONENT_CONTEXT_ORIGINAL_ARG4.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) -> i32 = std::mem::transmute(trampoline);
                Ok(original(arg1, arg2, arg3, component))
            }
        }
    }
}

fn set_component_context_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    match replacement {
        "stormworks_video_get_component_context_hook_arg1" => {
            COMPONENT_CONTEXT_ORIGINAL_ARG1.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_component_context_hook_arg2" => {
            COMPONENT_CONTEXT_ORIGINAL_ARG2.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_component_context_hook_arg3" => {
            COMPONENT_CONTEXT_ORIGINAL_ARG3.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_component_context_hook_arg4" => {
            COMPONENT_CONTEXT_ORIGINAL_ARG4.store(value, Ordering::SeqCst)
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Copy)]
enum InputVideoHookOriginalCall {
    Arg1(*mut c_void),
    Arg2(*mut c_void, *mut c_void),
    Arg3(*mut c_void, *mut c_void, *mut c_void),
    Arg4(*mut c_void, *mut c_void, *mut c_void, *mut c_void),
}

fn input_video_from_hook_chained(original_call: InputVideoHookOriginalCall) -> Result<i32, String> {
    let source = input_video_source_from_hook(&original_call);
    let result = call_input_video_original(original_call)?;
    let Some(component) = current_lua_component_context() else {
        return bind_input_video_from_lua_script_node(&original_call, source as u64)
            .map(|_| result);
    };
    let mut source_handles = input_video_node_from_hook(&original_call)
        .map(input_video_source_handles_from_node)
        .unwrap_or_default();
    source_handles.candidate = source as u64;
    if source_handles.effective() == 0 && source != 0 {
        source_handles.selected = source as u64;
    }
    let update = bind_current_component_video_inputs(&component, source_handles)?;
    if update.updated_slots == 0 {
        set_last_error(format!(
            "input video hook found no initialized video slots for component `{component}`"
        ));
    }
    Ok(result)
}

fn input_video_node_update_from_hook_chained(
    node: *mut c_void,
    source: *mut c_void,
) -> Result<(), String> {
    call_input_video_node_update_original_arg2(node, source);
    let original_call = InputVideoHookOriginalCall::Arg2(node, source);
    let mut source_handles = input_video_source_handles_from_node(node as usize);
    source_handles.candidate = source as u64;
    record_video_node_source_handles(node as usize, source_handles);
    log_input_video_source_layout_diagnostic(node as usize, source_handles);
    let update =
        bind_input_video_from_lua_script_node_with_sources(&original_call, source_handles)?;
    if update.updated_slots > 0 {
        let state = runtime_snapshot();
        log_runtime_diagnostic(
            &state,
            &format!(
                "input video node update bound Lua script node=0x{:x} candidate={} selected={} resolved={} upstream={} effective={} updated_slots={} source_layouts={} slots={}",
                node as usize,
                format_hex_or_zero(source_handles.candidate),
                format_hex_or_zero(source_handles.selected),
                format_hex_or_zero(source_handles.resolved),
                format_hex_or_zero(source_handles.upstream),
                format_hex_or_zero(source_handles.effective()),
                update.updated_slots,
                format_input_video_source_layouts(node as usize, source_handles),
                describe_slots(&state)
            ),
            &VIDEO_INIT_DIAGNOSTIC_COUNT,
            16,
        );
    }
    Ok(())
}

fn input_video_node_select_from_hook_chained(
    node: *mut c_void,
    collection: *mut c_void,
    arg3: *mut c_void,
    arg4: *mut c_void,
    selected_index: *mut c_void,
) -> Result<(), String> {
    call_input_video_node_select_original_arg5(node, collection, arg3, arg4, selected_index);
    let source_handles = input_video_source_handles_from_node(node as usize);
    record_video_node_source_handles(node as usize, source_handles);
    log_input_video_source_layout_diagnostic(node as usize, source_handles);
    let original_call =
        InputVideoHookOriginalCall::Arg2(node, source_handles.effective() as usize as *mut c_void);
    let update =
        bind_input_video_from_lua_script_node_with_sources(&original_call, source_handles)?;
    let state = runtime_snapshot();
    let component = component_context_from_lua_script_input_video_node(node as usize)
        .or_else(|| component_context_from_input_video_sources(source_handles))
        .unwrap_or_else(|| "unmapped".to_string());
    log_runtime_diagnostic(
        &state,
        &format!(
            "input video node select node={} collection={} arg3={} arg4={} selected_index={} selected_index_value={} component={} candidate={} selected={} resolved={} effective={} updated_slots={} collection_layout={} source_layouts={} slots={}",
            format_hex_or_zero(node as u64),
            format_hex_or_zero(collection as u64),
            format_hex_or_zero(arg3 as u64),
            format_hex_or_zero(arg4 as u64),
            format_hex_or_zero(selected_index as u64),
            read_i32_pointer(selected_index as usize)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unreadable".to_string()),
            component,
            format_hex_or_zero(source_handles.candidate),
            format_hex_or_zero(source_handles.selected),
            format_hex_or_zero(source_handles.resolved),
            format_hex_or_zero(source_handles.effective()),
            update.updated_slots,
            describe_input_video_source_collection(collection as usize),
            format_input_video_source_layouts(node as usize, source_handles),
            describe_slots(&state)
        ),
        &VIDEO_INIT_DIAGNOSTIC_COUNT,
        16,
    );
    Ok(())
}

fn record_video_node_source_handles(node: usize, source_handles: InputVideoNodeSourceHandles) {
    if node == 0 {
        return;
    }
    if let Ok(mut state) = request_runtime_state() {
        state.video_node_sources.insert(node as u64, source_handles);
        if let Some(component) = component_context_from_lua_script_input_video_node(node)
            .or_else(|| component_context_from_input_video_sources(source_handles))
        {
            for (_, handle) in source_handles.handles() {
                if handle != 0 {
                    state
                        .video_source_components
                        .insert(handle, component.clone());
                    if let Some(key) = video_source_handle_structural_key(handle) {
                        state.video_source_components.insert(key, component.clone());
                    }
                }
            }
        }
        set_runtime(state);
    }
    log_video_node_registry_diagnostic("record_video_node_source_handles");
}

fn bind_input_video_from_lua_script_node(
    original_call: &InputVideoHookOriginalCall,
    fallback_source_handle: u64,
) -> Result<InputVideoBindUpdate, String> {
    let Some(node) = input_video_node_from_hook(original_call) else {
        set_last_error("input video hook has no current component context".to_string());
        return Ok(InputVideoBindUpdate {
            updated_slots: 0,
            skipped_fps_slots: 0,
        });
    };
    let mut source_handles = input_video_source_handles_from_node(node);
    source_handles.candidate = fallback_source_handle;
    if source_handles.effective() == 0 && fallback_source_handle != 0 {
        source_handles.selected = fallback_source_handle;
    }
    bind_input_video_from_lua_script_node_with_sources(original_call, source_handles)
}

fn bind_input_video_from_lua_script_node_with_sources(
    original_call: &InputVideoHookOriginalCall,
    source_handles: InputVideoNodeSourceHandles,
) -> Result<InputVideoBindUpdate, String> {
    let Some(node) = input_video_node_from_hook(original_call) else {
        set_last_error("input video hook has no current component context".to_string());
        return Ok(InputVideoBindUpdate {
            updated_slots: 0,
            skipped_fps_slots: 0,
        });
    };
    let Some(component) = component_context_from_lua_script_input_video_node(node)
        .or_else(|| component_context_from_input_video_sources(source_handles))
    else {
        record_input_video_unmapped_node_diagnostic(node, source_handles);
        return Ok(InputVideoBindUpdate {
            updated_slots: 0,
            skipped_fps_slots: 0,
        });
    };
    let update = bind_current_component_video_inputs(&component, source_handles)?;
    if update.updated_slots == 0 {
        record_input_video_no_slot_diagnostic(node, &component, source_handles);
    } else if component_context_from_lua_script_input_video_node(node).is_none() {
        let state = runtime_snapshot();
        log_runtime_diagnostic(
            &state,
            &format!(
                "input video source component mapped node=0x{node:x} component={} candidate={} selected={} resolved={} effective={} source_layouts={} slots={}",
                component,
                format_hex_or_zero(source_handles.candidate),
                format_hex_or_zero(source_handles.selected),
                format_hex_or_zero(source_handles.resolved),
                format_hex_or_zero(source_handles.effective()),
                format_input_video_source_layouts(node, source_handles),
                describe_slots(&state)
            ),
            &VIDEO_INIT_DIAGNOSTIC_COUNT,
            16,
        );
    }
    Ok(update)
}

fn record_input_video_unmapped_node_diagnostic(
    node: usize,
    source_handles: InputVideoNodeSourceHandles,
) {
    let state = runtime_snapshot();
    log_runtime_diagnostic_no_snapshot(
        &state,
        &format!(
            "input video hook skipped non-lua node=0x{node:x} candidate={} selected={} resolved={} effective={} source_layouts={}",
            format_hex_or_zero(source_handles.candidate),
            format_hex_or_zero(source_handles.selected),
            format_hex_or_zero(source_handles.resolved),
            format_hex_or_zero(source_handles.effective()),
            format_input_video_source_layouts(node, source_handles)
        ),
        &INPUT_VIDEO_UNMAPPED_NODE_DIAGNOSTIC_COUNT,
        16,
    );
}

fn record_input_video_no_slot_diagnostic(
    node: usize,
    component: &str,
    source_handles: InputVideoNodeSourceHandles,
) {
    let state = runtime_snapshot();
    log_runtime_diagnostic_no_snapshot(
        &state,
        &format!(
            "input video hook mapped node=0x{node:x} component={} but no initialized slot candidate={} selected={} resolved={} effective={} source_layouts={} slots={}",
            component,
            format_hex_or_zero(source_handles.candidate),
            format_hex_or_zero(source_handles.selected),
            format_hex_or_zero(source_handles.resolved),
            format_hex_or_zero(source_handles.effective()),
            format_input_video_source_layouts(node, source_handles),
            describe_slots(&state)
        ),
        &INPUT_VIDEO_NO_SLOT_DIAGNOSTIC_COUNT,
        16,
    );
}

fn input_video_source_from_hook(original_call: &InputVideoHookOriginalCall) -> usize {
    let ptr = match *original_call {
        InputVideoHookOriginalCall::Arg1(source)
        | InputVideoHookOriginalCall::Arg2(_, source)
        | InputVideoHookOriginalCall::Arg3(_, _, source)
        | InputVideoHookOriginalCall::Arg4(_, _, _, source) => source,
    };
    ptr as usize
}

fn input_video_node_from_hook(original_call: &InputVideoHookOriginalCall) -> Option<usize> {
    let node = match *original_call {
        InputVideoHookOriginalCall::Arg1(_) => ptr::null_mut(),
        InputVideoHookOriginalCall::Arg2(node, _)
        | InputVideoHookOriginalCall::Arg3(node, _, _)
        | InputVideoHookOriginalCall::Arg4(node, _, _, _) => node,
    };
    let node = node as usize;
    if node == 0 {
        None
    } else {
        Some(node)
    }
}

fn component_context_from_lua_script_input_video_node(node: usize) -> Option<String> {
    if !memory_range_is_readable(node as *const c_void, 0x38) {
        return None;
    }
    let context = node.checked_add(LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA)?;
    is_registered_game_lua_component_context(context)
        .then(|| format!("component_lua_context:{context:x}"))
}

fn component_context_from_input_video_sources(
    source_handles: InputVideoNodeSourceHandles,
) -> Option<String> {
    for (_, handle) in source_handles.handles() {
        if handle == 0 {
            continue;
        }
        if let Some(component) = component_context_from_input_video_source(handle) {
            return Some(component);
        }
    }
    None
}

fn component_context_from_input_video_source(source_handle: u64) -> Option<String> {
    if source_handle == 0 || !pointer_value_looks_process_address(source_handle) {
        return None;
    }
    let base = source_handle as usize;
    if !memory_range_is_readable(base as *const c_void, INPUT_VIDEO_SOURCE_LAYOUT_BYTES) {
        return None;
    }
    for offset in (0..INPUT_VIDEO_SOURCE_LAYOUT_BYTES).step_by(size_of::<usize>()) {
        let Some(value) = read_usize_field(base, offset) else {
            continue;
        };
        if is_registered_game_lua_component_context(value) {
            return Some(format!("component_lua_context:{value:x}"));
        }
    }
    None
}

fn lua_script_input_video_node_from_component(component: &str) -> Option<usize> {
    let context = component.strip_prefix("component_lua_context:")?;
    let context = usize::from_str_radix(context, 16).ok()?;
    if !is_registered_game_lua_component_context(context) {
        return None;
    }
    let node = context.checked_sub(LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA)?;
    if !memory_range_is_readable(node as *const c_void, 0x38) {
        return None;
    }
    Some(node)
}

fn is_registered_game_lua_component_context(context: usize) -> bool {
    game_lua_component_contexts_cell()
        .lock()
        .map(|contexts| contexts.values().any(|value| *value == context))
        .unwrap_or(false)
}

/// How long a component is considered "alive" after its last Lua callback. A live
/// microcontroller calls `video.*` (or any registered callback) every tick, so a component
/// that has not called back within this window has been despawned / edited away / reset.
/// This is the reliable liveness signal because the game never notifies us of teardown and a
/// dead Lua component context address stays in the registration map forever.
const COMPONENT_LIVENESS_WINDOW: Duration = Duration::from_millis(2000);

/// Record that `component` is currently executing Lua (called from every `video.*` callback
/// via `record_game_lua_callback`). Skips the synthetic/default and diagnostic keys.
fn mark_component_alive(component: &str) {
    if component == DEFAULT_COMPONENT || !component.starts_with("component_lua_context:") {
        return;
    }
    if let Ok(mut seen) = component_liveness_cell().lock() {
        seen.insert(component.to_string(), Instant::now());
        // Bound the map: drop entries far past the liveness window so repeated respawns
        // (each a new context address) cannot grow it without limit.
        if seen.len() > 64 {
            let now = Instant::now();
            seen.retain(|_, at| now.duration_since(*at) < Duration::from_secs(30));
        }
    }
}

/// True when the component has executed Lua within the liveness window. Non-context keys
/// (`__default__`, `lua_state:*`) are always treated as alive so diagnostics/tests are
/// unaffected.
fn component_is_alive(component: &str) -> bool {
    if component == DEFAULT_COMPONENT || !component.starts_with("component_lua_context:") {
        return true;
    }
    component_liveness_cell()
        .lock()
        .ok()
        .and_then(|seen| {
            seen.get(component)
                .map(|at| Instant::now().duration_since(*at) < COMPONENT_LIVENESS_WINDOW)
        })
        .unwrap_or(false)
}

fn lua_script_input_video_source_handles_for_component(
    component: &str,
) -> Option<InputVideoNodeSourceHandles> {
    let node = lua_script_input_video_node_from_component(component)?;
    let direct = input_video_source_handles_from_any(node as u64);
    if direct.effective() != 0 {
        return Some(direct);
    }
    recorded_input_video_source_handles_for_component(component)
}

#[derive(Debug, Clone, Copy, Default)]
struct InputVideoNodeSourceHandles {
    candidate: u64,
    selected: u64,
    resolved: u64,
    upstream: u64,
}

impl InputVideoNodeSourceHandles {
    fn effective(self) -> u64 {
        if self.upstream != 0 {
            self.upstream
        } else if self.selected != 0 {
            self.selected
        } else if self.resolved != 0 {
            self.resolved
        } else {
            self.candidate
        }
    }

    fn handles(self) -> [(&'static str, u64); 4] {
        [
            ("candidate", self.candidate),
            ("selected", self.selected),
            ("resolved", self.resolved),
            ("upstream", self.upstream),
        ]
    }
}

fn input_video_source_handles_from_node(node: usize) -> InputVideoNodeSourceHandles {
    let mut handles = InputVideoNodeSourceHandles {
        candidate: 0,
        selected: input_video_source_handle_at_offset(
            node,
            INPUT_VIDEO_NODE_SELECTED_SOURCE_OFFSET,
        )
        .unwrap_or(0),
        resolved: input_video_source_handle_at_offset(
            node,
            INPUT_VIDEO_NODE_RESOLVED_SOURCE_OFFSET,
        )
        .unwrap_or(0),
        upstream: 0,
    };
    handles.upstream = upstream_input_video_source_from_bridge_output(handles.effective());
    handles
}

fn input_video_source_handles_from_any(node: u64) -> InputVideoNodeSourceHandles {
    if node == 0 {
        return InputVideoNodeSourceHandles::default();
    }
    let recorded = runtime_snapshot().video_node_sources.get(&node).copied();
    let vtable_static = video_source_vtable_static(node);
    if vtable_static == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO) {
        return recorded.unwrap_or(InputVideoNodeSourceHandles {
            candidate: node,
            selected: 0,
            resolved: 0,
            upstream: 0,
        });
    }
    if vtable_static.is_some()
        && vtable_static != Some(VTABLE_MICROPROCESSOR_LOGIC_NODE_INPUT_VIDEO)
    {
        return recorded.unwrap_or_default();
    }
    let from_memory = input_video_source_handles_from_node(node as usize);
    if from_memory.effective() != 0 {
        let mut merged = from_memory;
        if let Some(recorded) = recorded {
            merged.candidate = recorded.candidate;
            if merged.upstream == 0 {
                merged.upstream = recorded.upstream;
            }
        }
        return merged;
    }
    recorded.unwrap_or_default()
}

fn upstream_input_video_source_from_bridge_output(source_handle: u64) -> u64 {
    if source_handle == 0 || !pointer_value_looks_process_address(source_handle) {
        return 0;
    }
    let source = source_handle as usize;
    if !memory_range_is_readable(source as *const c_void, INPUT_VIDEO_SOURCE_LAYOUT_BYTES) {
        return 0;
    }
    if video_source_vtable_static(source_handle)
        != Some(VTABLE_MICROPROCESSOR_LOGIC_NODE_OUTPUT_VIDEO)
    {
        return 0;
    }
    let Some(owner) = read_usize_field(source, 0x08) else {
        return 0;
    };
    if owner == 0 || !pointer_value_looks_process_address(owner as u64) {
        return 0;
    }
    let expected_output = owner
        .checked_add(MICROPROCESSOR_BRIDGE_VIDEO_OUTPUT_NODE_OFFSET)
        .unwrap_or(0);
    if expected_output != source {
        return 0;
    }
    let Some(input_node) = owner.checked_add(MICROPROCESSOR_BRIDGE_VIDEO_INPUT_NODE_OFFSET) else {
        return 0;
    };
    if !memory_range_is_readable(input_node as *const c_void, INPUT_VIDEO_SOURCE_LAYOUT_BYTES) {
        return 0;
    }
    let upstream = input_video_source_handles_from_node_raw(input_node).effective();
    if upstream == source_handle {
        0
    } else {
        upstream
    }
}

fn input_video_source_handles_from_node_raw(node: usize) -> InputVideoNodeSourceHandles {
    InputVideoNodeSourceHandles {
        candidate: 0,
        selected: input_video_source_handle_at_offset(
            node,
            INPUT_VIDEO_NODE_SELECTED_SOURCE_OFFSET,
        )
        .unwrap_or(0),
        resolved: input_video_source_handle_at_offset(
            node,
            INPUT_VIDEO_NODE_RESOLVED_SOURCE_OFFSET,
        )
        .unwrap_or(0),
        upstream: 0,
    }
}

fn video_source_vtable_static(source_handle: u64) -> Option<u64> {
    let base = source_handle as usize;
    let vtable = read_usize_field(base, 0)? as u64;
    runtime_to_static_va(vtable)
}

fn output_video_logic_kind(source_handle: u64) -> Option<u64> {
    if source_handle == 0
        || !pointer_value_looks_process_address(source_handle)
        || video_source_vtable_static(source_handle)
            != Some(VTABLE_MICROPROCESSOR_LOGIC_NODE_OUTPUT_VIDEO)
    {
        return None;
    }
    for offset in [0x38usize, 0x20] {
        let value = read_usize_field(source_handle as usize, offset)? as u64;
        let low = logic_video_ref_low(value);
        if low != 0 {
            return Some(low);
        }
    }
    None
}

fn output_video_component_marker(source_handle: u64) -> Option<usize> {
    if source_handle == 0
        || !pointer_value_looks_process_address(source_handle)
        || video_source_vtable_static(source_handle)
            != Some(VTABLE_MICROPROCESSOR_LOGIC_NODE_OUTPUT_VIDEO)
    {
        return None;
    }
    let marker = read_usize_field(source_handle as usize, 0x40)?;
    (marker != 0 && pointer_value_looks_process_address(marker as u64)).then_some(marker)
}

fn slot_upstream_source_handle(slot: &SlotState) -> u64 {
    if slot.input_upstream_source_handle != 0 {
        return slot.input_upstream_source_handle;
    }
    let bridge_upstream =
        upstream_input_video_source_from_bridge_output(slot.input_resolved_source_handle);
    if bridge_upstream != 0 {
        return bridge_upstream;
    }
    if video_source_vtable_static(slot.input_source_handle)
        == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO)
    {
        slot.input_source_handle
    } else {
        0
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, Default)]
struct MonitorVideoInputHandles {
    slot_object: u64,
    slot_ref: u64,
    object_handles: InputVideoNodeSourceHandles,
    ref_handles: InputVideoNodeSourceHandles,
}

#[cfg(windows)]
impl MonitorVideoInputHandles {
    fn effective(self) -> u64 {
        [
            self.object_handles.effective(),
            self.slot_object,
            self.ref_handles.effective(),
            self.slot_ref,
        ]
        .into_iter()
        .find(|value| *value != 0)
        .unwrap_or(0)
    }

    fn relation_handles(self) -> Vec<u64> {
        let mut handles = Vec::new();
        for value in [
            self.slot_object,
            self.object_handles.effective(),
            self.slot_ref,
            self.ref_handles.effective(),
        ] {
            if value != 0 && !handles.contains(&value) {
                handles.push(value);
            }
        }
        handles
    }

    fn summary(self) -> String {
        format!(
            "slot_object={} object_effective={} slot_ref={} ref_effective={} component_marker={}",
            format_hex_or_zero(self.slot_object),
            format_hex_or_zero(self.object_handles.effective()),
            format_hex_or_zero(self.slot_ref),
            format_hex_or_zero(self.ref_handles.effective()),
            monitor_input_component_marker_text(self.slot_object)
        )
    }

    fn diagnostic(self) -> String {
        format!(
            "{} object_layout={} ref_layout={}",
            self.summary(),
            compact_relation_handle_layout_text("slot_object", self.slot_object),
            compact_relation_handle_layout_text("slot_ref", self.slot_ref)
        )
    }
}

fn monitor_input_component_marker_text(slot_object: u64) -> String {
    let Some(marker) = monitor_input_component_marker(slot_object) else {
        return "none".to_string();
    };
    let registered = if is_registered_game_lua_component_context(marker) {
        "registered_lua"
    } else {
        "unregistered"
    };
    format!("{}:{}", format_hex_or_zero(marker as u64), registered)
}

fn monitor_input_component_marker(slot_object: u64) -> Option<usize> {
    if slot_object == 0 || !pointer_value_looks_process_address(slot_object) {
        return None;
    }
    if video_source_vtable_static(slot_object) != Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO) {
        return None;
    }
    let base = slot_object as usize;
    let marker = read_usize_field(base, 0x38)?;
    (marker != 0 && pointer_value_looks_process_address(marker as u64)).then_some(marker)
}

fn monitor_input_logic_kind(slot_object: u64) -> Option<u64> {
    if slot_object == 0
        || !pointer_value_looks_process_address(slot_object)
        || video_source_vtable_static(slot_object) != Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO)
    {
        return None;
    }
    read_usize_field(slot_object as usize, 0x30).map(|value| logic_video_ref_low(value as u64))
}

#[cfg(windows)]
fn monitor_video_input_handles(monitor: usize) -> MonitorVideoInputHandles {
    if monitor == 0 {
        return MonitorVideoInputHandles::default();
    }
    let slot_ref = read_usize_field(monitor, MONITOR_VIDEO_INPUT_SLOT_REF_OFFSET)
        .map(|value| value as u64)
        .unwrap_or(0);
    let embedded_slot = monitor
        .checked_add(MONITOR_VIDEO_INPUT_SLOT_OBJECT_OFFSET)
        .map(|value| value as u64)
        .unwrap_or(0);
    let slot_object = if video_source_vtable_static(embedded_slot)
        == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO)
    {
        embedded_slot
    } else {
        read_usize_field(monitor, MONITOR_VIDEO_INPUT_SLOT_OBJECT_OFFSET)
            .map(|value| value as u64)
            .filter(|value| {
                video_source_vtable_static(*value) == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO)
            })
            .unwrap_or(0)
    };
    MonitorVideoInputHandles {
        slot_object,
        slot_ref,
        object_handles: input_video_source_handles_from_any(slot_object),
        ref_handles: input_video_source_handles_from_any(slot_ref),
    }
}

fn recorded_input_video_source_handles_for_component(
    component: &str,
) -> Option<InputVideoNodeSourceHandles> {
    let state = runtime_snapshot();
    state
        .video_node_sources
        .iter()
        .filter_map(|(node, handles)| {
            let node_component = component_context_from_lua_script_input_video_node(*node as usize);
            let source_component = component_context_from_input_video_sources(*handles)
                .or_else(|| registered_component_for_video_source_handle(handles.effective()));
            (node_component.as_deref() == Some(component)
                || source_component.as_deref() == Some(component))
            .then_some(*handles)
        })
        .max_by_key(|handles| input_video_source_handles_component_rank(component, *handles))
}

fn input_video_source_handles_component_rank(
    component: &str,
    handles: InputVideoNodeSourceHandles,
) -> usize {
    let effective = handles.effective();
    if effective == 0 {
        return 0;
    }
    let mut rank = 1usize;
    if memory_range_is_readable(
        effective as *const c_void,
        INPUT_VIDEO_SOURCE_LAYOUT_BYTES.min(0x48),
    ) {
        rank = rank.saturating_add(8);
    }
    if video_source_structural_values(effective).len() >= 3 {
        rank = rank.saturating_add(32);
    }
    if registered_component_for_video_source_handle(effective).as_deref() == Some(component) {
        rank = rank.saturating_add(48);
    }
    if component_context_from_input_video_source(effective).as_deref() == Some(component) {
        rank = rank.saturating_add(96);
    }
    rank
}

fn registered_component_for_video_source_handle(handle: u64) -> Option<String> {
    if handle == 0 {
        return None;
    }
    let state = runtime_snapshot();
    if let Some(component) = state.video_source_components.get(&handle) {
        return Some(component.clone());
    }
    let key = video_source_handle_structural_key(handle)?;
    state.video_source_components.get(&key).cloned()
}

fn slot_matches_input_handle_or_source_key(slot: &SlotState, handle: u64) -> bool {
    if slot_matches_input_handle(slot, handle) {
        return true;
    }
    let Some(handle_key) = video_source_handle_structural_key(handle) else {
        return false;
    };
    slot_input_handles(slot)
        .into_iter()
        .any(|(_, slot_handle)| {
            video_source_handle_structural_key(slot_handle)
                .is_some_and(|slot_key| slot_key == handle_key)
        })
}

fn format_input_video_source_layouts(
    node: usize,
    source_handles: InputVideoNodeSourceHandles,
) -> String {
    let mut parts = Vec::new();
    parts.push(input_video_source_debug_layout_text("node", node as u64));
    for (label, handle) in source_handles.handles() {
        if handle != 0 {
            parts.push(input_video_source_debug_layout_text(label, handle));
        }
    }
    parts.join(" | ")
}

fn input_video_source_debug_layout(address: u64) -> serde_json::Value {
    if address == 0 || !pointer_value_looks_process_address(address) {
        return serde_json::json!(null);
    }
    let readable =
        memory_range_is_readable(address as *const c_void, INPUT_VIDEO_SOURCE_LAYOUT_BYTES);
    if !readable {
        return serde_json::json!({
            "address": format_hex_or_zero(address),
            "readable": false
        });
    }
    let base = address as usize;
    let vtable = read_usize_field(base, 0).unwrap_or(0) as u64;
    let qwords = (0..INPUT_VIDEO_SOURCE_LAYOUT_BYTES)
        .step_by(size_of::<usize>())
        .filter_map(|offset| {
            read_usize_field(base, offset).map(|value| {
                serde_json::json!({
                    "offset": format!("0x{offset:x}"),
                    "value": format_hex_or_zero(value as u64)
                })
            })
        })
        .collect::<Vec<_>>();
    let structural_values = video_source_structural_values(address);
    let structural_key = video_source_structural_key_from_values(&structural_values);
    serde_json::json!({
        "address": format_hex_or_zero(address),
        "readable": true,
        "structural_key": structural_key.map(format_hex_or_zero),
        "structural_value_count": structural_values.len(),
        "vtable": format_hex_or_zero(vtable),
        "vtable_static": runtime_to_static_va(vtable).map(format_hex_or_zero),
        "qwords": qwords
    })
}

fn input_video_source_debug_layout_text(label: &str, address: u64) -> String {
    if address == 0 {
        return format!("{label}=0");
    }
    if !pointer_value_looks_process_address(address)
        || !memory_range_is_readable(address as *const c_void, INPUT_VIDEO_SOURCE_LAYOUT_BYTES)
    {
        return format!("{label}={} unreadable", format_hex_or_zero(address));
    }
    let base = address as usize;
    let vtable = read_usize_field(base, 0).unwrap_or(0) as u64;
    let vtable_static_value = runtime_to_static_va(vtable);
    let vtable_static = vtable_static_value
        .map(format_hex_or_zero)
        .unwrap_or_else(|| "unknown".to_string());
    let fields = (0..INPUT_VIDEO_SOURCE_LAYOUT_BYTES)
        .step_by(size_of::<usize>())
        .filter_map(|offset| {
            read_usize_field(base, offset)
                .map(|value| format!("+0x{offset:x}={}", format_hex_or_zero(value as u64)))
        })
        .take(16)
        .collect::<Vec<_>>()
        .join(",");
    let structural_values = video_source_structural_values(address);
    let structural_key = video_source_structural_key_from_values(&structural_values)
        .map(format_hex_or_zero)
        .unwrap_or_else(|| "none".to_string());
    let nested = if vtable_static_value == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO) {
        format!(
            " nested=[{}]",
            input_video_source_nested_layout_text(base, 6, 8)
        )
    } else {
        String::new()
    };
    format!(
        "{label}={} vtable={} static={} key={} values={} fields=[{}]{}",
        format_hex_or_zero(address),
        format_hex_or_zero(vtable),
        vtable_static,
        structural_key,
        structural_values.len(),
        fields,
        nested
    )
}

fn log_video_node_registry_diagnostic(reason: &str) {
    if !verbose_runtime_diagnostics_enabled() {
        return;
    }
    let state = runtime_snapshot();
    if state.video_node_sources.is_empty() && state.slots.is_empty() {
        return;
    }
    let (counter, limit) = if reason == "video.init" {
        (&VIDEO_NODE_INIT_REGISTRY_DIAGNOSTIC_COUNT, 16)
    } else {
        (&VIDEO_NODE_REGISTRY_DIAGNOSTIC_COUNT, 8)
    };
    let should_log = counter.fetch_add(1, Ordering::Relaxed) < limit;
    if !should_log {
        return;
    }
    if let Some(path) = &state.log_path {
        let _ = append_log(
            path,
            &format!(
                "video node registry reason={} nodes=[{}] slots=[{}] components=[{}]",
                reason,
                video_node_registry_summary(&state),
                lua_slot_registry_summary(&state),
                registered_lua_component_contexts_summary()
            ),
        );
    }
}

fn video_node_registry_summary(state: &RuntimeState) -> String {
    if state.video_node_sources.is_empty() {
        return "none".to_string();
    }
    state
        .video_node_sources
        .iter()
        .take(16)
        .map(|(node, handles)| {
            let owner = read_usize_field(*node as usize, 0x08).unwrap_or(0) as u64;
            let node_component = component_context_from_lua_script_input_video_node(*node as usize)
                .or_else(|| component_context_from_input_video_sources(*handles))
                .or_else(|| registered_component_for_video_source_handle(handles.effective()))
                .unwrap_or_else(|| "none".to_string());
            format!(
                "node={} component={} owner={} local={} handles=[{}] node_fields=[{}] owner_refs=[{}] candidate_refs=[{}]",
                format_hex_or_zero(*node),
                node_component,
                format_hex_or_zero(owner),
                video_logic_ref_low_at(*node, 0x18),
                format_input_video_handles_compact(*handles),
                compact_input_node_fields(*node),
                pointer_relation_refs_summary(owner),
                pointer_relation_refs_summary(handles.effective())
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn lua_slot_registry_summary(state: &RuntimeState) -> String {
    if state.slots.is_empty() {
        return "none".to_string();
    }
    state
        .slots
        .values()
        .take(8)
        .map(|slot| {
            let node = lua_script_input_video_node_from_component(&slot.component)
                .map(|node| format_hex_or_zero(node as u64))
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{}:{} node={} connected={} ready={} handles=[{}] node_fields=[{}]",
                slot.component,
                slot.slot,
                node,
                slot.connected,
                is_slot_ready_for_lua(slot),
                format_slot_input_handles(slot),
                lua_script_input_video_node_from_component(&slot.component)
                    .map(|node| compact_input_node_fields(node as u64))
                    .unwrap_or_else(|| "none".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

fn registered_lua_component_contexts_summary() -> String {
    game_lua_component_contexts_cell()
        .lock()
        .map(|contexts| {
            if contexts.is_empty() {
                return "none".to_string();
            }
            contexts
                .iter()
                .take(12)
                .map(|(lua_state, context)| {
                    format!(
                        "{}->{}",
                        format_hex_or_zero(*lua_state as u64),
                        format_hex_or_zero(*context as u64)
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_else(|_| "poisoned".to_string())
}

fn format_input_video_handles_compact(handles: InputVideoNodeSourceHandles) -> String {
    handles
        .handles()
        .into_iter()
        .filter(|(_, handle)| *handle != 0)
        .map(|(label, handle)| {
            format!(
                "{}={} static={} refs=[{}]",
                label,
                format_hex_or_zero(handle),
                video_source_vtable_static(handle)
                    .map(format_hex_or_zero)
                    .unwrap_or_else(|| "unknown".to_string()),
                pointer_relation_refs_summary(handle)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn compact_input_node_fields(address: u64) -> String {
    if address == 0 || !pointer_value_looks_process_address(address) {
        return "unreadable".to_string();
    }
    let base = address as usize;
    if !memory_range_is_readable(base as *const c_void, size_of::<usize>()) {
        return "unreadable".to_string();
    }
    [
        0x08usize, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x48, 0x50, 0x58,
    ]
    .into_iter()
    .filter_map(|offset| {
        read_usize_field(base, offset).map(|value| {
            format!(
                "+0x{offset:x}={}({})",
                format_hex_or_zero(value as u64),
                describe_logic_video_ref(value as u64)
            )
        })
    })
    .collect::<Vec<_>>()
    .join(",")
}

fn pointer_relation_refs_summary(address: u64) -> String {
    if address == 0 || !pointer_value_looks_process_address(address) {
        return "none".to_string();
    }
    let base = address as usize;
    if !memory_range_is_readable(base as *const c_void, size_of::<usize>()) {
        return "unreadable".to_string();
    }
    let mut parts = Vec::new();
    for offset in (0..VIDEO_NODE_RELATION_SCAN_BYTES).step_by(size_of::<usize>()) {
        if parts.len() >= 14 {
            break;
        }
        let Some(value) = read_usize_field(base, offset).map(|value| value as u64) else {
            continue;
        };
        let low = logic_video_ref_low(value);
        let high = value >> 32;
        let is_interesting_low = low == 0xb
            || low == LOGIC_KIND_EXTERNAL_VIDEO_INPUT
            || low == LOGIC_KIND_LUA_VIDEO_OUTPUT;
        let is_registered_context = (value != 0)
            && pointer_value_looks_process_address(value)
            && is_registered_game_lua_component_context(value as usize);
        let is_static_video_vtable = matches!(
            runtime_to_static_va(value),
            Some(VTABLE_MICROPROCESSOR_LOGIC_NODE_INPUT_VIDEO)
                | Some(VTABLE_MICROPROCESSOR_LOGIC_NODE_OUTPUT_VIDEO)
                | Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO)
        );
        if is_interesting_low || is_registered_context || is_static_video_vtable {
            parts.push(format!(
                "+0x{offset:x}={}({})",
                format_hex_or_zero(value),
                if is_registered_context {
                    "registered_lua".to_string()
                } else if is_static_video_vtable {
                    runtime_to_static_va(value)
                        .map(|static_va| format!("static_vtable={}", format_hex_or_zero(static_va)))
                        .unwrap_or_else(|| describe_logic_video_ref(value))
                } else {
                    format!("high=0x{high:x} low=0x{low:x}")
                }
            ));
        }
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(",")
    }
}

fn video_logic_ref_low_at(address: u64, offset: usize) -> String {
    if address == 0 || !pointer_value_looks_process_address(address) {
        return "none".to_string();
    }
    read_usize_field(address as usize, offset)
        .map(|value| format!("0x{:x}", logic_video_ref_low(value as u64)))
        .unwrap_or_else(|| "unreadable".to_string())
}

fn input_video_source_nested_layout_text(
    base: usize,
    pointer_limit: usize,
    qwords_per_pointer: usize,
) -> String {
    let mut parts = Vec::new();
    let mut seen = BTreeSet::new();
    for offset in (0..INPUT_VIDEO_SOURCE_LAYOUT_BYTES).step_by(size_of::<usize>()) {
        if parts.len() >= pointer_limit {
            break;
        }
        let Some(pointer) = read_usize_field(base, offset).map(|value| value as u64) else {
            continue;
        };
        if !pointer_value_looks_process_address(pointer) || pointer == base as u64 {
            continue;
        }
        if !seen.insert(pointer) {
            continue;
        }
        let Some(summary) = pointer_qword_summary(pointer, qwords_per_pointer) else {
            continue;
        };
        parts.push(format!(
            "+0x{offset:x}->{}:{}",
            format_hex_or_zero(pointer),
            summary
        ));
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join("|")
    }
}

fn log_input_video_source_layout_diagnostic(
    node: usize,
    source_handles: InputVideoNodeSourceHandles,
) {
    let state = runtime_snapshot();
    log_runtime_diagnostic_no_snapshot(
        &state,
        &format!(
            "input video source layout {}",
            format_input_video_source_layouts(node, source_handles)
        ),
        &INPUT_VIDEO_SOURCE_LAYOUT_DIAGNOSTIC_COUNT,
        24,
    );
}

fn describe_input_video_source_collection(collection: usize) -> String {
    if collection == 0 {
        return "collection=0".to_string();
    }
    if !memory_range_is_readable(collection as *const c_void, 0x18) {
        return format!(
            "collection={} unreadable",
            format_hex_or_zero(collection as u64)
        );
    }
    let buffer = read_usize_field(collection, 0).unwrap_or(0);
    let capacity = read_u32_field(collection, 0x8).unwrap_or(0);
    let start = read_u32_field(collection, 0xc).unwrap_or(0);
    let count = read_u32_field(collection, 0x10).unwrap_or(0);
    let mut entries = Vec::new();
    if buffer != 0 && capacity != 0 && count != 0 {
        let limit = count.min(4);
        for index in 0..limit {
            let wrapped_index = (start.wrapping_add(index) % capacity) as usize;
            let offset = wrapped_index.saturating_mul(size_of::<usize>());
            if let Some(entry) = read_usize_field(buffer, offset) {
                entries.push(format!("{index}:{}", format_hex_or_zero(entry as u64)));
            }
        }
    }
    format!(
        "collection={} buffer={} capacity={} start={} count={} entries=[{}]",
        format_hex_or_zero(collection as u64),
        format_hex_or_zero(buffer as u64),
        capacity,
        start,
        count,
        entries.join(",")
    )
}

fn runtime_to_static_va(runtime_address: u64) -> Option<u64> {
    if runtime_address == 0 {
        return None;
    }
    #[cfg(windows)]
    {
        let runtime_base = current_process_module_base()?;
        let rva = runtime_address.checked_sub(runtime_base)?;
        Some(STORMWORKS_IMAGE_BASE + rva)
    }
    #[cfg(not(windows))]
    {
        Some(runtime_address)
    }
}

fn input_video_source_handle_at_offset(node: usize, offset: usize) -> Option<u64> {
    read_input_video_source_handle_at_offset(node, offset)
        .and_then(|value| (value != 0).then_some(value))
}

fn read_input_video_source_handle_at_offset(node: usize, offset: usize) -> Option<u64> {
    if node == 0 {
        return None;
    }
    let slot = node.checked_add(offset)?;
    if !memory_range_is_readable(slot as *const c_void, size_of::<usize>()) {
        return None;
    }
    let value = unsafe { *(slot as *const usize) };
    Some(value as u64)
}

#[derive(Debug, Clone, Copy)]
struct InputVideoBindUpdate {
    updated_slots: usize,
    skipped_fps_slots: usize,
}

fn bind_current_component_video_inputs(
    component: &str,
    source_handles: InputVideoNodeSourceHandles,
) -> Result<InputVideoBindUpdate, String> {
    let component = normalize_component(Some(component));
    let mut state = request_runtime_state()?;
    let source_handle = source_handles.effective();
    let connected = source_handle != 0;
    let mut updated_slots = 0usize;
    let mut updated_slot_ids = Vec::new();
    for slot in state.slots.values_mut() {
        if slot.component == component {
            let previous_source = slot.input_source_handle;
            let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
            slot.frame_id = frame_id;
            slot.connected = connected;
            slot.input_source_handle = if connected { source_handle } else { 0 };
            slot.input_candidate_source_handle = if connected {
                source_handles.candidate
            } else {
                0
            };
            slot.input_selected_source_handle = if connected {
                source_handles.selected
            } else {
                0
            };
            slot.input_resolved_source_handle = if connected {
                source_handles.resolved
            } else {
                0
            };
            slot.input_upstream_source_handle = if connected {
                source_handles.upstream
            } else {
                0
            };
            // Only invalidate a captured frame when the video source genuinely switches to a
            // DIFFERENT non-zero camera. A transient `source=0` (the input-node hook fires every
            // frame in busy multi-camera vehicles and momentarily reads a null source) must NOT
            // clear the frame: doing so races the render-thread capture and leaves the slot stuck
            // at ready=false even though additive_monitor_video keeps writing valid frames. The
            // slot stays `connected=false` for that tick, but its last good frame/ready survive so
            // the next capture (or the very next node update that re-reads the real source) keeps
            // video.isReady() stable instead of flickering.
            let switched_to_different_source =
                connected && previous_source != 0 && previous_source != source_handle;
            if switched_to_different_source {
                slot.ready = false;
                slot.latest_frame = None;
                slot.texture_upload_handle = None;
                slot.source_texture_handle = None;
                slot.last_texture_upload_at = None;
            }
            updated_slot_ids.push(slot.slot);
            updated_slots += 1;
        }
    }
    if connected {
        for slot_id in updated_slot_ids {
            let _ = apply_latest_texture_upload_frame_to_slot(&mut state, &component, slot_id);
        }
    }
    if updated_slots > 0 {
        state.hook_runtime.input_video_bridge_updates = state
            .hook_runtime
            .input_video_bridge_updates
            .saturating_add(updated_slots as u64);
        log_runtime_diagnostic(
            &state,
            &format!(
                "video input component bind component={} source={} candidate_source={} selected_source={} resolved_source={} upstream_source={} connected={} updated_slots={} source_layouts={} slots={}",
                component,
                format_hex_or_zero(source_handle),
                format_hex_or_zero(source_handles.candidate),
                format_hex_or_zero(source_handles.selected),
                format_hex_or_zero(source_handles.resolved),
                format_hex_or_zero(source_handles.upstream),
                connected,
                updated_slots,
                format_input_video_source_layouts(0, source_handles),
                describe_slots(&state)
            ),
            &VIDEO_INIT_DIAGNOSTIC_COUNT,
            16,
        );
        set_runtime(state);
    }
    Ok(InputVideoBindUpdate {
        updated_slots,
        skipped_fps_slots: 0,
    })
}

fn call_input_video_original(original_call: InputVideoHookOriginalCall) -> Result<i32, String> {
    unsafe {
        match original_call {
            InputVideoHookOriginalCall::Arg1(source) => {
                let trampoline = INPUT_VIDEO_ORIGINAL_ARG1.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void) -> i32 = std::mem::transmute(trampoline);
                Ok(original(source))
            }
            InputVideoHookOriginalCall::Arg2(arg1, source) => {
                let trampoline = INPUT_VIDEO_ORIGINAL_ARG2.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(original(arg1, source))
            }
            InputVideoHookOriginalCall::Arg3(arg1, arg2, source) => {
                let trampoline = INPUT_VIDEO_ORIGINAL_ARG3.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(original(arg1, arg2, source))
            }
            InputVideoHookOriginalCall::Arg4(arg1, arg2, arg3, source) => {
                let trampoline = INPUT_VIDEO_ORIGINAL_ARG4.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) -> i32 = std::mem::transmute(trampoline);
                Ok(original(arg1, arg2, arg3, source))
            }
        }
    }
}

fn set_input_video_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    match replacement {
        "stormworks_video_get_input_video_hook_arg1" => {
            INPUT_VIDEO_ORIGINAL_ARG1.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_input_video_hook_arg2" => {
            INPUT_VIDEO_ORIGINAL_ARG2.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_input_video_hook_arg3" => {
            INPUT_VIDEO_ORIGINAL_ARG3.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_input_video_hook_arg4" => {
            INPUT_VIDEO_ORIGINAL_ARG4.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_input_video_node_update_hook_arg2" => {
            INPUT_VIDEO_NODE_UPDATE_ORIGINAL_ARG2.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_input_video_node_select_hook_arg5" => {
            INPUT_VIDEO_NODE_SELECT_ORIGINAL_ARG5.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_video_output_slot_add_hook_arg2" => {
            VIDEO_OUTPUT_SLOT_ADD_ORIGINAL_ARG2.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_video_output_slot_remove_hook_arg2" => {
            VIDEO_OUTPUT_SLOT_REMOVE_ORIGINAL_ARG2.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_video_output_slot_clear_hook_arg1" => {
            VIDEO_OUTPUT_SLOT_CLEAR_ORIGINAL_ARG1.store(value, Ordering::SeqCst)
        }
        _ => {}
    }
}

fn input_video_original_trampoline_status() -> serde_json::Value {
    serde_json::json!({
        "arg1": INPUT_VIDEO_ORIGINAL_ARG1.load(Ordering::SeqCst) != 0,
        "arg2": INPUT_VIDEO_ORIGINAL_ARG2.load(Ordering::SeqCst) != 0,
        "arg3": INPUT_VIDEO_ORIGINAL_ARG3.load(Ordering::SeqCst) != 0,
        "arg4": INPUT_VIDEO_ORIGINAL_ARG4.load(Ordering::SeqCst) != 0,
        "node_update_arg2": INPUT_VIDEO_NODE_UPDATE_ORIGINAL_ARG2.load(Ordering::SeqCst) != 0,
        "node_select_arg5": INPUT_VIDEO_NODE_SELECT_ORIGINAL_ARG5.load(Ordering::SeqCst) != 0,
        "video_output_slot_add_arg2": VIDEO_OUTPUT_SLOT_ADD_ORIGINAL_ARG2.load(Ordering::SeqCst) != 0,
        "video_output_slot_remove_arg2": VIDEO_OUTPUT_SLOT_REMOVE_ORIGINAL_ARG2.load(Ordering::SeqCst) != 0,
        "video_output_slot_clear_arg1": VIDEO_OUTPUT_SLOT_CLEAR_ORIGINAL_ARG1.load(Ordering::SeqCst) != 0
    })
}

fn video_output_slot_add_from_hook_chained(
    output_slot: *mut c_void,
    input_slot: *mut c_void,
) -> Result<(), String> {
    call_video_output_slot_original_arg2(
        &VIDEO_OUTPUT_SLOT_ADD_ORIGINAL_ARG2,
        output_slot,
        input_slot,
    );
    record_video_logic_edge("add", output_slot as usize, input_slot as usize);
    Ok(())
}

fn video_output_slot_remove_from_hook_chained(
    output_slot: *mut c_void,
    input_slot: *mut c_void,
) -> Result<(), String> {
    call_video_output_slot_original_arg2(
        &VIDEO_OUTPUT_SLOT_REMOVE_ORIGINAL_ARG2,
        output_slot,
        input_slot,
    );
    remove_video_logic_edge(output_slot as usize, input_slot as usize);
    Ok(())
}

fn video_output_slot_clear_from_hook_chained(output_slot: *mut c_void) -> Result<(), String> {
    let trampoline = VIDEO_OUTPUT_SLOT_CLEAR_ORIGINAL_ARG1.load(Ordering::SeqCst);
    if trampoline != 0 {
        let original: extern "C" fn(*mut c_void) = unsafe { std::mem::transmute(trampoline) };
        original(output_slot);
    }
    clear_video_logic_output(output_slot as usize);
    Ok(())
}

fn call_video_output_slot_original_arg2(
    trampoline: &AtomicUsize,
    output_slot: *mut c_void,
    input_slot: *mut c_void,
) {
    let trampoline = trampoline.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { std::mem::transmute(trampoline) };
    original(output_slot, input_slot);
}

fn video_logic_edge_types_are_valid(output_slot: usize, input_slot: usize) -> bool {
    output_slot != 0
        && input_slot != 0
        && video_source_vtable_static(output_slot as u64)
            == Some(VTABLE_VEHICLE_LOGIC_SLOT_OUTPUT_VIDEO)
        && video_source_vtable_static(input_slot as u64)
            == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO)
}

fn record_video_logic_edge(action: &'static str, output_slot: usize, input_slot: usize) {
    if !video_logic_edge_types_are_valid(output_slot, input_slot) {
        return;
    }
    let (changed, edge_count) = if let Ok(mut edges) = VIDEO_INPUT_TO_OUTPUT_EDGES.lock() {
        let changed = edges.insert(input_slot, output_slot) != Some(output_slot);
        (changed, edges.len())
    } else {
        return;
    };
    log_video_logic_edge(action, output_slot, input_slot, changed, edge_count);
}

fn remove_video_logic_edge(output_slot: usize, input_slot: usize) {
    if !video_logic_edge_types_are_valid(output_slot, input_slot) {
        return;
    }
    let (changed, edge_count) = if let Ok(mut edges) = VIDEO_INPUT_TO_OUTPUT_EDGES.lock() {
        let changed = edges.get(&input_slot).copied() == Some(output_slot);
        if changed {
            edges.remove(&input_slot);
        }
        (changed, edges.len())
    } else {
        return;
    };
    log_video_logic_edge("remove", output_slot, input_slot, changed, edge_count);
}

fn clear_video_logic_output(output_slot: usize) {
    if video_source_vtable_static(output_slot as u64)
        != Some(VTABLE_VEHICLE_LOGIC_SLOT_OUTPUT_VIDEO)
    {
        return;
    }
    let (removed, edge_count) = if let Ok(mut edges) = VIDEO_INPUT_TO_OUTPUT_EDGES.lock() {
        let before = edges.len();
        edges.retain(|_, mapped_output| *mapped_output != output_slot);
        (before.saturating_sub(edges.len()), edges.len())
    } else {
        return;
    };
    log_video_logic_edge("clear", output_slot, 0, removed > 0, edge_count);
}

fn log_video_logic_edge(
    action: &'static str,
    output_slot: usize,
    input_slot: usize,
    changed: bool,
    edge_count: usize,
) {
    let Ok(state) = runtime_cell().lock() else {
        return;
    };
    if !state.configured {
        return;
    }
    log_runtime_diagnostic_no_snapshot(
        &state,
        &format!(
            "video logic edge action={} output={} input={} changed={} edges={}",
            action,
            format_hex_or_zero(output_slot as u64),
            format_hex_or_zero(input_slot as u64),
            changed,
            edge_count
        ),
        &VIDEO_LOGIC_EDGE_DIAGNOSTIC_COUNT,
        96,
    );
}

fn video_logic_output_for_input(input_slot: u64) -> Option<usize> {
    if video_source_vtable_static(input_slot) != Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO) {
        return None;
    }
    let edges = VIDEO_INPUT_TO_OUTPUT_EDGES.lock().ok()?;
    let output = graph_output_for_input(&edges, input_slot as usize)?;
    (video_source_vtable_static(output as u64) == Some(VTABLE_VEHICLE_LOGIC_SLOT_OUTPUT_VIDEO))
        .then_some(output)
}

fn graph_output_for_input(edges: &BTreeMap<usize, usize>, input_slot: usize) -> Option<usize> {
    edges.get(&input_slot).copied()
}

#[cfg(test)]
fn graph_inputs_share_output(
    edges: &BTreeMap<usize, usize>,
    first_input: usize,
    second_input: usize,
) -> bool {
    graph_output_for_input(edges, first_input)
        .zip(graph_output_for_input(edges, second_input))
        .is_some_and(|(first, second)| first == second)
}

fn video_logic_edge_count() -> usize {
    VIDEO_INPUT_TO_OUTPUT_EDGES
        .lock()
        .map(|edges| edges.len())
        .unwrap_or(0)
}

fn call_input_video_node_update_original_arg2(node: *mut c_void, source: *mut c_void) {
    let trampoline = INPUT_VIDEO_NODE_UPDATE_ORIGINAL_ARG2.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { std::mem::transmute(trampoline) };
    original(node, source);
}

fn call_input_video_node_select_original_arg5(
    node: *mut c_void,
    collection: *mut c_void,
    arg3: *mut c_void,
    arg4: *mut c_void,
    selected_index: *mut c_void,
) {
    let trampoline = INPUT_VIDEO_NODE_SELECT_ORIGINAL_ARG5.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) =
        unsafe { std::mem::transmute(trampoline) };
    original(node, collection, arg3, arg4, selected_index);
}

#[derive(Debug, Clone, Copy)]
enum TextureSourceHookOriginalCall {
    Arg1(*mut c_void),
    Arg2(*mut c_void, *mut c_void),
    Arg3(*mut c_void, *mut c_void, *mut c_void),
    Arg4(*mut c_void, *mut c_void, *mut c_void, *mut c_void),
}

fn texture_source_from_hook_chained(
    original_call: TextureSourceHookOriginalCall,
) -> Result<i32, String> {
    let source = texture_source_from_hook(&original_call);
    let result = call_texture_source_original(original_call)?;
    let update = push_texture_source_frames(source as u64)?;
    if update.updated_slots == 0 {
        set_last_error(format!(
            "texture source hook found no connected capture requests for source 0x{source:x}"
        ));
    }
    Ok(result)
}

fn texture_source_from_hook(original_call: &TextureSourceHookOriginalCall) -> usize {
    let ptr = match *original_call {
        TextureSourceHookOriginalCall::Arg1(source)
        | TextureSourceHookOriginalCall::Arg2(_, source)
        | TextureSourceHookOriginalCall::Arg3(_, _, source)
        | TextureSourceHookOriginalCall::Arg4(_, _, _, source) => source,
    };
    ptr as usize
}

fn push_texture_source_frames(source_handle: u64) -> Result<InputVideoBindUpdate, String> {
    if source_handle == 0 {
        return Ok(InputVideoBindUpdate {
            updated_slots: 0,
            skipped_fps_slots: 0,
        });
    }
    let state = request_runtime_state()?;
    let requests = state
        .slots
        .values()
        .filter(|slot| slot.connected && slot.input_source_handle == source_handle)
        .map(|slot| (slot.component.clone(), capture_request_from_slot(slot)))
        .collect::<Vec<_>>();
    let mut updated_slots = 0usize;
    for (component, request) in requests {
        if request.width == 0 || request.height == 0 {
            continue;
        }
        let frame_id = FRAME_ID.load(Ordering::Relaxed);
        let rgb = texture_source_rgb_frame(
            frame_id,
            source_handle,
            &component,
            request.slot,
            request.width,
            request.height,
        );
        let bytes = flatten_rgb_pixels(&rgb);
        push_rgb_frame_for_capture_request_with_source(
            request.component_hash,
            request.slot,
            request.width,
            request.height,
            bytes.as_ptr(),
            bytes.len(),
            1,
            "texture_source",
        )?;
        updated_slots += 1;
    }
    if updated_slots > 0 {
        let mut state = request_runtime_state()?;
        state.hook_runtime.texture_source_bridge_frames = state
            .hook_runtime
            .texture_source_bridge_frames
            .saturating_add(updated_slots as u64);
        set_runtime(state);
    }
    Ok(InputVideoBindUpdate {
        updated_slots,
        skipped_fps_slots: 0,
    })
}

fn texture_source_rgb_frame(
    frame_id: u64,
    source_handle: u64,
    component: &str,
    slot: u32,
    width: u32,
    height: u32,
) -> Vec<[u8; 3]> {
    let seed = component.bytes().fold(
        (source_handle as u32)
            .wrapping_add((source_handle >> 32) as u32)
            .wrapping_add(slot.wrapping_mul(97))
            .wrapping_add(frame_id as u32),
        |acc, byte| acc.wrapping_mul(16777619).wrapping_add(byte as u32),
    );
    let mut rgb = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            rgb.push([
                ((x.wrapping_mul(11).wrapping_add(seed)) & 0xff) as u8,
                ((y.wrapping_mul(13).wrapping_add(seed >> 5)) & 0xff) as u8,
                ((x.wrapping_add(y).wrapping_mul(17).wrapping_add(seed >> 11)) & 0xff) as u8,
            ]);
        }
    }
    rgb
}

fn call_texture_source_original(
    original_call: TextureSourceHookOriginalCall,
) -> Result<i32, String> {
    unsafe {
        match original_call {
            TextureSourceHookOriginalCall::Arg1(source) => {
                let trampoline = TEXTURE_SOURCE_ORIGINAL_ARG1.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void) -> i32 = std::mem::transmute(trampoline);
                Ok(original(source))
            }
            TextureSourceHookOriginalCall::Arg2(arg1, source) => {
                let trampoline = TEXTURE_SOURCE_ORIGINAL_ARG2.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(original(arg1, source))
            }
            TextureSourceHookOriginalCall::Arg3(arg1, arg2, source) => {
                let trampoline = TEXTURE_SOURCE_ORIGINAL_ARG3.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 =
                    std::mem::transmute(trampoline);
                Ok(original(arg1, arg2, source))
            }
            TextureSourceHookOriginalCall::Arg4(arg1, arg2, arg3, source) => {
                let trampoline = TEXTURE_SOURCE_ORIGINAL_ARG4.load(Ordering::SeqCst);
                if trampoline == 0 {
                    return Ok(1);
                }
                let original: extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                ) -> i32 = std::mem::transmute(trampoline);
                Ok(original(arg1, arg2, arg3, source))
            }
        }
    }
}

fn set_texture_source_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    match replacement {
        "stormworks_video_get_texture_source_hook_arg1" => {
            TEXTURE_SOURCE_ORIGINAL_ARG1.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_texture_source_hook_arg2" => {
            TEXTURE_SOURCE_ORIGINAL_ARG2.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_texture_source_hook_arg3" => {
            TEXTURE_SOURCE_ORIGINAL_ARG3.store(value, Ordering::SeqCst)
        }
        "stormworks_video_get_texture_source_hook_arg4" => {
            TEXTURE_SOURCE_ORIGINAL_ARG4.store(value, Ordering::SeqCst)
        }
        _ => {}
    }
}

fn texture_source_original_trampoline_status() -> serde_json::Value {
    serde_json::json!({
        "arg1": TEXTURE_SOURCE_ORIGINAL_ARG1.load(Ordering::SeqCst) != 0,
        "arg2": TEXTURE_SOURCE_ORIGINAL_ARG2.load(Ordering::SeqCst) != 0,
        "arg3": TEXTURE_SOURCE_ORIGINAL_ARG3.load(Ordering::SeqCst) != 0,
        "arg4": TEXTURE_SOURCE_ORIGINAL_ARG4.load(Ordering::SeqCst) != 0
    })
}

fn texture_upload_from_hook_chained(upload_context: *mut c_void) -> Result<(), String> {
    // Keep the retired bridge's conversion coverage in unit tests without compiling the
    // experimental capture path into normal hook behavior.
    #[cfg(test)]
    let test_frame = read_texture_upload_frame(upload_context).ok();
    call_texture_upload_original_arg1(upload_context);
    #[cfg(test)]
    if let Some(frame) = test_frame {
        let _ = push_texture_upload_frame(frame)?;
    }
    Ok(())
}

fn call_texture_upload_original_arg1(upload_context: *mut c_void) {
    let trampoline = TEXTURE_UPLOAD_ORIGINAL_ARG1.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(*mut c_void) = unsafe { std::mem::transmute(trampoline) };
    original(upload_context);
}

fn push_texture_upload_frame(frame: TextureUploadFrame) -> Result<InputVideoBindUpdate, String> {
    let mut state = request_runtime_state()?;
    record_texture_upload_resource_binding(&mut state, &frame);
    let update = apply_texture_upload_frame_to_state(&mut state, &frame, Instant::now())?;
    state.latest_texture_upload_frame = Some(frame);
    if update.updated_slots == 0 && update.skipped_fps_slots == 0 {
        let frame = state.latest_texture_upload_frame.as_ref();
        let source_stats = frame
            .map(|frame| format_pixel_stats(&pixel_stats_from_rgb(&frame.rgb)))
            .unwrap_or_else(|| "none".to_string());
        let message = format!(
            "texture upload hook found no compatible initialized video slots upload={}x{} format={} type={} data_ptr={} context_ptr={} destination_texture_handle={} texture_owner_ptr={} texture_resource_ptr={} slots={} skipped_bound_slots={} skipped_small_unbound_slots={} skipped_size_slots={} skipped_fps_slots={} auto_bound_slots={} source_stats={} slot_state={}",
            frame.map(|frame| frame.width).unwrap_or(0),
            frame.map(|frame| frame.height).unwrap_or(0),
            frame
                .map(|frame| format!("0x{:x}", frame.format))
                .unwrap_or_else(|| "none".to_string()),
            frame
                .map(|frame| format!("0x{:x}", frame.ty))
                .unwrap_or_else(|| "none".to_string()),
            frame
                .map(|frame| format_hex_usize(frame.data_ptr))
                .unwrap_or_else(|| "none".to_string()),
            frame
                .map(|frame| format_hex_usize(frame.context_ptr))
                .unwrap_or_else(|| "none".to_string()),
            frame
                .and_then(|frame| frame.destination_texture_handle)
                .map(|handle| format!("0x{handle:x}"))
                .unwrap_or_else(|| "none".to_string()),
            frame
                .and_then(|frame| frame.texture_owner_ptr)
                .map(|handle| format!("0x{handle:x}"))
                .unwrap_or_else(|| "none".to_string()),
            frame
                .and_then(|frame| frame.texture_resource_ptr)
                .map(|handle| format!("0x{handle:x}"))
                .unwrap_or_else(|| "none".to_string()),
            state.slots.len(),
            update.skipped_bound_slots,
            update.skipped_small_unbound_slots,
            update.skipped_size_slots,
            update.skipped_fps_slots,
            update.auto_bound_slots,
            source_stats,
            describe_slots(&state)
        );
        state.last_error = Some(message.clone());
        let (diagnostic_kind, diagnostic_counter, diagnostic_limit) = if state.slots.is_empty() {
            ("no_slot", &TEXTURE_UPLOAD_NO_SLOT_LOGGED_COUNT, 4usize)
        } else {
            ("no_match", &TEXTURE_UPLOAD_NO_MATCH_LOGGED_COUNT, 3usize)
        };
        if verbose_runtime_diagnostics_enabled()
            && diagnostic_counter.fetch_add(1, Ordering::SeqCst) < diagnostic_limit
        {
            if let Some(path) = &state.log_path {
                let _ = append_log(
                    path,
                    &format!("texture upload diagnostic {diagnostic_kind} {message}"),
                );
            }
        }
    }
    set_runtime(state);
    Ok(InputVideoBindUpdate {
        updated_slots: update.updated_slots,
        skipped_fps_slots: update.skipped_fps_slots,
    })
}

fn record_texture_upload_resource_binding(state: &mut RuntimeState, frame: &TextureUploadFrame) {
    let Some(handle) = frame
        .destination_texture_handle
        .and_then(|handle| u32::try_from(handle).ok())
        .filter(|handle| *handle != 0)
    else {
        return;
    };
    let now = Instant::now();
    let binding = GlTextureBinding {
        handle,
        owner_ptr: frame.texture_owner_ptr.unwrap_or(0),
        texture_ptr: frame.texture_resource_ptr.unwrap_or(0),
        width: frame.width,
        height: frame.height,
        last_seen: now,
    };
    if let Some(owner) = frame.texture_owner_ptr.filter(|value| *value != 0) {
        state.gl_texture_bindings.insert(owner, binding.clone());
    }
    if let Some(texture) = frame.texture_resource_ptr.filter(|value| *value != 0) {
        state.gl_texture_bindings.insert(texture, binding);
    }
}

#[derive(Debug)]
struct TextureUploadStateUpdate {
    updated_slots: usize,
    skipped_bound_slots: usize,
    skipped_small_unbound_slots: usize,
    skipped_size_slots: usize,
    skipped_fps_slots: usize,
    auto_bound_slots: usize,
    updated_slot_sizes: Vec<String>,
}

fn apply_texture_upload_frame_to_state(
    state: &mut RuntimeState,
    frame: &TextureUploadFrame,
    now: Instant,
) -> Result<TextureUploadStateUpdate, String> {
    let mut update = TextureUploadStateUpdate {
        updated_slots: 0,
        skipped_bound_slots: 0,
        skipped_small_unbound_slots: 0,
        skipped_size_slots: 0,
        skipped_fps_slots: 0,
        auto_bound_slots: 0,
        updated_slot_sizes: Vec::new(),
    };
    let capture_interval = capture_frame_interval(state.config.capture.max_fps);
    let min_unbound_width = state.config.capture.min_unbound_texture_upload_width.max(1);
    let min_unbound_height = state
        .config
        .capture
        .min_unbound_texture_upload_height
        .max(1);
    let frame_is_below_minimum =
        frame.width < min_unbound_width || frame.height < min_unbound_height;
    let frame_is_blank = rgb_is_blank(&frame.rgb);
    let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
    let matching_bound_texture_handle = frame.destination_texture_handle.filter(|handle| {
        state
            .slots
            .values()
            .any(|slot| slot.texture_upload_handle == Some(*handle))
    });
    for slot in state.slots.values_mut() {
        let slot_has_confirmed_input = slot.connected && slot.input_source_handle != 0;
        if let Some(handle) = matching_bound_texture_handle {
            let slot_matches_handle = slot.texture_upload_handle == Some(handle);
            let connected_unbound_slot =
                slot.texture_upload_handle.is_none() && slot_has_confirmed_input;
            if !slot_matches_handle && !connected_unbound_slot {
                update.skipped_bound_slots += 1;
                continue;
            }
        } else if slot.texture_upload_handle.is_some() {
            update.skipped_bound_slots += 1;
            continue;
        }
        if matching_bound_texture_handle.is_none()
            && slot.texture_upload_handle.is_none()
            && !slot_has_confirmed_input
            && frame_is_below_minimum
        {
            update.skipped_small_unbound_slots += 1;
            continue;
        }
        if slot.texture_upload_handle.is_none() && frame_is_below_minimum && frame_is_blank {
            update.skipped_small_unbound_slots += 1;
            continue;
        }
        if !slot_has_confirmed_input && (frame.width < slot.width || frame.height < slot.height) {
            update.skipped_size_slots += 1;
            continue;
        }
        if texture_upload_slot_is_rate_limited(slot, now, capture_interval) {
            update.skipped_fps_slots += 1;
            continue;
        }
        let rgb = resize_rgb_nearest(
            &frame.rgb,
            frame.width,
            frame.height,
            slot.width,
            slot.height,
        )?;
        slot.frame_id = frame_id;
        slot.ready = true;
        slot.connected = true;
        slot.latest_frame = Some(FrameBuffer {
            frame_id,
            width: slot.width,
            height: slot.height,
            source: "texture_upload".to_string(),
            rgb,
        });
        if slot.texture_upload_handle.is_none() {
            if let Some(handle) = frame.destination_texture_handle {
                slot.texture_upload_handle = Some(handle);
                update.auto_bound_slots += 1;
            }
        }
        slot.last_texture_upload_at = Some(now);
        update
            .updated_slot_sizes
            .push(format!("{}:{}x{}", slot.slot, slot.width, slot.height));
        update.updated_slots += 1;
    }
    if update.skipped_bound_slots > 0 {
        state.hook_runtime.texture_upload_skipped_bound_slots = state
            .hook_runtime
            .texture_upload_skipped_bound_slots
            .saturating_add(update.skipped_bound_slots as u64);
    }
    if update.skipped_small_unbound_slots > 0 {
        state
            .hook_runtime
            .texture_upload_skipped_small_unbound_slots = state
            .hook_runtime
            .texture_upload_skipped_small_unbound_slots
            .saturating_add(update.skipped_small_unbound_slots as u64);
    }
    if update.skipped_fps_slots > 0 {
        state.hook_runtime.texture_upload_skipped_fps_slots = state
            .hook_runtime
            .texture_upload_skipped_fps_slots
            .saturating_add(update.skipped_fps_slots as u64);
    }
    if update.auto_bound_slots > 0 {
        state.hook_runtime.texture_upload_auto_bound_slots = state
            .hook_runtime
            .texture_upload_auto_bound_slots
            .saturating_add(update.auto_bound_slots as u64);
    }
    if update.updated_slots > 0 {
        state.hook_runtime.real_video_capture = true;
        state.hook_runtime.texture_upload_bridge_frames = state
            .hook_runtime
            .texture_upload_bridge_frames
            .saturating_add(update.updated_slots as u64);
        let source_stats = pixel_stats_from_rgb(&frame.rgb);
        if verbose_runtime_diagnostics_enabled()
            && !TEXTURE_UPLOAD_FRAME_LOGGED.swap(true, Ordering::SeqCst)
        {
            if let Some(path) = &state.log_path {
                let _ = append_log(
                    path,
                    &format!(
                        "texture_upload captured upload={}x{} destination_texture_handle={} texture_owner_ptr={} texture_resource_ptr={} updated_slots={} slot_sizes={} auto_bound_slots={} skipped_bound_slots={} skipped_small_unbound_slots={} min_unbound_upload={}x{} skipped_size_slots={} skipped_fps_slots={} source_stats={}",
                        frame.width,
                        frame.height,
                        frame
                            .destination_texture_handle
                            .map(|handle| format!("0x{handle:x}"))
                            .unwrap_or_else(|| "none".to_string()),
                        frame
                            .texture_owner_ptr
                            .map(|handle| format!("0x{handle:x}"))
                            .unwrap_or_else(|| "none".to_string()),
                        frame
                            .texture_resource_ptr
                            .map(|handle| format!("0x{handle:x}"))
                            .unwrap_or_else(|| "none".to_string()),
                        update.updated_slots,
                        update.updated_slot_sizes.join(","),
                        update.auto_bound_slots,
                        update.skipped_bound_slots,
                        update.skipped_small_unbound_slots,
                        min_unbound_width,
                        min_unbound_height,
                        update.skipped_size_slots,
                        update.skipped_fps_slots,
                        format_pixel_stats(&source_stats)
                    ),
                );
            }
        }
        log_runtime_diagnostic(
            state,
            &format!(
                "texture_upload captured upload={}x{} format=0x{:x} type=0x{:x} data_ptr={} context_ptr={} destination_texture_handle={} texture_owner_ptr={} texture_resource_ptr={} updated_slots={} slot_sizes={} auto_bound_slots={} skipped_bound_slots={} skipped_small_unbound_slots={} skipped_size_slots={} skipped_fps_slots={} source_stats={} slots={}",
                frame.width,
                frame.height,
                frame.format,
                frame.ty,
                format_hex_usize(frame.data_ptr),
                format_hex_usize(frame.context_ptr),
                frame
                    .destination_texture_handle
                    .map(|handle| format!("0x{handle:x}"))
                    .unwrap_or_else(|| "none".to_string()),
                frame
                    .texture_owner_ptr
                    .map(|handle| format!("0x{handle:x}"))
                    .unwrap_or_else(|| "none".to_string()),
                frame
                    .texture_resource_ptr
                    .map(|handle| format!("0x{handle:x}"))
                    .unwrap_or_else(|| "none".to_string()),
                update.updated_slots,
                update.updated_slot_sizes.join(","),
                update.auto_bound_slots,
                update.skipped_bound_slots,
                update.skipped_small_unbound_slots,
                update.skipped_size_slots,
                update.skipped_fps_slots,
                format_pixel_stats(&source_stats),
                describe_slots(state)
            ),
            &TEXTURE_UPLOAD_CAPTURE_DIAGNOSTIC_COUNT,
            32,
        );
    }
    Ok(update)
}

fn apply_latest_texture_upload_frame_to_slot(
    state: &mut RuntimeState,
    component: &str,
    slot_id: u32,
) -> Result<(), String> {
    let Some(frame) = state.latest_texture_upload_frame.clone() else {
        return Ok(());
    };
    if !state.slots.contains_key(&slot_key(component, slot_id)) {
        return Ok(());
    }
    apply_texture_upload_frame_to_state(state, &frame, Instant::now()).map(|_| ())
}

#[derive(Debug, Clone)]
struct SourceTextureProbeSlot {
    key: SlotKey,
    component: String,
    slot: u32,
    width: u32,
    height: u32,
    input_source_handle: u64,
    source_texture_handle: Option<u64>,
    last_texture_upload_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct SourceTextureCandidate {
    handle: u32,
    source_handle: u64,
    source_offset: usize,
    pointer_offset: Option<usize>,
}

#[derive(Debug, Clone)]
struct SourceTextureReadback {
    candidate: SourceTextureCandidate,
    width: u32,
    height: u32,
    rgb: Vec<[u8; 3]>,
}

#[derive(Debug, Clone)]
struct SourceTextureProbeReport {
    attempts: usize,
    candidates: usize,
    read_errors: usize,
    blank_reads: usize,
    updated_slots: usize,
    skipped_fps_slots: usize,
    details: Vec<String>,
}

#[derive(Debug, Clone)]
struct SourceTextureStateUpdate {
    updated: bool,
    skipped_fps: bool,
    stats: PixelStats,
}

fn probe_connected_source_textures_after_upload() -> Result<usize, String> {
    #[cfg(windows)]
    {
        probe_connected_source_textures_after_upload_windows()
    }
    #[cfg(not(windows))]
    {
        Ok(0)
    }
}

#[cfg(windows)]
fn probe_connected_source_textures_after_upload_windows() -> Result<usize, String> {
    let slots = source_texture_probe_slots()?;
    if slots.is_empty() {
        return Ok(0);
    }
    let mut report = SourceTextureProbeReport {
        attempts: slots.len(),
        candidates: 0,
        read_errors: 0,
        blank_reads: 0,
        updated_slots: 0,
        skipped_fps_slots: 0,
        details: Vec::new(),
    };
    let capture_interval = source_texture_capture_interval()?;
    let now = Instant::now();
    for slot in slots {
        if source_texture_probe_slot_is_rate_limited(&slot, now, capture_interval) {
            report.skipped_fps_slots = report.skipped_fps_slots.saturating_add(1);
            report.details.push(format!(
                "{}:{} request={}x{} source={} skipped_fps",
                slot.component,
                slot.slot,
                slot.width,
                slot.height,
                format_hex_or_zero(slot.input_source_handle)
            ));
            continue;
        }
        let candidates =
            collect_source_texture_candidates(slot.input_source_handle, slot.source_texture_handle);
        report.candidates = report.candidates.saturating_add(candidates.len());
        if candidates.is_empty() {
            report.details.push(format!(
                "{}:{} request={}x{} source={} candidates=0",
                slot.component,
                slot.slot,
                slot.width,
                slot.height,
                format_hex_or_zero(slot.input_source_handle)
            ));
            continue;
        }
        let candidate_summary = format_source_texture_candidates(&candidates);
        let mut slot_updated = false;
        for candidate in candidates {
            match read_gl_texture_candidate(candidate) {
                Ok(readback) => {
                    let stats = pixel_stats_from_rgb(&readback.rgb);
                    if stats.nonzero_pixels == 0 {
                        report.blank_reads = report.blank_reads.saturating_add(1);
                        report.details.push(format!(
                            "{}:{} request={}x{} source={} candidate=0x{:x} native={}x{} blank {}",
                            slot.component,
                            slot.slot,
                            slot.width,
                            slot.height,
                            format_hex_or_zero(slot.input_source_handle),
                            readback.candidate.handle,
                            readback.width,
                            readback.height,
                            format_pixel_stats(&stats)
                        ));
                        continue;
                    }
                    let update = apply_source_texture_readback_to_slot(&slot.key, readback)?;
                    if update.skipped_fps {
                        report.skipped_fps_slots = report.skipped_fps_slots.saturating_add(1);
                    }
                    if update.updated {
                        report.updated_slots = report.updated_slots.saturating_add(1);
                        report.details.push(format!(
                            "{}:{} request={}x{} source={} candidates={} captured {}",
                            slot.component,
                            slot.slot,
                            slot.width,
                            slot.height,
                            format_hex_or_zero(slot.input_source_handle),
                            candidate_summary,
                            format_pixel_stats(&update.stats)
                        ));
                        slot_updated = true;
                        break;
                    }
                }
                Err(error) => {
                    report.read_errors = report.read_errors.saturating_add(1);
                    if report.details.len() < 12 {
                        report.details.push(format!(
                            "{}:{} request={}x{} source={} candidate=0x{:x} read_error={}",
                            slot.component,
                            slot.slot,
                            slot.width,
                            slot.height,
                            format_hex_or_zero(slot.input_source_handle),
                            candidate.handle,
                            error
                        ));
                    }
                }
            }
        }
        if !slot_updated && report.details.len() < 12 {
            report.details.push(format!(
                "{}:{} request={}x{} source={} no_nonblank_candidate candidates={}",
                slot.component,
                slot.slot,
                slot.width,
                slot.height,
                format_hex_or_zero(slot.input_source_handle),
                candidate_summary
            ));
        }
    }
    record_source_texture_probe_report(&report)?;
    Ok(report.updated_slots)
}

fn source_texture_probe_slots() -> Result<Vec<SourceTextureProbeSlot>, String> {
    let state = request_runtime_state()?;
    if !state.config.capture.source_texture_probe_enabled
        || !state.config.capture.source_texture_probe_unsafe_confirm
    {
        return Ok(Vec::new());
    }
    Ok(state
        .slots
        .iter()
        .filter(|(_, slot)| slot.connected && slot.input_source_handle != 0)
        .filter(|(_, slot)| should_probe_source_texture_slot(slot))
        .map(|(key, slot)| SourceTextureProbeSlot {
            key: key.clone(),
            component: slot.component.clone(),
            slot: slot.slot,
            width: slot.width,
            height: slot.height,
            input_source_handle: slot.input_source_handle,
            source_texture_handle: slot.source_texture_handle,
            last_texture_upload_at: slot.last_texture_upload_at,
        })
        .collect())
}

fn source_texture_probe_enabled() -> bool {
    let config = runtime_snapshot().config.capture;
    config.source_texture_probe_enabled && config.source_texture_probe_unsafe_confirm
}

#[cfg(windows)]
fn probe_trusted_monitor_source_textures_from_gl_context(
    reason: &'static str,
) -> Result<usize, String> {
    let slots = trusted_monitor_source_texture_slots()?;
    if slots.is_empty() {
        return Ok(0);
    }
    {
        let mut state = request_runtime_state()?;
        state.hook_runtime.source_texture_probe_attempts = state
            .hook_runtime
            .source_texture_probe_attempts
            .saturating_add(slots.len() as u64);
        state.hook_runtime.source_texture_probe_candidates = state
            .hook_runtime
            .source_texture_probe_candidates
            .saturating_add(slots.len() as u64);
        set_runtime(state);
    }
    let mut updated = 0usize;
    for (component, slot) in slots {
        match refresh_trusted_monitor_source_texture_for_component_slot(&component, slot) {
            Ok(true) => {
                updated = updated.saturating_add(1);
            }
            Ok(false) => {}
            Err(error) => {
                let state = runtime_snapshot();
                log_runtime_diagnostic(
                    &state,
                    &format!(
                        "trusted monitor source refresh error reason={} component={} slot={} error={}",
                        reason, component, slot, error
                    ),
                    &SOURCE_TEXTURE_PROBE_DIAGNOSTIC_COUNT,
                    64,
                );
            }
        }
    }
    Ok(updated)
}

#[cfg(windows)]
fn trusted_monitor_source_texture_slots() -> Result<Vec<(String, u32)>, String> {
    let state = request_runtime_state()?;
    Ok(state
        .slots
        .values()
        .filter(|slot| {
            slot.connected
                && is_slot_ready_for_lua(slot)
                && slot.input_source_handle != 0
                && slot.source_texture_handle.is_some()
                && slot.latest_frame.as_ref().is_some_and(|frame| {
                    matches!(frame.source.as_str(), "monitor_render" | "source_texture")
                })
        })
        .map(|slot| (slot.component.clone(), slot.slot))
        .collect())
}

#[cfg(windows)]
fn refresh_trusted_monitor_source_texture_for_component_slot(
    component: &str,
    slot_id: u32,
) -> Result<bool, String> {
    let key = slot_key(component, slot_id);
    let state = request_runtime_state()?;
    let Some(slot) = state.slots.get(&key) else {
        set_runtime(state);
        return Ok(false);
    };
    if !slot.connected || !is_slot_ready_for_lua(slot) || slot.input_source_handle == 0 {
        set_runtime(state);
        return Ok(false);
    }
    let Some(handle) = slot.source_texture_handle else {
        set_runtime(state);
        return Ok(false);
    };
    if handle == 0 || handle > u64::from(u32::MAX) {
        set_runtime(state);
        return Ok(false);
    }
    let trusted_source = slot
        .latest_frame
        .as_ref()
        .is_some_and(|frame| matches!(frame.source.as_str(), "monitor_render" | "source_texture"));
    if !trusted_source {
        set_runtime(state);
        return Ok(false);
    }
    let capture_interval = capture_frame_interval(state.config.capture.max_fps);
    let now = Instant::now();
    if texture_upload_slot_is_rate_limited(slot, now, capture_interval) {
        set_runtime(state);
        return Ok(false);
    }
    let input_source_handle = slot.input_source_handle;
    let request_width = slot.width;
    let request_height = slot.height;
    let preferred_handle = handle as u32;
    let preferred_shape = state
        .monitor_pbo_readbacks
        .get(&u64::from(preferred_handle))
        .map(|entry| (entry.width, entry.height));
    let mut handles = vec![preferred_handle];
    for (known_handle, entry) in &state.monitor_pbo_readbacks {
        if *known_handle > u64::from(u32::MAX) {
            continue;
        }
        let known_handle = *known_handle as u32;
        if handles.contains(&known_handle) {
            continue;
        }
        if preferred_shape.is_none_or(|shape| shape == (entry.width, entry.height)) {
            handles.push(known_handle);
        }
    }
    set_runtime(state);

    let mut errors = Vec::new();
    for handle in handles {
        let (mapped_width, mapped_height) = current_gl_texture_size_for_handle(handle)
            .unwrap_or_else(|| preferred_shape.unwrap_or((request_width, request_height)));
        let candidate = MonitorRenderResourceCandidate {
            handle,
            monitor: 0,
            resource: 0,
            resource_offset: 0,
            monitor_resource_offset: 0,
            mapped_key: u64::from(handle),
            mapped_from: "trusted_cached_slot_source_texture",
            mapped_width,
            mapped_height,
            binding_owner_ptr: 0,
            binding_texture_ptr: 0,
            binding_age_ms: 0,
        };
        match read_monitor_render_texture_with_pbo(candidate, input_source_handle) {
            Ok(readback) => {
                let stats = pixel_stats_from_rgb(&readback.rgb);
                if stats.nonzero_pixels == 0 {
                    errors.push(format!(
                        "0x{:x}:blank native={}x{} {}",
                        handle,
                        readback.width,
                        readback.height,
                        format_pixel_stats(&stats)
                    ));
                    continue;
                }
                let update =
                    apply_monitor_render_readback_to_slots(&[key.clone()], candidate, readback)?;
                if update.updated {
                    return Ok(true);
                }
            }
            Err(error) => {
                errors.push(format!(
                    "0x{:x}:hint={}x{} {}",
                    handle, mapped_width, mapped_height, error
                ));
            }
        }
    }
    let state = runtime_snapshot();
    log_runtime_diagnostic(
        &state,
        &format!(
            "trusted monitor source refresh no_frame component={} slot={} errors={}",
            component,
            slot_id,
            if errors.is_empty() {
                "none".to_string()
            } else {
                errors.join(" | ")
            }
        ),
        &SOURCE_TEXTURE_PROBE_DIAGNOSTIC_COUNT,
        96,
    );
    Ok(false)
}

fn should_probe_source_texture_slot(slot: &SlotState) -> bool {
    if !is_slot_ready_for_lua(slot) {
        return true;
    }
    slot.latest_frame
        .as_ref()
        .is_some_and(|frame| frame.source == "source_texture")
}

fn source_texture_capture_interval() -> Result<Duration, String> {
    let state = request_runtime_state()?;
    Ok(capture_frame_interval(state.config.capture.max_fps))
}

fn source_texture_probe_slot_is_rate_limited(
    slot: &SourceTextureProbeSlot,
    now: Instant,
    capture_interval: Duration,
) -> bool {
    match slot.last_texture_upload_at {
        Some(previous) => now.duration_since(previous) < capture_interval,
        None => false,
    }
}

fn collect_source_texture_candidates(
    source_handle: u64,
    preferred_handle: Option<u64>,
) -> Vec<SourceTextureCandidate> {
    let source = source_handle as usize;
    if source == 0 {
        return Vec::new();
    }
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    let direct_scan_bytes =
        SOURCE_TEXTURE_SCAN_DIRECT_BYTES.saturating_sub(SOURCE_TEXTURE_SCAN_DIRECT_MIN_OFFSET);
    collect_texture_candidates_from_object(
        source,
        source_handle,
        SOURCE_TEXTURE_SCAN_DIRECT_MIN_OFFSET,
        direct_scan_bytes,
        None,
        &mut seen,
        &mut candidates,
    );
    let pointer_scan_bytes =
        SOURCE_TEXTURE_SCAN_POINTER_BYTES.min(SOURCE_TEXTURE_SCAN_DIRECT_BYTES);
    for source_offset in (0..pointer_scan_bytes).step_by(size_of::<usize>()) {
        if candidates.len() >= MAX_SOURCE_TEXTURE_CANDIDATES_PER_SOURCE {
            break;
        }
        let Some(pointer) = read_usize_field(source, source_offset) else {
            continue;
        };
        if pointer == 0 || pointer == source {
            continue;
        }
        collect_texture_candidates_from_object(
            pointer,
            source_handle,
            0,
            SOURCE_TEXTURE_SCAN_POINTER_DEREF_BYTES,
            Some(source_offset),
            &mut seen,
            &mut candidates,
        );
    }
    sort_source_texture_candidates(&mut candidates, preferred_handle);
    candidates
}

fn collect_texture_candidates_from_object(
    object: usize,
    source_handle: u64,
    start_offset: usize,
    byte_len: usize,
    pointer_offset: Option<usize>,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<SourceTextureCandidate>,
) {
    if candidates.len() >= MAX_SOURCE_TEXTURE_CANDIDATES_PER_SOURCE {
        return;
    }
    let readable_len = start_offset
        .checked_add(byte_len)
        .unwrap_or(usize::MAX)
        .max(size_of::<u32>());
    if !memory_range_is_readable(object as *const c_void, readable_len) {
        return;
    }
    let end_offset = start_offset.saturating_add(byte_len);
    let mut offset = start_offset;
    while offset + size_of::<u32>() <= end_offset {
        if candidates.len() >= MAX_SOURCE_TEXTURE_CANDIDATES_PER_SOURCE {
            break;
        }
        if let Some(handle) = read_u32_field(object, offset)
            .filter(|value| plausible_gl_texture_handle(*value) && seen.insert(*value))
        {
            candidates.push(SourceTextureCandidate {
                handle,
                source_handle,
                source_offset: offset,
                pointer_offset,
            });
        }
        offset = offset.saturating_add(size_of::<u32>());
    }
}

fn sort_source_texture_candidates(
    candidates: &mut [SourceTextureCandidate],
    preferred_handle: Option<u64>,
) {
    candidates.sort_by_key(|candidate| {
        let preferred_rank = if preferred_handle == Some(u64::from(candidate.handle)) {
            0u8
        } else {
            1u8
        };
        (
            preferred_rank,
            source_texture_candidate_rank(candidate),
            candidate.pointer_offset.unwrap_or(usize::MAX),
            candidate.source_offset,
            candidate.handle,
        )
    });
}

fn source_texture_candidate_rank(candidate: &SourceTextureCandidate) -> u8 {
    if candidate.pointer_offset.is_some() {
        0
    } else if candidate.source_offset >= SOURCE_TEXTURE_SCAN_DIRECT_MIN_OFFSET {
        1
    } else {
        2
    }
}

fn plausible_gl_texture_handle(handle: u32) -> bool {
    (1..0x0100_0000).contains(&handle)
}

fn format_source_texture_candidates(candidates: &[SourceTextureCandidate]) -> String {
    if candidates.is_empty() {
        return "none".to_string();
    }
    candidates
        .iter()
        .take(8)
        .map(|candidate| match candidate.pointer_offset {
            Some(pointer_offset) => format!(
                "0x{:x}@ptr+0x{:x}/+0x{:x}",
                candidate.handle, pointer_offset, candidate.source_offset
            ),
            None => format!("0x{:x}@+0x{:x}", candidate.handle, candidate.source_offset),
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(windows)]
fn read_gl_texture_candidate(
    candidate: SourceTextureCandidate,
) -> Result<SourceTextureReadback, String> {
    if unsafe { glIsTexture(candidate.handle) } == 0 {
        return Err("glIsTexture=false".to_string());
    }
    drain_gl_errors();
    let previous_binding = current_gl_texture_binding_2d().unwrap_or(0);
    drain_gl_errors();
    call_original_gl_bind_texture(GL_TEXTURE_2D, candidate.handle);
    let bind_error = gl_error();
    if bind_error != GL_NO_ERROR {
        restore_gl_texture_binding_2d(previous_binding);
        return Err(format!("glBindTexture error=0x{bind_error:x}"));
    }
    let mut width = 0i32;
    let mut height = 0i32;
    unsafe {
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_WIDTH, &mut width);
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_HEIGHT, &mut height);
    }
    let level_error = gl_error();
    if level_error != GL_NO_ERROR {
        restore_gl_texture_binding_2d(previous_binding);
        return Err(format!("glGetTexLevelParameteriv error=0x{level_error:x}"));
    }
    if width <= 0 || height <= 0 {
        restore_gl_texture_binding_2d(previous_binding);
        return Err(format!("invalid texture size {}x{}", width, height));
    }
    let width_u32 = width as u32;
    let height_u32 = height as u32;
    let pixel_count = width_u32
        .checked_mul(height_u32)
        .ok_or_else(|| "texture readback pixel count overflow".to_string())?
        as usize;
    if pixel_count > MAX_SOURCE_TEXTURE_READ_PIXELS {
        restore_gl_texture_binding_2d(previous_binding);
        return Err(format!(
            "texture readback {}x{} exceeds pixel cap {}",
            width_u32, height_u32, MAX_SOURCE_TEXTURE_READ_PIXELS
        ));
    }
    let byte_len = pixel_count
        .checked_mul(4)
        .ok_or_else(|| "texture readback byte count overflow".to_string())?;
    let mut rgba = vec![0u8; byte_len];
    drain_gl_errors();
    unsafe {
        glGetTexImage(
            GL_TEXTURE_2D,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            rgba.as_mut_ptr().cast::<c_void>(),
        );
    }
    let read_error = gl_error();
    restore_gl_texture_binding_2d(previous_binding);
    if read_error != GL_NO_ERROR {
        return Err(format!("glGetTexImage error=0x{read_error:x}"));
    }
    let rgb = rgba
        .chunks_exact(4)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect::<Vec<_>>();
    Ok(SourceTextureReadback {
        candidate,
        width: width_u32,
        height: height_u32,
        rgb,
    })
}

#[cfg(windows)]
fn read_monitor_render_texture_with_pbo(
    candidate: MonitorRenderResourceCandidate,
    input_slot_handle: u64,
) -> Result<SourceTextureReadback, String> {
    read_monitor_render_texture_with_pbo_for_handles(
        candidate,
        normalize_monitor_input_handles(input_slot_handle, std::iter::empty()),
    )
}

#[cfg(windows)]
fn normalize_monitor_input_handles<I>(primary: u64, handles: I) -> Vec<u64>
where
    I: IntoIterator<Item = u64>,
{
    let mut normalized = Vec::new();
    if primary != 0 {
        normalized.push(primary);
    }
    for handle in handles {
        if handle != 0 && !normalized.contains(&handle) {
            normalized.push(handle);
        }
    }
    normalized
}

#[cfg(windows)]
fn primary_monitor_input_handle(input_handles: &[u64]) -> u64 {
    input_handles
        .iter()
        .copied()
        .find(|handle| *handle != 0)
        .unwrap_or(0)
}

#[cfg(windows)]
fn format_monitor_input_handles(input_handles: &[u64]) -> String {
    let handles = input_handles
        .iter()
        .copied()
        .filter(|handle| *handle != 0)
        .take(8)
        .map(format_hex_or_zero)
        .collect::<Vec<_>>();
    if handles.is_empty() {
        "none".to_string()
    } else {
        handles.join(",")
    }
}

#[cfg(windows)]
fn read_monitor_render_texture_with_pbo_for_handles(
    candidate: MonitorRenderResourceCandidate,
    input_handles: Vec<u64>,
) -> Result<SourceTextureReadback, String> {
    let input_handles = normalize_monitor_input_handles(0, input_handles);
    let input_slot_handle = primary_monitor_input_handle(&input_handles);
    let pbo_functions = load_gl_pbo_functions();
    let missing = pbo_functions.missing_required();
    if unsafe { glIsTexture(candidate.handle) } == 0 {
        return Err("glIsTexture=false".to_string());
    }
    drain_gl_errors();
    let previous_texture = current_gl_texture_binding_2d().unwrap_or(0);
    drain_gl_errors();
    let previous_pack = current_gl_pixel_pack_buffer_binding().unwrap_or(0);
    drain_gl_errors();
    call_original_gl_bind_texture(GL_TEXTURE_2D, candidate.handle);
    let result = (|| {
        let bind_error = gl_error();
        if bind_error != GL_NO_ERROR {
            return Err(format!("glBindTexture error=0x{bind_error:x}"));
        }
        let mut width = 0i32;
        let mut height = 0i32;
        unsafe {
            glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_WIDTH, &mut width);
            glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_HEIGHT, &mut height);
        }
        let level_error = gl_error();
        if level_error != GL_NO_ERROR {
            return Err(format!("glGetTexLevelParameteriv error=0x{level_error:x}"));
        }
        if width <= 0 || height <= 0 {
            return Err(format!(
                "invalid texture size {}x{} mapped_size={}x{}",
                width, height, candidate.mapped_width, candidate.mapped_height
            ));
        }
        let width_u32 = width as u32;
        let height_u32 = height as u32;
        let pixel_count = width_u32
            .checked_mul(height_u32)
            .ok_or_else(|| "pbo readback pixel count overflow".to_string())?
            as usize;
        if pixel_count > MAX_SOURCE_TEXTURE_READ_PIXELS {
            return Err(format!(
                "pbo readback {}x{} exceeds pixel cap {}",
                width_u32, height_u32, MAX_SOURCE_TEXTURE_READ_PIXELS
            ));
        }
        if !missing.is_empty() {
            return read_monitor_render_texture_sync(
                candidate,
                input_slot_handle,
                width_u32,
                height_u32,
                &missing,
            );
        }
        let byte_len = pixel_count
            .checked_mul(4)
            .ok_or_else(|| "pbo readback byte count overflow".to_string())?;
        let mut state = request_runtime_state()?;
        let entry = state
            .monitor_pbo_readbacks
            .entry(u64::from(candidate.handle))
            .or_insert_with(|| MonitorPboReadback::new(candidate.handle, width_u32, height_u32));
        entry.reset_if_shape_changed(width_u32, height_u32);
        let ready = poll_monitor_pbo_ready(entry, &pbo_functions)?;
        let scheduled = match schedule_monitor_pbo_readback(
            entry,
            &pbo_functions,
            byte_len,
            candidate,
            input_handles.clone(),
        ) {
            Ok(scheduled) => scheduled,
            Err(error) if monitor_render_candidate_should_try_fbo_readback(&candidate, &error) => {
                schedule_monitor_fbo_pbo_readback(
                    entry,
                    &pbo_functions,
                    byte_len,
                    candidate,
                    input_handles.clone(),
                )
                .map_err(|fallback_error| {
                    format!("{error}; fbo_readback_failed={fallback_error}")
                })?
            }
            Err(error) => return Err(error),
        };
        let readback_handle = entry.handle;
        set_runtime(state);
        if let Some(ready) = ready {
            return Ok(SourceTextureReadback {
                candidate: SourceTextureCandidate {
                    handle: ready.candidate.handle,
                    source_handle: ready.input_slot_handle,
                    source_offset: ready.candidate.resource_offset,
                    pointer_offset: Some(ready.candidate.monitor_resource_offset),
                },
                width: ready.width,
                height: ready.height,
                rgb: ready.rgb,
            });
        }
        Err(format!(
            "pbo_pending handle=0x{:x} scheduled={} native={}x{} mapped_size={}x{}",
            readback_handle,
            scheduled,
            width_u32,
            height_u32,
            candidate.mapped_width,
            candidate.mapped_height
        ))
    })();
    restore_gl_pixel_pack_buffer_binding(previous_pack, &pbo_functions);
    restore_gl_texture_binding_2d(previous_texture);
    result
}

#[cfg(windows)]
fn read_monitor_render_texture_sync(
    candidate: MonitorRenderResourceCandidate,
    input_slot_handle: u64,
    width: u32,
    height: u32,
    missing_pbo_functions: &[&'static str],
) -> Result<SourceTextureReadback, String> {
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| "sync readback pixel count overflow".to_string())?
        as usize;
    let byte_len = pixel_count
        .checked_mul(4)
        .ok_or_else(|| "sync readback byte count overflow".to_string())?;
    let mut rgba = vec![0u8; byte_len];
    drain_gl_errors();
    unsafe {
        glGetTexImage(
            GL_TEXTURE_2D,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            rgba.as_mut_ptr().cast::<c_void>(),
        );
    }
    let read_error = gl_error();
    if read_error != GL_NO_ERROR {
        return Err(format!(
            "sync_readback glGetTexImage error=0x{read_error:x} pbo_missing={}",
            missing_pbo_functions.join(",")
        ));
    }
    let rgb = rgba
        .chunks_exact(4)
        .map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect::<Vec<_>>();
    Ok(SourceTextureReadback {
        candidate: SourceTextureCandidate {
            handle: candidate.handle,
            source_handle: input_slot_handle,
            source_offset: candidate.resource_offset,
            pointer_offset: Some(candidate.monitor_resource_offset),
        },
        width,
        height,
        rgb,
    })
}

#[cfg(windows)]
fn drain_ready_monitor_pbo_readbacks(source: &str) -> Result<usize, String> {
    let pbo_functions = load_gl_pbo_functions();
    if !pbo_functions.missing_required().is_empty() {
        return Ok(0);
    }

    let previous_pack = current_gl_pixel_pack_buffer_binding().unwrap_or(0);
    let mut state = request_runtime_state()?;
    let mut ready_frames = Vec::new();
    let mut stale_entries = Vec::new();
    let mut drain_errors = Vec::new();
    for (handle, entry) in state.monitor_pbo_readbacks.iter_mut() {
        match poll_monitor_pbo_ready(entry, &pbo_functions) {
            Ok(Some(frame)) => ready_frames.push(frame),
            Ok(None) => {}
            Err(error) => {
                stale_entries.push(*handle);
                drain_errors.push((*handle, error));
            }
        }
    }
    for (handle, error) in drain_errors {
        log_runtime_diagnostic(
            &state,
            &format!(
                "monitor pbo drain error source={} handle=0x{:x} error={}",
                source, handle, error
            ),
            &MONITOR_RENDER_PBO_DRAIN_DIAGNOSTIC_COUNT,
            24,
        );
    }
    for handle in stale_entries {
        state.monitor_pbo_readbacks.remove(&handle);
    }
    set_runtime(state);
    restore_gl_pixel_pack_buffer_binding(previous_pack, &pbo_functions);

    let mut updated = 0usize;
    for frame in ready_frames {
        let slots = monitor_render_probe_slots_for_handles(frame.input_handles.clone())?;
        if slots.is_empty() {
            let state = runtime_snapshot();
            let stats = pixel_stats_from_rgb(&frame.rgb);
            log_runtime_diagnostic(
                &state,
                &format!(
                    "monitor pbo drain no_slot source={} handle=0x{:x} input_handles={} primary_input={} native={}x{} source_stats={}",
                    source,
                    frame.candidate.handle,
                    format_monitor_input_handles(&frame.input_handles),
                    format_hex_or_zero(frame.input_slot_handle),
                    frame.width,
                    frame.height,
                    format_pixel_stats(&stats)
                ),
                &MONITOR_RENDER_PBO_DRAIN_DIAGNOSTIC_COUNT,
                24,
            );
            continue;
        }

        let readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: frame.candidate.handle,
                source_handle: frame.input_slot_handle,
                source_offset: frame.candidate.resource_offset,
                pointer_offset: Some(frame.candidate.monitor_resource_offset),
            },
            width: frame.width,
            height: frame.height,
            rgb: frame.rgb,
        };
        let stats = pixel_stats_from_rgb(&readback.rgb);
        if stats.nonzero_pixels == 0 {
            let state = runtime_snapshot();
            log_runtime_diagnostic(
                &state,
                &format!(
                    "monitor pbo drain blank source={} handle=0x{:x} mapped_from={} native={}x{} {}",
                    source,
                    frame.candidate.handle,
                    frame.candidate.mapped_from,
                    readback.width,
                    readback.height,
                    format_pixel_stats(&stats)
                ),
                &MONITOR_RENDER_PBO_DRAIN_DIAGNOSTIC_COUNT,
                24,
            );
            continue;
        }

        let update = apply_monitor_render_readback_to_slots(&slots, frame.candidate, readback)?;
        if update.updated {
            updated = updated.saturating_add(update.updated_slots);
        } else {
            let state = runtime_snapshot();
            log_runtime_diagnostic(
                &state,
                &format!(
                    "monitor pbo drain rejected source={} handle=0x{:x} mapped_from={} native={}x{} skipped_fps={} skipped_fps_slots={} source_stats={} slots={}",
                    source,
                    frame.candidate.handle,
                    frame.candidate.mapped_from,
                    frame.width,
                    frame.height,
                    update.skipped_fps,
                    update.skipped_fps_slots,
                    format_pixel_stats(&update.stats),
                    describe_slots(&state)
                ),
                &MONITOR_RENDER_PBO_DRAIN_DIAGNOSTIC_COUNT,
                24,
            );
        }
    }

    Ok(updated)
}

#[cfg(windows)]
impl MonitorPboReadback {
    fn new(handle: u32, width: u32, height: u32) -> Self {
        Self {
            handle,
            width,
            height,
            pbos: vec![0; MONITOR_PBO_RING],
            next_index: 0,
            pending: vec![None; MONITOR_PBO_RING],
        }
    }

    fn reset_if_shape_changed(&mut self, width: u32, height: u32) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        self.next_index = 0;
        self.pending.fill(None);
    }
}

#[cfg(windows)]
fn poll_monitor_pbo_ready(
    entry: &mut MonitorPboReadback,
    functions: &GlPboFunctions,
) -> Result<Option<MonitorPboReadyFrame>, String> {
    let Some(bind_buffer) = functions.bind_buffer else {
        return Err("pbo bind function missing".to_string());
    };
    let Some(map_buffer_range) = functions.map_buffer_range else {
        return Err("pbo map function missing".to_string());
    };
    let Some(unmap_buffer) = functions.unmap_buffer else {
        return Err("pbo unmap function missing".to_string());
    };
    for index in 0..entry.pending.len() {
        let Some(pending) = entry.pending[index].take() else {
            continue;
        };
        if pending.submitted_at.elapsed() > Duration::from_millis(MONITOR_PBO_PENDING_MAX_AGE_MS) {
            let age_ms = pending.submitted_at.elapsed().as_millis();
            delete_monitor_pbo_sync(pending.sync, functions);
            return Err(format!(
                "pbo pending stale handle=0x{:x} index={} age_ms={} native={}x{} mapped_from={}",
                pending.candidate.handle,
                index,
                age_ms,
                pending.width,
                pending.height,
                pending.candidate.mapped_from
            ));
        }
        if !monitor_pbo_pending_is_ready(&pending, functions)? {
            entry.pending[index] = Some(pending);
            continue;
        }
        delete_monitor_pbo_sync(pending.sync, functions);
        unsafe {
            bind_buffer(GL_PIXEL_PACK_BUFFER, pending.pbo);
        }
        let bind_error = gl_error();
        if bind_error != GL_NO_ERROR {
            return Err(format!("glBindBuffer poll error=0x{bind_error:x}"));
        }
        let ptr = unsafe {
            map_buffer_range(
                GL_PIXEL_PACK_BUFFER,
                0,
                pending.byte_len as isize,
                GL_MAP_READ_BIT,
            )
        };
        if ptr.is_null() {
            let error = gl_error();
            return Err(format!("glMapBufferRange failed error=0x{error:x}"));
        }
        let rgba = unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), pending.byte_len) };
        let rgb = rgba
            .chunks_exact(4)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<Vec<_>>();
        let unmapped = unsafe { unmap_buffer(GL_PIXEL_PACK_BUFFER) };
        let unmap_error = gl_error();
        if unmapped == 0 || unmap_error != GL_NO_ERROR {
            return Err(format!(
                "glUnmapBuffer failed returned={} error=0x{unmap_error:x}",
                unmapped
            ));
        }
        return Ok(Some(MonitorPboReadyFrame {
            candidate: pending.candidate,
            input_slot_handle: pending.input_slot_handle,
            input_handles: pending.input_handles,
            width: pending.width,
            height: pending.height,
            rgb,
        }));
    }
    Ok(None)
}

#[cfg(windows)]
fn schedule_monitor_pbo_readback(
    entry: &mut MonitorPboReadback,
    functions: &GlPboFunctions,
    byte_len: usize,
    candidate: MonitorRenderResourceCandidate,
    input_handles: Vec<u64>,
) -> Result<bool, String> {
    ensure_monitor_pbos(entry, functions)?;
    let Some(bind_buffer) = functions.bind_buffer else {
        return Err("pbo bind function missing".to_string());
    };
    let Some(buffer_data) = functions.buffer_data else {
        return Err("pbo buffer_data function missing".to_string());
    };
    let Some(index) = next_free_monitor_pbo_slot(entry) else {
        return Ok(false);
    };
    let pbo = entry.pbos[index];
    drain_gl_errors();
    unsafe {
        bind_buffer(GL_PIXEL_PACK_BUFFER, pbo);
        buffer_data(
            GL_PIXEL_PACK_BUFFER,
            byte_len as isize,
            ptr::null(),
            GL_STREAM_READ,
        );
    }
    let buffer_error = gl_error();
    if buffer_error != GL_NO_ERROR {
        return Err(format!("glBufferData error=0x{buffer_error:x}"));
    }
    drain_gl_errors();
    unsafe {
        glGetTexImage(GL_TEXTURE_2D, 0, GL_RGBA, GL_UNSIGNED_BYTE, ptr::null_mut());
    }
    let read_error = gl_error();
    if read_error != GL_NO_ERROR {
        return Err(format!("glGetTexImage pbo error=0x{read_error:x}"));
    }
    let sync = if let Some(fence_sync) = functions.fence_sync {
        unsafe { fence_sync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0) as usize }
    } else {
        0
    };
    unsafe {
        glFlush();
    }
    let input_handles = normalize_monitor_input_handles(0, input_handles);
    let input_slot_handle = primary_monitor_input_handle(&input_handles);
    entry.pending[index] = Some(MonitorPboPending {
        candidate,
        input_slot_handle,
        input_handles,
        pbo,
        width: entry.width,
        height: entry.height,
        byte_len,
        sync,
        submitted_at: Instant::now(),
    });
    entry.next_index = (index + 1) % entry.pbos.len();
    Ok(true)
}

#[cfg(windows)]
fn schedule_monitor_fbo_pbo_readback(
    entry: &mut MonitorPboReadback,
    functions: &GlPboFunctions,
    byte_len: usize,
    candidate: MonitorRenderResourceCandidate,
    input_handles: Vec<u64>,
) -> Result<bool, String> {
    ensure_monitor_pbos(entry, functions)?;
    let Some(bind_buffer) = functions.bind_buffer else {
        return Err("pbo bind function missing".to_string());
    };
    let Some(buffer_data) = functions.buffer_data else {
        return Err("pbo buffer_data function missing".to_string());
    };
    let Some(gen_framebuffers) = functions.gen_framebuffers else {
        return Err("glGenFramebuffers missing".to_string());
    };
    let Some(bind_framebuffer) = functions.bind_framebuffer else {
        return Err("glBindFramebuffer missing".to_string());
    };
    let Some(framebuffer_texture_2d) = functions.framebuffer_texture_2d else {
        return Err("glFramebufferTexture2D missing".to_string());
    };
    let Some(check_framebuffer_status) = functions.check_framebuffer_status else {
        return Err("glCheckFramebufferStatus missing".to_string());
    };
    let Some(index) = next_free_monitor_pbo_slot(entry) else {
        return Ok(false);
    };
    let previous_read_framebuffer = current_gl_read_framebuffer_binding().unwrap_or(0);
    let pbo = entry.pbos[index];
    let mut framebuffer = 0u32;
    drain_gl_errors();
    unsafe {
        gen_framebuffers(1, &mut framebuffer);
    }
    let gen_error = gl_error();
    if gen_error != GL_NO_ERROR || framebuffer == 0 {
        return Err(format!(
            "glGenFramebuffers failed framebuffer={} error=0x{gen_error:x}",
            framebuffer
        ));
    }

    let result = (|| {
        drain_gl_errors();
        unsafe {
            bind_framebuffer(GL_READ_FRAMEBUFFER, framebuffer);
            framebuffer_texture_2d(
                GL_READ_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_TEXTURE_2D,
                candidate.handle,
                0,
            );
        }
        let attach_error = gl_error();
        if attach_error != GL_NO_ERROR {
            return Err(format!("glFramebufferTexture2D error=0x{attach_error:x}"));
        }
        let status = unsafe { check_framebuffer_status(GL_READ_FRAMEBUFFER) };
        let status_error = gl_error();
        if status_error != GL_NO_ERROR {
            return Err(format!("glCheckFramebufferStatus error=0x{status_error:x}"));
        }
        if status != GL_FRAMEBUFFER_COMPLETE {
            return Err(format!("framebuffer incomplete status=0x{status:x}"));
        }
        drain_gl_errors();
        unsafe {
            glReadBuffer(GL_COLOR_ATTACHMENT0_READ_BUFFER);
        }
        let read_buffer_error = gl_error();
        if read_buffer_error != GL_NO_ERROR {
            return Err(format!("glReadBuffer error=0x{read_buffer_error:x}"));
        }
        drain_gl_errors();
        unsafe {
            bind_buffer(GL_PIXEL_PACK_BUFFER, pbo);
            buffer_data(
                GL_PIXEL_PACK_BUFFER,
                byte_len as isize,
                ptr::null(),
                GL_STREAM_READ,
            );
        }
        let buffer_error = gl_error();
        if buffer_error != GL_NO_ERROR {
            return Err(format!("glBufferData fbo error=0x{buffer_error:x}"));
        }
        drain_gl_errors();
        unsafe {
            glReadPixels(
                0,
                0,
                entry.width as i32,
                entry.height as i32,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                ptr::null_mut(),
            );
        }
        let read_error = gl_error();
        if read_error != GL_NO_ERROR {
            return Err(format!("glReadPixels pbo error=0x{read_error:x}"));
        }
        let sync = if let Some(fence_sync) = functions.fence_sync {
            unsafe { fence_sync(GL_SYNC_GPU_COMMANDS_COMPLETE, 0) as usize }
        } else {
            0
        };
        unsafe {
            glFlush();
        }
        let input_handles = normalize_monitor_input_handles(0, input_handles);
        let input_slot_handle = primary_monitor_input_handle(&input_handles);
        entry.pending[index] = Some(MonitorPboPending {
            candidate,
            input_slot_handle,
            input_handles,
            pbo,
            width: entry.width,
            height: entry.height,
            byte_len,
            sync,
            submitted_at: Instant::now(),
        });
        entry.next_index = (index + 1) % entry.pbos.len();
        Ok(true)
    })();

    unsafe {
        framebuffer_texture_2d(
            GL_READ_FRAMEBUFFER,
            GL_COLOR_ATTACHMENT0,
            GL_TEXTURE_2D,
            0,
            0,
        );
        bind_framebuffer(GL_READ_FRAMEBUFFER, previous_read_framebuffer);
    }
    let _ = gl_error();
    if let Some(delete_framebuffers) = functions.delete_framebuffers {
        unsafe {
            delete_framebuffers(1, &framebuffer);
        }
        let _ = gl_error();
    }
    result
}

#[cfg(windows)]
fn ensure_monitor_pbos(
    entry: &mut MonitorPboReadback,
    functions: &GlPboFunctions,
) -> Result<(), String> {
    let Some(gen_buffers) = functions.gen_buffers else {
        return Err("pbo gen_buffers function missing".to_string());
    };
    if entry.pbos.len() != MONITOR_PBO_RING {
        entry.pbos.resize(MONITOR_PBO_RING, 0);
    }
    if entry.pending.len() != MONITOR_PBO_RING {
        entry.pending.resize(MONITOR_PBO_RING, None);
    }
    for pbo in &mut entry.pbos {
        if *pbo != 0 {
            continue;
        }
        unsafe {
            gen_buffers(1, pbo as *mut u32);
        }
        let error = gl_error();
        if error != GL_NO_ERROR || *pbo == 0 {
            return Err(format!(
                "glGenBuffers failed pbo={} error=0x{error:x}",
                *pbo
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn next_free_monitor_pbo_slot(entry: &MonitorPboReadback) -> Option<usize> {
    if entry.pbos.is_empty() {
        return None;
    }
    for offset in 0..entry.pbos.len() {
        let index = (entry.next_index + offset) % entry.pbos.len();
        if entry
            .pending
            .get(index)
            .is_some_and(|pending| pending.is_none())
        {
            return Some(index);
        }
    }
    None
}

#[cfg(windows)]
fn monitor_pbo_pending_is_ready(
    pending: &MonitorPboPending,
    functions: &GlPboFunctions,
) -> Result<bool, String> {
    let Some(client_wait_sync) = functions.client_wait_sync else {
        return Ok(pending.submitted_at.elapsed() > Duration::from_millis(0));
    };
    if pending.sync == 0 {
        return Ok(true);
    }
    let status =
        unsafe { client_wait_sync(pending.sync as *mut c_void, GL_SYNC_FLUSH_COMMANDS_BIT, 0) };
    match status {
        GL_ALREADY_SIGNALED | GL_CONDITION_SATISFIED => Ok(true),
        GL_TIMEOUT_EXPIRED => Ok(false),
        GL_WAIT_FAILED => Err("glClientWaitSync failed".to_string()),
        other => Err(format!("glClientWaitSync unexpected=0x{other:x}")),
    }
}

#[cfg(windows)]
fn delete_monitor_pbo_sync(sync: usize, functions: &GlPboFunctions) {
    if sync == 0 {
        return;
    }
    if let Some(delete_sync) = functions.delete_sync {
        unsafe {
            delete_sync(sync as *mut c_void);
        }
    }
}

#[cfg(windows)]
fn current_gl_pixel_pack_buffer_binding() -> Option<u32> {
    let mut binding = 0i32;
    unsafe {
        glGetIntegerv(GL_PIXEL_PACK_BUFFER_BINDING, &mut binding);
    }
    if gl_error() == GL_NO_ERROR && binding >= 0 {
        Some(binding as u32)
    } else {
        None
    }
}

#[cfg(windows)]
fn restore_gl_pixel_pack_buffer_binding(binding: u32, functions: &GlPboFunctions) {
    if let Some(bind_buffer) = functions.bind_buffer {
        unsafe {
            bind_buffer(GL_PIXEL_PACK_BUFFER, binding);
        }
        let _ = gl_error();
    }
}

#[cfg(windows)]
fn current_gl_read_framebuffer_binding() -> Option<u32> {
    let mut binding = 0i32;
    unsafe {
        glGetIntegerv(GL_READ_FRAMEBUFFER_BINDING, &mut binding);
    }
    if gl_error() == GL_NO_ERROR && binding >= 0 {
        Some(binding as u32)
    } else {
        None
    }
}

#[cfg(windows)]
fn load_gl_active_texture() -> Option<GlActiveTextureFn> {
    unsafe {
        std::mem::transmute::<*const c_void, Option<GlActiveTextureFn>>(load_wgl_proc_raw(
            "glActiveTexture",
        ))
    }
}

#[cfg(windows)]
fn load_gl_pbo_functions() -> GlPboFunctions {
    GlPboFunctions {
        gen_buffers: unsafe {
            std::mem::transmute::<*const c_void, Option<GlGenBuffersFn>>(load_wgl_proc_raw(
                "glGenBuffers",
            ))
        },
        bind_buffer: unsafe {
            std::mem::transmute::<*const c_void, Option<GlBindBufferFn>>(load_wgl_proc_raw(
                "glBindBuffer",
            ))
        },
        buffer_data: unsafe {
            std::mem::transmute::<*const c_void, Option<GlBufferDataFn>>(load_wgl_proc_raw(
                "glBufferData",
            ))
        },
        map_buffer_range: unsafe {
            std::mem::transmute::<*const c_void, Option<GlMapBufferRangeFn>>(load_wgl_proc_raw(
                "glMapBufferRange",
            ))
        },
        unmap_buffer: unsafe {
            std::mem::transmute::<*const c_void, Option<GlUnmapBufferFn>>(load_wgl_proc_raw(
                "glUnmapBuffer",
            ))
        },
        fence_sync: unsafe {
            std::mem::transmute::<*const c_void, Option<GlFenceSyncFn>>(load_wgl_proc_raw(
                "glFenceSync",
            ))
        },
        client_wait_sync: unsafe {
            std::mem::transmute::<*const c_void, Option<GlClientWaitSyncFn>>(load_wgl_proc_raw(
                "glClientWaitSync",
            ))
        },
        delete_sync: unsafe {
            std::mem::transmute::<*const c_void, Option<GlDeleteSyncFn>>(load_wgl_proc_raw(
                "glDeleteSync",
            ))
        },
        gen_framebuffers: unsafe {
            std::mem::transmute::<*const c_void, Option<GlGenFramebuffersFn>>(load_wgl_proc_raw(
                "glGenFramebuffers",
            ))
        },
        bind_framebuffer: unsafe {
            std::mem::transmute::<*const c_void, Option<GlBindFramebufferFn>>(load_wgl_proc_raw(
                "glBindFramebuffer",
            ))
        },
        framebuffer_texture_2d: unsafe {
            std::mem::transmute::<*const c_void, Option<GlFramebufferTexture2DFn>>(
                load_wgl_proc_raw("glFramebufferTexture2D"),
            )
        },
        check_framebuffer_status: unsafe {
            std::mem::transmute::<*const c_void, Option<GlCheckFramebufferStatusFn>>(
                load_wgl_proc_raw("glCheckFramebufferStatus"),
            )
        },
        delete_framebuffers: unsafe {
            std::mem::transmute::<*const c_void, Option<GlDeleteFramebuffersFn>>(load_wgl_proc_raw(
                "glDeleteFramebuffers",
            ))
        },
    }
}

#[cfg(windows)]
unsafe fn load_wgl_proc_raw(name: &str) -> *const c_void {
    let Ok(name) = CString::new(name) else {
        return ptr::null();
    };
    let proc = wglGetProcAddress(name.as_ptr());
    if valid_wgl_proc(proc) {
        proc
    } else {
        ptr::null()
    }
}

#[cfg(windows)]
fn valid_wgl_proc(proc: *const c_void) -> bool {
    let value = proc as usize;
    !proc.is_null() && value > 3 && value != usize::MAX
}

#[cfg(windows)]
impl GlPboFunctions {
    fn missing_required(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.gen_buffers.is_none() {
            missing.push("glGenBuffers");
        }
        if self.bind_buffer.is_none() {
            missing.push("glBindBuffer");
        }
        if self.buffer_data.is_none() {
            missing.push("glBufferData");
        }
        if self.map_buffer_range.is_none() {
            missing.push("glMapBufferRange");
        }
        if self.unmap_buffer.is_none() {
            missing.push("glUnmapBuffer");
        }
        missing
    }
}

#[cfg(windows)]
fn current_gl_texture_binding_2d() -> Option<u32> {
    let mut binding = 0i32;
    unsafe {
        glGetIntegerv(GL_TEXTURE_BINDING_2D, &mut binding);
    }
    if gl_error() == GL_NO_ERROR && binding >= 0 {
        Some(binding as u32)
    } else {
        None
    }
}

#[cfg(windows)]
fn current_gl_active_texture() -> Option<u32> {
    let mut active = 0i32;
    unsafe {
        glGetIntegerv(GL_ACTIVE_TEXTURE, &mut active);
    }
    if gl_error() == GL_NO_ERROR && active >= 0 {
        Some(active as u32)
    } else {
        None
    }
}

#[cfg(windows)]
fn current_gl_texture_binding_2d_for_unit(unit: u32) -> Result<u32, String> {
    let Some(active_texture) = load_gl_active_texture() else {
        return Err("glActiveTexture unavailable".to_string());
    };
    let previous_active = current_gl_active_texture();
    drain_gl_errors();
    unsafe {
        active_texture(GL_TEXTURE0 + unit);
    }
    let active_error = gl_error();
    if active_error != GL_NO_ERROR {
        if let Some(previous) = previous_active {
            unsafe {
                active_texture(previous);
            }
            let _ = gl_error();
        }
        return Err(format!(
            "glActiveTexture unit={} error=0x{active_error:x}",
            unit
        ));
    }
    let binding = current_gl_texture_binding_2d().unwrap_or(0);
    if let Some(previous) = previous_active {
        unsafe {
            active_texture(previous);
        }
        let _ = gl_error();
    }
    Ok(binding)
}

#[cfg(windows)]
fn additive_monitor_bound_texture_units() -> String {
    [0u32, 1, 2, 3, 4, 5]
        .iter()
        .map(|unit| match current_gl_texture_binding_2d_for_unit(*unit) {
            Ok(handle) => format!("unit{}=0x{:x}", unit, handle),
            Err(error) => format!("unit{}={}", unit, error),
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(windows)]
fn current_gl_i32x4(pname: u32) -> Option<[i32; 4]> {
    let mut values = [0i32; 4];
    drain_gl_errors();
    unsafe {
        glGetIntegerv(pname, values.as_mut_ptr());
    }
    (gl_error() == GL_NO_ERROR).then_some(values)
}

#[cfg(windows)]
fn describe_current_gl_rects() -> String {
    let viewport = current_gl_i32x4(GL_VIEWPORT)
        .map(|rect| format!("{},{},{},{}", rect[0], rect[1], rect[2], rect[3]))
        .unwrap_or_else(|| "unreadable".to_string());
    let scissor = current_gl_i32x4(GL_SCISSOR_BOX)
        .map(|rect| format!("{},{},{},{}", rect[0], rect[1], rect[2], rect[3]))
        .unwrap_or_else(|| "unreadable".to_string());
    format!("viewport=[{viewport}] scissor=[{scissor}]")
}

#[cfg(windows)]
fn restore_gl_texture_binding_2d(binding: u32) {
    call_original_gl_bind_texture(GL_TEXTURE_2D, binding);
    let _ = gl_error();
}

#[cfg(windows)]
fn call_original_gl_bind_texture(target: u32, texture: u32) {
    let original = GL_BIND_TEXTURE_ORIGINAL.load(Ordering::SeqCst);
    if original != 0 {
        let original: extern "system" fn(u32, u32) = unsafe { std::mem::transmute(original) };
        original(target, texture);
    } else {
        unsafe {
            glBindTexture(target, texture);
        }
    }
}

#[cfg(windows)]
fn store_original_dynamic_gl_proc(storage: &AtomicUsize, proc: *const c_void) {
    if valid_wgl_proc(proc) {
        let _ = storage.compare_exchange(0, proc as usize, Ordering::SeqCst, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn call_original_wgl_get_proc_address(name: *const c_char) -> *const c_void {
    let original = WGL_GET_PROC_ADDRESS_ORIGINAL.load(Ordering::SeqCst);
    if original != 0 {
        let original: WglGetProcAddressFn = unsafe { std::mem::transmute(original) };
        unsafe { original(name) }
    } else {
        unsafe { wglGetProcAddress(name) }
    }
}

#[cfg(windows)]
fn call_original_gl_bind_texture_unit(unit: u32, texture: u32) {
    let original = GL_BIND_TEXTURE_UNIT_ORIGINAL.load(Ordering::SeqCst);
    if original != 0 {
        let original: GlBindTextureUnitFn = unsafe { std::mem::transmute(original) };
        unsafe { original(unit, texture) };
    }
}

#[cfg(windows)]
fn call_original_gl_bind_textures(first: u32, count: i32, textures: *const u32) {
    let original = GL_BIND_TEXTURES_ORIGINAL.load(Ordering::SeqCst);
    if original != 0 {
        let original: GlBindTexturesFn = unsafe { std::mem::transmute(original) };
        unsafe { original(first, count, textures) };
    }
}

#[cfg(windows)]
fn call_original_gl_framebuffer_texture_2d(
    target: u32,
    attachment: u32,
    textarget: u32,
    texture: u32,
    level: i32,
) {
    let original = GL_FRAMEBUFFER_TEXTURE_2D_ORIGINAL.load(Ordering::SeqCst);
    if original != 0 {
        let original: GlFramebufferTexture2DFn = unsafe { std::mem::transmute(original) };
        unsafe { original(target, attachment, textarget, texture, level) };
    }
}

#[cfg(windows)]
fn call_original_gl_framebuffer_texture(target: u32, attachment: u32, texture: u32, level: i32) {
    let original = GL_FRAMEBUFFER_TEXTURE_ORIGINAL.load(Ordering::SeqCst);
    if original != 0 {
        let original: GlFramebufferTextureFn = unsafe { std::mem::transmute(original) };
        unsafe { original(target, attachment, texture, level) };
    }
}

#[cfg(windows)]
fn call_original_gl_framebuffer_texture_layer(
    target: u32,
    attachment: u32,
    texture: u32,
    level: i32,
    layer: i32,
) {
    let original = GL_FRAMEBUFFER_TEXTURE_LAYER_ORIGINAL.load(Ordering::SeqCst);
    if original != 0 {
        let original: GlFramebufferTextureLayerFn = unsafe { std::mem::transmute(original) };
        unsafe { original(target, attachment, texture, level, layer) };
    }
}

#[cfg(windows)]
fn drain_gl_errors() {
    for _ in 0..16 {
        if gl_error() == GL_NO_ERROR {
            break;
        }
    }
}

#[cfg(windows)]
fn gl_error() -> u32 {
    unsafe { glGetError() }
}

fn apply_source_texture_readback_to_slot(
    key: &SlotKey,
    readback: SourceTextureReadback,
) -> Result<SourceTextureStateUpdate, String> {
    let mut state = request_runtime_state()?;
    let capture_interval = capture_frame_interval(state.config.capture.max_fps);
    let now = Instant::now();
    let Some(slot) = state.slots.get_mut(key) else {
        set_runtime(state);
        return Err(format!(
            "source texture slot disappeared component={} slot={}",
            key.component, key.slot
        ));
    };
    if texture_upload_slot_is_rate_limited(slot, now, capture_interval) {
        state.hook_runtime.source_texture_probe_skipped_fps_slots = state
            .hook_runtime
            .source_texture_probe_skipped_fps_slots
            .saturating_add(1);
        set_runtime(state);
        return Ok(SourceTextureStateUpdate {
            updated: false,
            skipped_fps: true,
            stats: pixel_stats_from_rgb(&readback.rgb),
        });
    }
    let rgb = resize_rgb_nearest(
        &readback.rgb,
        readback.width,
        readback.height,
        slot.width,
        slot.height,
    )?;
    let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
    slot.frame_id = frame_id;
    slot.ready = true;
    slot.connected = true;
    slot.input_source_handle = readback.candidate.source_handle;
    slot.latest_frame = Some(FrameBuffer {
        frame_id,
        width: slot.width,
        height: slot.height,
        source: "source_texture".to_string(),
        rgb,
    });
    slot.source_texture_handle = Some(u64::from(readback.candidate.handle));
    slot.last_texture_upload_at = Some(now);
    state.hook_runtime.real_video_capture = true;
    state.hook_runtime.source_texture_probe_frames = state
        .hook_runtime
        .source_texture_probe_frames
        .saturating_add(1);
    let stats = pixel_stats_from_rgb(&readback.rgb);
    let hash = rgb_content_hash(&readback.rgb);
    log_runtime_diagnostic(
        &state,
        &format!(
            "source_texture captured component={} slot={} frame_id={} request={}x{} native={}x{} source={} handle=0x{:x} offset={} pointer_offset={} hash=0x{:016x} source_stats={} slots={}",
            key.component,
            key.slot,
            frame_id,
            state.slots.get(key).map(|slot| slot.width).unwrap_or(0),
            state.slots.get(key).map(|slot| slot.height).unwrap_or(0),
            readback.width,
            readback.height,
            format_hex_or_zero(readback.candidate.source_handle),
            readback.candidate.handle,
            format!("0x{:x}", readback.candidate.source_offset),
            readback
                .candidate
                .pointer_offset
                .map(|offset| format!("0x{offset:x}"))
                .unwrap_or_else(|| "none".to_string()),
            hash,
            format_pixel_stats(&stats),
            describe_slots(&state)
        ),
        &SOURCE_TEXTURE_CAPTURE_DIAGNOSTIC_COUNT,
        64,
    );
    set_runtime(state);
    Ok(SourceTextureStateUpdate {
        updated: true,
        skipped_fps: false,
        stats,
    })
}

fn record_source_texture_probe_report(report: &SourceTextureProbeReport) -> Result<(), String> {
    let mut state = request_runtime_state()?;
    state.hook_runtime.source_texture_probe_attempts = state
        .hook_runtime
        .source_texture_probe_attempts
        .saturating_add(report.attempts as u64);
    state.hook_runtime.source_texture_probe_candidates = state
        .hook_runtime
        .source_texture_probe_candidates
        .saturating_add(report.candidates as u64);
    state.hook_runtime.source_texture_probe_read_errors = state
        .hook_runtime
        .source_texture_probe_read_errors
        .saturating_add(report.read_errors as u64);
    state.hook_runtime.source_texture_probe_blank_reads = state
        .hook_runtime
        .source_texture_probe_blank_reads
        .saturating_add(report.blank_reads as u64);
    state.hook_runtime.source_texture_probe_skipped_fps_slots = state
        .hook_runtime
        .source_texture_probe_skipped_fps_slots
        .saturating_add(report.skipped_fps_slots as u64);
    if report.updated_slots == 0
        && diagnostic_budget_available(&MONITOR_RENDER_PROBE_DIAGNOSTIC_COUNT, 8)
    {
        log_runtime_diagnostic(
            &state,
            &format!(
                "source_texture probe no_frame attempts={} candidates={} read_errors={} blank_reads={} skipped_fps_slots={} details={} slots={}",
                report.attempts,
                report.candidates,
                report.read_errors,
                report.blank_reads,
                report.skipped_fps_slots,
                if report.details.is_empty() {
                    "none".to_string()
                } else {
                    report.details.join(" | ")
                },
                describe_slots(&state)
            ),
            &SOURCE_TEXTURE_PROBE_DIAGNOSTIC_COUNT,
            32,
        );
    }
    set_runtime(state);
    Ok(())
}

fn capture_frame_interval(max_fps: u32) -> Duration {
    Duration::from_secs_f64(1.0 / f64::from(normalized_capture_fps(max_fps)))
}

fn normalized_capture_fps(value: u32) -> u32 {
    value.clamp(1, 60)
}

fn texture_upload_slot_is_rate_limited(
    slot: &SlotState,
    now: Instant,
    capture_interval: Duration,
) -> bool {
    match slot.last_texture_upload_at {
        Some(previous) => now.duration_since(previous) < capture_interval,
        None => false,
    }
}

#[derive(Debug, Clone)]
struct TextureUploadFrame {
    width: u32,
    height: u32,
    format: u32,
    ty: u32,
    data_ptr: usize,
    context_ptr: usize,
    destination_texture_handle: Option<u64>,
    texture_owner_ptr: Option<u64>,
    texture_resource_ptr: Option<u64>,
    rgb: Vec<[u8; 3]>,
}

#[derive(Debug, Clone, Copy)]
struct TextureUploadDestination {
    texture_owner_ptr: u64,
    texture_resource_ptr: u64,
    texture_handle: Option<u64>,
}

fn read_texture_upload_frame(upload_context: *mut c_void) -> Result<TextureUploadFrame, String> {
    if upload_context.is_null() {
        return Err("missing texture upload context".to_string());
    }
    if !memory_range_is_readable(upload_context, 0x38) {
        return Err("texture upload context is not readable".to_string());
    }
    let base = upload_context as *const u8;
    let data = unsafe { read_unaligned_at::<usize>(base, 0x10) } as *const u8;
    let width = unsafe { read_unaligned_at::<u32>(base, 0x28) };
    let height = unsafe { read_unaligned_at::<u32>(base, 0x2c) };
    let format = unsafe { read_unaligned_at::<u32>(base, 0x30) };
    let ty = unsafe { read_unaligned_at::<u32>(base, 0x34) };
    let destination = unsafe { read_texture_upload_destination(base) };
    let mut frame = rgb_from_gl_upload(data, width, height, format, ty)?;
    frame.format = format;
    frame.ty = ty;
    frame.data_ptr = data as usize;
    frame.context_ptr = upload_context as usize;
    frame.destination_texture_handle =
        destination.and_then(|destination| destination.texture_handle);
    frame.texture_owner_ptr = destination.map(|destination| destination.texture_owner_ptr);
    frame.texture_resource_ptr = destination.map(|destination| destination.texture_resource_ptr);
    Ok(frame)
}

unsafe fn read_texture_upload_destination(base: *const u8) -> Option<TextureUploadDestination> {
    if !memory_range_is_readable(base.cast::<c_void>(), 0x38) {
        return None;
    }
    let texture_owner = read_unaligned_at::<usize>(base, 0x08) as *const u8;
    if texture_owner.is_null() {
        return None;
    }
    if !memory_range_is_readable(texture_owner.cast::<c_void>(), 0x10) {
        return None;
    }
    let texture = read_unaligned_at::<usize>(texture_owner, 0x08) as *const u8;
    if texture.is_null() {
        return None;
    }
    if !memory_range_is_readable(texture.cast::<c_void>(), 0x2c) {
        return None;
    }
    let texture_id = read_unaligned_at::<u32>(texture, 0x28);
    let texture_handle = if texture_id == 0 {
        None
    } else {
        Some(u64::from(texture_id))
    };
    Some(TextureUploadDestination {
        texture_owner_ptr: texture_owner as u64,
        texture_resource_ptr: texture as u64,
        texture_handle,
    })
}

fn rgb_from_gl_upload(
    data: *const u8,
    width: u32,
    height: u32,
    format: u32,
    ty: u32,
) -> Result<TextureUploadFrame, String> {
    if data.is_null() {
        return Err("texture upload has null pixel pointer".to_string());
    }
    if width == 0 || height == 0 {
        return Err("texture upload width and height must be >= 1".to_string());
    }
    if ty != 0x1401 {
        return Err(format!("unsupported texture upload GL type 0x{ty:x}"));
    }
    let layout = gl_upload_layout(format)?;
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| "texture upload pixel count overflow".to_string())?;
    let row_bytes = width
        .checked_mul(u32::from(layout.bytes_per_pixel()))
        .ok_or_else(|| "texture upload row byte count overflow".to_string())?;
    let row_stride = align_to(row_bytes, 4)?;
    let total_read_bytes = if height == 1 {
        row_bytes
    } else {
        row_stride
            .checked_mul(height - 1)
            .and_then(|bytes| bytes.checked_add(row_bytes))
            .ok_or_else(|| "texture upload byte count overflow".to_string())?
    };
    if !memory_range_is_readable(data.cast::<c_void>(), total_read_bytes as usize) {
        return Err("texture upload pixel buffer is not readable".to_string());
    }
    let mut rgb = Vec::with_capacity(pixel_count as usize);
    for y in 0..height as usize {
        let row_start = y
            .checked_mul(row_stride as usize)
            .ok_or_else(|| "texture upload row offset overflow".to_string())?;
        let row = unsafe { std::slice::from_raw_parts(data.add(row_start), row_bytes as usize) };
        for chunk in row.chunks_exact(layout.bytes_per_pixel() as usize) {
            match layout {
                GlUploadLayout::Gray => rgb.push([chunk[0], chunk[0], chunk[0]]),
                GlUploadLayout::Rg => rgb.push([chunk[0], chunk[0], chunk[0]]),
                GlUploadLayout::Rgb => rgb.push([chunk[0], chunk[1], chunk[2]]),
                GlUploadLayout::Rgba => rgb.push([chunk[0], chunk[1], chunk[2]]),
                GlUploadLayout::Bgr => rgb.push([chunk[2], chunk[1], chunk[0]]),
                GlUploadLayout::Bgra => rgb.push([chunk[2], chunk[1], chunk[0]]),
            }
        }
    }
    Ok(TextureUploadFrame {
        width,
        height,
        format,
        ty,
        data_ptr: data as usize,
        context_ptr: 0,
        destination_texture_handle: None,
        texture_owner_ptr: None,
        texture_resource_ptr: None,
        rgb,
    })
}

#[derive(Debug, Clone, Copy)]
enum GlUploadLayout {
    Gray,
    Rg,
    Rgb,
    Rgba,
    Bgr,
    Bgra,
}

impl GlUploadLayout {
    fn bytes_per_pixel(self) -> u8 {
        match self {
            GlUploadLayout::Gray => 1,
            GlUploadLayout::Rg => 2,
            GlUploadLayout::Rgb | GlUploadLayout::Bgr => 3,
            GlUploadLayout::Rgba | GlUploadLayout::Bgra => 4,
        }
    }
}

fn gl_upload_layout(format: u32) -> Result<GlUploadLayout, String> {
    match format {
        0x1903 | 0x1909 | 0x8d94 => Ok(GlUploadLayout::Gray), // GL_RED, GL_LUMINANCE, GL_RED_INTEGER
        0x190a | 0x8227 | 0x8228 => Ok(GlUploadLayout::Rg), // GL_LUMINANCE_ALPHA, GL_RG, GL_RG_INTEGER
        0x1907 => Ok(GlUploadLayout::Rgb),                  // GL_RGB
        0x1908 => Ok(GlUploadLayout::Rgba),                 // GL_RGBA
        0x80e0 => Ok(GlUploadLayout::Bgr),                  // GL_BGR
        0x80e1 => Ok(GlUploadLayout::Bgra),                 // GL_BGRA
        _ => Err(format!("unsupported texture upload GL format 0x{format:x}")),
    }
}

fn align_to(value: u32, alignment: u32) -> Result<u32, String> {
    if alignment == 0 {
        return Err("texture upload row alignment must be >= 1".to_string());
    }
    let add = alignment - 1;
    value
        .checked_add(add)
        .map(|value| value / alignment * alignment)
        .ok_or_else(|| "texture upload row alignment overflow".to_string())
}

fn set_texture_upload_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    if replacement == "stormworks_video_get_texture_upload_hook_arg1" {
        TEXTURE_UPLOAD_ORIGINAL_ARG1.store(value, Ordering::SeqCst);
    }
}

fn monitor_render_queue_from_hook_chained(
    monitor: *mut c_void,
    render_context: *mut c_void,
    arg3: *mut c_void,
    arg4: *mut c_void,
    arg5: *mut c_void,
    arg6: u8,
) -> Result<(), String> {
    call_monitor_render_queue_original_arg6(monitor, render_context, arg3, arg4, arg5, arg6);
    // Logic-side bridge. This hook receives the LOGIC monitor object (arg1). The logic monitor
    // carries both:
    //   - its resolved camera INPUT source (monitor_video_input_handles: reads +0x1a8/+0x1b8),
    //   - and its render resource at +0x4c8 = S (camera render source), where *(S+8) = video_object.
    // So here — and only here — we can associate a video_object (render side, what the bind hook
    // sees) with the wire-accurate camera source (logic side, what a Lua slot resolves to). We store
    // that association in a GLOBAL map (not thread-local: this hook and the bind hook run on
    // different threads). The bind hook does an O(log n) lookup, no per-bind monitor scan.
    #[cfg(windows)]
    {
        let mon = monitor as usize;
        if mon != 0 {
            let camera_source = monitor_video_input_handles(mon).effective();
            let resource_a = read_usize_field(mon, MONITOR_RENDER_RESOURCE_A_OFFSET).unwrap_or(0);
            let resource_b = read_usize_field(mon, MONITOR_RENDER_RESOURCE_B_OFFSET).unwrap_or(0);
            let mut inserted: Option<(usize, usize, u64)> = None;
            if camera_source != 0 {
                for res in [resource_a, resource_b] {
                    if res == 0 || !is_game_heap_pointer(res) {
                        continue;
                    }
                    if let Some(video_object) = read_usize_field(res, 0x8) {
                        if video_object != 0 && is_game_heap_pointer(video_object) {
                            if let Ok(mut map) = RENDERER_VIDEO_MONITOR_SOURCE_MAP.lock() {
                                if map.len() > 256 {
                                    map.clear();
                                }
                                map.insert(video_object, camera_source);
                            }
                            inserted = Some((res, video_object, camera_source));
                            break;
                        }
                    }
                }
            }
            if let Ok(mut state) = request_runtime_state() {
                let msg = match inserted {
                    Some((res, vobj, src)) => format!(
                        "mrq_bridge monitor=0x{mon:x} camera_source=0x{src:x} resource=0x{res:x} video_object=0x{vobj:x} INSERT"
                    ),
                    None => format!(
                        "mrq_bridge monitor=0x{mon:x} camera_source=0x{camera_source:x} res_a=0x{resource_a:x} res_b=0x{resource_b:x} no_insert"
                    ),
                };
                log_runtime_diagnostic_no_snapshot(&state, &msg, &MRQ_BRIDGE_DIAGNOSTIC_COUNT, 48);
                let _ = &mut state;
            }
        }
    }
    Ok(())
}

fn call_monitor_render_queue_original_arg6(
    monitor: *mut c_void,
    render_context: *mut c_void,
    arg3: *mut c_void,
    arg4: *mut c_void,
    arg5: *mut c_void,
    arg6: u8,
) {
    let trampoline = MONITOR_RENDER_QUEUE_ORIGINAL_ARG6.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        u8,
    ) = unsafe { std::mem::transmute(trampoline) };
    original(monitor, render_context, arg3, arg4, arg5, arg6);
}

fn set_monitor_render_queue_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    if replacement == "stormworks_video_get_monitor_render_queue_hook_arg6" {
        MONITOR_RENDER_QUEUE_ORIGINAL_ARG6.store(value, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn render_queue_alloc_from_hook_chained(queue: *mut c_void) -> Result<*mut c_void, String> {
    let item = call_render_queue_alloc_original_arg1(queue);
    record_render_queue_alloc_result(queue as usize, item as usize)?;
    Ok(item)
}

#[cfg(not(windows))]
fn render_queue_alloc_from_hook_chained(_queue: *mut c_void) -> Result<*mut c_void, String> {
    Ok(ptr::null_mut())
}

fn call_render_queue_alloc_original_arg1(queue: *mut c_void) -> *mut c_void {
    let trampoline = RENDER_QUEUE_ALLOC_ORIGINAL_ARG1.load(Ordering::SeqCst);
    if trampoline == 0 {
        return ptr::null_mut();
    }
    let original: extern "C" fn(*mut c_void) -> *mut c_void =
        unsafe { std::mem::transmute(trampoline) };
    original(queue)
}

fn set_render_queue_alloc_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    if replacement == "stormworks_video_get_render_queue_alloc_hook_arg1" {
        RENDER_QUEUE_ALLOC_ORIGINAL_ARG1.store(value, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn render_queue_submit_copy_from_hook_chained(
    render_context: *mut c_void,
    source_item: *mut c_void,
) -> Result<(), String> {
    call_render_queue_submit_copy_original_arg2(render_context, source_item);
    Ok(())
}

#[cfg(not(windows))]
fn render_queue_submit_copy_from_hook_chained(
    _render_context: *mut c_void,
    _source_item: *mut c_void,
) -> Result<(), String> {
    Ok(())
}

fn call_render_queue_submit_copy_original_arg2(
    render_context: *mut c_void,
    source_item: *mut c_void,
) {
    let trampoline = RENDER_QUEUE_SUBMIT_COPY_ORIGINAL_ARG2.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { std::mem::transmute(trampoline) };
    original(render_context, source_item);
}

fn set_render_queue_submit_copy_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    if replacement == "stormworks_video_get_render_queue_submit_copy_hook_arg2" {
        RENDER_QUEUE_SUBMIT_COPY_ORIGINAL_ARG2.store(value, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn push_render_queue_alloc_context(frame: RenderQueueAllocFrame) {
    RENDER_QUEUE_ALLOC_CONTEXT.with(|stack| stack.borrow_mut().push(frame));
}

#[cfg(windows)]
fn pop_render_queue_alloc_context() -> Option<RenderQueueAllocFrame> {
    RENDER_QUEUE_ALLOC_CONTEXT.with(|stack| stack.borrow_mut().pop())
}

#[cfg(windows)]
fn record_render_queue_alloc_result(queue: usize, item: usize) -> Result<(), String> {
    if item == 0 {
        return Ok(());
    }
    let frame = RENDER_QUEUE_ALLOC_CONTEXT.with(|stack| {
        let mut stack = stack.borrow_mut();
        let frame = stack.last_mut()?;
        frame.allocated_item = item;
        Some(*frame)
    });
    let Some(frame) = frame else {
        return Ok(());
    };
    if !runtime_has_video_slots() {
        return Ok(());
    }
    let kind = render_queue_alloc_kind_name(frame.kind);
    let probe = renderer_queue_item_probe(item, kind)
        .unwrap_or_else(|| render_queue_item_probe_unscored(item, kind));
    let state = request_runtime_state()?;
    log_runtime_diagnostic(
        &state,
        &format!(
            "render queue alloc kind={} queue={} owner={} source_item={} item={} monitor={} size={}x{} resource_refs=[{}->{},{}->{}] input={} relation={} slots={}",
            kind,
            format_hex_usize(queue),
            format_hex_usize(frame.owner),
            format_hex_usize(frame.source_item),
            format_hex_usize(item),
            format_hex_usize(probe.monitor),
            probe.width,
            probe.height,
            format_hex_usize(probe.resource_a_ref),
            format_hex_usize(probe.resource_a_value),
            format_hex_usize(probe.resource_b_ref),
            format_hex_usize(probe.resource_b_value),
            format_hex_or_zero(render_queue_item_input_slot_ref(item)),
            describe_render_queue_item_input_relation(&state, item),
            describe_slots(&state)
        ),
        &RENDER_QUEUE_ALLOC_DIAGNOSTIC_COUNT,
        12,
    );
    Ok(())
}

#[cfg(windows)]
fn log_monitor_render_queue_allocated_item(
    monitor: usize,
    render_context: usize,
    item: usize,
) -> Result<(), String> {
    if !runtime_has_video_slots() {
        return Ok(());
    }
    if !diagnostic_budget_available(&RENDER_QUEUE_ALLOC_DIAGNOSTIC_COUNT, 12) {
        return Ok(());
    }
    let state = request_runtime_state()?;
    let monitor_inputs = monitor_video_input_handles(monitor);
    let input_slot_ref = monitor_inputs.slot_ref;
    let item_relation = if item != 0 {
        describe_render_queue_item_input_relation(&state, item)
    } else {
        "item=0".to_string()
    };
    let probe = if item != 0 {
        renderer_queue_item_probe(item, "monitor_render_allocated")
            .unwrap_or_else(|| render_queue_item_probe_unscored(item, "monitor_render_allocated"))
    } else {
        render_queue_item_probe_empty("none")
    };
    log_runtime_diagnostic(
        &state,
        &format!(
            "monitor render queued monitor={} render_context={} item={} monitor_inputs=[{}] monitor_relation={} item_monitor={} item_size={}x{} item_resources=[{}->{},{}->{}] item_relation={} slots={}",
            format_hex_usize(monitor),
            format_hex_usize(render_context),
            format_hex_usize(item),
            monitor_inputs.summary(),
            describe_monitor_input_slot_relation(&state, input_slot_ref),
            format_hex_usize(probe.monitor),
            probe.width,
            probe.height,
            format_hex_usize(probe.resource_a_ref),
            format_hex_usize(probe.resource_a_value),
            format_hex_usize(probe.resource_b_ref),
            format_hex_usize(probe.resource_b_value),
            item_relation,
            describe_slots(&state)
        ),
        &RENDER_QUEUE_ALLOC_DIAGNOSTIC_COUNT,
        12,
    );
    Ok(())
}

#[cfg(windows)]
fn log_render_queue_submit_copy(
    render_context: usize,
    source_item: usize,
    copied_item: usize,
) -> Result<(), String> {
    if !runtime_has_video_slots() {
        return Ok(());
    }
    let state = request_runtime_state()?;
    let source_probe = renderer_queue_item_probe(source_item, "submit_source")
        .unwrap_or_else(|| render_queue_item_probe_unscored(source_item, "submit_source"));
    let copied_probe = if copied_item != 0 {
        renderer_queue_item_probe(copied_item, "submit_copy")
            .unwrap_or_else(|| render_queue_item_probe_unscored(copied_item, "submit_copy"))
    } else {
        render_queue_item_probe_empty("submit_copy_missing")
    };
    log_runtime_diagnostic(
        &state,
        &format!(
            "render queue submit copy render_context={} source={} copied={} source_monitor={} source_size={}x{} source_input={} source_relation={} copied_monitor={} copied_size={}x{} copied_input={} copied_relation={} source_resources=[{}->{},{}->{}] copied_resources=[{}->{},{}->{}] slots={}",
            format_hex_usize(render_context),
            format_hex_usize(source_item),
            format_hex_usize(copied_item),
            format_hex_usize(source_probe.monitor),
            source_probe.width,
            source_probe.height,
            format_hex_or_zero(render_queue_item_input_slot_ref(source_item)),
            describe_render_queue_item_input_relation(&state, source_item),
            format_hex_usize(copied_probe.monitor),
            copied_probe.width,
            copied_probe.height,
            format_hex_or_zero(render_queue_item_input_slot_ref(copied_item)),
            describe_render_queue_item_input_relation(&state, copied_item),
            format_hex_usize(source_probe.resource_a_ref),
            format_hex_usize(source_probe.resource_a_value),
            format_hex_usize(source_probe.resource_b_ref),
            format_hex_usize(source_probe.resource_b_value),
            format_hex_usize(copied_probe.resource_a_ref),
            format_hex_usize(copied_probe.resource_a_value),
            format_hex_usize(copied_probe.resource_b_ref),
            format_hex_usize(copied_probe.resource_b_value),
            describe_slots(&state)
        ),
        &RENDER_QUEUE_SUBMIT_DIAGNOSTIC_COUNT,
        12,
    );
    Ok(())
}

#[cfg(windows)]
fn render_queue_alloc_kind_name(kind: RenderQueueAllocKind) -> &'static str {
    match kind {
        RenderQueueAllocKind::MonitorRender => "monitor_render",
        RenderQueueAllocKind::SubmitCopy => "submit_copy",
    }
}

#[cfg(windows)]
fn render_queue_item_has_monitor_shape(item: usize) -> bool {
    if item == 0 {
        return false;
    }
    let monitor = render_queue_item_monitor(item);
    monitor != 0
        || read_u32_field(item, RENDERER_COMMAND_MONITOR_WIDTH_OFFSET)
            .zip(read_u32_field(item, RENDERER_COMMAND_MONITOR_HEIGHT_OFFSET))
            .map(|(width, height)| (1..=4096).contains(&width) && (1..=4096).contains(&height))
            .unwrap_or(false)
}

#[cfg(windows)]
fn render_queue_item_monitor(item: usize) -> usize {
    if item == 0 {
        return 0;
    }
    read_usize_field(item, RENDERER_COMMAND_MONITOR_OFFSET)
        .and_then(monitor_if_plausible)
        .unwrap_or(0)
}

#[cfg(windows)]
fn render_queue_item_input_slot_ref(item: usize) -> u64 {
    let monitor = render_queue_item_monitor(item);
    if monitor == 0 {
        return 0;
    }
    monitor_video_input_handles(monitor).slot_ref
}

#[cfg(windows)]
fn render_queue_item_input_handles(item: usize) -> MonitorVideoInputHandles {
    monitor_video_input_handles(render_queue_item_monitor(item))
}

#[cfg(windows)]
fn describe_render_queue_item_input_relation(state: &RuntimeState, item: usize) -> String {
    if item == 0 {
        return "item=0".to_string();
    }
    let input = render_queue_item_input_slot_ref(item);
    if input == 0 {
        return "input_slot_ref=0".to_string();
    }
    let handles = render_queue_item_input_handles(item);
    format!(
        "{} {}",
        handles.summary(),
        describe_monitor_input_slot_relation_with_candidates(state, handles.relation_handles())
    )
}

#[cfg(windows)]
fn render_queue_item_probe_unscored(item: usize, source: &'static str) -> RendererQueueItemProbe {
    if item == 0
        || !memory_range_is_readable(
            item as *const c_void,
            RENDERER_COMMAND_MONITOR_HEIGHT_OFFSET + size_of::<u32>(),
        )
    {
        return render_queue_item_probe_empty(source);
    }
    let raw_monitor = read_usize_field(item, RENDERER_COMMAND_MONITOR_OFFSET).unwrap_or(0);
    let monitor = monitor_if_plausible(raw_monitor).unwrap_or(0);
    let resource_a_ref =
        read_usize_field(item, RENDERER_COMMAND_RESOURCE_A_REF_OFFSET).unwrap_or(0);
    let resource_b_ref =
        read_usize_field(item, RENDERER_COMMAND_RESOURCE_B_REF_OFFSET).unwrap_or(0);
    RendererQueueItemProbe {
        base: item,
        source,
        score: 0,
        monitor,
        width: read_u32_field(item, RENDERER_COMMAND_MONITOR_WIDTH_OFFSET).unwrap_or(0),
        height: read_u32_field(item, RENDERER_COMMAND_MONITOR_HEIGHT_OFFSET).unwrap_or(0),
        resource_a_ref,
        resource_b_ref,
        resource_a_value: read_pointer_target_usize(resource_a_ref).unwrap_or(0),
        resource_b_value: read_pointer_target_usize(resource_b_ref).unwrap_or(0),
    }
}

fn render_queue_item_probe_empty(source: &'static str) -> RendererQueueItemProbe {
    RendererQueueItemProbe {
        base: 0,
        source,
        score: 0,
        monitor: 0,
        width: 0,
        height: 0,
        resource_a_ref: 0,
        resource_b_ref: 0,
        resource_a_value: 0,
        resource_b_value: 0,
    }
}

#[cfg(windows)]
fn render_target_texture_create_from_hook_chained(
    texture_slot: *mut c_void,
    width: u32,
    height: u32,
) -> Result<(), String> {
    call_render_target_texture_create_original_arg3(texture_slot, width, height);
    record_render_target_texture_binding(texture_slot as usize, width, height)
}

#[cfg(not(windows))]
fn render_target_texture_create_from_hook_chained(
    _texture_slot: *mut c_void,
    _width: u32,
    _height: u32,
) -> Result<(), String> {
    Ok(())
}

fn call_render_target_texture_create_original_arg3(
    texture_slot: *mut c_void,
    width: u32,
    height: u32,
) {
    let trampoline = RENDER_TARGET_TEXTURE_CREATE_ORIGINAL_ARG3.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(*mut c_void, u32, u32) = unsafe { std::mem::transmute(trampoline) };
    original(texture_slot, width, height);
}

fn set_render_target_texture_create_original_trampoline(
    replacement: &str,
    trampoline: Option<u64>,
) {
    let value = trampoline.unwrap_or(0) as usize;
    if replacement == "stormworks_video_get_render_target_texture_create_hook_arg3" {
        RENDER_TARGET_TEXTURE_CREATE_ORIGINAL_ARG3.store(value, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn record_render_target_texture_binding(
    texture_slot: usize,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let already_recording = RENDER_TARGET_TEXTURE_CREATE_RECORDING.with(|recording| {
        if recording.get() {
            true
        } else {
            recording.set(true);
            false
        }
    });
    if already_recording {
        return Ok(());
    }
    struct RecordingGuard;
    impl Drop for RecordingGuard {
        fn drop(&mut self) {
            RENDER_TARGET_TEXTURE_CREATE_RECORDING.with(|recording| recording.set(false));
        }
    }
    let _recording_guard = RecordingGuard;
    record_render_target_texture_binding_inner(texture_slot, width, height)
}

#[cfg(windows)]
fn record_render_target_texture_binding_inner(
    texture_slot: usize,
    width: u32,
    height: u32,
) -> Result<(), String> {
    if texture_slot == 0 || width == 0 || height == 0 {
        return Ok(());
    }
    if !memory_range_is_readable(texture_slot as *const c_void, size_of::<u32>()) {
        return Ok(());
    }
    let Some(handle) = read_u32_field(texture_slot, 0).filter(|handle| *handle != 0) else {
        return Ok(());
    };
    if !plausible_gl_texture_handle(handle) {
        return Ok(());
    }
    let resource = texture_slot.saturating_sub(ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET);
    let now = Instant::now();
    let binding = GlTextureBinding {
        handle,
        owner_ptr: resource as u64,
        texture_ptr: texture_slot as u64,
        width,
        height,
        last_seen: now,
    };
    let (log_path, known_bindings, slot_count) = {
        let mut state = runtime_cell()
            .lock()
            .map_err(|_| "runtime mutex poisoned".to_string())?;
        if !state.configured {
            return Ok(());
        }
        state
            .gl_texture_bindings
            .insert(texture_slot as u64, binding.clone());
        if resource != 0 {
            state.gl_texture_bindings.insert(resource as u64, binding);
        }
        trim_gl_texture_bindings(&mut state.gl_texture_bindings, GL_TEXTURE_BINDING_LIMIT);
        (
            state.log_path.clone(),
            state.gl_texture_bindings.len(),
            state.slots.len(),
        )
    };
    log_render_target_texture_create_binding(
        log_path,
        slot_count,
        known_bindings,
        texture_slot,
        resource,
        handle,
        width,
        height,
    );
    Ok(())
}

fn trim_gl_texture_bindings(bindings: &mut BTreeMap<u64, GlTextureBinding>, limit: usize) {
    if bindings.len() <= limit {
        return;
    }
    let remove_count = bindings.len().saturating_sub(limit);
    let mut oldest = bindings
        .iter()
        .map(|(key, binding)| (*key, binding.last_seen))
        .collect::<Vec<_>>();
    oldest.sort_by_key(|(_, last_seen)| *last_seen);
    for (key, _) in oldest.into_iter().take(remove_count) {
        bindings.remove(&key);
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn log_render_target_texture_create_binding(
    log_path: Option<PathBuf>,
    slot_count: usize,
    known_bindings: usize,
    texture_slot: usize,
    resource: usize,
    handle: u32,
    width: u32,
    height: u32,
) {
    if !verbose_runtime_diagnostics_enabled() {
        return;
    }
    let (counter, limit) = if slot_count > 0 {
        (
            &RENDER_TARGET_TEXTURE_CREATE_WITH_SLOTS_DIAGNOSTIC_COUNT,
            48usize,
        )
    } else {
        (&RENDER_TARGET_TEXTURE_CREATE_DIAGNOSTIC_COUNT, 12usize)
    };
    if counter.fetch_add(1, Ordering::Relaxed) >= limit {
        return;
    }
    let Some(path) = log_path else {
        return;
    };
    let _ = append_log(
        &path,
        &format!(
            "monitor resource texture create texture_slot={} resource={} handle=0x{:x} size={}x{} known_bindings={} slots={}",
            format_hex_or_zero(texture_slot as u64),
            format_hex_or_zero(resource as u64),
            handle,
            width,
            height,
            known_bindings,
            slot_count
        ),
    );
}

#[cfg(not(windows))]
fn record_render_target_texture_binding(
    _texture_slot: usize,
    _width: u32,
    _height: u32,
) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn renderer_video_pass_from_hook_chained(
    renderer: *mut c_void,
    render_context: *mut c_void,
    scene_state: *mut c_void,
    arg4: *mut c_void,
    command: *mut c_void,
    frame_a: *mut c_void,
    frame_b: *mut c_void,
    frame_c: *mut c_void,
) -> Result<(), String> {
    // LIGHTWEIGHT renderer_video_pass hook. We deliberately do NOT push the probe context, do NOT
    // log (which locks the runtime mutex every frame), and do NOT record events. Those are what
    // caused the earlier lag: pushing the probe context makes `gl_bind_probe_context_active()` true
    // for the whole original call, so every `glBindTexture` the game issues runs GL queries.
    //
    // All we need here is scene_state, the container that holds the monitor draw-list. We stash it
    // in a thread-local cell for the duration of the original call. The additive_monitor bind hook
    // (`140688ec0`), invoked synchronously from inside this same original call on the same thread,
    // reads the cell to walk the draw list and find the camera source object that owns the texture
    // being bound. We restore the previous value on the way out so nested passes are safe.
    let previous_scene_state =
        RENDERER_VIDEO_PASS_SCENE_STATE.with(|cell| cell.replace(scene_state as usize));
    // NOTE: the video_object→camera_source map is NOT built here. The render-side monitor list at
    // scene_state+0x558 has no logic-side input slot (+0x1a8), so scanning it yields an empty map.
    // Instead the logic-side monitor_render_queue hook (140366e90) fills the global
    // ADDITIVE_VIDEO_MONITOR_CAMERA_SOURCES map, which is shared across threads and persists across
    // passes (aged/capacity-bounded), so we must NOT overwrite or clear it here.
    call_renderer_video_pass_original_arg8(
        renderer,
        render_context,
        scene_state,
        arg4,
        command,
        frame_a,
        frame_b,
        frame_c,
    );
    RENDERER_VIDEO_PASS_SCENE_STATE.with(|cell| cell.set(previous_scene_state));
    Ok(())
}

#[cfg(not(windows))]
#[allow(clippy::too_many_arguments)]
fn renderer_video_pass_from_hook_chained(
    _renderer: *mut c_void,
    _render_context: *mut c_void,
    _scene_state: *mut c_void,
    _arg4: *mut c_void,
    _command: *mut c_void,
    _frame_a: *mut c_void,
    _frame_b: *mut c_void,
    _frame_c: *mut c_void,
) -> Result<(), String> {
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn call_renderer_video_pass_original_arg8(
    renderer: *mut c_void,
    render_context: *mut c_void,
    scene_state: *mut c_void,
    arg4: *mut c_void,
    command: *mut c_void,
    frame_a: *mut c_void,
    frame_b: *mut c_void,
    frame_c: *mut c_void,
) {
    let trampoline = RENDERER_VIDEO_PASS_ORIGINAL_ARG8.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
    ) = unsafe { std::mem::transmute(trampoline) };
    original(
        renderer,
        render_context,
        scene_state,
        arg4,
        command,
        frame_a,
        frame_b,
        frame_c,
    );
}

fn set_renderer_video_pass_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    if replacement == "stormworks_video_get_renderer_video_pass_hook_arg8" {
        RENDERER_VIDEO_PASS_ORIGINAL_ARG8.store(value, Ordering::SeqCst);
    }
}

#[cfg(windows)]
fn push_renderer_video_pass_context(frame: RendererVideoPassFrame) {
    RENDERER_VIDEO_PASS_CONTEXT.with(|stack| stack.borrow_mut().push(frame));
}

#[cfg(windows)]
fn pop_renderer_video_pass_context() {
    RENDERER_VIDEO_PASS_CONTEXT.with(|stack| {
        let _ = stack.borrow_mut().pop();
    });
}

#[cfg(windows)]
fn current_renderer_video_pass_context() -> Option<RendererVideoPassFrame> {
    RENDERER_VIDEO_PASS_CONTEXT.with(|stack| stack.borrow().last().copied())
}

#[cfg(windows)]
fn log_renderer_video_pass_hook_entry(
    frame: RendererVideoPassFrame,
    phase: &'static str,
) -> Result<(), String> {
    if !runtime_has_video_slots() {
        return Ok(());
    }
    let state = request_runtime_state()?;
    log_runtime_diagnostic(
        &state,
        &format!(
            "renderer video pass hook {} renderer={} render_context={} scene_state={} command={} frames=[{},{},{}] slots={}",
            phase,
            format_hex_usize(frame.renderer),
            format_hex_usize(frame.render_context),
            format_hex_usize(frame.scene_state),
            format_hex_usize(frame.command),
            format_hex_usize(frame.frame_a),
            format_hex_usize(frame.frame_b),
            format_hex_usize(frame.frame_c),
            describe_slots(&state)
        ),
        &RENDERER_VIDEO_PASS_DIAGNOSTIC_COUNT,
        16,
    );
    Ok(())
}

#[cfg(windows)]
fn gl_bind_probe_context_active() -> bool {
    let monitor_active = MONITOR_RENDER_GL_BIND_CONTEXT.with(|stack| !stack.borrow().is_empty());
    if monitor_active {
        return true;
    }
    let additive_active = ADDITIVE_MONITOR_GL_BIND_CONTEXT.with(|stack| !stack.borrow().is_empty());
    if additive_active {
        return true;
    }
    RENDERER_VIDEO_PASS_CONTEXT.with(|stack| !stack.borrow().is_empty())
}

#[cfg(windows)]
fn current_monitor_render_gl_bind_context() -> Option<MonitorRenderGlBindFrame> {
    MONITOR_RENDER_GL_BIND_CONTEXT.with(|stack| stack.borrow().last().copied())
}

#[cfg(windows)]
fn monitor_render_gl_bind_context_for_dynamic_gl() -> Option<MonitorRenderGlBindFrame> {
    if let Some(frame) = current_monitor_render_gl_bind_context() {
        return Some(frame);
    }
    let renderer = current_renderer_video_pass_context()?;
    let state = request_runtime_state().ok()?;
    let event = renderer_video_pass_event_from_frame(&state, renderer);
    if event.queue_monitor == 0 {
        return None;
    }
    Some(MonitorRenderGlBindFrame {
        monitor: event.queue_monitor,
        render_context: renderer.render_context,
        arg3: renderer.scene_state,
        arg4: renderer.command,
        arg5: event.queue_item,
        arg6: 0,
        bind_index: 0,
    })
}

#[cfg(windows)]
fn record_renderer_video_pass_event(frame: RendererVideoPassFrame) -> Result<(), String> {
    if !runtime_has_video_slots() {
        return Ok(());
    }
    if runtime_video_slots_all_ready_for_lua() {
        return Ok(());
    }
    let mut state = request_runtime_state()?;
    let event = renderer_video_pass_event_from_frame(&state, frame);
    state.renderer_video_pass_events.push(event.clone());
    if state.renderer_video_pass_events.len() > RENDERER_VIDEO_PASS_EVENT_LIMIT {
        let remove_count = state
            .renderer_video_pass_events
            .len()
            .saturating_sub(RENDERER_VIDEO_PASS_EVENT_LIMIT);
        state.renderer_video_pass_events.drain(0..remove_count);
    }
    set_runtime(state.clone());
    log_runtime_diagnostic(
        &state,
        &format!(
            "renderer video pass renderer={} render_context={} scene_state={} command={} frames=[a:{} tex=0x{:x},b:{} tex=0x{:x},c:{} tex=0x{:x}] targets=[primary={},secondary={},video={}] queue=[item={} from={} score={} monitor={} size={}x{} resource_refs=[{}->{},{}->{}] input_object={} input_ref={} effective={} input_relation={}] flags=[0xc8=0x{:x},0xd8=0x{:x},0xdc=0x{:x}] object_relation={} source_relation={} slots={}",
            format_hex_usize(event.renderer),
            format_hex_usize(event.render_context),
            format_hex_usize(event.scene_state),
            format_hex_usize(event.command),
            format_hex_usize(event.frame_a),
            event.frame_a_texture,
            format_hex_usize(event.frame_b),
            event.frame_b_texture,
            format_hex_usize(event.frame_c),
            event.frame_c_texture,
            format_hex_usize(event.render_target_primary),
            format_hex_usize(event.render_target_secondary),
            format_hex_usize(event.render_target_video),
            format_hex_usize(event.queue_item),
            event.queue_item_from,
            event.queue_item_score,
            format_hex_usize(event.queue_monitor),
            event.queue_width,
            event.queue_height,
            format_hex_usize(event.queue_resource_a_ref),
            format_hex_usize(event.queue_resource_a_value),
            format_hex_usize(event.queue_resource_b_ref),
            format_hex_usize(event.queue_resource_b_value),
            format_hex_or_zero(event.queue_monitor_input_slot_object),
            format_hex_or_zero(event.queue_monitor_input_slot_ref),
            format_hex_or_zero(event.queue_monitor_effective_handle),
            event.queue_monitor_input_relation,
            event.command_flags_0xc8,
            event.command_flags_0xd8,
            event.command_flags_0xdc,
            event.object_relation,
            event.source_relation,
            event.slots
        ),
        &RENDERER_VIDEO_PASS_DIAGNOSTIC_COUNT,
        8,
    );
    Ok(())
}

#[cfg(windows)]
fn renderer_video_pass_event_from_frame(
    state: &RuntimeState,
    frame: RendererVideoPassFrame,
) -> RendererVideoPassEvent {
    let render_target_primary = read_usize_field(frame.command, 0x78).unwrap_or(0);
    let render_target_secondary = read_usize_field(frame.command, 0x88).unwrap_or(0);
    let render_target_video =
        read_usize_field(frame.command, 0xf * size_of::<usize>()).unwrap_or(0);
    let queue_item = renderer_queue_item_probe_from_frame(frame);
    let queue_item_score = queue_item.score;
    let queue_monitor = queue_item.monitor;
    let queue_width = queue_item.width;
    let queue_height = queue_item.height;
    let queue_resource_a_ref = queue_item.resource_a_ref;
    let queue_resource_b_ref = queue_item.resource_b_ref;
    let queue_resource_a_value = queue_item.resource_a_value;
    let queue_resource_b_value = queue_item.resource_b_value;
    let queue_monitor_inputs = monitor_video_input_handles(queue_monitor);
    let queue_monitor_input_slot_object = queue_monitor_inputs.slot_object;
    let queue_monitor_input_slot_ref = queue_monitor_inputs.slot_ref;
    let queue_monitor_effective_handle = queue_monitor_inputs.effective();
    let queue_monitor_input_relation = describe_monitor_input_slot_relation_with_candidates(
        state,
        queue_monitor_inputs.relation_handles(),
    );
    let frame_a_texture = read_u32_field(
        frame.frame_a,
        ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
    )
    .unwrap_or(0);
    let frame_b_texture =
        read_u32_field(frame.frame_b, ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET).unwrap_or(0);
    let frame_c_texture =
        read_u32_field(frame.frame_c, ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET).unwrap_or(0);
    let source_relation = describe_renderer_video_pass_relation(
        state,
        frame,
        queue_monitor_input_slot_ref,
        queue_monitor_effective_handle,
        [
            queue_resource_a_ref as u64,
            queue_resource_b_ref as u64,
            queue_resource_a_value as u64,
            queue_resource_b_value as u64,
        ],
    );
    let object_relation = describe_renderer_video_pass_object_relation(
        state,
        frame,
        &queue_item,
        [
            render_target_primary,
            render_target_secondary,
            render_target_video,
            queue_resource_a_ref,
            queue_resource_b_ref,
            queue_resource_a_value,
            queue_resource_b_value,
        ],
    );
    RendererVideoPassEvent {
        renderer: frame.renderer,
        render_context: frame.render_context,
        scene_state: frame.scene_state,
        command: frame.command,
        frame_a: frame.frame_a,
        frame_b: frame.frame_b,
        frame_c: frame.frame_c,
        frame_a_texture,
        frame_b_texture,
        frame_c_texture,
        render_target_primary,
        render_target_secondary,
        render_target_video,
        queue_item: queue_item.base,
        queue_item_from: queue_item.source,
        queue_item_score,
        queue_monitor,
        queue_width,
        queue_height,
        queue_resource_a_ref,
        queue_resource_b_ref,
        queue_resource_a_value,
        queue_resource_b_value,
        queue_monitor_input_slot_object,
        queue_monitor_input_slot_ref,
        queue_monitor_effective_handle,
        queue_monitor_input_relation,
        command_flags_0xc8: read_u32_field(frame.command, 0xc8).unwrap_or(0),
        command_flags_0xd8: read_u32_field(frame.command, 0xd8).unwrap_or(0),
        command_flags_0xdc: read_u32_field(frame.command, 0xdc).unwrap_or(0),
        object_relation,
        source_relation,
        slots: describe_slots(state),
        observed_at: Instant::now(),
    }
}

#[cfg(windows)]
fn renderer_queue_item_probe_from_frame(frame: RendererVideoPassFrame) -> RendererQueueItemProbe {
    let state = runtime_snapshot();
    let mut probes = Vec::new();
    probes.extend(renderer_queue_item_probes_from_context_queue(
        &state,
        frame.scene_state,
        "scene_state+0x5a0",
    ));
    probes.extend(renderer_queue_item_probes_from_context_queue(
        &state,
        frame.render_context,
        "render_context+0x5a0",
    ));
    probes.extend(
        [
            ("scene_state", frame.scene_state),
            ("command", frame.command),
            ("frame_a", frame.frame_a),
            ("frame_b", frame.frame_b),
            ("frame_c", frame.frame_c),
        ]
        .into_iter()
        .filter_map(|(source, base)| renderer_queue_item_probe_with_state(&state, base, source)),
    );
    probes
        .into_iter()
        .max_by_key(|probe| probe.score)
        .unwrap_or_else(|| render_queue_item_probe_empty("none"))
}

#[cfg(windows)]
fn renderer_queue_item_probe(base: usize, source: &'static str) -> Option<RendererQueueItemProbe> {
    let state = runtime_snapshot();
    renderer_queue_item_probe_with_state(&state, base, source)
}

#[cfg(windows)]
fn renderer_queue_item_probe_with_state(
    state: &RuntimeState,
    base: usize,
    source: &'static str,
) -> Option<RendererQueueItemProbe> {
    if base == 0
        || !memory_range_is_readable(
            base as *const c_void,
            RENDERER_COMMAND_MONITOR_HEIGHT_OFFSET + size_of::<u32>(),
        )
    {
        return None;
    }
    let raw_monitor = read_usize_field(base, RENDERER_COMMAND_MONITOR_OFFSET).unwrap_or(0);
    let monitor = monitor_if_plausible(raw_monitor).unwrap_or(0);
    let width = read_u32_field(base, RENDERER_COMMAND_MONITOR_WIDTH_OFFSET).unwrap_or(0);
    let height = read_u32_field(base, RENDERER_COMMAND_MONITOR_HEIGHT_OFFSET).unwrap_or(0);
    let resource_a_ref =
        read_usize_field(base, RENDERER_COMMAND_RESOURCE_A_REF_OFFSET).unwrap_or(0);
    let resource_b_ref =
        read_usize_field(base, RENDERER_COMMAND_RESOURCE_B_REF_OFFSET).unwrap_or(0);
    let resource_a_value = read_pointer_target_usize(resource_a_ref).unwrap_or(0);
    let resource_b_value = read_pointer_target_usize(resource_b_ref).unwrap_or(0);
    let mut score = 0usize;
    if monitor != 0 {
        score = score.saturating_add(8);
    }
    if (1..=4096).contains(&width) && (1..=4096).contains(&height) {
        score = score.saturating_add(4);
    }
    if monitor != 0 && resource_a_ref == monitor.saturating_add(MONITOR_RENDER_RESOURCE_A_OFFSET) {
        score = score.saturating_add(8);
    }
    if monitor != 0 && resource_b_ref == monitor.saturating_add(MONITOR_RENDER_RESOURCE_B_OFFSET) {
        score = score.saturating_add(8);
    }
    if resource_a_value != 0 {
        score = score.saturating_add(1);
    }
    if resource_b_value != 0 {
        score = score.saturating_add(1);
    }
    let input_slot_ref = if monitor != 0 {
        monitor_video_input_handles(monitor).slot_ref
    } else {
        0
    };
    if input_slot_ref != 0 {
        let monitor_inputs = monitor_video_input_handles(monitor);
        score = score.saturating_add(3);
        if state.slots.values().any(|slot| {
            slot.connected
                && monitor_inputs
                    .relation_handles()
                    .into_iter()
                    .any(|handle| slot_matches_input_handle_or_source_key(slot, handle))
        }) {
            score = score.saturating_add(32);
        }
    }
    if score == 0 {
        return None;
    }
    Some(RendererQueueItemProbe {
        base,
        source,
        score,
        monitor,
        width,
        height,
        resource_a_ref,
        resource_b_ref,
        resource_a_value,
        resource_b_value,
    })
}

#[cfg(windows)]
fn renderer_queue_item_probes_from_context_queue(
    state: &RuntimeState,
    context: usize,
    source: &'static str,
) -> Vec<RendererQueueItemProbe> {
    let Some(queue) = context.checked_add(RENDER_CONTEXT_SUBMIT_QUEUE_OFFSET) else {
        return Vec::new();
    };
    renderer_queue_item_probes_from_queue(state, queue, source)
}

#[cfg(windows)]
fn renderer_queue_item_probes_from_queue(
    state: &RuntimeState,
    queue: usize,
    source: &'static str,
) -> Vec<RendererQueueItemProbe> {
    if queue == 0
        || !memory_range_is_readable(
            queue as *const c_void,
            RENDER_QUEUE_COUNT_OFFSET + size_of::<u32>(),
        )
    {
        return Vec::new();
    }
    let buffer = read_usize_field(queue, RENDER_QUEUE_BUFFER_OFFSET).unwrap_or(0);
    let capacity = read_u32_field(queue, RENDER_QUEUE_CAPACITY_OFFSET).unwrap_or(0) as usize;
    let start = read_u32_field(queue, RENDER_QUEUE_START_OFFSET).unwrap_or(0) as usize;
    let count = read_u32_field(queue, RENDER_QUEUE_COUNT_OFFSET).unwrap_or(0) as usize;
    if buffer == 0
        || !pointer_value_looks_process_address(buffer as u64)
        || capacity == 0
        || capacity > 4096
        || count == 0
        || count > capacity
    {
        return Vec::new();
    }
    let scan = count.min(RENDER_QUEUE_SCAN_LIMIT);
    let mut probes = Vec::new();
    let mut seen = BTreeSet::new();
    for offset in 0..scan {
        let index = (start + offset) % capacity;
        let Some(item) = buffer.checked_add(index.saturating_mul(RENDER_QUEUE_ITEM_SIZE)) else {
            continue;
        };
        if seen.insert(item) {
            if let Some(probe) = renderer_queue_item_probe_with_state(state, item, source) {
                probes.push(probe);
            }
        }
    }
    probes
}

#[cfg(windows)]
fn describe_renderer_video_pass_relation(
    state: &RuntimeState,
    frame: RendererVideoPassFrame,
    queue_monitor_input_slot_ref: u64,
    queue_monitor_effective_handle: u64,
    queue_resource_handles: [u64; 4],
) -> String {
    let mut parts = Vec::new();
    if queue_monitor_input_slot_ref != 0 {
        parts.push(format!(
            "queue_monitor_input={} {}",
            format_hex_or_zero(queue_monitor_input_slot_ref),
            describe_logic_video_ref(queue_monitor_input_slot_ref)
        ));
        let relation = describe_monitor_input_slot_relation(state, queue_monitor_input_slot_ref);
        if !relation.starts_with("no_relation") {
            parts.push(format!("queue_monitor_input_relation={relation}"));
        }
    }
    if queue_monitor_effective_handle != 0
        && queue_monitor_effective_handle != queue_monitor_input_slot_ref
    {
        parts.push(format!(
            "queue_monitor_effective={}",
            format_hex_or_zero(queue_monitor_effective_handle)
        ));
    }
    for slot in state.slots.values() {
        for (label, handle) in slot_input_handles(slot) {
            let handle_usize = handle as usize;
            if pointer_range_contains_u64(frame.command as u64, 0x180, handle) {
                parts.push(format!("{} command_contains_{label}", slot_key_label(slot)));
            }
            if pointer_range_contains_u64(frame.scene_state as u64, 0x900, handle) {
                parts.push(format!(
                    "{} scene_state_contains_{label}",
                    slot_key_label(slot)
                ));
            }
            if frame.frame_a == handle_usize
                || frame.frame_b == handle_usize
                || frame.frame_c == handle_usize
            {
                parts.push(format!("{} frame_arg_is_{label}", slot_key_label(slot)));
            }
            if frame.command == handle_usize || frame.scene_state == handle_usize {
                parts.push(format!("{} context_arg_is_{label}", slot_key_label(slot)));
            }
            if queue_monitor_effective_handle == handle {
                parts.push(format!(
                    "{} queue_effective_is_{label}",
                    slot_key_label(slot)
                ));
            }
            if queue_monitor_input_slot_ref == handle {
                parts.push(format!("{} queue_input_is_{label}", slot_key_label(slot)));
            }
            for resource_handle in queue_resource_handles {
                if resource_handle == handle {
                    parts.push(format!(
                        "{} queue_resource_is_{label}",
                        slot_key_label(slot)
                    ));
                } else if pointer_range_contains_u64(resource_handle, 0x120, handle) {
                    parts.push(format!(
                        "{} queue_resource_contains_{label}",
                        slot_key_label(slot)
                    ));
                }
            }
        }
    }
    if parts.is_empty() {
        "no_slot_relation".to_string()
    } else {
        parts.join(",")
    }
}

#[cfg(windows)]
fn describe_renderer_video_pass_object_relation(
    state: &RuntimeState,
    frame: RendererVideoPassFrame,
    queue_item: &RendererQueueItemProbe,
    related_objects: [usize; 7],
) -> String {
    let mut parts = Vec::new();
    let object_specs = [
        ("command", frame.command, 0x220usize),
        ("scene_state", frame.scene_state, 0x900usize),
        ("render_context", frame.render_context, 0x900usize),
        ("frame_a", frame.frame_a, 0x180usize),
        ("frame_b", frame.frame_b, 0x180usize),
        ("frame_c", frame.frame_c, 0x180usize),
        ("queue_item", queue_item.base, RENDER_QUEUE_ITEM_SIZE),
        ("queue_monitor", queue_item.monitor, 0x600usize),
        ("target_primary", related_objects[0], 0x180usize),
        ("target_secondary", related_objects[1], 0x180usize),
        ("target_video", related_objects[2], 0x180usize),
        ("resource_a_ref", related_objects[3], 0x120usize),
        ("resource_b_ref", related_objects[4], 0x120usize),
        ("resource_a_value", related_objects[5], 0x180usize),
        ("resource_b_value", related_objects[6], 0x180usize),
    ];
    for slot in state.slots.values() {
        let slot_label = slot_key_label(slot);
        for (handle_label, handle) in slot_input_handles(slot) {
            for (object_label, object, scan_bytes) in object_specs {
                if object == 0 {
                    continue;
                }
                if object as u64 == handle {
                    parts.push(format!("{slot_label} {object_label}_is_{handle_label}"));
                    continue;
                }
                for path in
                    pointer_graph_contains_u64_paths(object as u64, scan_bytes, 0x180, handle, 1)
                        .into_iter()
                        .take(2)
                {
                    parts.push(format!(
                        "{slot_label} {object_label}_contains_{handle_label}@{path}"
                    ));
                }
            }
            let handle_structural_values = video_source_structural_values(handle);
            for (object_label, object, scan_bytes) in object_specs {
                if object == 0 {
                    continue;
                }
                let matched_values = object_video_source_structural_match_count(
                    object,
                    scan_bytes,
                    &handle_structural_values,
                );
                if matched_values >= 3 {
                    parts.push(format!(
                        "{slot_label} {object_label}_matches_{handle_label}_source_key values={matched_values}"
                    ));
                }
            }
        }
    }
    dedup_relation_parts(parts, 32).unwrap_or_else(|| "no_object_relation".to_string())
}

fn dedup_relation_parts(parts: Vec<String>, limit: usize) -> Option<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for part in parts {
        if seen.insert(part.clone()) {
            deduped.push(part);
        }
        if deduped.len() >= limit {
            break;
        }
    }
    if deduped.is_empty() {
        None
    } else {
        Some(deduped.join(","))
    }
}

fn video_source_handle_structural_key(handle: u64) -> Option<u64> {
    let values = video_source_structural_values(handle);
    video_source_structural_key_from_values(&values)
}

fn video_source_structural_key_from_values(values: &[u64]) -> Option<u64> {
    if values.len() < 3 {
        return None;
    }
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    for value in values.iter().take(8) {
        key ^= *value;
        key = key.wrapping_mul(0x1000_0000_01b3);
    }
    Some(key)
}

fn video_source_structural_values(handle: u64) -> Vec<u64> {
    if handle == 0 || !pointer_value_looks_process_address(handle) {
        return Vec::new();
    }
    let base = handle as usize;
    if !memory_range_is_readable(base as *const c_void, INPUT_VIDEO_SOURCE_LAYOUT_BYTES) {
        return Vec::new();
    }
    let mut values = (0..INPUT_VIDEO_SOURCE_LAYOUT_BYTES)
        .step_by(size_of::<usize>())
        .filter_map(|offset| read_usize_field(base, offset).map(|value| value as u64))
        .filter(|value| video_source_structural_value_is_useful(*value))
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    values
}

fn video_source_structural_value_is_useful(value: u64) -> bool {
    value != 0
        && !pointer_value_looks_process_address(value)
        && (value >> 32) != 0
        && (value & 0xffff_ffff) != 0
        && value != 0x3f80_0000_0000_0005
}

fn object_video_source_structural_match_count(
    object: usize,
    scan_bytes: usize,
    source_values: &[u64],
) -> usize {
    if object == 0 || source_values.len() < 3 {
        return 0;
    }
    let mut matches = 0usize;
    for value in source_values {
        if pointer_range_contains_u64(object as u64, scan_bytes, *value) {
            matches = matches.saturating_add(1);
        }
    }
    matches
}

#[cfg(windows)]
fn push_monitor_render_gl_bind_context(frame: MonitorRenderGlBindFrame) {
    MONITOR_RENDER_GL_BIND_CONTEXT.with(|stack| stack.borrow_mut().push(frame));
}

#[cfg(windows)]
fn pop_monitor_render_gl_bind_context() {
    MONITOR_RENDER_GL_BIND_CONTEXT.with(|stack| {
        let _ = stack.borrow_mut().pop();
    });
}

#[cfg(windows)]
fn monitor_render_gl_bind_texture_after_original(target: u32, texture: u32) -> Result<(), String> {
    monitor_render_gl_texture_observation_after_original("glBindTexture", None, target, texture)
}

#[cfg(windows)]
fn monitor_render_gl_texture_observation_after_original(
    api: &'static str,
    explicit_unit: Option<u32>,
    target: u32,
    texture: u32,
) -> Result<(), String> {
    let frame = MONITOR_RENDER_GL_BIND_CONTEXT
        .with(|stack| {
            let mut stack = stack.borrow_mut();
            let frame = stack.last_mut()?;
            frame.bind_index = frame.bind_index.saturating_add(1);
            Some(*frame)
        })
        .or_else(monitor_render_gl_bind_context_for_dynamic_gl);
    let Some(frame) = frame else {
        return Ok(());
    };
    if target != GL_TEXTURE_2D || texture == 0 || !runtime_has_video_slots() {
        return Ok(());
    }
    let active_unit = explicit_unit.unwrap_or_else(|| {
        current_gl_active_texture()
            .map(|active| active.saturating_sub(GL_TEXTURE0))
            .unwrap_or(u32::MAX)
    });
    let (width, height) = current_gl_texture_size_for_handle(texture).unwrap_or((0, 0));
    record_monitor_render_gl_bind_event(api, frame, active_unit, texture, width, height)
}

#[cfg(windows)]
fn record_monitor_render_gl_bind_event(
    api: &'static str,
    frame: MonitorRenderGlBindFrame,
    active_unit: u32,
    texture: u32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let mut state = request_runtime_state()?;
    let monitor_inputs = monitor_video_input_handles(frame.monitor);
    let input_slot_ref = monitor_inputs.slot_ref;
    let effective_input_handle = monitor_inputs.effective();
    let input_slot_relation = describe_monitor_input_slot_relation_with_candidates(
        &state,
        monitor_inputs.relation_handles(),
    );
    let source_relation = describe_source_relation_to_slots(&state, effective_input_handle);
    let slots = describe_slots(&state);
    let event = MonitorGlBindEvent {
        monitor: frame.monitor,
        render_context: frame.render_context,
        arg3: frame.arg3,
        arg4: frame.arg4,
        arg5: frame.arg5,
        arg6: frame.arg6,
        bind_index: frame.bind_index,
        active_unit,
        texture,
        width,
        height,
        input_slot_object: monitor_inputs.slot_object,
        input_slot_ref,
        input_effective_handle: effective_input_handle,
        input_slot_relation,
        source_relation,
        slots,
        observed_at: Instant::now(),
    };
    state.monitor_gl_bind_events.push(event.clone());
    if state.monitor_gl_bind_events.len() > MONITOR_GL_BIND_EVENT_LIMIT {
        let remove_count = state
            .monitor_gl_bind_events
            .len()
            .saturating_sub(MONITOR_GL_BIND_EVENT_LIMIT);
        state.monitor_gl_bind_events.drain(0..remove_count);
    }
    set_runtime(state.clone());
    log_runtime_diagnostic(
        &state,
        &format!(
            "monitor render gl bind api={} monitor={} size={}x{} input_slot_object={} input_slot_ref={} input_effective={} input_relation={} source_relation={} bind_index={} unit={} texture=0x{:x} texture_size={}x{} args=[ctx={},arg3={},arg4={},arg5={},arg6={}] slots={}",
            api,
            format_hex_usize(frame.monitor),
            read_u32_field(frame.monitor, MONITOR_WIDTH_OFFSET).unwrap_or(0),
            read_u32_field(frame.monitor, MONITOR_HEIGHT_OFFSET).unwrap_or(0),
            format_hex_or_zero(event.input_slot_object),
            format_hex_or_zero(input_slot_ref),
            format_hex_or_zero(effective_input_handle),
            event.input_slot_relation,
            event.source_relation,
            event.bind_index,
            event.active_unit,
            event.texture,
            event.width,
            event.height,
            format_hex_usize(frame.render_context),
            format_hex_usize(frame.arg3),
            format_hex_usize(frame.arg4),
            format_hex_usize(frame.arg5),
            frame.arg6,
            event.slots
        ),
        &MONITOR_GL_BIND_DIAGNOSTIC_COUNT,
        32,
    );
    Ok(())
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn record_dynamic_gl_framebuffer_texture_observation(
    api: &'static str,
    target: u32,
    attachment: u32,
    textarget: u32,
    texture: u32,
    level: i32,
    layer: Option<i32>,
) -> Result<(), String> {
    if texture == 0 || !runtime_has_video_slots() {
        return Ok(());
    }
    let Some(frame) = monitor_render_gl_bind_context_for_dynamic_gl() else {
        return Ok(());
    };
    let (width, height) = current_gl_texture_size_for_handle(texture).unwrap_or((0, 0));
    let state = request_runtime_state()?;
    log_runtime_diagnostic(
        &state,
        &format!(
            "dynamic gl framebuffer texture api={} monitor={} monitor_size={}x{} target=0x{:x} attachment=0x{:x} textarget=0x{:x} texture=0x{:x} texture_size={}x{} level={} layer={} rects={} slots={}",
            api,
            format_hex_usize(frame.monitor),
            read_u32_field(frame.monitor, MONITOR_WIDTH_OFFSET).unwrap_or(0),
            read_u32_field(frame.monitor, MONITOR_HEIGHT_OFFSET).unwrap_or(0),
            target,
            attachment,
            textarget,
            texture,
            width,
            height,
            level,
            layer.map(|value| value.to_string()).unwrap_or_else(|| "none".to_string()),
            describe_current_gl_rects(),
            describe_slots(&state)
        ),
        &DYNAMIC_GL_BIND_DIAGNOSTIC_COUNT,
        48,
    );
    Ok(())
}

#[cfg(windows)]
fn push_additive_monitor_gl_bind_context(frame: AdditiveMonitorGlBindFrame) {
    ADDITIVE_MONITOR_GL_BIND_CONTEXT.with(|stack| stack.borrow_mut().push(frame));
}

#[cfg(windows)]
fn pop_additive_monitor_gl_bind_context() {
    ADDITIVE_MONITOR_GL_BIND_CONTEXT.with(|stack| {
        let _ = stack.borrow_mut().pop();
    });
}

#[cfg(windows)]
fn additive_monitor_gl_bind_texture_after_original(
    target: u32,
    texture: u32,
) -> Result<(), String> {
    let frame = ADDITIVE_MONITOR_GL_BIND_CONTEXT.with(|stack| {
        let mut stack = stack.borrow_mut();
        let frame = stack.last_mut()?;
        frame.bind_index = frame.bind_index.saturating_add(1);
        Some(*frame)
    });
    let Some(frame) = frame else {
        return Ok(());
    };
    let active_unit = current_gl_active_texture()
        .map(|active| active.saturating_sub(GL_TEXTURE0))
        .unwrap_or(u32::MAX);
    if target != GL_TEXTURE_2D || !runtime_has_video_slots() {
        return Ok(());
    }
    if active_unit == 3 && ADDITIVE_GL_BIND_UNIT_DIAGNOSTIC_COUNT.load(Ordering::Relaxed) < 12 {
        log_additive_gl_bind_unit_observation(
            frame,
            active_unit,
            texture,
            describe_current_bound_gl_texture_2d(texture),
        )?;
    }
    if active_unit != 3 {
        return Ok(());
    }
    let candidate = frame.candidate(texture, active_unit);
    if texture == 0 {
        log_additive_gl_bind_diagnostic(frame, candidate, active_unit, "zero_texture".to_string())?;
        return Ok(());
    }
    let Some(context) = record_additive_gl_bind_renderer_context(frame, texture, active_unit)?
    else {
        return Ok(());
    };
    probe_additive_gl_bind_candidate(frame, candidate, active_unit, context)
}

#[cfg(windows)]
fn record_additive_gl_bind_renderer_context(
    frame: AdditiveMonitorGlBindFrame,
    texture: u32,
    unit: u32,
) -> Result<Option<AdditiveGlBindContext>, String> {
    let Some(renderer_pass) = frame.renderer_pass else {
        return Ok(None);
    };
    let state = request_runtime_state()?;
    let event = renderer_video_pass_event_from_frame(&state, renderer_pass);
    let draw_item_inputs = monitor_from_additive_draw_item(frame.draw_item)
        .map(monitor_video_input_handles)
        .unwrap_or_default();
    let candidate_handles = [
        event.queue_monitor_input_slot_ref,
        event.queue_monitor_effective_handle,
        event.queue_monitor_input_slot_object,
        input_video_source_handles_from_any(event.queue_monitor_input_slot_ref).effective(),
        input_video_source_handles_from_any(event.queue_monitor_input_slot_object).effective(),
        draw_item_inputs.slot_ref,
        draw_item_inputs.effective(),
    ];
    let mut input_handles = [0u64; 6];
    let mut input_handle_count = 0usize;
    for value in candidate_handles {
        if value == 0 || input_handles.contains(&value) || input_handle_count >= input_handles.len()
        {
            continue;
        }
        input_handles[input_handle_count] = value;
        input_handle_count = input_handle_count.saturating_add(1);
    }
    let input_handle = input_handles
        .iter()
        .copied()
        .find(|value| *value != 0)
        .unwrap_or(0);
    if input_handle == 0 {
        return Ok(None);
    }
    let exact_slots = monitor_render_probe_slots_for_handles(
        input_handles
            .into_iter()
            .filter(|value| *value != 0)
            .collect(),
    )?;
    if exact_slots.is_empty() {
        return Ok(None);
    }
    log_runtime_diagnostic(
        &state,
        &format!(
            "additive gl bind renderer context unit={} texture=0x{:x} material={} draw_item={} texture_video={} object={} renderer={} command={} frame_a={} frame_a_tex=0x{:x} targets=[primary={},secondary={},video={}] queue=[item={} from={} score={} monitor={} input_object={} input_ref={} effective={} input_relation={}] relation={} slots={}",
            unit,
            texture,
            format_hex_usize(frame.material),
            format_hex_usize(frame.draw_item),
            format_hex_usize(frame.texture_video),
            format_hex_usize(frame.video_texture_object),
            format_hex_usize(event.renderer),
            format_hex_usize(event.command),
            format_hex_usize(event.frame_a),
            event.frame_a_texture,
            format_hex_usize(event.render_target_primary),
            format_hex_usize(event.render_target_secondary),
            format_hex_usize(event.render_target_video),
            format_hex_usize(event.queue_item),
            event.queue_item_from,
            event.queue_item_score,
            format_hex_usize(event.queue_monitor),
            format_hex_or_zero(event.queue_monitor_input_slot_object),
            format_hex_or_zero(event.queue_monitor_input_slot_ref),
            format_hex_or_zero(event.queue_monitor_effective_handle),
            event.queue_monitor_input_relation,
            event.source_relation,
            event.slots
        ),
        &RENDERER_VIDEO_PASS_DIAGNOSTIC_COUNT,
        8,
    );
    log_runtime_diagnostic(
        &state,
        &format!(
            "additive bind args bind_index={} unit={} {} args=[texture_video={},texture_overlay={},arg5_object={},arg6={},arg7={},arg8={},arg10={},arg13={},arg16={}] layouts=[{} | {} | {} | {} | {} | {}] floats=[{} | {} | {} | {}]",
            frame.bind_index,
            unit,
            describe_current_gl_rects(),
            format_hex_usize(frame.texture_video),
            format_hex_usize(frame.texture_overlay),
            format_hex_usize(frame.video_texture_object),
            format_hex_usize(frame.arg6),
            format_hex_usize(frame.arg7),
            format_hex_usize(frame.arg8),
            format_hex_usize(frame.arg10),
            format_hex_usize(frame.arg13),
            format_hex_usize(frame.arg16),
            compact_pointer_layout("texture_video", frame.texture_video),
            compact_pointer_layout("texture_overlay", frame.texture_overlay),
            compact_pointer_layout("arg5_object", frame.video_texture_object),
            compact_pointer_layout("arg6", frame.arg6),
            compact_pointer_layout("arg7", frame.arg7),
            compact_pointer_layout("arg8", frame.arg8),
            compact_float_layout("draw_item", frame.draw_item),
            compact_float_layout("texture_video", frame.texture_video),
            compact_float_layout("texture_overlay", frame.texture_overlay),
            compact_float_layout("arg5_object", frame.video_texture_object)
        ),
        &RENDERER_VIDEO_PASS_DIAGNOSTIC_COUNT,
        8,
    );
    Ok(Some(AdditiveGlBindContext {
        input_handle,
        input_handles,
        monitor: event.queue_monitor,
    }))
}

#[cfg(windows)]
fn runtime_has_video_slots() -> bool {
    request_runtime_state()
        .map(|state| !state.slots.is_empty())
        .unwrap_or(false)
}

#[cfg(windows)]
fn runtime_video_slots_all_ready_for_lua() -> bool {
    request_runtime_state()
        .map(|state| {
            !state.slots.is_empty()
                && state
                    .slots
                    .values()
                    .all(|slot| slot.connected && is_slot_ready_for_lua(slot))
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn runtime_has_connected_not_ready_video_slot() -> bool {
    request_runtime_state()
        .map(|state| {
            state
                .slots
                .values()
                .any(|slot| slot.connected && !is_slot_ready_for_lua(slot))
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn runtime_monotonic_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START
        .get_or_init(Instant::now)
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

#[cfg(windows)]
fn acquire_monitor_render_heavy_probe_budget() -> bool {
    if MONITOR_RENDER_HEAVY_PROBE_ATTEMPTS.load(Ordering::Relaxed)
        >= MONITOR_RENDER_HEAVY_PROBE_MAX_ATTEMPTS
    {
        return false;
    }
    let now = runtime_monotonic_ms().max(1);
    let last = MONITOR_RENDER_HEAVY_PROBE_LAST_MS.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < MONITOR_RENDER_HEAVY_PROBE_MIN_INTERVAL_MS {
        return false;
    }
    if MONITOR_RENDER_HEAVY_PROBE_LAST_MS
        .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return false;
    }
    if !runtime_has_connected_not_ready_video_slot() {
        return false;
    }
    MONITOR_RENDER_HEAVY_PROBE_ATTEMPTS.fetch_add(1, Ordering::Relaxed)
        < MONITOR_RENDER_HEAVY_PROBE_MAX_ATTEMPTS
}

#[cfg(windows)]
fn slot_keys_are_ready_for_lua(keys: &[SlotKey]) -> bool {
    let Ok(state) = request_runtime_state() else {
        return false;
    };
    !keys.is_empty()
        && keys
            .iter()
            .all(|key| state.slots.get(key).is_some_and(is_slot_ready_for_lua))
}

#[cfg(windows)]
fn additive_gl_bind_readback_candidates(
    frame: AdditiveMonitorGlBindFrame,
    bound_candidate: AdditiveMonitorTextureCandidate,
    context: AdditiveGlBindContext,
) -> Vec<AdditiveMonitorTextureCandidate> {
    let monitor = if context.monitor != 0 {
        context.monitor
    } else {
        bound_candidate.monitor
    };
    let mut candidates = collect_additive_monitor_texture_candidates(
        monitor,
        frame.draw_item,
        frame.texture_video,
        frame.video_texture_object,
    );
    let mut bound_candidate = bound_candidate;
    if bound_candidate.monitor == 0 && monitor != 0 {
        bound_candidate.monitor = monitor;
    }
    candidates.sort_by_key(additive_gl_bind_candidate_rank);
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    if bound_candidate.handle != 0 && seen.insert(bound_candidate.handle) {
        ordered.push(bound_candidate);
    }
    ordered.extend(
        candidates
            .into_iter()
            .filter(|candidate| candidate.handle != 0 && seen.insert(candidate.handle)),
    );
    ordered
}

#[cfg(windows)]
fn additive_gl_bind_candidate_rank(candidate: &AdditiveMonitorTextureCandidate) -> u8 {
    match candidate.mapped_from {
        "gl_bind_inside_additive_unit3" => 0,
        "gl_bound_unit3_after_additive_bind" => 1,
        "texture_video_arg+0x48" => 2,
        "texture_video_arg+0x8->+0x48" => 3,
        "texture_video_arg+0x28" => 4,
        "texture_video_arg+0x8->+0x28" => 5,
        "arg5_video_texture_object+0x48" => 6,
        _ => 7,
    }
}

#[cfg(windows)]
fn probe_additive_exact_unit3_diagnostic(
    slots: &[SlotKey],
    context: AdditiveGlBindContext,
    candidates: &[AdditiveMonitorTextureCandidate],
) -> Result<(), String> {
    if slots.is_empty() {
        return Ok(());
    }
    let _ = drain_ready_monitor_pbo_readbacks("additive_exact_unit3");
    if ADDITIVE_EXACT_READBACK_ATTEMPTS.load(Ordering::Relaxed)
        >= ADDITIVE_EXACT_READBACK_MAX_ATTEMPTS
    {
        return Ok(());
    }
    let now = runtime_monotonic_ms().max(1);
    let last = ADDITIVE_EXACT_READBACK_LAST_MS.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < ADDITIVE_EXACT_READBACK_MIN_INTERVAL_MS {
        return Ok(());
    }
    if ADDITIVE_EXACT_READBACK_LAST_MS
        .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
        .is_err()
    {
        return Ok(());
    }
    if ADDITIVE_EXACT_READBACK_ATTEMPTS.fetch_add(1, Ordering::Relaxed)
        >= ADDITIVE_EXACT_READBACK_MAX_ATTEMPTS
    {
        return Ok(());
    }
    let Some(candidate) = candidates.iter().copied().find(|candidate| {
        matches!(
            candidate.mapped_from,
            "gl_bind_inside_additive_unit3" | "gl_bound_unit3_after_additive_bind"
        )
    }) else {
        return Ok(());
    };
    let mut diagnostic_candidate = candidate;
    diagnostic_candidate.mapped_from = "additive_exact_unit3_diagnostic";
    let input_handles = additive_context_input_handles(context);
    match read_additive_monitor_texture_with_pbo_for_handles(
        diagnostic_candidate,
        input_handles.clone(),
    ) {
        Ok(readback) => {
            let stats = pixel_stats_from_rgb(&readback.rgb);
            let state = runtime_snapshot();
            log_runtime_diagnostic(
                &state,
                &format!(
                    "additive exact unit3 diagnostic handle=0x{:x} monitor={} input_handles={} native={}x{} source_stats={} slots={}",
                    diagnostic_candidate.handle,
                    format_hex_or_zero(diagnostic_candidate.monitor as u64),
                    format_monitor_input_handles(&input_handles),
                    readback.width,
                    readback.height,
                    format_pixel_stats(&stats),
                    describe_slots(&state)
                ),
                &ADDITIVE_GL_BIND_CAPTURE_DIAGNOSTIC_COUNT,
                24,
            );
        }
        Err(error) => {
            let state = runtime_snapshot();
            log_runtime_diagnostic(
                &state,
                &format!(
                    "additive exact unit3 diagnostic pending_or_error handle=0x{:x} monitor={} input_handles={} error={} slots={}",
                    diagnostic_candidate.handle,
                    format_hex_or_zero(diagnostic_candidate.monitor as u64),
                    format_monitor_input_handles(&input_handles),
                    error,
                    describe_slots(&state)
                ),
                &ADDITIVE_GL_BIND_CAPTURE_DIAGNOSTIC_COUNT,
                24,
            );
        }
    }
    Ok(())
}

#[cfg(windows)]
fn additive_context_input_handles(context: AdditiveGlBindContext) -> Vec<u64> {
    normalize_monitor_input_handles(context.input_handle, context.input_handles)
}

fn additive_monitor_candidate_can_update_lua(candidate: &AdditiveMonitorTextureCandidate) -> bool {
    matches!(
        candidate.mapped_from,
        "texture_video_arg+0x48"
            | "texture_video_arg+0x8->+0x48"
            | "gl_bound_unit3_after_additive_bind"
            | "gl_bind_inside_additive_unit3"
    )
}

fn additive_monitor_blank_candidate_can_update_lua(
    candidate: &AdditiveMonitorTextureCandidate,
) -> bool {
    matches!(
        candidate.mapped_from,
        "texture_video_arg+0x48" | "texture_video_arg+0x8->+0x48"
    )
}

fn additive_monitor_blank_readback_can_update_lua(
    candidate: &AdditiveMonitorTextureCandidate,
    width: u32,
    height: u32,
) -> bool {
    if additive_monitor_blank_candidate_can_update_lua(candidate) {
        return true;
    }
    let _ = width;
    let _ = height;
    false
}

#[cfg(windows)]
fn probe_additive_gl_bind_candidate(
    frame: AdditiveMonitorGlBindFrame,
    candidate: AdditiveMonitorTextureCandidate,
    unit: u32,
    context: AdditiveGlBindContext,
) -> Result<(), String> {
    let slots = additive_monitor_bind_probe_slots_for_handles(
        context
            .input_handles
            .into_iter()
            .filter(|value| *value != 0)
            .collect(),
    )?;
    if slots.is_empty() {
        log_additive_gl_bind_diagnostic(
            frame,
            candidate,
            unit,
            "no_matching_lua_slots".to_string(),
        )?;
        return Ok(());
    }
    if slot_keys_are_ready_for_lua(&slots) {
        return Ok(());
    }
    let candidates = additive_gl_bind_readback_candidates(frame, candidate, context);
    probe_additive_exact_unit3_diagnostic(&slots, context, &candidates)?;
    if probe_renderer_queue_monitor_resource_candidates(frame, unit, context, &slots)? {
        return Ok(());
    }
    if probe_pending_monitor_render_resources_from_gl(frame, unit)? {
        return Ok(());
    }
    let candidate_summary = if candidates.is_empty() {
        "none".to_string()
    } else {
        candidates
            .iter()
            .take(8)
            .map(|candidate| {
                format!(
                    "0x{:x}@{} monitor={}",
                    candidate.handle,
                    candidate.mapped_from,
                    format_hex_or_zero(candidate.monitor as u64)
                )
            })
            .collect::<Vec<_>>()
            .join(";")
    };
    log_additive_gl_bind_diagnostic(
        frame,
        candidate,
        unit,
        format!("diagnostic_only_readback_disabled candidates=[{candidate_summary}]"),
    )?;
    Ok(())
}

#[cfg(windows)]
fn probe_renderer_queue_monitor_resource_candidates(
    frame: AdditiveMonitorGlBindFrame,
    unit: u32,
    context: AdditiveGlBindContext,
    slots: &[SlotKey],
) -> Result<bool, String> {
    let Some(renderer_pass) = frame.renderer_pass else {
        return Ok(false);
    };
    let state = request_runtime_state()?;
    let event = renderer_video_pass_event_from_frame(&state, renderer_pass);
    let monitor = if event.queue_monitor != 0 {
        event.queue_monitor
    } else {
        context.monitor
    };
    if monitor == 0 || context.input_handle == 0 {
        return Ok(false);
    }
    let monitor_width = if event.queue_width != 0 {
        event.queue_width
    } else {
        read_u32_field(monitor, MONITOR_WIDTH_OFFSET).unwrap_or(0)
    };
    let monitor_height = if event.queue_height != 0 {
        event.queue_height
    } else {
        read_u32_field(monitor, MONITOR_HEIGHT_OFFSET).unwrap_or(0)
    };
    let texture_bindings = state.gl_texture_bindings.clone();
    let binding_count = texture_bindings.len();
    drop(state);

    let mut resource_details = Vec::new();
    let mut resources = Vec::new();
    push_monitor_render_resource_probe(
        &mut resources,
        MONITOR_RENDER_RESOURCE_A_OFFSET,
        event.queue_resource_a_value,
    );
    push_monitor_render_resource_probe(
        &mut resources,
        MONITOR_RENDER_RESOURCE_B_OFFSET,
        event.queue_resource_b_value,
    );
    if event.queue_resource_a_value == 0 {
        push_monitor_render_resource_probe(
            &mut resources,
            MONITOR_RENDER_RESOURCE_A_OFFSET,
            read_pointer_target_usize(event.queue_resource_a_ref).unwrap_or(0),
        );
    }
    if event.queue_resource_b_value == 0 {
        push_monitor_render_resource_probe(
            &mut resources,
            MONITOR_RENDER_RESOURCE_B_OFFSET,
            read_pointer_target_usize(event.queue_resource_b_ref).unwrap_or(0),
        );
    }
    push_monitor_render_resource_probe(
        &mut resources,
        MONITOR_RENDER_RESOURCE_A_OFFSET,
        read_usize_field(monitor, MONITOR_RENDER_RESOURCE_A_OFFSET).unwrap_or(0),
    );
    push_monitor_render_resource_probe(
        &mut resources,
        MONITOR_RENDER_RESOURCE_B_OFFSET,
        read_usize_field(monitor, MONITOR_RENDER_RESOURCE_B_OFFSET).unwrap_or(0),
    );

    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for (monitor_resource_offset, resource) in resources {
        collect_monitor_render_resource_candidates(
            monitor,
            resource,
            monitor_resource_offset,
            monitor_width,
            monitor_height,
            &texture_bindings,
            &mut seen,
            &mut candidates,
            &mut resource_details,
        );
    }
    append_cached_monitor_render_source_candidates(
        slots,
        monitor,
        monitor_width,
        monitor_height,
        &mut seen,
        &mut candidates,
        &mut resource_details,
    );
    collect_renderer_pass_target_candidates(
        &event,
        monitor,
        monitor_width,
        monitor_height,
        &texture_bindings,
        &mut seen,
        &mut candidates,
        &mut resource_details,
    );
    candidates.sort_by_key(|candidate| {
        monitor_render_resource_candidate_rank(candidate, monitor_width, monitor_height)
    });
    let mut report = MonitorRenderProbeReport {
        monitor,
        monitor_width,
        monitor_height,
        input_slot_handle: context.input_handle,
        candidates: candidates.len(),
        read_errors: 0,
        blank_reads: 0,
        updated_slots: 0,
        skipped_fps_slots: 0,
        details: Vec::new(),
    };
    report.details.push(format!(
        "renderer_queue_resource_probe unit={} bind_index={} queue_item={} queue_from={} queue_score={} resource_refs=[{}->{},{}->{}] known_bindings={} matched_slots={} resources={}",
        unit,
        frame.bind_index,
        format_hex_usize(event.queue_item),
        event.queue_item_from,
        event.queue_item_score,
        format_hex_usize(event.queue_resource_a_ref),
        format_hex_usize(event.queue_resource_a_value),
        format_hex_usize(event.queue_resource_b_ref),
        format_hex_usize(event.queue_resource_b_value),
        binding_count,
        slots.len(),
        if resource_details.is_empty() {
            "none".to_string()
        } else {
            resource_details.join(",")
        }
    ));
    if candidates.is_empty() {
        record_monitor_render_probe_report(&report)?;
        return Ok(false);
    }
    let monitor_inputs = monitor_video_input_handles(monitor);
    if let Some(reason) = monitor_render_is_lua_output_for_slots(slots, monitor_inputs) {
        report.details.push(format!(
            "rejected_lua_output_monitor reason={} monitor_inputs=[{}]",
            reason,
            monitor_inputs.summary()
        ));
        record_monitor_render_probe_report(&report)?;
        return Ok(false);
    }
    let mut readback_attempts = 0usize;
    for candidate in candidates {
        if let Some(reason) =
            monitor_render_candidate_readback_skip_reason(&candidate, monitor_width, monitor_height)
        {
            push_monitor_render_readback_skip_detail(&mut report, &candidate, reason);
            continue;
        }
        if readback_attempts >= MONITOR_RENDER_MAX_READBACK_CANDIDATES_PER_PROBE {
            push_monitor_render_readback_skip_detail(
                &mut report,
                &candidate,
                "readback_candidate_limit",
            );
            continue;
        }
        readback_attempts = readback_attempts.saturating_add(1);
        match read_monitor_render_texture_with_pbo_for_handles(
            candidate,
            additive_context_input_handles(context),
        ) {
            Ok(readback) => {
                let stats = pixel_stats_from_rgb(&readback.rgb);
                if stats.nonzero_pixels == 0 {
                    report.blank_reads = report.blank_reads.saturating_add(1);
                    if report.details.len() < 12 {
                        report.details.push(format!(
                            "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} native={}x{} blank {}",
                            candidate.handle,
                            format_hex_or_zero(candidate.resource as u64),
                            candidate.monitor_resource_offset,
                            candidate.mapped_from,
                            format_hex_or_zero(candidate.mapped_key),
                            format_hex_or_zero(candidate.binding_owner_ptr),
                            format_hex_or_zero(candidate.binding_texture_ptr),
                            candidate.binding_age_ms,
                            readback.width,
                            readback.height,
                            format_pixel_stats(&stats)
                        ));
                    }
                    continue;
                }
                if !monitor_render_candidate_can_update_lua(
                    &candidate,
                    monitor_width,
                    monitor_height,
                ) {
                    if report.details.len() < 12 {
                        report.details.push(format!(
                            "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} native={}x{} rejected_untrusted_candidate {}",
                            candidate.handle,
                            format_hex_or_zero(candidate.resource as u64),
                            candidate.monitor_resource_offset,
                            candidate.mapped_from,
                            format_hex_or_zero(candidate.mapped_key),
                            readback.width,
                            readback.height,
                            format_pixel_stats(&stats)
                        ));
                    }
                    continue;
                }
                let update = apply_monitor_render_readback_to_slots(slots, candidate, readback)?;
                if update.skipped_fps {
                    report.skipped_fps_slots = report
                        .skipped_fps_slots
                        .saturating_add(update.skipped_fps_slots);
                }
                if update.updated {
                    report.updated_slots =
                        report.updated_slots.saturating_add(update.updated_slots);
                    report.details.push(format!(
                        "captured candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} {}",
                        candidate.handle,
                        format_hex_or_zero(candidate.resource as u64),
                        candidate.monitor_resource_offset,
                        candidate.mapped_from,
                        format_hex_or_zero(candidate.mapped_key),
                        format_hex_or_zero(candidate.binding_owner_ptr),
                        format_hex_or_zero(candidate.binding_texture_ptr),
                        candidate.binding_age_ms,
                        format_pixel_stats(&update.stats)
                    ));
                    record_monitor_render_probe_report(&report)?;
                    return Ok(true);
                }
            }
            Err(error) => {
                report.read_errors = report.read_errors.saturating_add(1);
                if report.details.len() < 12 {
                    report.details.push(format!(
                        "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} read_error={}",
                        candidate.handle,
                        format_hex_or_zero(candidate.resource as u64),
                        candidate.monitor_resource_offset,
                        candidate.mapped_from,
                        format_hex_or_zero(candidate.mapped_key),
                        format_hex_or_zero(candidate.binding_owner_ptr),
                        format_hex_or_zero(candidate.binding_texture_ptr),
                        candidate.binding_age_ms,
                        error
                    ));
                }
            }
        }
    }
    record_monitor_render_probe_report(&report)?;
    Ok(false)
}

#[cfg(windows)]
fn push_monitor_render_resource_probe(
    resources: &mut Vec<(usize, usize)>,
    monitor_resource_offset: usize,
    resource: usize,
) {
    if resource == 0
        || resources
            .iter()
            .any(|(offset, existing)| *offset == monitor_resource_offset && *existing == resource)
    {
        return;
    }
    resources.push((monitor_resource_offset, resource));
}

#[cfg(windows)]
fn monitor_render_resource_candidate_rank(
    candidate: &MonitorRenderResourceCandidate,
    monitor_width: u32,
    monitor_height: u32,
) -> (u8, u8, u128, u32) {
    let shape_rank = if monitor_width != 0
        && monitor_height != 0
        && candidate.mapped_width == monitor_width
        && candidate.mapped_height == monitor_height
    {
        0
    } else if candidate.mapped_width != 0 && candidate.mapped_height != 0 {
        1
    } else {
        2
    };
    let source_rank = match candidate.mapped_from {
        "resource_wrapper" => 0,
        "resource_nested_+0x8" => 1,
        "resource_scan_raw_gl_u32_size_match" => 2,
        "renderer_pass_target_direct" => 3,
        "renderer_pass_target_nested" => 4,
        "resource_scan_ptr" => 5,
        "resource_scan_raw_gl_u32" => 6,
        _ => 7,
    };
    (
        shape_rank,
        source_rank,
        candidate.binding_age_ms,
        candidate.handle,
    )
}

#[cfg(windows)]
fn monitor_render_candidate_can_update_lua(
    candidate: &MonitorRenderResourceCandidate,
    monitor_width: u32,
    monitor_height: u32,
) -> bool {
    let trusted_mapping = matches!(
        candidate.mapped_from,
        "resource_wrapper"
            | "resource_nested_+0x8"
            | "resource_scan_ptr"
            | "renderer_pass_target_direct"
            | "renderer_pass_target_nested"
            | "cached_slot_source_texture"
            | "trusted_cached_slot_source_texture"
    );
    trusted_mapping
        && monitor_render_candidate_matches_accepted_size(candidate, monitor_width, monitor_height)
}

#[cfg(windows)]
fn monitor_render_candidate_should_try_fbo_readback(
    candidate: &MonitorRenderResourceCandidate,
    error: &str,
) -> bool {
    candidate.mapped_from == "resource_scan_raw_gl_u32_size_match"
        && error.contains("glGetTexImage pbo error=0x502")
}

#[cfg(windows)]
fn monitor_render_candidate_matches_accepted_size(
    candidate: &MonitorRenderResourceCandidate,
    monitor_width: u32,
    monitor_height: u32,
) -> bool {
    if monitor_width == 0 || monitor_height == 0 {
        return false;
    }
    if candidate.mapped_width == monitor_width && candidate.mapped_height == monitor_height {
        return true;
    }
    if !monitor_render_candidate_allows_supersample_size(candidate) {
        return false;
    }
    monitor_width
        .checked_mul(MONITOR_RENDER_SUPERSAMPLE_SCALE)
        .zip(monitor_height.checked_mul(MONITOR_RENDER_SUPERSAMPLE_SCALE))
        .is_some_and(|(width, height)| {
            candidate.mapped_width == width && candidate.mapped_height == height
        })
}

#[cfg(windows)]
fn monitor_render_candidate_allows_supersample_size(
    candidate: &MonitorRenderResourceCandidate,
) -> bool {
    matches!(
        candidate.mapped_from,
        "resource_wrapper"
            | "resource_nested_+0x8"
            | "resource_scan_ptr"
            | "renderer_pass_target_direct"
            | "renderer_pass_target_nested"
            | "cached_slot_source_texture"
            | "trusted_cached_slot_source_texture"
    )
}

#[cfg(windows)]
fn monitor_render_readback_can_update_lua(
    candidate: &MonitorRenderResourceCandidate,
    readback_width: u32,
    readback_height: u32,
) -> bool {
    if candidate.mapped_width == 0
        || candidate.mapped_height == 0
        || readback_width != candidate.mapped_width
        || readback_height != candidate.mapped_height
    {
        return false;
    }
    if candidate.monitor == 0 {
        return candidate.mapped_from == "trusted_cached_slot_source_texture";
    }
    let monitor_width = read_u32_field(candidate.monitor, MONITOR_WIDTH_OFFSET).unwrap_or(0);
    let monitor_height = read_u32_field(candidate.monitor, MONITOR_HEIGHT_OFFSET).unwrap_or(0);
    monitor_render_candidate_can_update_lua(candidate, monitor_width, monitor_height)
}

#[cfg(windows)]
fn append_cached_monitor_render_source_candidates(
    slots: &[SlotKey],
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
    details: &mut Vec<String>,
) {
    let state = runtime_snapshot();
    let mut added = 0usize;
    for key in slots {
        let Some(slot) = state.slots.get(key) else {
            continue;
        };
        let Some(handle) = slot.source_texture_handle else {
            continue;
        };
        if handle == 0 || handle > u64::from(u32::MAX) {
            continue;
        }
        let handle = handle as u32;
        if !seen.insert(handle) {
            continue;
        }
        let (mapped_width, mapped_height) =
            current_gl_texture_size_for_handle(handle).unwrap_or((monitor_width, monitor_height));
        candidates.push(MonitorRenderResourceCandidate {
            handle,
            monitor,
            resource: 0,
            resource_offset: 0,
            monitor_resource_offset: 0,
            mapped_key: u64::from(handle),
            mapped_from: "cached_slot_source_texture",
            mapped_width,
            mapped_height,
            binding_owner_ptr: 0,
            binding_texture_ptr: 0,
            binding_age_ms: 0,
        });
        added = added.saturating_add(1);
    }
    if added > 0 {
        details.push(format!("cached_slot_source_texture_candidates={added}"));
    }
}

#[cfg(windows)]
fn collect_renderer_pass_target_candidates(
    event: &RendererVideoPassEvent,
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    texture_bindings: &BTreeMap<u64, GlTextureBinding>,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
    details: &mut Vec<String>,
) {
    let before = candidates.len();
    let objects = [
        ("target_primary", event.render_target_primary),
        ("target_secondary", event.render_target_secondary),
        ("target_video", event.render_target_video),
        ("frame_a", event.frame_a),
        ("frame_b", event.frame_b),
        ("frame_c", event.frame_c),
        ("command", event.command),
    ];
    let mut object_summaries = Vec::new();
    for (label, object) in objects {
        let added = collect_renderer_pass_target_object_candidates(
            label,
            object,
            monitor,
            monitor_width,
            monitor_height,
            texture_bindings,
            seen,
            candidates,
        );
        if added > 0 {
            object_summaries.push(format!("{label}={added}"));
        }
    }
    let added = candidates.len().saturating_sub(before);
    if added > 0 {
        details.push(format!(
            "renderer_pass_target_candidates={} [{}]",
            added,
            object_summaries.join(",")
        ));
    } else {
        details.push("renderer_pass_target_candidates=0".to_string());
    }
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn collect_renderer_pass_target_object_candidates(
    label: &'static str,
    object: usize,
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    texture_bindings: &BTreeMap<u64, GlTextureBinding>,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
) -> usize {
    if object == 0
        || !memory_range_is_readable(
            object as *const c_void,
            RENDERER_PASS_TARGET_SCAN_BYTES.min(0x20),
        )
    {
        return 0;
    }
    let before = candidates.len();
    collect_monitor_binding_candidate(
        monitor,
        object,
        0,
        object as u64,
        0,
        "renderer_pass_target_direct",
        texture_bindings,
        seen,
        candidates,
    );
    if memory_range_is_readable(object as *const c_void, RENDERER_PASS_TARGET_SCAN_BYTES) {
        for offset in (0..RENDERER_PASS_TARGET_SCAN_BYTES).step_by(size_of::<usize>()) {
            let Some(mapped_key) = read_usize_field(object, offset).map(|value| value as u64)
            else {
                continue;
            };
            if mapped_key == 0 || mapped_key == object as u64 {
                continue;
            }
            collect_monitor_binding_candidate(
                monitor,
                object,
                0,
                mapped_key,
                offset,
                "renderer_pass_target_direct",
                texture_bindings,
                seen,
                candidates,
            );
        }
        collect_renderer_pass_nested_target_object_candidates(
            label,
            object,
            monitor,
            monitor_width,
            monitor_height,
            texture_bindings,
            seen,
            candidates,
        );
    }
    let _ = (label, monitor_width, monitor_height);
    candidates.len().saturating_sub(before)
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn collect_renderer_pass_nested_target_object_candidates(
    label: &'static str,
    object: usize,
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    texture_bindings: &BTreeMap<u64, GlTextureBinding>,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
) {
    let mut scanned = 0usize;
    for offset in (0..RENDERER_PASS_TARGET_SCAN_BYTES).step_by(size_of::<usize>()) {
        if scanned >= RENDERER_PASS_TARGET_NESTED_POINTER_LIMIT {
            break;
        }
        let Some(ptr_value) = read_usize_field(object, offset) else {
            continue;
        };
        if ptr_value == 0
            || ptr_value == object
            || !pointer_value_looks_process_address(ptr_value as u64)
            || !memory_range_is_readable(
                ptr_value as *const c_void,
                RENDERER_PASS_TARGET_NESTED_SCAN_BYTES.min(0x20),
            )
        {
            continue;
        }
        scanned = scanned.saturating_add(1);
        collect_monitor_binding_candidate(
            monitor,
            ptr_value,
            0,
            ptr_value as u64,
            offset,
            "renderer_pass_target_nested",
            texture_bindings,
            seen,
            candidates,
        );
        for nested_offset in (0..RENDERER_PASS_TARGET_NESTED_SCAN_BYTES).step_by(size_of::<usize>())
        {
            let Some(mapped_key) =
                read_usize_field(ptr_value, nested_offset).map(|value| value as u64)
            else {
                continue;
            };
            if mapped_key == 0 || mapped_key == ptr_value as u64 {
                continue;
            }
            collect_monitor_binding_candidate(
                monitor,
                ptr_value,
                0,
                mapped_key,
                nested_offset,
                "renderer_pass_target_nested",
                texture_bindings,
                seen,
                candidates,
            );
        }
    }
    let _ = (label, monitor_width, monitor_height);
}

#[cfg(windows)]
fn monitor_render_candidate_readback_skip_reason(
    candidate: &MonitorRenderResourceCandidate,
    monitor_width: u32,
    monitor_height: u32,
) -> Option<&'static str> {
    if monitor_width == 0 || monitor_height == 0 {
        return Some("zero_monitor_size");
    }
    let known_size = candidate.mapped_width != 0 && candidate.mapped_height != 0;
    if known_size
        && !monitor_render_candidate_matches_accepted_size(candidate, monitor_width, monitor_height)
    {
        return Some("mapped_size_mismatch");
    }
    if candidate.mapped_from == "resource_scan_raw_gl_u32" {
        return Some("raw_gl_without_monitor_size_match");
    }
    None
}

#[cfg(windows)]
fn push_monitor_render_readback_skip_detail(
    report: &mut MonitorRenderProbeReport,
    candidate: &MonitorRenderResourceCandidate,
    reason: &'static str,
) {
    if report.details.len() >= 12 {
        return;
    }
    report.details.push(format!(
        "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} mapped_size={}x{} skipped_readback={}",
        candidate.handle,
        format_hex_or_zero(candidate.resource as u64),
        candidate.monitor_resource_offset,
        candidate.mapped_from,
        format_hex_or_zero(candidate.mapped_key),
        format_hex_or_zero(candidate.binding_owner_ptr),
        format_hex_or_zero(candidate.binding_texture_ptr),
        candidate.binding_age_ms,
        candidate.mapped_width,
        candidate.mapped_height,
        reason
    ));
}

#[cfg(windows)]
fn enqueue_pending_monitor_render_probe(
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    mut input_handles: Vec<u64>,
    resource_a: usize,
    resource_b: usize,
    source: &'static str,
) -> Result<(), String> {
    if monitor == 0 || (resource_a == 0 && resource_b == 0) {
        return Ok(());
    }
    input_handles.retain(|value| *value != 0);
    input_handles.dedup();
    if input_handles.is_empty() {
        return Ok(());
    }
    let mut state = request_runtime_state()?;
    let relation =
        describe_monitor_input_slot_relation_with_candidates(&state, input_handles.clone());
    let matched_slots = monitor_render_probe_slots_for_handles(input_handles.clone())?;
    let input_handle_text = input_handles
        .iter()
        .map(|value| format_hex_or_zero(*value))
        .collect::<Vec<_>>()
        .join(",");
    let now = Instant::now();
    state.pending_monitor_render_probes.retain(|probe| {
        now.duration_since(probe.observed_at).as_millis() as u64
            <= PENDING_MONITOR_RENDER_PROBE_MAX_AGE_MS
    });
    if let Some(existing) = state
        .pending_monitor_render_probes
        .iter_mut()
        .find(|probe| probe.monitor == monitor)
    {
        existing.monitor_width = monitor_width;
        existing.monitor_height = monitor_height;
        existing.input_handles = input_handles;
        existing.resource_a = resource_a;
        existing.resource_b = resource_b;
        existing.source = source;
        existing.observed_at = now;
    } else {
        state
            .pending_monitor_render_probes
            .push(PendingMonitorRenderProbe {
                monitor,
                monitor_width,
                monitor_height,
                input_handles,
                resource_a,
                resource_b,
                source,
                observed_at: now,
            });
    }
    if state.pending_monitor_render_probes.len() > PENDING_MONITOR_RENDER_PROBE_LIMIT {
        let remove_count = state
            .pending_monitor_render_probes
            .len()
            .saturating_sub(PENDING_MONITOR_RENDER_PROBE_LIMIT);
        state.pending_monitor_render_probes.drain(0..remove_count);
    }
    log_runtime_diagnostic(
        &state,
        &format!(
            "pending monitor render probe queued source={} monitor={} size={}x{} resources=[{},{}] input_handles={} relation={} matched_slots={} pending_count={} slots={}",
            source,
            format_hex_usize(monitor),
            monitor_width,
            monitor_height,
            format_hex_usize(resource_a),
            format_hex_usize(resource_b),
            input_handle_text,
            relation,
            if matched_slots.is_empty() {
                "none".to_string()
            } else {
                matched_slots
                    .iter()
                    .map(|key| format!("{}:{}", key.component, key.slot))
                    .collect::<Vec<_>>()
                    .join(",")
            },
            state.pending_monitor_render_probes.len(),
            describe_slots(&state)
        ),
        &PENDING_MONITOR_PROBE_DIAGNOSTIC_COUNT,
        24,
    );
    set_runtime(state);
    Ok(())
}

#[cfg(windows)]
fn probe_pending_monitor_render_resources_from_gl(
    frame: AdditiveMonitorGlBindFrame,
    unit: u32,
) -> Result<bool, String> {
    let state = request_runtime_state()?;
    let now = Instant::now();
    let texture_bindings = state.gl_texture_bindings.clone();
    let pending = state
        .pending_monitor_render_probes
        .iter()
        .filter(|probe| {
            now.duration_since(probe.observed_at).as_millis() as u64
                <= PENDING_MONITOR_RENDER_PROBE_MAX_AGE_MS
        })
        .cloned()
        .collect::<Vec<_>>();
    drop(state);
    for probe in pending {
        let slots = monitor_render_probe_slots_for_handles(probe.input_handles.clone())?;
        if slots.is_empty() {
            continue;
        }
        if let Some(reason) = monitor_render_is_lua_output_for_slots(
            &slots,
            monitor_video_input_handles(probe.monitor),
        ) {
            let report = MonitorRenderProbeReport {
                monitor: probe.monitor,
                monitor_width: probe.monitor_width,
                monitor_height: probe.monitor_height,
                input_slot_handle: probe.input_handles.first().copied().unwrap_or(0),
                candidates: 0,
                read_errors: 0,
                blank_reads: 0,
                updated_slots: 0,
                skipped_fps_slots: 0,
                details: vec![format!(
                    "pending_renderer_queue_resource_probe unit={} bind_index={} source={} rejected_lua_output_monitor reason={}",
                    unit, frame.bind_index, probe.source, reason
                )],
            };
            record_monitor_render_probe_report(&report)?;
            continue;
        }
        let mut candidates = Vec::new();
        let mut seen = BTreeSet::new();
        let mut resource_details = Vec::new();
        for (monitor_resource_offset, resource) in [
            (MONITOR_RENDER_RESOURCE_A_OFFSET, probe.resource_a),
            (MONITOR_RENDER_RESOURCE_B_OFFSET, probe.resource_b),
        ] {
            collect_monitor_render_resource_candidates(
                probe.monitor,
                resource,
                monitor_resource_offset,
                probe.monitor_width,
                probe.monitor_height,
                &texture_bindings,
                &mut seen,
                &mut candidates,
                &mut resource_details,
            );
        }
        append_cached_monitor_render_source_candidates(
            &slots,
            probe.monitor,
            probe.monitor_width,
            probe.monitor_height,
            &mut seen,
            &mut candidates,
            &mut resource_details,
        );
        candidates.sort_by_key(|candidate| {
            monitor_render_resource_candidate_rank(
                candidate,
                probe.monitor_width,
                probe.monitor_height,
            )
        });
        let mut report = MonitorRenderProbeReport {
            monitor: probe.monitor,
            monitor_width: probe.monitor_width,
            monitor_height: probe.monitor_height,
            input_slot_handle: probe.input_handles.first().copied().unwrap_or(0),
            candidates: candidates.len(),
            read_errors: 0,
            blank_reads: 0,
            updated_slots: 0,
            skipped_fps_slots: 0,
            details: vec![format!(
                "pending_renderer_queue_resource_probe unit={} bind_index={} source={} age_ms={} resources={}",
                unit,
                frame.bind_index,
                probe.source,
                now.duration_since(probe.observed_at).as_millis(),
                if resource_details.is_empty() {
                    "none".to_string()
                } else {
                    resource_details.join(",")
                }
            )],
        };
        for candidate in candidates {
            if let Some(reason) = monitor_render_candidate_readback_skip_reason(
                &candidate,
                probe.monitor_width,
                probe.monitor_height,
            ) {
                push_monitor_render_readback_skip_detail(&mut report, &candidate, reason);
                continue;
            }
            match read_monitor_render_texture_with_pbo_for_handles(
                candidate,
                probe.input_handles.clone(),
            ) {
                Ok(readback) => {
                    let stats = pixel_stats_from_rgb(&readback.rgb);
                    if stats.nonzero_pixels == 0 {
                        report.blank_reads = report.blank_reads.saturating_add(1);
                        if report.details.len() < 12 {
                            report.details.push(format!(
                                "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} native={}x{} blank {}",
                                candidate.handle,
                                format_hex_or_zero(candidate.resource as u64),
                                candidate.monitor_resource_offset,
                                candidate.mapped_from,
                                format_hex_or_zero(candidate.mapped_key),
                                readback.width,
                                readback.height,
                                format_pixel_stats(&stats)
                            ));
                        }
                        continue;
                    }
                    if !monitor_render_candidate_can_update_lua(
                        &candidate,
                        probe.monitor_width,
                        probe.monitor_height,
                    ) {
                        if report.details.len() < 12 {
                            report.details.push(format!(
                                "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} native={}x{} rejected_untrusted_candidate {}",
                                candidate.handle,
                                format_hex_or_zero(candidate.resource as u64),
                                candidate.monitor_resource_offset,
                                candidate.mapped_from,
                                format_hex_or_zero(candidate.mapped_key),
                                readback.width,
                                readback.height,
                                format_pixel_stats(&stats)
                            ));
                        }
                        continue;
                    }
                    let update =
                        apply_monitor_render_readback_to_slots(&slots, candidate, readback)?;
                    if update.skipped_fps {
                        report.skipped_fps_slots = report
                            .skipped_fps_slots
                            .saturating_add(update.skipped_fps_slots);
                    }
                    if update.updated {
                        report.updated_slots =
                            report.updated_slots.saturating_add(update.updated_slots);
                        report.details.push(format!(
                            "captured candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} {}",
                            candidate.handle,
                            format_hex_or_zero(candidate.resource as u64),
                            candidate.monitor_resource_offset,
                            candidate.mapped_from,
                            format_hex_or_zero(candidate.mapped_key),
                            format_pixel_stats(&update.stats)
                        ));
                        record_monitor_render_probe_report(&report)?;
                        return Ok(true);
                    }
                }
                Err(error) => {
                    report.read_errors = report.read_errors.saturating_add(1);
                    if report.details.len() < 12 {
                        report.details.push(format!(
                            "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} read_error={}",
                            candidate.handle,
                            format_hex_or_zero(candidate.resource as u64),
                            candidate.monitor_resource_offset,
                            candidate.mapped_from,
                            format_hex_or_zero(candidate.mapped_key),
                            error
                        ));
                    }
                }
            }
        }
        record_monitor_render_probe_report(&report)?;
    }
    Ok(false)
}

#[cfg(windows)]
fn monitor_render_is_lua_output_for_slots(
    slots: &[SlotKey],
    monitor_inputs: MonitorVideoInputHandles,
) -> Option<String> {
    let state = runtime_snapshot();
    for key in slots {
        let Some(slot) = state.slots.get(key) else {
            continue;
        };
        if monitor_video_input_mentions_component(monitor_inputs, &slot.component) {
            return Some(format!(
                "{}:{} monitor_input_mentions_component",
                key.component, key.slot
            ));
        }
    }
    None
}

#[cfg(windows)]
fn monitor_video_input_mentions_component(
    monitor_inputs: MonitorVideoInputHandles,
    component: &str,
) -> bool {
    for handle in monitor_inputs.relation_handles() {
        if handle == 0 {
            continue;
        }
        if component_context_from_input_video_source(handle).as_deref() == Some(component) {
            return true;
        }
        if registered_component_for_video_source_handle(handle).as_deref() == Some(component) {
            return true;
        }
        if let Some(context) = component.strip_prefix("component_lua_context:") {
            let Ok(context) = usize::from_str_radix(context, 16) else {
                continue;
            };
            if pointer_graph_contains_u64_paths(
                handle,
                MONITOR_INPUT_REF_RELATION_SCAN_BYTES,
                MONITOR_INPUT_REF_NESTED_SCAN_BYTES,
                context as u64,
                2,
            )
            .is_empty()
            {
                continue;
            }
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn log_additive_gl_bind_unit_observation(
    frame: AdditiveMonitorGlBindFrame,
    unit: u32,
    handle: u32,
    gl: String,
) -> Result<(), String> {
    let state = request_runtime_state()?;
    log_runtime_diagnostic(
        &state,
        &format!(
            "additive gl bind unit observation bind_index={} unit={} {} material={} draw_item={} texture_video={} texture_overlay={} object={} handle=0x{:x} gl={} slots={}",
            frame.bind_index,
            unit,
            describe_current_gl_rects(),
            format_hex_or_zero(frame.material as u64),
            format_hex_or_zero(frame.draw_item as u64),
            format_hex_or_zero(frame.texture_video as u64),
            format_hex_or_zero(frame.texture_overlay as u64),
            format_hex_or_zero(frame.video_texture_object as u64),
            handle,
            gl,
            describe_slots(&state)
        ),
        &ADDITIVE_GL_BIND_UNIT_DIAGNOSTIC_COUNT,
        12,
    );
    Ok(())
}

#[cfg(windows)]
fn log_additive_gl_bind_diagnostic(
    frame: AdditiveMonitorGlBindFrame,
    candidate: AdditiveMonitorTextureCandidate,
    unit: u32,
    result: String,
) -> Result<(), String> {
    let state = request_runtime_state()?;
    log_runtime_diagnostic(
        &state,
        &format!(
            "additive gl bind probe bind_index={} unit={} {} material={} draw_item={} texture_video={} texture_overlay={} object={} handle=0x{:x} mapped_from={} result={} slots={}",
            frame.bind_index,
            unit,
            describe_current_gl_rects(),
            format_hex_or_zero(frame.material as u64),
            format_hex_or_zero(frame.draw_item as u64),
            format_hex_or_zero(frame.texture_video as u64),
            format_hex_or_zero(frame.texture_overlay as u64),
            format_hex_or_zero(frame.video_texture_object as u64),
            candidate.handle,
            candidate.mapped_from,
            result,
            describe_slots(&state)
        ),
        &ADDITIVE_GL_BIND_DIAGNOSTIC_COUNT,
        24,
    );
    Ok(())
}

#[cfg(windows)]
fn additive_gl_bind_candidate_source(unit: u32) -> &'static str {
    match unit {
        3 => "gl_bind_inside_additive_unit3",
        4 => "gl_bind_inside_additive_unit4",
        5 => "gl_bind_inside_additive_unit5",
        0 => "gl_bind_inside_additive_unit0",
        1 => "gl_bind_inside_additive_unit1",
        2 => "gl_bind_inside_additive_unit2",
        _ => "gl_bind_inside_additive_other",
    }
}

#[allow(clippy::too_many_arguments)]
fn additive_monitor_bind_from_hook_chained(
    material: *mut c_void,
    draw_item: *mut c_void,
    texture_video: *mut c_void,
    texture_overlay: *mut c_void,
    arg5: *mut c_void,
    arg6: *mut c_void,
    arg7: *mut c_void,
    arg8: *mut c_void,
    arg9: u32,
    arg10: *mut c_void,
    arg11: u8,
    arg12: u32,
    arg13: *mut c_void,
    arg14: u32,
    arg15: u32,
    arg16: *mut c_void,
) -> Result<(), String> {
    #[cfg(windows)]
    push_additive_monitor_gl_bind_context(AdditiveMonitorGlBindFrame {
        material: material as usize,
        draw_item: draw_item as usize,
        texture_video: texture_video as usize,
        texture_overlay: texture_overlay as usize,
        video_texture_object: arg5 as usize,
        arg6: arg6 as usize,
        arg7: arg7 as usize,
        arg8: arg8 as usize,
        arg10: arg10 as usize,
        arg13: arg13 as usize,
        arg16: arg16 as usize,
        renderer_pass: current_renderer_video_pass_context(),
        bind_index: 0,
    });
    call_additive_monitor_bind_original(
        material,
        draw_item,
        texture_video,
        texture_overlay,
        arg5,
        arg6,
        arg7,
        arg8,
        arg9,
        arg10,
        arg11,
        arg12,
        arg13,
        arg14,
        arg15,
        arg16,
    );
    #[cfg(windows)]
    pop_additive_monitor_gl_bind_context();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn call_additive_monitor_bind_original(
    material: *mut c_void,
    draw_item: *mut c_void,
    texture_video: *mut c_void,
    texture_overlay: *mut c_void,
    arg5: *mut c_void,
    arg6: *mut c_void,
    arg7: *mut c_void,
    arg8: *mut c_void,
    arg9: u32,
    arg10: *mut c_void,
    arg11: u8,
    arg12: u32,
    arg13: *mut c_void,
    arg14: u32,
    arg15: u32,
    arg16: *mut c_void,
) {
    let trampoline = ADDITIVE_MONITOR_BIND_ORIGINAL.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        u32,
        *mut c_void,
        u8,
        u32,
        *mut c_void,
        u32,
        u32,
        *mut c_void,
    ) = unsafe { std::mem::transmute(trampoline) };
    original(
        material,
        draw_item,
        texture_video,
        texture_overlay,
        arg5,
        arg6,
        arg7,
        arg8,
        arg9,
        arg10,
        arg11,
        arg12,
        arg13,
        arg14,
        arg15,
        arg16,
    );
}

fn set_additive_monitor_bind_original_trampoline(replacement: &str, trampoline: Option<u64>) {
    let value = trampoline.unwrap_or(0) as usize;
    if replacement == "stormworks_video_get_additive_monitor_bind_hook" {
        ADDITIVE_MONITOR_BIND_ORIGINAL.store(value, Ordering::SeqCst);
    }
    if replacement == "stormworks_video_get_additive_monitor_video_bind_hook_arg3" {
        ADDITIVE_MONITOR_VIDEO_BIND_ORIGINAL.store(value, Ordering::SeqCst);
    }
}

fn additive_monitor_bind_original_trampoline_status() -> serde_json::Value {
    serde_json::json!({
        "bind": ADDITIVE_MONITOR_BIND_ORIGINAL.load(Ordering::SeqCst) != 0,
        "video_bind": ADDITIVE_MONITOR_VIDEO_BIND_ORIGINAL.load(Ordering::SeqCst) != 0
    })
}

#[allow(clippy::too_many_arguments)]
fn additive_monitor_video_bind_from_hook_chained(
    descriptor: *mut c_void,
    buffers: *mut c_void,
    video_texture_object: *mut c_void,
    overlay_texture_object: *mut c_void,
    arg5: *mut c_void,
    arg6: *mut c_void,
    arg7: *mut c_void,
    arg8: u64,
) -> Result<(), String> {
    call_additive_monitor_video_bind_original(
        descriptor,
        buffers,
        video_texture_object,
        overlay_texture_object,
        arg5,
        arg6,
        arg7,
        arg8,
    );
    // After the original binds and draws, the texture_video sampler (unit 0) is
    // populated. Read the GL id back from the video texture wrapper (+0x48) and copy
    // the frame into connected Lua slots on this render thread.
    #[cfg(windows)]
    {
        let _ = capture_additive_monitor_video_texture(
            video_texture_object as usize,
            overlay_texture_object as usize,
        );
    }
    #[cfg(not(windows))]
    let _ = overlay_texture_object;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn call_additive_monitor_video_bind_original(
    descriptor: *mut c_void,
    buffers: *mut c_void,
    video_texture_object: *mut c_void,
    overlay_texture_object: *mut c_void,
    arg5: *mut c_void,
    arg6: *mut c_void,
    arg7: *mut c_void,
    arg8: u64,
) {
    let trampoline = ADDITIVE_MONITOR_VIDEO_BIND_ORIGINAL.load(Ordering::SeqCst);
    if trampoline == 0 {
        return;
    }
    let original: extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        u64,
    ) = unsafe { std::mem::transmute(trampoline) };
    original(
        descriptor,
        buffers,
        video_texture_object,
        overlay_texture_object,
        arg5,
        arg6,
        arg7,
        arg8,
    );
}

#[cfg(windows)]
/// Per-monitor-texture sampling state, keyed by the GL texture id bound to `texture_video`,
/// so every distinct additive_monitor draw is sampled at capture FPS. A single global one-shot
/// gate would only ever sample the first monitor drawn each frame, starving later monitors
/// (e.g. the camera screen) whenever a blank Lua-overlay screen is drawn first.
#[cfg(windows)]
struct AdditiveVideoHandleObs {
    last_attempt: Instant,
    native_w: u32,
    native_h: u32,
    video_nonzero: usize,
}
#[cfg(windows)]
static ADDITIVE_VIDEO_HANDLE_OBS: Mutex<BTreeMap<u32, AdditiveVideoHandleObs>> =
    Mutex::new(BTreeMap::new());
#[cfg(windows)]
static ADDITIVE_MONITOR_VIDEO_DIAGNOSTIC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Sticky camera→slot binding. Keyed by the camera video-texture-wrapper address; the value is the
/// SlotKey it was first assigned to plus the time it was last seen. This is the routing mechanism of
/// record after heap-pointer joins were disproven live SEVEN times (the engine wires cameras by
/// logic-graph IDs, and those tag values, e.g. `0xbf400000`, even polluted the pointer scan and
/// produced false shared-object matches). Instead of trying to read the structural link, we assign
/// each distinct camera to a distinct free slot the first time we see it and NEVER change that
/// binding while both stay alive. Deterministic, zero flicker. Bindings age out (~3s unseen) so a
/// removed camera or reloaded vehicle frees its slot. The pairing may be swapped versus the physical
/// wiring (a player corrects that by swapping the two input wires), but each Lua always shows ONE
/// fixed camera.
#[cfg(windows)]
struct CameraBinding {
    slot: SlotKey,
    last_seen: Instant,
}
#[cfg(windows)]
static ADDITIVE_VIDEO_CAMERA_BINDINGS: Mutex<BTreeMap<usize, CameraBinding>> =
    Mutex::new(BTreeMap::new());

/// GLOBAL (cross-thread) map: video_object → monitor's resolved camera source. Filled by the
/// monitor_render_queue hook (140366e90), which runs on the logic/render-prep thread and sees the
/// LOGIC-side monitor object (it has +0x4c8 = render source S and +0x1a8 = input-video slot, so
/// monitor_video_input_handles resolves the wired camera). Consumed by the additive_monitor bind
/// hook (140688ec0), which runs on the GL render thread. A thread_local map does NOT work here
/// because the two hooks are on different threads. Entries are overwritten in place (never cleared
/// on a pass boundary) and capped so stale cameras from old vehicle generations age out.
#[cfg(windows)]
static RENDERER_VIDEO_MONITOR_SOURCE_MAP: Mutex<BTreeMap<usize, u64>> = Mutex::new(BTreeMap::new());

/// Cheap gate run on every additive-monitor bind: locks the runtime briefly (no clone) and
/// returns the capture interval whenever ANY connected slot exists. It deliberately does NOT
/// gate on per-slot rate limiting.
///
/// Earlier this required "at least one slot is due", which caused a multi-monitor starvation
/// bug: `apply` writes the captured frame to slots and stamps their `last_texture_upload_at`,
/// so once the FIRST monitor drawn in a frame was captured, EVERY connected slot became
/// rate-limited, and this gate then returned `None` for every subsequent monitor drawn in the
/// same frame. The second camera's texture was therefore never read (`captured` was always the
/// first monitor's handle). Per-monitor-texture pacing is the job of the per-`gl_id` rate gate
/// (`additive_video_handle_rate_gate`), and per-slot pacing is enforced in `apply`; this entry
/// gate only decides "is there any point doing GL work at all".
#[cfg(windows)]
fn additive_video_connected_capture_interval() -> Option<Duration> {
    let guard = runtime_cell().lock().ok()?;
    if !guard.configured {
        return None;
    }
    let any_connected = guard
        .slots
        .values()
        .any(|slot| slot.connected && component_is_alive(&slot.component));
    if any_connected {
        Some(capture_frame_interval(guard.config.capture.max_fps))
    } else {
        None
    }
}

/// Per-handle rate gate for readback attempts (success or failure) so a persistently failing
/// or blank readback cannot run `glGetTexImage` on every rendered frame and stall the render
/// thread, while still sampling every distinct monitor texture at capture FPS. Returns true
/// when this specific `texture_video` GL id is due for a fresh readback.
#[cfg(windows)]
fn additive_video_handle_rate_gate(gl_id: u32, interval: Duration) -> bool {
    let mut guard = ADDITIVE_VIDEO_HANDLE_OBS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let now = Instant::now();
    // Bound the map so a churn of transient texture ids cannot grow it without limit.
    if guard.len() > 64 {
        guard.retain(|_, obs| now.duration_since(obs.last_attempt) < Duration::from_secs(5));
    }
    match guard.get_mut(&gl_id) {
        Some(obs) if now.duration_since(obs.last_attempt) < interval => false,
        Some(obs) => {
            obs.last_attempt = now;
            true
        }
        None => {
            guard.insert(
                gl_id,
                AdditiveVideoHandleObs {
                    last_attempt: now,
                    native_w: 0,
                    native_h: 0,
                    video_nonzero: 0,
                },
            );
            true
        }
    }
}

/// Record the latest readback result for a handle and, when the summary is due, emit one line
/// listing every monitor texture observed this window with its non-zero pixel count. This makes
/// it obvious which additive_monitor draw carries the camera frame.
#[cfg(windows)]
fn additive_video_record_handle_obs(
    gl_id: u32,
    native_w: u32,
    native_h: u32,
    video_nonzero: usize,
) {
    let mut guard = ADDITIVE_VIDEO_HANDLE_OBS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(obs) = guard.get_mut(&gl_id) {
        obs.native_w = native_w;
        obs.native_h = native_h;
        obs.video_nonzero = video_nonzero;
    }
}

/// Format the per-handle observation table for the throttled status line.
#[cfg(windows)]
fn additive_video_handle_obs_summary() -> String {
    let guard = ADDITIVE_VIDEO_HANDLE_OBS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if guard.is_empty() {
        return "none".to_string();
    }
    guard
        .iter()
        .map(|(handle, obs)| {
            format!(
                "0x{:x}={}x{}:nonzero={}",
                handle, obs.native_w, obs.native_h, obs.video_nonzero
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(windows)]
fn bump_additive_video_counter(update: impl FnOnce(&mut HookRuntimeState)) {
    if let Ok(mut guard) = runtime_cell().lock() {
        update(&mut guard.hook_runtime);
    }
}

#[cfg(windows)]
fn additive_video_diag(message: &str) {
    if let Ok(guard) = runtime_cell().lock() {
        log_runtime_diagnostic_no_snapshot(
            &guard,
            message,
            &ADDITIVE_MONITOR_VIDEO_DIAGNOSTIC_COUNT,
            48,
        );
    }
}

#[cfg(windows)]
static ADDITIVE_VIDEO_LAST_SUMMARY: Mutex<Option<Instant>> = Mutex::new(None);
#[cfg(windows)]
static ADDITIVE_MONITOR_VIDEO_STATUS_COUNT: AtomicUsize = AtomicUsize::new(0);
#[cfg(windows)]
static ADDITIVE_VIDEO_LAST_DEEP_SCAN: Mutex<Option<Instant>> = Mutex::new(None);
#[cfg(windows)]
static ADDITIVE_VIDEO_DEEP_SCAN_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Throttle the continuous status line to ~1/second so it can run every frame without
/// spamming the log.
#[cfg(windows)]
fn additive_video_summary_due() -> bool {
    let mut guard = ADDITIVE_VIDEO_LAST_SUMMARY
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let now = Instant::now();
    match *guard {
        Some(last) if now.duration_since(last) < Duration::from_millis(1000) => false,
        _ => {
            *guard = Some(now);
            true
        }
    }
}

/// Throttle the (relatively expensive) unmatched deep-scan to ~1 every 2s.
#[cfg(windows)]
fn additive_video_deep_scan_due() -> bool {
    let mut guard = ADDITIVE_VIDEO_LAST_DEEP_SCAN
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    let now = Instant::now();
    match *guard {
        Some(last) if now.duration_since(last) < Duration::from_millis(2000) => false,
        _ => {
            *guard = Some(now);
            true
        }
    }
}

/// True for a value that looks like a game heap object pointer: aligned, above the low reserved
/// range, and below the module/stack region (Stormworks module + vtables sit at ~0x7ff7_xxxx_xxxx,
/// game heap objects observed at 0x1c1.../0x208...). Used to avoid matching vtables, module
/// globals, or small tag values when intersecting object graphs.
#[cfg(windows)]
fn is_game_heap_pointer(value: usize) -> bool {
    value >= 0x10000 && value < 0x0000_7f00_0000_0000 && (value & 0x7 == 0)
}

/// The candidate camera-source pointers a slot holds, resolved first (the input-node hook stores
/// the resolved type-6 source at node+0x30).
#[cfg(windows)]
fn slot_source_handles(slot: &SlotState) -> [(&'static str, u64); 5] {
    [
        ("resolved", slot.input_resolved_source_handle),
        ("candidate", slot.input_candidate_source_handle),
        ("selected", slot.input_selected_source_handle),
        ("upstream", slot.input_upstream_source_handle),
        ("input", slot.input_source_handle),
    ]
}

/// Walk the renderer's monitor draw-list (a ring buffer at `scene_state + 0x468`) to find the
/// draw entry whose camera source object `S` binds `video_object`, and return `S`.
///
/// This is the authoritative render-side camera object that all pointer/heap matching from
/// `video_object` alone could never reach: the decompiled monitor loop in `FUN_1406d1960` derives
/// the texture wrapper as `video_object = *(S + 8)` where `S = entry[0]`, then calls the bind hook
/// `FUN_140688ec0` with only `video_object`. `S` (which carries the camera's logic-ref identity) is
/// therefore invisible at the bind hook — we must recover it from the draw list.
///
/// Iterator layout decoded from the game's own thunks (begin `1408aa4c0`, deref `1408a9750`,
/// end `1408aa4b0`): the container at `scene_state + 0x468` is a ring buffer
///   `+0x00` = element data base pointer
///   `+0x08` = capacity (u32, the modulus)
///   `+0x0c` = head    (u32, ring start)
///   `+0x10` = count   (u32, live element count)
/// element stride = 0xd8, and `element(i) = base + ((head + i) % capacity) * 0xd8`.
/// Every pointer read is bounds-checked and the scan is capped, so a malformed container can never
/// walk into unmapped memory — on any inconsistency we simply return None and the caller falls back
/// to the sticky binding.
/// Walk the renderer monitor draw-list (ring buffer at `scene_state + 0x468`) and return the draw
/// ENTRY (0xd8-byte element base) whose camera source object `S = *(entry+0)` binds `video_object`
/// (`*(S+8) == video_object`), together with `S`. Returning the whole entry lets the caller inspect
/// every field of the draw record — the monitor/display object that owns this draw is expected to be
/// one of those fields, and that display object is what carries the wire-accurate input-slot link
/// (`monitor_video_input_handles`). Bounds-checked and capped, so a malformed container returns None.
#[cfg(windows)]
fn additive_video_draw_entry_for_video_object(video_object: usize) -> Option<(usize, usize)> {
    let scene_state = RENDERER_VIDEO_PASS_SCENE_STATE.with(|cell| cell.get());
    if scene_state == 0 {
        return None;
    }
    let container = scene_state.checked_add(0x468)?;
    let data_base = read_usize_field(container, 0x0)?;
    let capacity = read_u32_field(container, 0x8)?;
    let head = read_u32_field(container, 0xc)?;
    let count = read_u32_field(container, 0x10)?;
    if data_base == 0 || capacity == 0 || !is_game_heap_pointer(data_base) {
        return None;
    }
    let scan = count.min(256);
    const ELEMENT_STRIDE: usize = 0xd8;
    for i in 0..scan {
        let ring_index = (head.wrapping_add(i) % capacity) as usize;
        let Some(element) = data_base.checked_add(ring_index.wrapping_mul(ELEMENT_STRIDE)) else {
            continue;
        };
        // entry[0] = S (camera source object); video_object = *(S + 8).
        let Some(s) = read_usize_field(element, 0x0) else {
            continue;
        };
        if s == 0 || !is_game_heap_pointer(s) {
            continue;
        }
        if read_usize_field(s, 0x8) == Some(video_object) {
            return Some((element, s));
        }
    }
    None
}

/// One-shot forensic dump (throttled) of a matched draw entry, its camera source object S, and every
/// live Lua slot's source object — printing the raw heap-pointer fields of each so the true
/// monitor↔camera↔Lua link can be established offline in a SINGLE live test instead of another blind
/// guess. Prints, for the draw entry, every 8-byte slot in its 0xd8 bytes that is a strict heap
/// pointer AND whose target has the vehicle-logic input-video vtable (i.e. looks like a display's
/// input slot) — that is the field we expect to bridge to `monitor_video_input_handles`.
#[cfg(windows)]
fn additive_video_forensic_dump(guard: &RuntimeState, video_object: usize, entry: usize, s: usize) {
    if !additive_video_deep_scan_due() {
        return;
    }
    let field_dump = |base: usize, span: usize| -> String {
        let mut parts = Vec::new();
        for off in (0..span).step_by(8) {
            if let Some(v) = read_usize_field(base, off) {
                if is_strict_heap_pointer(v) {
                    let vtag = if video_source_vtable_static(v as u64)
                        == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO)
                    {
                        "!inputvideo"
                    } else {
                        ""
                    };
                    parts.push(format!("+0x{off:x}=0x{v:x}{vtag}"));
                }
            }
        }
        format!("[{}]", parts.join(","))
    };
    // Also scan raw values (not just strict pointers) for an exact match with video_object.
    // This catches the case where upstream_source stores the video texture wrapper directly,
    // even if its address doesn't pass the strict-pointer test.
    let raw_scan = |base: usize, target: usize| -> String {
        let page = target & !0xfff;
        let mut hits = Vec::new();
        for off in (0..0x180usize).step_by(8) {
            if let Some(v) = read_usize_field(base, off) {
                if v == target {
                    hits.push(format!("+0x{off:x}=EXACT"));
                } else if v & !0xfff == page && v != 0 && v > 0x10000 {
                    hits.push(format!("+0x{off:x}=SAMEPAGE(0x{v:x})"));
                }
            }
        }
        if hits.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", hits.join(","))
        }
    };
    let mut lines = vec![
        format!("video_object=0x{video_object:x}"),
        format!("entry=0x{entry:x} entry_ptrs={}", field_dump(entry, 0xd8)),
        format!("S=0x{s:x} S_ptrs={}", field_dump(s, 0xc0)),
    ];
    for (key, slot) in guard.slots.iter() {
        if !slot.connected || !component_is_alive(&slot.component) {
            continue;
        }
        let src = slot.input_resolved_source_handle;
        let up = slot.input_upstream_source_handle;
        lines.push(format!(
            "slot{}:resolved=0x{:x} resolved_ptrs={} | upstream=0x{:x} upstream_scan={}",
            key.slot,
            src,
            field_dump(src as usize, 0xc0),
            up,
            raw_scan(up as usize, video_object),
        ));
    }
    log_runtime_diagnostic_no_snapshot(
        guard,
        &format!("additive_monitor_video forensic {}", lines.join(" | ")),
        &ADDITIVE_VIDEO_DEEP_SCAN_COUNT,
        48,
    );
}

/// STRICT game heap-object pointer test. Real game heap objects observed live are at ~`0x16d..`,
/// `0x1c1..`, `0x208..` — all ABOVE 2^40 and 8-aligned. The values that polluted every earlier join
/// attempt were logic-graph tag words like `0xbf400000` (below 2^40) and `0x10000000004` (not
/// 8-aligned): this predicate rejects both. Used for the shared-camera-object intersection so only
/// genuine object pointers, never tag values, can match.
#[cfg(windows)]
fn is_strict_heap_pointer(value: usize) -> bool {
    let v = value as u64;
    v >= 0x0000_0100_0000_0000 && v < 0x0000_8000_0000_0000 && (v & 0x7 == 0)
}

/// Collect every distinct camera `video_object` currently in the monitor draw-list, sorted ascending
/// by address. This gives a STABLE, render-order-independent rank for each camera so the fallback can
/// pair camera-rank <-> slot-rank deterministically (both sides sorted the same way), fixing the
/// "always swapped" pairing that first-come binding produced. Bounds-checked and capped identically
/// to the single-entry walk.
#[cfg(windows)]
fn additive_video_all_camera_video_objects() -> Vec<usize> {
    let mut cams: Vec<usize> = Vec::new();
    let scene_state = RENDERER_VIDEO_PASS_SCENE_STATE.with(|cell| cell.get());
    if scene_state == 0 {
        return cams;
    }
    let Some(container) = scene_state.checked_add(0x468) else {
        return cams;
    };
    let Some(data_base) = read_usize_field(container, 0x0) else {
        return cams;
    };
    let Some(capacity) = read_u32_field(container, 0x8) else {
        return cams;
    };
    let Some(head) = read_u32_field(container, 0xc) else {
        return cams;
    };
    let Some(count) = read_u32_field(container, 0x10) else {
        return cams;
    };
    if data_base == 0 || capacity == 0 || !is_game_heap_pointer(data_base) {
        return cams;
    }
    let scan = count.min(256);
    const ELEMENT_STRIDE: usize = 0xd8;
    for i in 0..scan {
        let ring_index = (head.wrapping_add(i) % capacity) as usize;
        let Some(element) = data_base.checked_add(ring_index.wrapping_mul(ELEMENT_STRIDE)) else {
            continue;
        };
        let Some(s) = read_usize_field(element, 0x0) else {
            continue;
        };
        if s == 0 || !is_game_heap_pointer(s) {
            continue;
        }
        if let Some(vobj) = read_usize_field(s, 0x8) {
            if is_strict_heap_pointer(vobj) && !cams.contains(&vobj) {
                cams.push(vobj);
            }
        }
    }
    cams.sort_unstable();
    cams
}

/// Walk the monitor ring-buffer (at `scene_state + 0x558`, stride 0x3b8 confirmed from deref-thunk
/// bytes: `imul rax, rcx, 0x3b8`) and return the monitor element whose camera render source S
/// satisfies `*(S + 8) == video_object`. The monitor struct has `+0x3a8` = S-pointer and
/// `+0x370` = enabled flag (both confirmed from render-loop decompile line 974/989). Returns the
/// pointer to the monitor element in the ring-buffer allocation. The caller can then pass it to
/// `monitor_video_input_handles` to get the monitor's resolved camera source for wire-accurate
/// slot matching.
#[cfg(windows)]
fn additive_video_monitor_for_video_object(video_object: usize) -> Option<usize> {
    let scene_state = RENDERER_VIDEO_PASS_SCENE_STATE.with(|cell| cell.get());
    if scene_state == 0 {
        return None;
    }
    // Monitor list container at scene_state + 0x558 (= scene_state + 0xab * 8).
    let container = scene_state.checked_add(0x558)?;
    let data_base = read_usize_field(container, 0x0)?;
    let capacity = read_u32_field(container, 0x8)?;
    let head = read_u32_field(container, 0xc)?;
    let count = read_u32_field(container, 0x10)?;
    if data_base == 0 || capacity == 0 || !is_game_heap_pointer(data_base) {
        return None;
    }
    let scan = count.min(64);
    const MONITOR_STRIDE: usize = 0x3b8;
    for i in 0..scan {
        let ring_index = (head.wrapping_add(i) % capacity) as usize;
        let Some(monitor) = data_base.checked_add(ring_index.wrapping_mul(MONITOR_STRIDE)) else {
            continue;
        };
        // monitor+0x370 = enabled flag; skip disabled monitors.
        if read_u8_field(monitor, 0x370) == Some(0) {
            continue;
        }
        // monitor+0x3a8 = pointer to S (camera render source).
        let Some(s_ptr) = read_usize_field(monitor, 0x3a8) else {
            continue;
        };
        if s_ptr == 0 || !is_game_heap_pointer(s_ptr) {
            continue;
        }
        // *(S + 8) = video_object.
        if read_usize_field(s_ptr, 0x8) == Some(video_object) {
            return Some(monitor);
        }
    }
    None
}

/// intersecting these sets across the render-side source `S` and a Lua slot's source finds that
/// shared camera object.
#[cfg(windows)]
fn collect_strict_heap_pointers(base: usize) -> BTreeSet<usize> {
    let mut set = BTreeSet::new();
    if base == 0 {
        return set;
    }
    for off in (0..0xC0usize).step_by(8) {
        if let Some(value) = read_usize_field(base, off) {
            if is_strict_heap_pointer(value) {
                set.insert(value);
            }
        }
    }
    set
}

/// Resolve which Lua slot owns this camera texture by a STICKY, first-come binding.
///
/// Every structural attempt to link the render-side video object to the Lua-side input source
/// failed live (7 iterations): forward offset scans (`recipes=[]`), and shared-heap-object joins
/// which produced FALSE matches because object graphs contain logic-graph tag values (e.g.
/// `0xbf400000`) that look like heap pointers and are "shared" by every camera, collapsing both
/// cameras onto one slot. The engine wires cameras by logic-graph IDs, not by pointers we can
/// intersect, so structural routing is abandoned.
///
/// Instead: the first time a distinct camera texture-wrapper object is observed, bind it to one
/// live connected slot and keep that binding fixed. A bound camera always returns its slot (no
/// flicker). A camera that disappears (not drawn for the aging window) frees its slot; a slot whose
/// Lua component dies (vehicle reload) drops its binding so the slot can be reused. When cameras
/// outnumber slots the extra camera gets no binding and the caller uses the all-connected fallback.
///
/// The pairing may be swapped relative to the wiring (first-drawn camera takes the first free slot);
/// the player corrects that by swapping the two input wires. Determinism and zero flicker are the
/// goal here, not wire-accurate routing (which needs the render loop's logic-ref, not available at
/// this hook).
#[cfg(windows)]
fn additive_video_sticky_owner(guard: &RuntimeState, video_object: usize) -> Option<SlotKey> {
    let now = Instant::now();
    // Live connected slots, sorted ascending by their resolved camera-source address (NOT by
    // SlotKey/component string). This is the key change: the earlier "first-come" binding paired
    // draw order ↔ SlotKey order, which is unrelated to which camera each slot is wired to, so the
    // pairing came out consistently swapped. Both sides are now ordered by the same monotonic key
    // (heap address), on the assumption that the render-side camera `video_object` addresses and the
    // Lua-side resolved-source addresses share the engine's creation order. camera-rank ↔ slot-rank.
    let mut ordered_slots: Vec<(u64, SlotKey)> = guard
        .slots
        .iter()
        .filter(|(_, slot)| slot.connected && component_is_alive(&slot.component))
        .map(|(key, slot)| (slot.input_resolved_source_handle, key.clone()))
        .collect();
    ordered_slots.sort_by_key(|(src, key)| (*src, key.clone()));
    let live_slots: Vec<SlotKey> = ordered_slots.into_iter().map(|(_, key)| key).collect();
    if live_slots.is_empty() {
        return None;
    }
    // Slots that still exist with a live component keep their binding across brief disconnects.
    let alive_existing: BTreeSet<SlotKey> = guard
        .slots
        .iter()
        .filter(|(_, slot)| component_is_alive(&slot.component))
        .map(|(key, _)| key.clone())
        .collect();

    let mut bindings = ADDITIVE_VIDEO_CAMERA_BINDINGS
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    // Drop bindings for cameras not drawn recently, or whose slot's component is gone.
    bindings.retain(|_, binding| {
        now.duration_since(binding.last_seen) < Duration::from_millis(3000)
            && alive_existing.contains(&binding.slot)
    });

    // Already bound → refresh and return the same slot (this is what kills the flicker).
    if let Some(binding) = bindings.get_mut(&video_object) {
        binding.last_seen = now;
        return Some(binding.slot.clone());
    }

    // Not bound yet: assign by ADDRESS-ORDER RANK. This camera's rank among all cameras currently in
    // the draw-list (sorted by address) selects the same-rank slot (slots sorted by resolved-source
    // address). This makes camera N ↔ slot N deterministically, fixing the swapped pairing. If the
    // camera is not found in the draw list (race), fall back to the first unclaimed slot.
    let cameras = additive_video_all_camera_video_objects();
    let claimed: BTreeSet<SlotKey> = bindings.values().map(|b| b.slot.clone()).collect();
    let chosen = if let Some(rank) = cameras.iter().position(|c| *c == video_object) {
        live_slots
            .get(rank)
            .filter(|key| !claimed.contains(*key))
            .cloned()
            .or_else(|| {
                live_slots
                    .iter()
                    .find(|key| !claimed.contains(*key))
                    .cloned()
            })
    } else {
        live_slots
            .iter()
            .find(|key| !claimed.contains(*key))
            .cloned()
    };
    let free = chosen?;
    bindings.insert(
        video_object,
        CameraBinding {
            slot: free.clone(),
            last_seen: now,
        },
    );
    Some(free)
}

/// Throttled raw dump of the monitor video object and every live slot's source objects, so the
/// true join can be confirmed offline if the automatic intersection does not resolve a unique
/// owner. Dumps the first 0xC0 bytes of each object as `offset=value` heap-pointer pairs.
#[cfg(windows)]
fn additive_video_deep_scan(guard: &RuntimeState, video_object: usize) {
    if !additive_video_deep_scan_due() {
        return;
    }
    let dump = |base: usize| -> String {
        let mut fields = Vec::new();
        for off in (0..0xC0usize).step_by(8) {
            if let Some(value) = read_usize_field(base, off) {
                if is_game_heap_pointer(value) {
                    fields.push(format!("+0x{:x}=0x{:x}", off, value));
                }
            }
        }
        fields.join(",")
    };
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "video_object=0x{:x} inner8=0x{:x} refs=[{}]",
        video_object,
        read_usize_field(video_object, 8).unwrap_or(0),
        dump(video_object)
    ));
    for slot in guard.slots.values() {
        if !slot.connected || !component_is_alive(&slot.component) {
            continue;
        }
        for (label, handle) in slot_source_handles(slot) {
            if handle == 0 {
                continue;
            }
            lines.push(format!(
                "slot{}:{}=0x{:x} refs=[{}]",
                slot.slot,
                label,
                handle,
                dump(handle as usize)
            ));
        }
    }
    log_runtime_diagnostic_no_snapshot(
        guard,
        &format!("additive_monitor_video object_dump {}", lines.join(" | ")),
        &ADDITIVE_VIDEO_DEEP_SCAN_COUNT,
        32,
    );
}

/// Continuous (not budget-capped) status log, used at ~1/second so a live test can
/// correlate what is visible on a monitor with the texture_video contents at that time.
#[cfg(windows)]
fn additive_video_status(message: &str) {
    if let Ok(guard) = runtime_cell().lock() {
        log_runtime_diagnostic_no_snapshot(
            &guard,
            message,
            &ADDITIVE_MONITOR_VIDEO_STATUS_COUNT,
            usize::MAX,
        );
    }
}

/// Read the GL texture id from a bound additive-monitor texture wrapper (`obj+0x48`) and
/// return its non-zero pixel count. Used to probe the overlay texture for diagnosis when
/// the video texture reads back blank.
#[cfg(windows)]
fn additive_texture_nonzero_pixels(texture_object: usize) -> Option<usize> {
    if texture_object == 0 {
        return None;
    }
    let gl_id = read_u32_field(texture_object, 0x48)?;
    if gl_id == 0 {
        return None;
    }
    let candidate = SourceTextureCandidate {
        handle: gl_id,
        source_handle: texture_object as u64,
        source_offset: 0x48,
        pointer_offset: None,
    };
    read_gl_texture_candidate(candidate)
        .ok()
        .map(|readback| pixel_stats_from_rgb(&readback.rgb).nonzero_pixels)
}

#[cfg(windows)]
fn capture_additive_monitor_video_texture(
    video_texture_object: usize,
    overlay_texture_object: usize,
) -> Result<(), String> {
    if video_texture_object == 0 {
        return Ok(());
    }
    // Only do GL work when a connected slot actually needs a frame.
    let Some(interval) = additive_video_connected_capture_interval() else {
        return Ok(());
    };
    // Read the destination GL texture id for this specific monitor's texture_video sampler
    // BEFORE the rate gate, so the gate can be keyed per monitor texture. A single global
    // gate would sample only the first additive_monitor drawn each frame and starve the
    // camera screen whenever a Lua-overlay-only screen is drawn first.
    let gl_id = read_u32_field(video_texture_object, 0x48).unwrap_or(0);
    if gl_id == 0 {
        additive_video_diag(&format!(
            "additive_monitor_video no_texture object=0x{:x} offset=0x48",
            video_texture_object
        ));
        return Ok(());
    }
    // Per-handle rate gate: every distinct monitor texture is sampled at capture FPS.
    if !additive_video_handle_rate_gate(gl_id, interval) {
        return Ok(());
    }
    bump_additive_video_counter(|hook| {
        hook.additive_monitor_bind_attempts = hook.additive_monitor_bind_attempts.saturating_add(1);
    });
    let candidate = SourceTextureCandidate {
        handle: gl_id,
        source_handle: video_texture_object as u64,
        source_offset: 0x48,
        pointer_offset: None,
    };
    match read_gl_texture_candidate(candidate) {
        Ok(readback) => {
            let (native_w, native_h) = (readback.width, readback.height);
            let video_nonzero = pixel_stats_from_rgb(&readback.rgb).nonzero_pixels;
            additive_video_record_handle_obs(gl_id, native_w, native_h, video_nonzero);
            if video_nonzero > 0 {
                apply_additive_monitor_video_readback_to_slots(
                    gl_id,
                    video_texture_object,
                    readback,
                )?;
            } else {
                bump_additive_video_counter(|hook| {
                    hook.additive_monitor_bind_blank_reads =
                        hook.additive_monitor_bind_blank_reads.saturating_add(1);
                });
            }
            // Continuous ~1/s status listing EVERY monitor texture observed this window with
            // its non-zero pixel count, so a live test can see which additive_monitor draw
            // carries the camera frame instead of only the first one drawn.
            if additive_video_summary_due() {
                let overlay = additive_texture_nonzero_pixels(overlay_texture_object)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "unreadable".to_string());
                additive_video_status(&format!(
                    "additive_monitor_video status monitors=[{}] last_handle=0x{:x} last={}x{} last_video_nonzero={} last_overlay_nonzero={}",
                    additive_video_handle_obs_summary(),
                    gl_id,
                    native_w,
                    native_h,
                    video_nonzero,
                    overlay,
                ));
            }
        }
        Err(error) => {
            bump_additive_video_counter(|hook| {
                hook.additive_monitor_bind_read_errors =
                    hook.additive_monitor_bind_read_errors.saturating_add(1);
            });
            additive_video_diag(&format!(
                "additive_monitor_video readback_error object=0x{:x} handle=0x{:x} error={}",
                video_texture_object, gl_id, error
            ));
        }
    }
    Ok(())
}

/// Copy a captured additive_monitor `texture_video` frame into the Lua video slots that
/// belong to *this specific monitor*.
///
/// Correct per-camera routing is the key to independent multi-component capture. The
/// decompiled monitor draw loop in `FUN_1406d1960` derives the video texture wrapper as
/// `uVar22 = *(*plVar26 + 8)`, where `plVar26[0]` is the resolved video source object. Each
/// Lua slot already stores that resolved source object as `input_source_handle` (set by the
/// `140373050` input-node hook). So a slot owns this frame iff
/// `*(slot.input_source_handle + 8) == video_texture_object`.
///
/// We therefore write the frame ONLY to slots whose source maps to this monitor's video
/// texture wrapper. When no slot maps precisely (e.g. the mapping pointer chain is not
/// readable on this build), we fall back to a single connected slot only when the whole
/// runtime has exactly one connected slot, so a lone-camera vehicle still works while a
/// multi-camera vehicle never bleeds one camera's frame into another slot.
#[cfg(windows)]
fn apply_additive_monitor_video_readback_to_slots(
    gl_id: u32,
    video_texture_object: usize,
    readback: SourceTextureReadback,
) -> Result<usize, String> {
    let stats = pixel_stats_from_rgb(&readback.rgb);
    let mut guard = runtime_cell()
        .lock()
        .map_err(|_| "runtime mutex poisoned".to_string())?;
    if !guard.configured {
        return Ok(0);
    }
    if stats.nonzero_pixels == 0 {
        // All-zero frame: the texture is not carrying a real camera image yet. Do not
        // flip video.isReady() on a blank frame, but record it so a live test can see the
        // hook fires and reads a real texture id.
        guard.hook_runtime.additive_monitor_bind_blank_reads = guard
            .hook_runtime
            .additive_monitor_bind_blank_reads
            .saturating_add(1);
        log_runtime_diagnostic_no_snapshot(
            &guard,
            &format!(
                "additive_monitor_video blank handle=0x{:x} native={}x{} stats={}",
                gl_id,
                readback.width,
                readback.height,
                format_pixel_stats(&stats)
            ),
            &ADDITIVE_MONITOR_VIDEO_DIAGNOSTIC_COUNT,
            48,
        );
        return Ok(0);
    }
    let capture_interval = capture_frame_interval(guard.config.capture.max_fps);
    let now = Instant::now();

    // Lifecycle hygiene, evaluated every captured frame (not just at video.init): drop slots
    // whose Lua component has stopped ticking. After a vehicle reload the game builds a new
    // component context and abandons the old one without ever telling us, so a stale slot would
    // otherwise linger `connected` forever and inflate the connected count. Liveness = "did this
    // component call video.* recently", which self-heals within the liveness window.
    let dead_keys: Vec<SlotKey> = guard
        .slots
        .keys()
        .filter(|key| !component_is_alive(&key.component))
        .cloned()
        .collect();
    for key in dead_keys {
        guard.slots.remove(&key);
    }

    // Determine which connected slots own THIS monitor's video texture wrapper.
    //
    // The decompiled monitor draw derives the wrapper as `video_object = *(camera_source + 8)`,
    // where `camera_source` is the resolved type-6 video source object. Each slot records that
    // source under several handle fields (candidate/selected/resolved/upstream); which field
    // holds the true camera source varies, and the earlier code only scanned
    // `input_source_handle` (== upstream) which is why `source_match` never fired. We now scan
    // ALL of a slot's deduped handles, direct and one level of indirection, over a small offset
    // set, and log the exact handle+offset that hits so the mapping recipe is confirmed live.
    let live_connected_slot_count = guard
        .slots
        .values()
        .filter(|slot| slot.connected && component_is_alive(&slot.component))
        .count();
    // STAGE 1 — shallow forward match: is any slot handle the monitor's video source object
    // `plVar26[0]` itself (`*(handle+off)==video_object` direct, or `*(*(handle+off))+8` one hop)?
    // Live logs proved this never fires on this build (the Lua-side source object and the
    // render-queue-side source object are distinct references), but it is cheap and correct when it
    // does, so we keep it as the highest-confidence path.
    let mut precise_keys: Vec<SlotKey> = Vec::new();
    let mut source_probe: Vec<String> = Vec::new();
    const SCAN_OFFSETS: [usize; 7] = [0x0, 0x8, 0x10, 0x18, 0x20, 0x28, 0x30];
    for (key, slot) in guard.slots.iter() {
        if !slot.connected || !component_is_alive(&slot.component) {
            continue;
        }
        let handles = slot_source_handles(slot);
        let mut matched = false;
        'handle: for (label, handle) in handles {
            if handle == 0 {
                continue;
            }
            let base = handle as usize;
            for off in SCAN_OFFSETS {
                if read_usize_field(base, off) == Some(video_texture_object) {
                    matched = true;
                    if source_probe.len() < 8 {
                        source_probe.push(format!("{}:{}+0x{:x}", slot.slot, label, off));
                    }
                    break 'handle;
                }
                if let Some(inner) = read_usize_field(base, off) {
                    if inner != 0 && read_usize_field(inner, 8) == Some(video_texture_object) {
                        matched = true;
                        if source_probe.len() < 8 {
                            source_probe.push(format!("{}:*({}+0x{:x})+8", slot.slot, label, off));
                        }
                        break 'handle;
                    }
                }
            }
        }
        if matched {
            precise_keys.push(key.clone());
        }
    }

    // STAGE 0 — monitor-list scan. Walk the monitor ring-buffer at scene_state+0x558 (stride
    // 0x3b8, confirmed from deref-thunk bytes). Each monitor has +0x3a8 = S-pointer and
    // *(S+8) = video_object. Find the monitor whose camera matches video_texture_object, then call
    // monitor_video_input_handles to get its resolved camera source. Match that against Lua slot
    // input_resolved_source_handle: same source == same camera wire == correct pairing.
    let mut video_logic_graph_match_used = false;
    let mut monitor_source_match_used = false;
    if precise_keys.is_empty() {
        // O(log n) lookup into the global cache filled by the monitor_render_queue hook (140366e90)
        // on the logic/render-prep thread. Global Mutex (not thread_local) so the GL-thread bind
        // hook sees entries written by the other thread. No per-call VirtualQuery scanning.
        let monitor_source = RENDERER_VIDEO_MONITOR_SOURCE_MAP
            .lock()
            .ok()
            .and_then(|map| map.get(&video_texture_object).copied())
            .unwrap_or(0);
        if monitor_source != 0 {
            source_probe.push(format!("monitor_src=0x{:x}", monitor_source));
            if let Some(monitor_output) = video_logic_output_for_input(monitor_source) {
                let mut graph_keys: Vec<SlotKey> = Vec::new();
                for (key, slot) in guard.slots.iter() {
                    if !slot.connected || !component_is_alive(&slot.component) {
                        continue;
                    }
                    let lua_input = slot_upstream_source_handle(slot);
                    if video_logic_output_for_input(lua_input) == Some(monitor_output) {
                        graph_keys.push(key.clone());
                    }
                }
                source_probe.push(format!(
                    "graph:monitor_input=0x{:x}->output=0x{:x}:matches={}",
                    monitor_source,
                    monitor_output,
                    graph_keys.len()
                ));
                if graph_keys.len() == 1 {
                    precise_keys.extend(graph_keys);
                    video_logic_graph_match_used = true;
                }
            }
            if precise_keys.is_empty() {
                let mut matched_keys: Vec<SlotKey> = Vec::new();
                for (key, slot) in guard.slots.iter() {
                    if !slot.connected || !component_is_alive(&slot.component) {
                        continue;
                    }
                    if slot.input_resolved_source_handle == monitor_source {
                        matched_keys.push(key.clone());
                    }
                }
                if matched_keys.len() == 1 {
                    source_probe.push(format!("monitor_match:slot{}", matched_keys[0].slot));
                    precise_keys.extend(matched_keys);
                    monitor_source_match_used = true;
                } else {
                    source_probe.push(format!("monitor_match_ambiguous:{}", matched_keys.len()));
                }
            }
        } else {
            source_probe.push("monitor_src=0(no_cache_hit)".to_string());
        }
    }

    // STAGE 1b — upstream_source direct scan.
    // `input_upstream_source_handle` for a field that equals `video_texture_object` exactly.
    // If the camera's logic node stores its video texture wrapper at a fixed offset, this is the
    // cheapest and most direct match: one equality check per 8-byte slot, no pointer chasing.
    // This is tried BEFORE the draw-entry intersection because it operates purely on logic-side
    // data we already have from the node-binding hook.
    if precise_keys.is_empty() {
        let mut upstream_match_keys: Vec<SlotKey> = Vec::new();
        for (key, slot) in guard.slots.iter() {
            if !slot.connected || !component_is_alive(&slot.component) {
                continue;
            }
            let up = slot.input_upstream_source_handle as usize;
            if up == 0 {
                continue;
            }
            for off in (0..0x180usize).step_by(8) {
                if read_usize_field(up, off) == Some(video_texture_object) {
                    upstream_match_keys.push(key.clone());
                    source_probe.push(format!("upstream_match:slot{}+0x{:x}", key.slot, off));
                    break;
                }
            }
        }
        if upstream_match_keys.len() == 1 {
            precise_keys.extend(upstream_match_keys);
        } else if upstream_match_keys.len() > 1 {
            // Multiple slots matched the same video_object — ambiguous, fall through.
            source_probe.push(format!(
                "upstream_match_ambiguous:{}",
                upstream_match_keys.len()
            ));
        }
    }

    // STAGE 2 — draw-entry join. Recover the render-side monitor DRAW ENTRY (the full 0xd8-byte
    // element, `entry[0]==S`, `video_object==*(S+8)`) by walking the draw list, then find which Lua
    // slot owns it.
    //
    // The camera output node feeds BOTH the on-screen monitor input node and the Lua input node; the
    // engine's `140373050` resolves each input node's `source->vtable[0x38]()` and stores the camera
    // object at `node+0x30`. So the wire-accurate key is: an object referenced by the monitor draw
    // entry that is ALSO referenced by exactly one Lua slot's source. We gather a strict-pointer set
    // from the whole draw entry (its own 0xd8 bytes plus one level through each pointer it holds — S,
    // and any embedded monitor/camera object), and intersect it with each slot's source pointer set
    // (source handles plus one level). Unique single-slot match wins; multi-slot objects are globals
    // and are dropped. A full raw dump of the entry, S, and every slot source is logged (throttled)
    // so the exact link can be confirmed offline if the automatic match does not resolve.
    let mut logic_join_used = false;
    if precise_keys.is_empty() {
        if let Some((entry, s)) = additive_video_draw_entry_for_video_object(video_texture_object) {
            // Render-side pointer set: the draw entry's own strict pointers, plus one level through
            // each (so S's fields and any embedded monitor/camera object's fields are included).
            let mut render_set: BTreeSet<usize> = collect_strict_heap_pointers(entry);
            let direct: Vec<usize> = render_set.iter().copied().collect();
            for p in direct {
                for q in collect_strict_heap_pointers(p) {
                    render_set.insert(q);
                }
            }
            source_probe.push(format!(
                "entry=0x{:x} S=0x{:x} render_ptrs={}",
                entry,
                s,
                render_set.len()
            ));

            let mut per_slot: Vec<(SlotKey, BTreeSet<usize>)> = Vec::new();
            let mut reach_count: BTreeMap<usize, u32> = BTreeMap::new();
            for (key, slot) in guard.slots.iter() {
                if !slot.connected || !component_is_alive(&slot.component) {
                    continue;
                }
                // Lua-side pointer set: each source handle itself, plus its strict pointers, plus one
                // level deeper — mirrors the render side so a shared camera object at either depth is
                // found.
                let mut slot_set: BTreeSet<usize> = BTreeSet::new();
                for (_, handle) in slot_source_handles(slot) {
                    if handle == 0 {
                        continue;
                    }
                    let h = handle as usize;
                    if is_strict_heap_pointer(h) {
                        slot_set.insert(h);
                    }
                    for p in collect_strict_heap_pointers(h) {
                        slot_set.insert(p);
                        for q in collect_strict_heap_pointers(p) {
                            slot_set.insert(q);
                        }
                    }
                }
                let shared: BTreeSet<usize> = render_set.intersection(&slot_set).copied().collect();
                for &obj in &shared {
                    *reach_count.entry(obj).or_insert(0) += 1;
                }
                if !shared.is_empty() {
                    per_slot.push((key.clone(), shared));
                }
            }
            // Keep slots sharing an object reachable from EXACTLY ONE slot (per-camera, not global).
            let mut unique_owners: Vec<(SlotKey, usize)> = Vec::new();
            for (key, shared) in &per_slot {
                if let Some(&obj) = shared
                    .iter()
                    .find(|obj| reach_count.get(obj).copied().unwrap_or(0) == 1)
                {
                    unique_owners.push((key.clone(), obj));
                }
            }
            if source_probe.len() < 8 {
                let desc: Vec<String> = unique_owners
                    .iter()
                    .map(|(k, obj)| format!("slot{}~cam0x{:x}", k.slot, obj))
                    .collect();
                source_probe.push(format!(
                    "join=[{}] slots_with_shared={}",
                    desc.join(";"),
                    per_slot.len()
                ));
            }
            // One-shot raw dump (throttled) so the true link is visible offline regardless of match.
            additive_video_forensic_dump(&guard, video_texture_object, entry, s);
            if unique_owners.len() == 1 {
                precise_keys.push(unique_owners[0].0.clone());
                logic_join_used = true;
            }
        }
    }

    // STAGE 3 — sticky owner binding (deterministic fallback). If the logic-ref join did not resolve
    // a unique owner, bind each distinct camera texture to a slot on first sight and keep it fixed so
    // there is no flicker. The pairing may be swapped vs the wiring (fix by swapping input wires).
    if precise_keys.is_empty() {
        if let Some(owner) = additive_video_sticky_owner(&guard, video_texture_object) {
            source_probe.push(format!("sticky:slot{}", owner.slot));
            precise_keys.push(owner);
        }
    }

    // STAGE 4 — routing decision:
    // - shared-object join (from render-side S) found a unique owning slot: wire-accurate routing.
    // - sticky binding found the owning slot: deterministic (possibly swapped) routing.
    // - otherwise fall back to every live connected slot so a single-Lua vehicle is always correct
    //   and nothing goes all-black.
    let routing = if precise_keys.is_empty() {
        additive_video_deep_scan(&guard, video_texture_object);
        "all_connected_fallback"
    } else if video_logic_graph_match_used {
        "video_logic_graph_match"
    } else if monitor_source_match_used {
        "monitor_source_match"
    } else if logic_join_used {
        "shared_object_join"
    } else if source_probe.iter().any(|p| p.starts_with("sticky:")) {
        "sticky_binding"
    } else {
        "source_match"
    };

    let target_keys: Vec<SlotKey> = if precise_keys.is_empty() {
        guard
            .slots
            .iter()
            .filter(|(_, slot)| slot.connected && component_is_alive(&slot.component))
            .map(|(key, _)| key.clone())
            .collect()
    } else {
        precise_keys
    };
    let _ = live_connected_slot_count;

    let mut updated = 0usize;
    let mut updated_slot_sizes = Vec::new();
    for key in target_keys {
        let Some(slot) = guard.slots.get_mut(&key) else {
            continue;
        };
        if texture_upload_slot_is_rate_limited(slot, now, capture_interval) {
            continue;
        }
        // Convert once to RGB at the slot's requested size. Storage is mode-agnostic; the
        // gray/rgb getters derive their own shape from this RGB frame, so a slot initialized
        // as either mode becomes ready from the same capture.
        let rgb = resize_rgb_nearest(
            &readback.rgb,
            readback.width,
            readback.height,
            slot.width,
            slot.height,
        )?;
        let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
        slot.frame_id = frame_id;
        slot.ready = true;
        slot.connected = true;
        slot.latest_frame = Some(FrameBuffer {
            frame_id,
            width: slot.width,
            height: slot.height,
            source: "monitor_render".to_string(),
            rgb,
        });
        slot.source_texture_handle = Some(u64::from(gl_id));
        slot.last_texture_upload_at = Some(now);
        updated_slot_sizes.push(format!(
            "{}:{}x{}:{}",
            slot.slot, slot.width, slot.height, slot.mode
        ));
        updated = updated.saturating_add(1);
    }
    if updated > 0 {
        guard.hook_runtime.real_video_capture = true;
        guard.hook_runtime.additive_monitor_bind_frames = guard
            .hook_runtime
            .additive_monitor_bind_frames
            .saturating_add(updated as u64);
        log_runtime_diagnostic_no_snapshot(
            &guard,
            &format!(
                "additive_monitor_video captured build={} handle=0x{:x} video_object=0x{:x} native={}x{} routing={} source_probe=[{}] updated_slots={} slot_sizes={} source_stats={}",
                VIDEO_GET_BUILD_TAG,
                gl_id,
                video_texture_object,
                readback.width,
                readback.height,
                routing,
                source_probe.join(","),
                updated,
                updated_slot_sizes.join(","),
                format_pixel_stats(&stats),
            ),
            &ADDITIVE_MONITOR_BIND_CAPTURE_DIAGNOSTIC_COUNT,
            64,
        );
    }
    Ok(updated)
}

#[cfg(windows)]
fn gl_render_iat_status_value() -> serde_json::Value {
    serde_json::json!({
        "wgl_get_proc_address": {
            "installed": WGL_GET_PROC_ADDRESS_IAT_INSTALLED.load(Ordering::SeqCst),
            "iat_va": hex_u64(STORMWORKS_WGL_GET_PROC_ADDRESS_IAT_VA),
            "original": hex_u64(WGL_GET_PROC_ADDRESS_ORIGINAL.load(Ordering::SeqCst) as u64),
            "hook": hex_u64(stormworks_video_get_wgl_get_proc_address_hook as *const c_void as u64)
        },
        "gl_bind_texture": {
            "installed": GL_BIND_TEXTURE_IAT_INSTALLED.load(Ordering::SeqCst),
            "iat_va": hex_u64(STORMWORKS_GL_BIND_TEXTURE_IAT_VA),
            "original": hex_u64(GL_BIND_TEXTURE_ORIGINAL.load(Ordering::SeqCst) as u64),
            "hook": hex_u64(stormworks_video_get_gl_bind_texture_hook as *const c_void as u64)
        },
        "dynamic_originals": {
            "glBindTextureUnit": hex_u64(GL_BIND_TEXTURE_UNIT_ORIGINAL.load(Ordering::SeqCst) as u64),
            "glBindTextures": hex_u64(GL_BIND_TEXTURES_ORIGINAL.load(Ordering::SeqCst) as u64),
            "glFramebufferTexture2D": hex_u64(GL_FRAMEBUFFER_TEXTURE_2D_ORIGINAL.load(Ordering::SeqCst) as u64),
            "glFramebufferTexture": hex_u64(GL_FRAMEBUFFER_TEXTURE_ORIGINAL.load(Ordering::SeqCst) as u64),
            "glFramebufferTextureLayer": hex_u64(GL_FRAMEBUFFER_TEXTURE_LAYER_ORIGINAL.load(Ordering::SeqCst) as u64)
        }
    })
}

#[cfg(not(windows))]
fn gl_render_iat_status_value() -> serde_json::Value {
    serde_json::json!({
        "installed": false
    })
}

#[cfg(windows)]
fn gl_bind_texture_iat_status_value() -> serde_json::Value {
    gl_render_iat_status_value()["gl_bind_texture"].clone()
}

#[cfg(not(windows))]
fn gl_bind_texture_iat_status_value() -> serde_json::Value {
    serde_json::json!({
        "installed": false
    })
}

#[derive(Debug, Clone, Copy)]
struct AdditiveMonitorTextureCandidate {
    handle: u32,
    monitor: usize,
    draw_item: usize,
    texture_arg: usize,
    texture_object: usize,
    handle_offset: usize,
    pointer_offset: Option<usize>,
    mapped_from: &'static str,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct AdditiveMonitorGlBindFrame {
    material: usize,
    draw_item: usize,
    texture_video: usize,
    texture_overlay: usize,
    video_texture_object: usize,
    arg6: usize,
    arg7: usize,
    arg8: usize,
    arg10: usize,
    arg13: usize,
    arg16: usize,
    renderer_pass: Option<RendererVideoPassFrame>,
    bind_index: u32,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct AdditiveGlBindContext {
    input_handle: u64,
    input_handles: [u64; 6],
    monitor: usize,
}

#[cfg(windows)]
impl AdditiveMonitorGlBindFrame {
    fn candidate(self, handle: u32, unit: u32) -> AdditiveMonitorTextureCandidate {
        AdditiveMonitorTextureCandidate {
            handle,
            monitor: monitor_from_additive_draw_item(self.draw_item).unwrap_or(0),
            draw_item: self.draw_item,
            texture_arg: self.texture_video,
            texture_object: self.video_texture_object,
            handle_offset: ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
            pointer_offset: None,
            mapped_from: additive_gl_bind_candidate_source(unit),
        }
    }
}

#[derive(Debug, Clone)]
struct AdditiveMonitorBindProbeReport {
    material: usize,
    draw_item: usize,
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    input_slot_handle: u64,
    texture_video: usize,
    texture_overlay: usize,
    candidates: usize,
    read_errors: usize,
    blank_reads: usize,
    selected_slots: usize,
    updated_slots: usize,
    skipped_fps_slots: usize,
    details: Vec<String>,
}

fn probe_additive_monitor_bind_texture(
    material: usize,
    draw_item: usize,
    texture_video: usize,
    texture_overlay: usize,
    video_texture_object: usize,
    arg6: usize,
    arg7: usize,
    arg8: usize,
) -> Result<usize, String> {
    #[cfg(windows)]
    {
        probe_additive_monitor_bind_texture_windows(
            material,
            draw_item,
            texture_video,
            texture_overlay,
            video_texture_object,
            arg6,
            arg7,
            arg8,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = material;
        let _ = draw_item;
        let _ = texture_video;
        let _ = texture_overlay;
        let _ = video_texture_object;
        let _ = arg6;
        let _ = arg7;
        let _ = arg8;
        Ok(0)
    }
}

#[cfg(windows)]
fn probe_additive_monitor_bind_texture_windows(
    material: usize,
    draw_item: usize,
    texture_video: usize,
    texture_overlay: usize,
    video_texture_object: usize,
    arg6: usize,
    arg7: usize,
    arg8: usize,
) -> Result<usize, String> {
    let monitor = monitor_from_additive_draw_item(draw_item);
    let active = monitor
        .map(|monitor| read_u8_field(monitor, MONITOR_ACTIVE_OFFSET).unwrap_or(0) != 0)
        .unwrap_or(false);
    let monitor_inputs = monitor.map(monitor_video_input_handles).unwrap_or_default();
    let input_slot_handle = monitor_inputs.slot_ref;
    let effective_input_handle = monitor_inputs.effective();
    let width = monitor
        .and_then(|monitor| read_u32_field(monitor, MONITOR_WIDTH_OFFSET))
        .unwrap_or(0);
    let height = monitor
        .and_then(|monitor| read_u32_field(monitor, MONITOR_HEIGHT_OFFSET))
        .unwrap_or(0);
    let mut report = AdditiveMonitorBindProbeReport {
        material,
        draw_item,
        monitor: monitor.unwrap_or(0),
        monitor_width: width,
        monitor_height: height,
        input_slot_handle: effective_input_handle,
        texture_video,
        texture_overlay,
        candidates: 0,
        read_errors: 0,
        blank_reads: 0,
        selected_slots: 0,
        updated_slots: 0,
        skipped_fps_slots: 0,
        details: Vec::new(),
    };
    let slots = if monitor.is_none() {
        report.details.push(format!(
            "monitor_unmapped readback_disabled draw_item={} texture_video={} texture_object={} arg6={} arg7={} arg8={} details_skipped=heavy_additive_diagnostic",
            format_hex_or_zero(draw_item as u64),
            format_hex_or_zero(texture_video as u64),
            format_hex_or_zero(video_texture_object as u64),
            format_hex_or_zero(arg6 as u64),
            format_hex_or_zero(arg7 as u64),
            format_hex_or_zero(arg8 as u64)
        ));
        record_additive_monitor_bind_probe_report(&report)?;
        return Ok(0);
    } else if !active || effective_input_handle == 0 {
        report.details.push(format!(
            "monitor_inactive_or_unbound active={} monitor_inputs=[{}] effective={}",
            active,
            monitor_inputs.diagnostic(),
            format_hex_or_zero(effective_input_handle)
        ));
        record_additive_monitor_bind_probe_report(&report)?;
        return Ok(0);
    } else {
        let slots =
            additive_monitor_bind_probe_slots_for_handles(monitor_inputs.relation_handles())?;
        if slots.is_empty() {
            report.details.push(format!(
                "no_matching_lua_slots monitor_inputs=[{}] effective={}",
                monitor_inputs.diagnostic(),
                format_hex_or_zero(effective_input_handle)
            ));
            record_additive_monitor_bind_probe_report(&report)?;
            return Ok(0);
        }
        if slot_keys_are_ready_for_lua(&slots) {
            return Ok(0);
        }
        slots
    };
    let mut candidates = collect_additive_monitor_texture_candidates(
        monitor.unwrap_or(0),
        draw_item,
        texture_video,
        video_texture_object,
    );
    let mut bound_unit_error = None;
    match additive_monitor_bound_unit_candidate(
        monitor.unwrap_or(0),
        draw_item,
        texture_video,
        video_texture_object,
    ) {
        Ok(candidate) => {
            upsert_additive_monitor_bound_unit_candidate(&mut candidates, candidate);
        }
        Err(error) => {
            bound_unit_error = Some(error);
        }
    }
    report.candidates = candidates.len();
    record_additive_monitor_bind_with_slots_diagnostic(
        material,
        draw_item,
        texture_video,
        texture_overlay,
        video_texture_object,
        arg6,
        arg7,
        arg8,
        monitor,
        active,
        input_slot_handle,
        candidates.len(),
    )?;
    report.selected_slots = slots.len();
    if candidates.is_empty() {
        report
            .details
            .push(additive_monitor_texture_arg_layout(texture_video));
        report
            .details
            .push(additive_monitor_texture_object_layout(video_texture_object));
        report.details.push(format!(
            "bound_units_after_bind={}",
            additive_monitor_bound_texture_units()
        ));
        report.details.push(format!(
            "arg_layouts={} | {} | {}",
            compact_pointer_layout("arg6", arg6),
            compact_pointer_layout("arg7", arg7),
            compact_pointer_layout("arg8", arg8)
        ));
        if let Some(error) = bound_unit_error {
            report
                .details
                .push(format!("bound_unit_candidate_error={error}"));
        }
        record_additive_monitor_bind_probe_report(&report)?;
        return Ok(0);
    }
    report.details.push(format!(
        "diagnostic_only_readback_disabled candidates=[{}]",
        format_additive_monitor_candidates(&candidates)
    ));
    record_additive_monitor_bind_probe_report(&report)?;
    Ok(report.updated_slots)
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn record_additive_monitor_bind_with_slots_diagnostic(
    material: usize,
    draw_item: usize,
    texture_video: usize,
    texture_overlay: usize,
    video_texture_object: usize,
    arg6: usize,
    arg7: usize,
    arg8: usize,
    monitor: Option<usize>,
    active: bool,
    input_slot_handle: u64,
    candidate_count: usize,
) -> Result<(), String> {
    let state = request_runtime_state()?;
    if state.slots.is_empty() {
        return Ok(());
    }
    if !diagnostic_budget_available(&ADDITIVE_MONITOR_BIND_WITH_SLOTS_DIAGNOSTIC_COUNT, 32) {
        return Ok(());
    }
    log_runtime_diagnostic(
        &state,
        &format!(
            "additive monitor bind hit slots={} material={} draw_item={} monitor={} active={} input_slot_handle={} texture_video={} texture_overlay={} {} {} candidates={} slots={}",
            state.slots.len(),
            format_hex_or_zero(material as u64),
            format_hex_or_zero(draw_item as u64),
            monitor
                .map(|value| format_hex_or_zero(value as u64))
                .unwrap_or_else(|| "none".to_string()),
            active,
            format_hex_or_zero(input_slot_handle),
            format_hex_or_zero(texture_video as u64),
            format_hex_or_zero(texture_overlay as u64),
            additive_monitor_texture_arg_layout(texture_video),
            additive_monitor_texture_object_layout(video_texture_object),
            candidate_count,
            describe_slots(&state)
        ),
        &ADDITIVE_MONITOR_BIND_WITH_SLOTS_DIAGNOSTIC_COUNT,
        32,
    );
    if diagnostic_budget_available(&ADDITIVE_MONITOR_BIND_WITH_SLOTS_DIAGNOSTIC_COUNT, 32) {
        log_runtime_diagnostic(
            &state,
            &format!(
                "additive monitor bind arg layouts material={} draw_item={} arg6={} arg7={} arg8={} layouts=[{} | {} | {}]",
                format_hex_or_zero(material as u64),
                format_hex_or_zero(draw_item as u64),
                format_hex_or_zero(arg6 as u64),
                format_hex_or_zero(arg7 as u64),
                format_hex_or_zero(arg8 as u64),
                compact_pointer_layout("arg6", arg6),
                compact_pointer_layout("arg7", arg7),
                compact_pointer_layout("arg8", arg8)
            ),
            &ADDITIVE_MONITOR_BIND_WITH_SLOTS_DIAGNOSTIC_COUNT,
            32,
        );
    }
    Ok(())
}

#[cfg(windows)]
fn format_additive_monitor_candidates(candidates: &[AdditiveMonitorTextureCandidate]) -> String {
    if candidates.is_empty() {
        return "none".to_string();
    }
    candidates
        .iter()
        .take(8)
        .map(|candidate| {
            format!(
                "0x{:x}@{} object={} handle_offset=0x{:x} pointer_offset={}",
                candidate.handle,
                candidate.mapped_from,
                format_hex_or_zero(candidate.texture_object as u64),
                candidate.handle_offset,
                candidate
                    .pointer_offset
                    .map(|value| format!("0x{value:x}"))
                    .unwrap_or_else(|| "none".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(windows)]
fn describe_additive_monitor_candidates_gl(
    candidates: &[AdditiveMonitorTextureCandidate],
) -> String {
    if candidates.is_empty() {
        return "none".to_string();
    }
    candidates
        .iter()
        .take(8)
        .map(|candidate| {
            format!(
                "0x{:x}@{}:{}",
                candidate.handle,
                candidate.mapped_from,
                describe_gl_texture_handle(candidate.handle)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(windows)]
fn describe_gl_texture_handle(handle: u32) -> String {
    if handle == 0 {
        return "zero".to_string();
    }
    if unsafe { glIsTexture(handle) } == 0 {
        return "glIsTexture=false".to_string();
    }
    drain_gl_errors();
    let previous_texture = current_gl_texture_binding_2d().unwrap_or(0);
    drain_gl_errors();
    call_original_gl_bind_texture(GL_TEXTURE_2D, handle);
    let bind_error = gl_error();
    if bind_error != GL_NO_ERROR {
        restore_gl_texture_binding_2d(previous_texture);
        return format!("glBindTexture_error=0x{bind_error:x}");
    }
    let mut width = 0i32;
    let mut height = 0i32;
    unsafe {
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_WIDTH, &mut width);
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_HEIGHT, &mut height);
    }
    let level_error = gl_error();
    restore_gl_texture_binding_2d(previous_texture);
    if level_error != GL_NO_ERROR {
        return format!("glGetTexLevelParameteriv_error=0x{level_error:x}");
    }
    format!("glIsTexture=true size={}x{}", width, height)
}

#[cfg(windows)]
fn describe_current_bound_gl_texture_2d(expected_handle: u32) -> String {
    if expected_handle == 0 {
        return "zero".to_string();
    }
    let binding = current_gl_texture_binding_2d().unwrap_or(0);
    if binding != expected_handle {
        return format!("binding=0x{binding:x} expected=0x{expected_handle:x} mismatch");
    }
    if unsafe { glIsTexture(expected_handle) } == 0 {
        return format!("binding=0x{binding:x} glIsTexture=false");
    }
    drain_gl_errors();
    let mut width = 0i32;
    let mut height = 0i32;
    unsafe {
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_WIDTH, &mut width);
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_HEIGHT, &mut height);
    }
    let level_error = gl_error();
    if level_error != GL_NO_ERROR {
        return format!("binding=0x{binding:x} glGetTexLevelParameteriv_error=0x{level_error:x}");
    }
    format!(
        "binding=0x{binding:x} glIsTexture=true size={}x{}",
        width, height
    )
}

fn monitor_from_additive_draw_item(draw_item: usize) -> Option<usize> {
    if draw_item == 0 || !memory_range_is_readable(draw_item as *const c_void, size_of::<usize>()) {
        return None;
    }
    let monitor = monitor_from_additive_draw_item_direct(draw_item)
        .or_else(|| monitor_from_additive_draw_item_scan(draw_item))
        .or_else(|| monitor_from_additive_draw_item_back_pointer(draw_item))?;
    Some(monitor)
}

fn monitor_from_additive_draw_item_direct(draw_item: usize) -> Option<usize> {
    let monitor = read_usize_field(draw_item, 0)?;
    monitor_if_plausible(monitor)
}

fn monitor_from_additive_draw_item_back_pointer(draw_item: usize) -> Option<usize> {
    let base = draw_item.checked_sub(ADDITIVE_MONITOR_DRAW_ITEM_MONITOR_BACK_OFFSET)?;
    let monitor = read_usize_field(base, 0)?;
    monitor_if_plausible(monitor)
}

fn monitor_from_additive_draw_item_scan(draw_item: usize) -> Option<usize> {
    if !memory_range_is_readable(
        draw_item as *const c_void,
        ADDITIVE_MONITOR_DRAW_ITEM_SCAN_BYTES,
    ) {
        return None;
    }
    for offset in (0..ADDITIVE_MONITOR_DRAW_ITEM_SCAN_BYTES).step_by(size_of::<usize>()) {
        let Some(candidate) = read_usize_field(draw_item, offset) else {
            continue;
        };
        if let Some(monitor) = monitor_if_plausible(candidate) {
            return Some(monitor);
        }
    }
    None
}

fn monitor_if_plausible(monitor: usize) -> Option<usize> {
    if monitor == 0
        || !memory_range_is_readable(
            monitor as *const c_void,
            MONITOR_RENDER_RESOURCE_B_OFFSET + size_of::<usize>(),
        )
    {
        return None;
    }
    let width = read_u32_field(monitor, MONITOR_WIDTH_OFFSET)?;
    let height = read_u32_field(monitor, MONITOR_HEIGHT_OFFSET)?;
    if read_u8_field(monitor, MONITOR_ACTIVE_OFFSET).is_none()
        || width == 0
        || width > 4096
        || height == 0
        || height > 4096
    {
        return None;
    }
    Some(monitor)
}

#[cfg(windows)]
fn additive_monitor_draw_item_layout(draw_item: usize) -> String {
    if draw_item == 0 {
        return "draw_item=0".to_string();
    }
    if !memory_range_is_readable(
        draw_item as *const c_void,
        ADDITIVE_MONITOR_DRAW_ITEM_LAYOUT_BYTES,
    ) {
        return format!(
            "draw_item_layout base={} unreadable",
            format_hex_or_zero(draw_item as u64)
        );
    }
    let mut words = Vec::new();
    let mut plausible_offsets = Vec::new();
    for offset in (0..ADDITIVE_MONITOR_DRAW_ITEM_LAYOUT_BYTES).step_by(size_of::<usize>()) {
        let value = read_usize_field(draw_item, offset).unwrap_or(0);
        if offset < 0x40 {
            words.push(format!(
                "+0x{offset:x}={}",
                format_hex_or_zero(value as u64)
            ));
        }
        if let Some(monitor) = monitor_if_plausible(value) {
            plausible_offsets.push(format!(
                "+0x{offset:x}->{}",
                format_hex_or_zero(monitor as u64)
            ));
        }
    }
    format!(
        "draw_item_layout base={} {} plausible_monitors={}",
        format_hex_or_zero(draw_item as u64),
        words.join(" "),
        if plausible_offsets.is_empty() {
            "none".to_string()
        } else {
            plausible_offsets.join(",")
        }
    )
}

#[cfg(not(windows))]
fn additive_monitor_draw_item_layout(draw_item: usize) -> String {
    let _ = draw_item;
    "draw_item_layout unavailable".to_string()
}

#[cfg(windows)]
fn collect_additive_monitor_texture_candidates(
    monitor: usize,
    draw_item: usize,
    texture_arg: usize,
    video_texture_object: usize,
) -> Vec<AdditiveMonitorTextureCandidate> {
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    if video_texture_object != 0 {
        collect_additive_monitor_texture_candidate(
            monitor,
            draw_item,
            video_texture_object,
            video_texture_object,
            None,
            ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
            "arg5_video_texture_object+0x48",
            &mut seen,
            &mut candidates,
        );
    }
    if texture_arg != 0 {
        collect_additive_monitor_texture_candidate(
            monitor,
            draw_item,
            texture_arg,
            texture_arg,
            None,
            ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET,
            "texture_video_arg+0x28",
            &mut seen,
            &mut candidates,
        );
        collect_additive_monitor_texture_candidate(
            monitor,
            draw_item,
            texture_arg,
            texture_arg,
            None,
            ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
            "texture_video_arg+0x48",
            &mut seen,
            &mut candidates,
        );
        if let Some(nested) =
            read_usize_field(texture_arg, ADDITIVE_MONITOR_TEXTURE_NESTED_POINTER_OFFSET)
                .filter(|value| *value != 0 && *value != texture_arg)
        {
            collect_additive_monitor_texture_candidate(
                monitor,
                draw_item,
                texture_arg,
                nested,
                Some(ADDITIVE_MONITOR_TEXTURE_NESTED_POINTER_OFFSET),
                ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET,
                "texture_video_arg+0x8->+0x28",
                &mut seen,
                &mut candidates,
            );
            collect_additive_monitor_texture_candidate(
                monitor,
                draw_item,
                texture_arg,
                nested,
                Some(ADDITIVE_MONITOR_TEXTURE_NESTED_POINTER_OFFSET),
                ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
                "texture_video_arg+0x8->+0x48",
                &mut seen,
                &mut candidates,
            );
        }
    }
    candidates
}

#[cfg(windows)]
#[allow(clippy::too_many_arguments)]
fn collect_additive_monitor_texture_candidate(
    monitor: usize,
    draw_item: usize,
    texture_arg: usize,
    texture_object: usize,
    pointer_offset: Option<usize>,
    handle_offset: usize,
    mapped_from: &'static str,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<AdditiveMonitorTextureCandidate>,
) {
    let Some(handle) = read_u32_field(texture_object, handle_offset)
        .filter(|value| plausible_gl_texture_handle(*value) && seen.insert(*value))
    else {
        return;
    };
    candidates.push(AdditiveMonitorTextureCandidate {
        handle,
        monitor,
        draw_item,
        texture_arg,
        texture_object,
        handle_offset,
        pointer_offset,
        mapped_from,
    });
}

fn upsert_additive_monitor_bound_unit_candidate(
    candidates: &mut Vec<AdditiveMonitorTextureCandidate>,
    candidate: AdditiveMonitorTextureCandidate,
) {
    if let Some(existing) = candidates
        .iter_mut()
        .find(|existing| existing.handle == candidate.handle)
    {
        *existing = candidate;
    } else {
        candidates.insert(0, candidate);
    }
}

#[cfg(windows)]
fn additive_monitor_texture_arg_layout(texture_arg: usize) -> String {
    if texture_arg == 0 {
        return "texture_video=0".to_string();
    }
    let direct_28 = read_u32_field(texture_arg, ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET)
        .map(|value| format!("0x{value:x}"))
        .unwrap_or_else(|| "unreadable".to_string());
    let direct_48 = read_u32_field(texture_arg, ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET)
        .map(|value| format!("0x{value:x}"))
        .unwrap_or_else(|| "unreadable".to_string());
    let nested = read_usize_field(texture_arg, ADDITIVE_MONITOR_TEXTURE_NESTED_POINTER_OFFSET);
    let nested_28 = nested
        .and_then(|value| read_u32_field(value, ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET))
        .map(|value| format!("0x{value:x}"))
        .unwrap_or_else(|| "unreadable".to_string());
    let nested_48 = nested
        .and_then(|value| read_u32_field(value, ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET))
        .map(|value| format!("0x{value:x}"))
        .unwrap_or_else(|| "unreadable".to_string());
    format!(
        "texture_video_layout arg={} +0x28={} +0x48={} nested+0x8={} nested+0x28={} nested+0x48={}",
        format_hex_or_zero(texture_arg as u64),
        direct_28,
        direct_48,
        nested
            .map(|value| format_hex_or_zero(value as u64))
            .unwrap_or_else(|| "none".to_string()),
        nested_28,
        nested_48
    )
}

#[cfg(windows)]
fn additive_monitor_texture_object_layout(texture_object: usize) -> String {
    if texture_object == 0 {
        return "video_texture_object=0".to_string();
    }
    let direct_28 = read_u32_field(
        texture_object,
        ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET,
    )
    .map(|value| format!("0x{value:x}"))
    .unwrap_or_else(|| "unreadable".to_string());
    let direct_48 = read_u32_field(
        texture_object,
        ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
    )
    .map(|value| format!("0x{value:x}"))
    .unwrap_or_else(|| "unreadable".to_string());
    format!(
        "video_texture_object_layout object={} +0x28={} +0x48={}",
        format_hex_or_zero(texture_object as u64),
        direct_28,
        direct_48
    )
}

#[cfg(windows)]
fn read_additive_monitor_texture_with_pbo(
    candidate: AdditiveMonitorTextureCandidate,
    input_slot_handle: u64,
) -> Result<SourceTextureReadback, String> {
    read_additive_monitor_texture_with_pbo_for_handles(
        candidate,
        normalize_monitor_input_handles(input_slot_handle, std::iter::empty()),
    )
}

#[cfg(windows)]
fn read_additive_monitor_texture_with_pbo_for_handles(
    candidate: AdditiveMonitorTextureCandidate,
    input_handles: Vec<u64>,
) -> Result<SourceTextureReadback, String> {
    let input_handles = normalize_monitor_input_handles(0, input_handles);
    let input_slot_handle = primary_monitor_input_handle(&input_handles);
    let monitor_candidate = MonitorRenderResourceCandidate {
        handle: candidate.handle,
        monitor: candidate.monitor,
        resource: candidate.texture_object,
        resource_offset: candidate.handle_offset,
        monitor_resource_offset: candidate.pointer_offset.unwrap_or(0),
        mapped_key: candidate.texture_arg as u64,
        mapped_from: candidate.mapped_from,
        mapped_width: 0,
        mapped_height: 0,
        binding_owner_ptr: 0,
        binding_texture_ptr: 0,
        binding_age_ms: 0,
    };
    read_monitor_render_texture_with_pbo_for_handles(monitor_candidate, input_handles).map(
        |readback| SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: candidate.handle,
                source_handle: input_slot_handle,
                source_offset: candidate.handle_offset,
                pointer_offset: candidate.pointer_offset,
            },
            width: readback.width,
            height: readback.height,
            rgb: readback.rgb,
        },
    )
}

#[cfg(windows)]
fn additive_monitor_bound_unit_candidate(
    monitor: usize,
    draw_item: usize,
    texture_arg: usize,
    texture_object: usize,
) -> Result<AdditiveMonitorTextureCandidate, String> {
    let handle = current_gl_texture_binding_2d_for_unit(3)?;
    if handle == 0 {
        return Err("bound_unit3_texture=0".to_string());
    }
    if !plausible_gl_texture_handle(handle) {
        return Err(format!("bound_unit3_texture=0x{handle:x} implausible"));
    }
    Ok(AdditiveMonitorTextureCandidate {
        handle,
        monitor,
        draw_item,
        texture_arg,
        texture_object,
        handle_offset: ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
        pointer_offset: None,
        mapped_from: "gl_bound_unit3_after_additive_bind",
    })
}

#[cfg(windows)]
#[allow(dead_code)]
fn additive_monitor_bind_probe_slots(
    input_slot_ref: u64,
    effective_input_handle: u64,
) -> Result<Vec<SlotKey>, String> {
    monitor_render_probe_slots(input_slot_ref, effective_input_handle)
}

#[cfg(windows)]
fn additive_monitor_bind_probe_slots_for_handles(
    input_handles: Vec<u64>,
) -> Result<Vec<SlotKey>, String> {
    monitor_render_probe_slots_for_handles(input_handles)
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct AdditiveMonitorStateUpdate {
    updated: bool,
    updated_slots: usize,
    skipped_fps: bool,
    skipped_fps_slots: usize,
    stats: PixelStats,
}

#[cfg(all(windows, test))]
fn apply_additive_monitor_readback_to_slots(
    keys: &[SlotKey],
    candidate: AdditiveMonitorTextureCandidate,
    readback: SourceTextureReadback,
) -> Result<AdditiveMonitorStateUpdate, String> {
    if !additive_monitor_candidate_can_update_lua(&candidate) {
        return Ok(AdditiveMonitorStateUpdate {
            updated: false,
            updated_slots: 0,
            skipped_fps: false,
            skipped_fps_slots: 0,
            stats: pixel_stats_from_rgb(&readback.rgb),
        });
    }
    let mut state = request_runtime_state()?;
    let capture_interval = capture_frame_interval(state.config.capture.max_fps);
    let now = Instant::now();
    let stats = pixel_stats_from_rgb(&readback.rgb);
    let mut updated_slots = 0usize;
    let mut skipped_fps_slots = 0usize;
    let mut updated_slot_sizes = Vec::new();
    for key in keys {
        let Some(slot) = state.slots.get_mut(key) else {
            continue;
        };
        if texture_upload_slot_is_rate_limited(slot, now, capture_interval) {
            skipped_fps_slots = skipped_fps_slots.saturating_add(1);
            continue;
        }
        let rgb = resize_rgb_nearest(
            &readback.rgb,
            readback.width,
            readback.height,
            slot.width,
            slot.height,
        )?;
        let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
        slot.frame_id = frame_id;
        slot.ready = true;
        slot.connected = true;
        slot.latest_frame = Some(FrameBuffer {
            frame_id,
            width: slot.width,
            height: slot.height,
            source: "monitor_render".to_string(),
            rgb,
        });
        slot.source_texture_handle = Some(u64::from(candidate.handle));
        slot.last_texture_upload_at = Some(now);
        updated_slot_sizes.push(format!("{}:{}x{}", slot.slot, slot.width, slot.height));
        updated_slots = updated_slots.saturating_add(1);
    }
    if updated_slots > 0 {
        state.hook_runtime.real_video_capture = true;
        state.hook_runtime.monitor_render_frames = state
            .hook_runtime
            .monitor_render_frames
            .saturating_add(updated_slots as u64);
        state.hook_runtime.additive_monitor_bind_frames = state
            .hook_runtime
            .additive_monitor_bind_frames
            .saturating_add(updated_slots as u64);
        log_runtime_diagnostic(
            &state,
            &format!(
                "additive_monitor_bind captured monitor={} draw_item={} texture_arg={} object={} handle=0x{:x} handle_offset=0x{:x} pointer_offset={} mapped_from={} native={}x{} updated_slots={} slot_sizes={} source_stats={} slots={}",
                format_hex_or_zero(candidate.monitor as u64),
                format_hex_or_zero(candidate.draw_item as u64),
                format_hex_or_zero(candidate.texture_arg as u64),
                format_hex_or_zero(candidate.texture_object as u64),
                candidate.handle,
                candidate.handle_offset,
                candidate.pointer_offset.map(|value| format!("0x{value:x}")).unwrap_or_else(|| "none".to_string()),
                candidate.mapped_from,
                readback.width,
                readback.height,
                updated_slots,
                updated_slot_sizes.join(","),
                format_pixel_stats(&stats),
                describe_slots(&state)
            ),
            &ADDITIVE_MONITOR_BIND_CAPTURE_DIAGNOSTIC_COUNT,
            64,
        );
    }
    if skipped_fps_slots > 0 {
        state.hook_runtime.monitor_render_skipped_fps_slots = state
            .hook_runtime
            .monitor_render_skipped_fps_slots
            .saturating_add(skipped_fps_slots as u64);
        state.hook_runtime.additive_monitor_bind_skipped_fps_slots = state
            .hook_runtime
            .additive_monitor_bind_skipped_fps_slots
            .saturating_add(skipped_fps_slots as u64);
    }
    set_runtime(state);
    Ok(AdditiveMonitorStateUpdate {
        updated: updated_slots > 0,
        updated_slots,
        skipped_fps: skipped_fps_slots > 0,
        skipped_fps_slots,
        stats,
    })
}

#[cfg(windows)]
fn record_additive_monitor_bind_probe_report(
    report: &AdditiveMonitorBindProbeReport,
) -> Result<(), String> {
    let mut state = request_runtime_state()?;
    state.hook_runtime.additive_monitor_bind_attempts = state
        .hook_runtime
        .additive_monitor_bind_attempts
        .saturating_add(1);
    state.hook_runtime.additive_monitor_bind_candidates = state
        .hook_runtime
        .additive_monitor_bind_candidates
        .saturating_add(report.candidates as u64);
    state.hook_runtime.additive_monitor_bind_blank_reads = state
        .hook_runtime
        .additive_monitor_bind_blank_reads
        .saturating_add(report.blank_reads as u64);
    state.hook_runtime.additive_monitor_bind_read_errors = state
        .hook_runtime
        .additive_monitor_bind_read_errors
        .saturating_add(report.read_errors as u64);
    if report.updated_slots == 0 {
        let counter = if report.selected_slots > 0 {
            &ADDITIVE_MONITOR_BIND_SLOT_PROBE_DIAGNOSTIC_COUNT
        } else {
            &ADDITIVE_MONITOR_BIND_PROBE_DIAGNOSTIC_COUNT
        };
        let prefix = if report.selected_slots > 0 {
            "additive monitor bind slot probe no_frame"
        } else {
            "additive monitor bind probe no_frame"
        };
        log_runtime_diagnostic(
            &state,
            &format!(
                "{prefix} material={} draw_item={} monitor={} size={}x{} input_slot_handle={} texture_video={} texture_overlay={} candidates={} selected_slots={} read_errors={} blank_reads={} skipped_fps_slots={} details={} slots={}",
                format_hex_or_zero(report.material as u64),
                format_hex_or_zero(report.draw_item as u64),
                format_hex_or_zero(report.monitor as u64),
                report.monitor_width,
                report.monitor_height,
                format_hex_or_zero(report.input_slot_handle),
                format_hex_or_zero(report.texture_video as u64),
                format_hex_or_zero(report.texture_overlay as u64),
                report.candidates,
                report.selected_slots,
                report.read_errors,
                report.blank_reads,
                report.skipped_fps_slots,
                if report.details.is_empty() {
                    "none".to_string()
                } else {
                    report.details.join(" | ")
                },
                describe_slots(&state)
            ),
            counter,
            16,
        );
    }
    set_runtime(state);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct MonitorRenderResourceCandidate {
    handle: u32,
    monitor: usize,
    resource: usize,
    resource_offset: usize,
    monitor_resource_offset: usize,
    mapped_key: u64,
    mapped_from: &'static str,
    mapped_width: u32,
    mapped_height: u32,
    binding_owner_ptr: u64,
    binding_texture_ptr: u64,
    binding_age_ms: u128,
}

#[derive(Debug, Clone)]
struct MonitorRenderProbeReport {
    monitor: usize,
    monitor_width: u32,
    monitor_height: u32,
    input_slot_handle: u64,
    candidates: usize,
    read_errors: usize,
    blank_reads: usize,
    updated_slots: usize,
    skipped_fps_slots: usize,
    details: Vec<String>,
}

fn probe_monitor_render_resources(monitor: usize) -> Result<usize, String> {
    #[cfg(windows)]
    {
        probe_monitor_render_resources_windows(monitor)
    }
    #[cfg(not(windows))]
    {
        let _ = monitor;
        Ok(0)
    }
}

#[cfg(windows)]
fn probe_monitor_render_resources_windows(monitor: usize) -> Result<usize, String> {
    if monitor == 0
        || !memory_range_is_readable(
            monitor as *const c_void,
            MONITOR_RENDER_RESOURCE_B_OFFSET + size_of::<usize>(),
        )
    {
        return Ok(0);
    }
    let active = read_u8_field(monitor, MONITOR_ACTIVE_OFFSET).unwrap_or(0) != 0;
    let monitor_inputs = monitor_video_input_handles(monitor);
    let effective_input_handle = monitor_inputs.effective();
    if !active || effective_input_handle == 0 {
        return Ok(0);
    }
    let width = read_u32_field(monitor, MONITOR_WIDTH_OFFSET).unwrap_or(0);
    let height = read_u32_field(monitor, MONITOR_HEIGHT_OFFSET).unwrap_or(0);
    let slots = monitor_render_probe_slots_for_handles(monitor_inputs.relation_handles())?;
    let mut report = MonitorRenderProbeReport {
        monitor,
        monitor_width: width,
        monitor_height: height,
        input_slot_handle: effective_input_handle,
        candidates: 0,
        read_errors: 0,
        blank_reads: 0,
        updated_slots: 0,
        skipped_fps_slots: 0,
        details: Vec::new(),
    };
    if slots.is_empty() {
        report.details.push(format!(
            "no_matching_lua_slots monitor_inputs=[{}] effective={} resource_scan=skipped_until_exact_slot_match",
            monitor_inputs.diagnostic(),
            format_hex_or_zero(effective_input_handle)
        ));
        record_monitor_render_probe_report(&report)?;
        return Ok(0);
    }
    if slot_keys_are_ready_for_lua(&slots) {
        return Ok(0);
    }
    if let Some(reason) = monitor_render_is_lua_output_for_slots(&slots, monitor_inputs) {
        report.details.push(format!(
            "rejected_lua_output_monitor reason={} monitor_inputs=[{}] resource_scan=skipped",
            reason,
            monitor_inputs.summary()
        ));
        record_monitor_render_probe_report(&report)?;
        return Ok(0);
    }
    let resources = [
        (
            MONITOR_RENDER_RESOURCE_A_OFFSET,
            read_usize_field(monitor, MONITOR_RENDER_RESOURCE_A_OFFSET),
        ),
        (
            MONITOR_RENDER_RESOURCE_B_OFFSET,
            read_usize_field(monitor, MONITOR_RENDER_RESOURCE_B_OFFSET),
        ),
    ];
    let texture_bindings = runtime_snapshot().gl_texture_bindings;
    let binding_count = texture_bindings.len();
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let mut resource_details = Vec::new();
    for (monitor_resource_offset, resource) in resources {
        let Some(resource) = resource else {
            resource_details.push(format!("resource@0x{monitor_resource_offset:x}=none"));
            continue;
        };
        collect_monitor_render_resource_candidates(
            monitor,
            resource,
            monitor_resource_offset,
            width,
            height,
            &texture_bindings,
            &mut seen,
            &mut candidates,
            &mut resource_details,
        );
    }
    append_cached_monitor_render_source_candidates(
        &slots,
        monitor,
        width,
        height,
        &mut seen,
        &mut candidates,
        &mut resource_details,
    );
    candidates
        .sort_by_key(|candidate| monitor_render_resource_candidate_rank(candidate, width, height));
    report.candidates = candidates.len();
    if candidates.is_empty() {
        report.details.push(format!(
            "mapped_candidates=0 known_bindings={} monitor_inputs=[{}] resources={}",
            binding_count,
            monitor_inputs.diagnostic(),
            if resource_details.is_empty() {
                "none".to_string()
            } else {
                resource_details.join(",")
            }
        ));
        record_monitor_render_probe_report(&report)?;
        return Ok(0);
    }
    enqueue_pending_monitor_render_probe(
        monitor,
        width,
        height,
        monitor_inputs.relation_handles(),
        resources[0].1.unwrap_or(0),
        resources[1].1.unwrap_or(0),
        "monitor_render_queue",
    )?;
    let mut readback_attempts = 0usize;
    for candidate in candidates {
        if let Some(reason) =
            monitor_render_candidate_readback_skip_reason(&candidate, width, height)
        {
            push_monitor_render_readback_skip_detail(&mut report, &candidate, reason);
            continue;
        }
        if readback_attempts >= MONITOR_RENDER_MAX_READBACK_CANDIDATES_PER_PROBE {
            push_monitor_render_readback_skip_detail(
                &mut report,
                &candidate,
                "readback_candidate_limit",
            );
            continue;
        }
        readback_attempts = readback_attempts.saturating_add(1);
        match read_monitor_render_texture_with_pbo_for_handles(
            candidate,
            monitor_inputs.relation_handles(),
        ) {
            Ok(readback) => {
                let stats = pixel_stats_from_rgb(&readback.rgb);
                if stats.nonzero_pixels == 0 {
                    report.blank_reads = report.blank_reads.saturating_add(1);
                    if report.details.len() < 12 {
                        report.details.push(format!(
                            "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} native={}x{} blank {}",
                            candidate.handle,
                            format_hex_or_zero(candidate.resource as u64),
                            candidate.monitor_resource_offset,
                            candidate.mapped_from,
                            format_hex_or_zero(candidate.mapped_key),
                            format_hex_or_zero(candidate.binding_owner_ptr),
                            format_hex_or_zero(candidate.binding_texture_ptr),
                            candidate.binding_age_ms,
                            readback.width,
                            readback.height,
                            format_pixel_stats(&stats)
                        ));
                    }
                    continue;
                }
                if !monitor_render_candidate_can_update_lua(&candidate, width, height) {
                    if report.details.len() < 12 {
                        report.details.push(format!(
                            "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} native={}x{} rejected_untrusted_candidate {}",
                            candidate.handle,
                            format_hex_or_zero(candidate.resource as u64),
                            candidate.monitor_resource_offset,
                            candidate.mapped_from,
                            format_hex_or_zero(candidate.mapped_key),
                            format_hex_or_zero(candidate.binding_owner_ptr),
                            format_hex_or_zero(candidate.binding_texture_ptr),
                            candidate.binding_age_ms,
                            readback.width,
                            readback.height,
                            format_pixel_stats(&stats)
                        ));
                    }
                    continue;
                }
                let update = apply_monitor_render_readback_to_slots(&slots, candidate, readback)?;
                if update.skipped_fps {
                    report.skipped_fps_slots = report
                        .skipped_fps_slots
                        .saturating_add(update.skipped_fps_slots);
                }
                if update.updated {
                    report.updated_slots =
                        report.updated_slots.saturating_add(update.updated_slots);
                    report.details.push(format!(
                        "captured candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} {}",
                        candidate.handle,
                        format_hex_or_zero(candidate.resource as u64),
                        candidate.monitor_resource_offset,
                        candidate.mapped_from,
                        format_hex_or_zero(candidate.mapped_key),
                        format_hex_or_zero(candidate.binding_owner_ptr),
                        format_hex_or_zero(candidate.binding_texture_ptr),
                        candidate.binding_age_ms,
                        format_pixel_stats(&update.stats)
                    ));
                    break;
                }
            }
            Err(error) => {
                report.read_errors = report.read_errors.saturating_add(1);
                if report.details.len() < 12 {
                    report.details.push(format!(
                        "candidate=0x{:x} resource={} monitor_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} read_error={}",
                        candidate.handle,
                        format_hex_or_zero(candidate.resource as u64),
                        candidate.monitor_resource_offset,
                        candidate.mapped_from,
                        format_hex_or_zero(candidate.mapped_key),
                        format_hex_or_zero(candidate.binding_owner_ptr),
                        format_hex_or_zero(candidate.binding_texture_ptr),
                        candidate.binding_age_ms,
                        error
                    ));
                }
            }
        }
    }
    record_monitor_render_probe_report(&report)?;
    Ok(report.updated_slots)
}

#[cfg(windows)]
fn collect_monitor_render_resource_candidates(
    monitor: usize,
    resource: usize,
    monitor_resource_offset: usize,
    monitor_width: u32,
    monitor_height: u32,
    texture_bindings: &BTreeMap<u64, GlTextureBinding>,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
    details: &mut Vec<String>,
) {
    if resource == 0 {
        details.push(format!("resource@0x{monitor_resource_offset:x}=0"));
        return;
    }
    let nested = read_usize_field(resource, 0x08).map(|value| value as u64);
    let before = candidates.len();
    collect_monitor_binding_candidate(
        monitor,
        resource,
        monitor_resource_offset,
        resource as u64,
        0,
        "resource_wrapper",
        texture_bindings,
        seen,
        candidates,
    );
    if let Some(nested_key) = nested.filter(|value| *value != 0) {
        collect_monitor_binding_candidate(
            monitor,
            resource,
            monitor_resource_offset,
            nested_key,
            0x08,
            "resource_nested_+0x8",
            texture_bindings,
            seen,
            candidates,
        );
    }
    collect_monitor_render_resource_scan_candidates(
        monitor,
        resource,
        monitor_resource_offset,
        monitor_width,
        monitor_height,
        texture_bindings,
        seen,
        candidates,
        details,
    );
    if let Some(nested_resource) = nested
        .filter(|value| *value != 0 && *value != resource as u64)
        .map(|value| value as usize)
    {
        collect_monitor_render_resource_scan_candidates(
            monitor,
            nested_resource,
            monitor_resource_offset,
            monitor_width,
            monitor_height,
            texture_bindings,
            seen,
            candidates,
            details,
        );
    }
    if candidates.len() == before {
        details.push(format!(
            "resource@0x{:x}={} nested+0x8={} no_binding",
            monitor_resource_offset,
            format_hex_or_zero(resource as u64),
            nested
                .map(format_hex_or_zero)
                .unwrap_or_else(|| "none".to_string())
        ));
    } else {
        details.push(format!(
            "resource@0x{:x}={} nested+0x8={} mapped={}",
            monitor_resource_offset,
            format_hex_or_zero(resource as u64),
            nested
                .map(format_hex_or_zero)
                .unwrap_or_else(|| "none".to_string()),
            candidates.len().saturating_sub(before)
        ));
    }
}

#[cfg(windows)]
fn collect_monitor_render_resource_scan_candidates(
    monitor: usize,
    resource: usize,
    monitor_resource_offset: usize,
    monitor_width: u32,
    monitor_height: u32,
    texture_bindings: &BTreeMap<u64, GlTextureBinding>,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
    details: &mut Vec<String>,
) {
    if !memory_range_is_readable(resource as *const c_void, MONITOR_RESOURCE_SCAN_BYTES) {
        details.push(format!(
            "resource@0x{:x}={} scan_unreadable",
            monitor_resource_offset,
            format_hex_or_zero(resource as u64)
        ));
        return;
    }
    for offset in (0..MONITOR_RESOURCE_SCAN_BYTES).step_by(size_of::<usize>()) {
        let Some(mapped_key) = read_usize_field(resource, offset).map(|value| value as u64) else {
            continue;
        };
        if mapped_key == 0 || mapped_key == resource as u64 {
            continue;
        }
        collect_monitor_binding_candidate(
            monitor,
            resource,
            monitor_resource_offset,
            mapped_key,
            offset,
            "resource_scan_ptr",
            texture_bindings,
            seen,
            candidates,
        );
    }
    collect_monitor_resource_raw_gl_handle_candidates(
        monitor,
        resource,
        monitor_resource_offset,
        monitor_width,
        monitor_height,
        seen,
        candidates,
        details,
    );
    let _ = monitor;
}

#[cfg(windows)]
fn collect_monitor_resource_raw_gl_handle_candidates(
    monitor: usize,
    resource: usize,
    monitor_resource_offset: usize,
    monitor_width: u32,
    monitor_height: u32,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
    details: &mut Vec<String>,
) {
    if monitor_width == 0 || monitor_height == 0 {
        details.push(format!(
            "resource@0x{:x}={} raw_gl_scan_skipped_zero_monitor_size",
            monitor_resource_offset,
            format_hex_or_zero(resource as u64)
        ));
        return;
    }
    if !memory_range_is_readable(resource as *const c_void, MONITOR_RESOURCE_SCAN_BYTES) {
        return;
    }
    let mut scanned = 0usize;
    let mut gl_true = 0usize;
    let mut size_matches = 0usize;
    let mut samples = Vec::new();
    for offset in (0..MONITOR_RESOURCE_SCAN_BYTES).step_by(size_of::<u32>()) {
        let Some(handle) = read_u32_field(resource, offset) else {
            continue;
        };
        if handle < 16 || seen.contains(&handle) {
            continue;
        }
        scanned = scanned.saturating_add(1);
        let Some((width, height)) = current_gl_texture_size_for_handle(handle) else {
            if samples.len() < 4 {
                samples.push(format!("0x{handle:x}@0x{offset:x}=not_texture"));
            }
            continue;
        };
        gl_true = gl_true.saturating_add(1);
        if samples.len() < 4 {
            samples.push(format!("0x{handle:x}@0x{offset:x}={}x{}", width, height));
        }
        let size_matches_monitor = width == monitor_width && height == monitor_height;
        if size_matches_monitor {
            size_matches = size_matches.saturating_add(1);
        } else {
            continue;
        }
        if !seen.insert(handle) {
            continue;
        }
        candidates.push(MonitorRenderResourceCandidate {
            handle,
            monitor,
            resource,
            resource_offset: offset,
            monitor_resource_offset,
            mapped_key: u64::from(handle),
            mapped_from: "resource_scan_raw_gl_u32_size_match",
            mapped_width: width,
            mapped_height: height,
            binding_owner_ptr: 0,
            binding_texture_ptr: 0,
            binding_age_ms: 0,
        });
    }
    if scanned > 0 || gl_true > 0 || size_matches > 0 {
        details.push(format!(
            "resource@0x{:x}={} raw_gl_scan scanned={} gl_true={} size_matches={} monitor_size={}x{} samples={}",
            monitor_resource_offset,
            format_hex_or_zero(resource as u64),
            scanned,
            gl_true,
            size_matches,
            monitor_width,
            monitor_height,
            if samples.is_empty() {
                "none".to_string()
            } else {
                samples.join(",")
            }
        ));
    }
}

#[cfg(windows)]
fn current_gl_texture_size_for_handle(handle: u32) -> Option<(u32, u32)> {
    if handle == 0 || unsafe { glIsTexture(handle) } == 0 {
        return None;
    }
    drain_gl_errors();
    let previous_texture = current_gl_texture_binding_2d().unwrap_or(0);
    drain_gl_errors();
    call_original_gl_bind_texture(GL_TEXTURE_2D, handle);
    if gl_error() != GL_NO_ERROR {
        restore_gl_texture_binding_2d(previous_texture);
        return None;
    }
    let mut width = 0i32;
    let mut height = 0i32;
    unsafe {
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_WIDTH, &mut width);
        glGetTexLevelParameteriv(GL_TEXTURE_2D, 0, GL_TEXTURE_HEIGHT, &mut height);
    }
    let level_error = gl_error();
    restore_gl_texture_binding_2d(previous_texture);
    if level_error != GL_NO_ERROR || width <= 0 || height <= 0 {
        return None;
    }
    Some((width as u32, height as u32))
}

#[cfg(windows)]
fn collect_monitor_binding_candidate(
    monitor: usize,
    resource: usize,
    monitor_resource_offset: usize,
    mapped_key: u64,
    resource_offset: usize,
    mapped_from: &'static str,
    texture_bindings: &BTreeMap<u64, GlTextureBinding>,
    seen: &mut BTreeSet<u32>,
    candidates: &mut Vec<MonitorRenderResourceCandidate>,
) {
    let Some(binding) = texture_bindings.get(&mapped_key) else {
        return;
    };
    if binding.handle == 0 || !seen.insert(binding.handle) {
        return;
    }
    candidates.push(MonitorRenderResourceCandidate {
        handle: binding.handle,
        monitor,
        resource,
        resource_offset,
        monitor_resource_offset,
        mapped_key,
        mapped_from,
        mapped_width: binding.width,
        mapped_height: binding.height,
        binding_owner_ptr: binding.owner_ptr,
        binding_texture_ptr: binding.texture_ptr,
        binding_age_ms: binding.last_seen.elapsed().as_millis(),
    });
}

#[cfg(windows)]
#[allow(dead_code)]
fn monitor_render_probe_slots(
    input_slot_ref: u64,
    effective_input_handle: u64,
) -> Result<Vec<SlotKey>, String> {
    monitor_render_probe_slots_for_handles(
        [input_slot_ref, effective_input_handle]
            .into_iter()
            .filter(|value| *value != 0)
            .collect(),
    )
}

#[cfg(windows)]
fn monitor_render_probe_slots_for_handles(input_handles: Vec<u64>) -> Result<Vec<SlotKey>, String> {
    let state = request_runtime_state()?;
    let primary_input = input_handles
        .iter()
        .copied()
        .find(|value| *value != 0)
        .unwrap_or(0);
    let effective_input = input_handles
        .iter()
        .copied()
        .find(|value| *value != primary_input && *value != 0)
        .unwrap_or(primary_input);
    let relation = monitor_input_slot_relation_with_candidates(&state, input_handles);
    let matched = state
        .slots
        .iter()
        .filter(|(key, slot)| {
            slot.connected
                && relation.matches.iter().any(|matched| {
                    matched.strength >= MonitorInputSlotMatchStrength::Exact
                        && matched.key.component == key.component
                        && matched.key.slot == key.slot
                })
        })
        .map(|(key, _)| key.clone())
        .collect::<Vec<_>>();
    if matched.is_empty() {
        log_monitor_input_relation_diagnostic(&state, primary_input, effective_input, &relation);
    }
    Ok(matched)
}

#[derive(Debug, Clone)]
struct MonitorRenderStateUpdate {
    updated: bool,
    updated_slots: usize,
    skipped_fps: bool,
    skipped_fps_slots: usize,
    stats: PixelStats,
}

#[cfg(windows)]
fn apply_monitor_render_readback_to_slots(
    keys: &[SlotKey],
    candidate: MonitorRenderResourceCandidate,
    readback: SourceTextureReadback,
) -> Result<MonitorRenderStateUpdate, String> {
    let stats = pixel_stats_from_rgb(&readback.rgb);
    if !monitor_render_readback_can_update_lua(&candidate, readback.width, readback.height) {
        return Ok(MonitorRenderStateUpdate {
            updated: false,
            updated_slots: 0,
            skipped_fps: false,
            skipped_fps_slots: 0,
            stats,
        });
    }
    let mut state = request_runtime_state()?;
    let capture_interval = capture_frame_interval(state.config.capture.max_fps);
    let now = Instant::now();
    let mut updated_slots = 0usize;
    let mut skipped_fps_slots = 0usize;
    let mut updated_slot_sizes = Vec::new();
    for key in keys {
        let Some(slot) = state.slots.get_mut(key) else {
            continue;
        };
        if texture_upload_slot_is_rate_limited(slot, now, capture_interval) {
            skipped_fps_slots = skipped_fps_slots.saturating_add(1);
            continue;
        }
        let rgb = resize_rgb_nearest(
            &readback.rgb,
            readback.width,
            readback.height,
            slot.width,
            slot.height,
        )?;
        let frame_id = FRAME_ID.fetch_add(1, Ordering::Relaxed);
        slot.frame_id = frame_id;
        slot.ready = true;
        slot.connected = true;
        slot.latest_frame = Some(FrameBuffer {
            frame_id,
            width: slot.width,
            height: slot.height,
            source: "monitor_render".to_string(),
            rgb,
        });
        slot.source_texture_handle = Some(u64::from(candidate.handle));
        slot.last_texture_upload_at = Some(now);
        updated_slot_sizes.push(format!("{}:{}x{}", slot.slot, slot.width, slot.height));
        updated_slots = updated_slots.saturating_add(1);
    }
    if updated_slots > 0 {
        state.hook_runtime.real_video_capture = true;
        state.hook_runtime.monitor_render_frames = state
            .hook_runtime
            .monitor_render_frames
            .saturating_add(updated_slots as u64);
        log_runtime_diagnostic(
            &state,
            &format!(
                "monitor_render captured monitor={} resource={} handle=0x{:x} resource_offset=0x{:x} monitor_resource_offset=0x{:x} mapped_from={} mapped_key={} binding_owner={} binding_texture={} binding_age_ms={} native={}x{} updated_slots={} slot_sizes={} source_stats={} slots={}",
                format_hex_or_zero(candidate.monitor as u64),
                format_hex_or_zero(candidate.resource as u64),
                candidate.handle,
                candidate.resource_offset,
                candidate.monitor_resource_offset,
                candidate.mapped_from,
                format_hex_or_zero(candidate.mapped_key),
                format_hex_or_zero(candidate.binding_owner_ptr),
                format_hex_or_zero(candidate.binding_texture_ptr),
                candidate.binding_age_ms,
                readback.width,
                readback.height,
                updated_slots,
                updated_slot_sizes.join(","),
                format_pixel_stats(&stats),
                describe_slots(&state)
            ),
            &MONITOR_RENDER_CAPTURE_DIAGNOSTIC_COUNT,
            64,
        );
    }
    if skipped_fps_slots > 0 {
        state.hook_runtime.monitor_render_skipped_fps_slots = state
            .hook_runtime
            .monitor_render_skipped_fps_slots
            .saturating_add(skipped_fps_slots as u64);
    }
    set_runtime(state);
    Ok(MonitorRenderStateUpdate {
        updated: updated_slots > 0,
        updated_slots,
        skipped_fps: skipped_fps_slots > 0,
        skipped_fps_slots,
        stats,
    })
}

#[cfg(windows)]
fn record_monitor_render_probe_report(report: &MonitorRenderProbeReport) -> Result<(), String> {
    let mut state = request_runtime_state()?;
    state.hook_runtime.monitor_render_attempts =
        state.hook_runtime.monitor_render_attempts.saturating_add(1);
    state.hook_runtime.monitor_render_candidates = state
        .hook_runtime
        .monitor_render_candidates
        .saturating_add(report.candidates as u64);
    state.hook_runtime.monitor_render_blank_reads = state
        .hook_runtime
        .monitor_render_blank_reads
        .saturating_add(report.blank_reads as u64);
    state.hook_runtime.monitor_render_read_errors = state
        .hook_runtime
        .monitor_render_read_errors
        .saturating_add(report.read_errors as u64);
    if report.updated_slots == 0 {
        log_runtime_diagnostic(
            &state,
            &format!(
                "monitor render probe no_frame monitor={} size={}x{} input_slot_handle={} candidates={} read_errors={} blank_reads={} skipped_fps_slots={} details={} slots={}",
                format_hex_or_zero(report.monitor as u64),
                report.monitor_width,
                report.monitor_height,
                format_hex_or_zero(report.input_slot_handle),
                report.candidates,
                report.read_errors,
                report.blank_reads,
                report.skipped_fps_slots,
                if report.details.is_empty() {
                    "none".to_string()
                } else {
                    report.details.join(" | ")
                },
                describe_slots(&state)
            ),
            &MONITOR_RENDER_PROBE_DIAGNOSTIC_COUNT,
            8,
        );
    }
    set_runtime(state);
    Ok(())
}

fn texture_upload_original_trampoline_status() -> serde_json::Value {
    serde_json::json!({
        "arg1": TEXTURE_UPLOAD_ORIGINAL_ARG1.load(Ordering::SeqCst) != 0
    })
}

fn monitor_render_original_trampoline_status() -> serde_json::Value {
    serde_json::json!({
        "arg6": MONITOR_RENDER_QUEUE_ORIGINAL_ARG6.load(Ordering::SeqCst) != 0,
        "render_target_texture_create_arg3": RENDER_TARGET_TEXTURE_CREATE_ORIGINAL_ARG3.load(Ordering::SeqCst) != 0
    })
}

fn component_context_original_trampoline_status() -> serde_json::Value {
    serde_json::json!({
        "arg1": COMPONENT_CONTEXT_ORIGINAL_ARG1.load(Ordering::SeqCst) != 0,
        "arg2": COMPONENT_CONTEXT_ORIGINAL_ARG2.load(Ordering::SeqCst) != 0,
        "arg3": COMPONENT_CONTEXT_ORIGINAL_ARG3.load(Ordering::SeqCst) != 0,
        "arg4": COMPONENT_CONTEXT_ORIGINAL_ARG4.load(Ordering::SeqCst) != 0
    })
}

fn lua_registration_original_trampoline_status() -> serde_json::Value {
    serde_json::json!({
        "direct": LUA_REGISTRATION_ORIGINAL_DIRECT.load(Ordering::SeqCst) != 0,
        "arg1": LUA_REGISTRATION_ORIGINAL_ARG1.load(Ordering::SeqCst) != 0,
        "arg2": LUA_REGISTRATION_ORIGINAL_ARG2.load(Ordering::SeqCst) != 0,
        "arg3": LUA_REGISTRATION_ORIGINAL_ARG3.load(Ordering::SeqCst) != 0,
        "arg4": LUA_REGISTRATION_ORIGINAL_ARG4.load(Ordering::SeqCst) != 0,
        "component_lua_init_arg1": COMPONENT_LUA_INIT_ORIGINAL_ARG1.load(Ordering::SeqCst) != 0
    })
}

fn game_lua_helper_status() -> serde_json::Value {
    serde_json::json!({
        "create_table": GAME_LUA_CREATE_TABLE.load(Ordering::SeqCst) != 0,
        "push_string": GAME_LUA_PUSH_STRING.load(Ordering::SeqCst) != 0,
        "rawseti": GAME_LUA_RAWSETI.load(Ordering::SeqCst) != 0,
        "register_table": GAME_LUA_REGISTER_TABLE.load(Ordering::SeqCst) != 0,
        "arg_slot": GAME_LUA_ARG_SLOT.load(Ordering::SeqCst) != 0
    })
}

fn validate_lua_api(api: &VideoGetLuaApiV1) -> Result<(), String> {
    if api.size < size_of::<VideoGetLuaApiV1>() as u32 {
        return Err(format!(
            "Lua API table too small: got {}, need {}",
            api.size,
            size_of::<VideoGetLuaApiV1>()
        ));
    }
    if api.lua_createtable.is_none()
        || api.lua_pushcclosure.is_none()
        || api.lua_setglobal.is_none()
        || api.lua_setfield.is_none()
        || api.lua_rawseti.is_none()
        || api.lua_pushnil.is_none()
        || api.lua_pushboolean.is_none()
        || api.lua_pushinteger.is_none()
        || api.lua_pushstring.is_none()
        || api.luaL_checkinteger.is_none()
        || api.luaL_checkstring.is_none()
    {
        return Err("Lua API table is missing required functions".to_string());
    }
    Ok(())
}

fn lua_c_functions() -> [(&'static str, VideoGetLuaCFunction); 10] {
    [
        ("init", video_lua_init),
        ("isConnected", video_lua_is_connected),
        ("isReady", video_lua_is_ready),
        ("getInfo", video_lua_get_info),
        ("getSize", video_lua_get_size),
        ("get", video_lua_get),
        ("getGray", video_lua_get_gray),
        ("getRGB", video_lua_get_rgb),
        ("getPackedGray", video_lua_get_packed_gray),
        ("getPackedRGB", video_lua_get_packed_rgb),
    ]
}

unsafe extern "C" fn video_lua_init(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        let width = lua_check_u32(lua_state, lua, 2, "width")?;
        let height = lua_check_u32(lua_state, lua, 3, "height")?;
        let mode = lua_check_string(lua_state, lua, 4, "mode")?;
        let native = init_slot(VideoInit {
            slot,
            width,
            height,
            mode,
            component: Some(component),
        });
        match native {
            Ok(_) => {
                lua_push_bool(lua_state, lua, true)?;
                lua_push_nil(lua_state, lua)?;
            }
            Err(error) => {
                lua_push_bool(lua_state, lua, false)?;
                lua_push_string(lua_state, lua, &error)?;
            }
        }
        Ok(2)
    })
}

unsafe extern "C" fn video_lua_is_connected(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        let value = require_slot_for_component(&component, slot)
            .map(|slot| slot.connected)
            .unwrap_or(false);
        lua_push_bool(lua_state, lua, value)?;
        Ok(1)
    })
}

unsafe extern "C" fn video_lua_is_ready(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        let value = require_slot_for_component(&component, slot)
            .map(|slot| is_slot_ready_for_lua(&slot))
            .unwrap_or(false);
        lua_push_bool(lua_state, lua, value)?;
        Ok(1)
    })
}

unsafe extern "C" fn video_lua_get_info(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        let info = frame_info_for_component_slot(&component, slot)?;
        lua_push_i64(
            lua_state,
            lua,
            info.get("frame_id")
                .and_then(|value| value.as_i64())
                .unwrap_or(0),
        )?;
        lua_push_i64(
            lua_state,
            lua,
            info.get("width")
                .and_then(|value| value.as_i64())
                .unwrap_or(0),
        )?;
        lua_push_i64(
            lua_state,
            lua,
            info.get("height")
                .and_then(|value| value.as_i64())
                .unwrap_or(0),
        )?;
        lua_push_string(
            lua_state,
            lua,
            info.get("mode")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        )?;
        Ok(4)
    })
}

unsafe extern "C" fn video_lua_get_size(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        let (width, height) = frame_size_for_component_slot(&component, slot)?;
        lua_push_i64(lua_state, lua, width as i64)?;
        lua_push_i64(lua_state, lua, height as i64)?;
        Ok(2)
    })
}

unsafe extern "C" fn video_lua_get(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        match frame_for_component_slot_auto(&component, slot) {
            Ok(matrix) => {
                push_json_lua_value(lua_state, lua, &matrix)?;
                Ok(1)
            }
            Err(error) => {
                lua_push_nil(lua_state, lua)?;
                lua_push_string(lua_state, lua, &error)?;
                Ok(2)
            }
        }
    })
}

unsafe extern "C" fn video_lua_get_gray(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        match frame_for_component_slot(&component, slot, "gray") {
            Ok(matrix) => {
                push_json_lua_value(lua_state, lua, &matrix)?;
                Ok(1)
            }
            Err(error) => {
                lua_push_nil(lua_state, lua)?;
                lua_push_string(lua_state, lua, &error)?;
                Ok(2)
            }
        }
    })
}

unsafe extern "C" fn video_lua_get_rgb(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        match frame_for_component_slot(&component, slot, "rgb") {
            Ok(matrix) => {
                push_json_lua_value(lua_state, lua, &matrix)?;
                Ok(1)
            }
            Err(error) => {
                lua_push_nil(lua_state, lua)?;
                lua_push_string(lua_state, lua, &error)?;
                Ok(2)
            }
        }
    })
}

unsafe extern "C" fn video_lua_get_packed_gray(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        push_packed_or_error(lua_state, lua, &component, slot, "gray")
    })
}

unsafe extern "C" fn video_lua_get_packed_rgb(lua_state: *mut c_void) -> i32 {
    lua_guard(lua_state, |lua_state, lua, component| {
        let slot = lua_check_u32(lua_state, lua, 1, "slot")?;
        push_packed_or_error(lua_state, lua, &component, slot, "rgb")
    })
}

unsafe extern "C" fn video_game_lua_init(lua_state: *mut c_void) -> i32 {
    game_lua_guard("init", lua_state, |lua_state, component| {
        let (slot, width, height, mode) = game_lua_init_args(lua_state)?;
        match init_slot(VideoInit {
            slot,
            width,
            height,
            mode,
            component: Some(component),
        }) {
            Ok(_) => {
                game_lua_push_bool(lua_state, true)?;
                game_lua_push_nil(lua_state)?;
            }
            Err(error) => {
                game_lua_push_bool(lua_state, false)?;
                game_lua_push_string(lua_state, &error)?;
            }
        }
        Ok(2)
    })
}

unsafe extern "C" fn video_game_lua_is_connected(lua_state: *mut c_void) -> i32 {
    game_lua_guard("isConnected", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        let value = require_slot_for_component(&component, slot)
            .map(|slot| slot.connected)
            .unwrap_or(false);
        game_lua_push_bool(lua_state, value)?;
        Ok(1)
    })
}

unsafe extern "C" fn video_game_lua_is_ready(lua_state: *mut c_void) -> i32 {
    game_lua_guard("isReady", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        let value = require_slot_for_component(&component, slot)
            .map(|slot| is_slot_ready_for_lua(&slot))
            .unwrap_or(false);
        game_lua_push_bool(lua_state, value)?;
        Ok(1)
    })
}

unsafe extern "C" fn video_game_lua_get_info(lua_state: *mut c_void) -> i32 {
    game_lua_guard("getInfo", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        let info = frame_info_for_component_slot(&component, slot)?;
        game_lua_push_number(
            lua_state,
            info.get("frame_id")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0),
        )?;
        game_lua_push_number(
            lua_state,
            info.get("width")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0),
        )?;
        game_lua_push_number(
            lua_state,
            info.get("height")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0),
        )?;
        game_lua_push_string(
            lua_state,
            info.get("mode")
                .and_then(|value| value.as_str())
                .unwrap_or(""),
        )?;
        Ok(4)
    })
}

unsafe extern "C" fn video_game_lua_get_size(lua_state: *mut c_void) -> i32 {
    game_lua_guard("getSize", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        let (width, height) = frame_size_for_component_slot(&component, slot)?;
        game_lua_push_number(lua_state, width as f64)?;
        game_lua_push_number(lua_state, height as f64)?;
        Ok(2)
    })
}

unsafe extern "C" fn video_game_lua_get(lua_state: *mut c_void) -> i32 {
    game_lua_guard("get", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        let mode = require_slot_for_component(&component, slot)?.mode;
        game_lua_push_matrix(lua_state, &component, slot, &mode)
    })
}

unsafe extern "C" fn video_game_lua_get_gray(lua_state: *mut c_void) -> i32 {
    game_lua_guard("getGray", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        game_lua_push_matrix(lua_state, &component, slot, "gray")
    })
}

unsafe extern "C" fn video_game_lua_get_rgb(lua_state: *mut c_void) -> i32 {
    game_lua_guard("getRGB", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        game_lua_push_matrix(lua_state, &component, slot, "rgb")
    })
}

unsafe extern "C" fn video_game_lua_get_packed_gray(lua_state: *mut c_void) -> i32 {
    game_lua_guard("getPackedGray", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        game_lua_push_packed(lua_state, &component, slot, "gray")
    })
}

unsafe extern "C" fn video_game_lua_get_packed_rgb(lua_state: *mut c_void) -> i32 {
    game_lua_guard("getPackedRGB", lua_state, |lua_state, component| {
        let slot = game_lua_slot_arg(lua_state)?;
        game_lua_push_packed(lua_state, &component, slot, "rgb")
    })
}

fn game_lua_guard<F>(callback: &'static str, lua_state: *mut c_void, action: F) -> i32
where
    F: FnOnce(*mut c_void, String) -> Result<i32, String>,
{
    let component = game_lua_component_from_state(lua_state);
    record_game_lua_callback(callback, lua_state, &component);
    match action(lua_state, component) {
        Ok(count) => count,
        Err(error) => {
            record_lua_adapter_error(error.clone());
            let _ = game_lua_push_nil(lua_state);
            let _ = game_lua_push_string(lua_state, &error);
            2
        }
    }
}

fn record_game_lua_callback(callback: &'static str, lua_state: *mut c_void, component: &str) {
    // Liveness: a component that is calling video.* right now is alive. This is the reliable
    // signal for slot lifecycle, because the game never notifies us when a vehicle/component
    // is despawned, so "is the context still registered" stays true forever for dead
    // components. We instead treat a component as dead when it stops making callbacks.
    mark_component_alive(component);
    if let Ok(mut state) = runtime_cell().lock() {
        state.hook_runtime.game_lua_callback_calls =
            state.hook_runtime.game_lua_callback_calls.saturating_add(1);
        state.hook_runtime.game_lua_last_callback = Some(callback.to_string());
        state.hook_runtime.game_lua_last_component = Some(component.to_string());
        if verbose_runtime_diagnostics_enabled() && state.hook_runtime.game_lua_callback_calls <= 32
        {
            if let Some(path) = &state.log_path {
                let _ = append_log(
                    path,
                    &format!(
                        "game lua callback name={} lua_state={} component={} slots={}",
                        callback,
                        format_hex_usize(lua_state as usize),
                        component,
                        describe_slots(&state)
                    ),
                );
            }
        }
    }
}

fn game_lua_component_from_state(lua_state: *mut c_void) -> String {
    if let Some(component) = current_lua_component_context() {
        return component;
    }
    if let Some(component) = game_lua_component_from_closure_upvalue(lua_state) {
        return component;
    }
    if let Some(component) = game_lua_registered_component_context(lua_state as usize) {
        return component;
    }
    format!("lua_state:{:x}", lua_state as usize)
}

fn game_lua_component_from_closure_upvalue(lua_state: *mut c_void) -> Option<String> {
    if lua_state.is_null() {
        return None;
    }
    let helpers = game_lua_helpers().ok()?;
    let arg_slot = helpers.arg_slot?;
    let slot = unsafe { arg_slot(lua_state as usize, GAME_LUA_FIRST_UPVALUE_INDEX) };
    if slot.is_null() || !memory_range_is_readable(slot.cast::<c_void>(), 0x10) {
        return None;
    }
    let tag = unsafe { *(slot.add(8) as *const u32) };
    let raw = unsafe { *(slot as *const usize) };
    let component_context = match tag & 0xf {
        2 => raw,
        7 => raw.checked_add(0x28)?,
        _ => return None,
    };
    if component_context == 0 {
        return None;
    }
    Some(format!("component_lua_context:{component_context:x}"))
}

fn game_lua_component_contexts_cell() -> &'static Mutex<BTreeMap<usize, usize>> {
    GAME_LUA_COMPONENT_CONTEXTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn component_liveness_cell() -> &'static Mutex<BTreeMap<String, Instant>> {
    GAME_LUA_COMPONENT_LAST_SEEN.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn remember_game_lua_component_context(lua_owner: usize, component_context: usize) {
    if lua_owner == 0 || component_context == 0 {
        return;
    }
    if let Ok(mut contexts) = game_lua_component_contexts_cell().lock() {
        contexts.insert(lua_owner, component_context);
    }
}

fn game_lua_registered_component_context(lua_owner: usize) -> Option<String> {
    if lua_owner == 0 {
        return None;
    }
    game_lua_component_contexts_cell()
        .lock()
        .ok()
        .and_then(|contexts| contexts.get(&lua_owner).copied())
        .map(|context| format!("component_lua_context:{context:x}"))
}

fn game_lua_push_packed(
    lua_state: *mut c_void,
    component: &str,
    slot: u32,
    mode: &str,
) -> Result<i32, String> {
    let frame = packed_frame_data_for_component_slot(component, slot, mode)?;
    let bytes = frame.bytes.clone();
    if let Ok(state) = request_runtime_state() {
        let stats = pixel_stats_from_bytes(&bytes, frame.stride as usize);
        log_runtime_diagnostic(
            &state,
            &format!(
                "game_lua packed return component={} slot={} mode={} frame_id={} source={} size={}x{} stride={} stats={}",
                component,
                slot,
                mode,
                frame.frame_id,
                frame.source,
                frame.width,
                frame.height,
                frame.stride,
                format_pixel_stats(&stats)
            ),
            &LUA_PACKED_DIAGNOSTIC_COUNT,
            32,
        );
    }
    game_lua_push_byte_array(lua_state, &bytes)?;
    Ok(1)
}

fn game_lua_push_matrix(
    lua_state: *mut c_void,
    component: &str,
    slot: u32,
    mode: &str,
) -> Result<i32, String> {
    let frame = packed_frame_data_for_component_slot(component, slot, mode)?;
    let helpers = game_lua_helpers()?;
    let width = frame.width as usize;
    let height = frame.height as usize;
    game_lua_push_table(lua_state, helpers)?;
    for y in 0..height {
        game_lua_push_table(lua_state, helpers)?;
        for x in 0..width {
            match mode {
                "gray" => {
                    let gray = frame.bytes[y * width + x];
                    game_lua_push_gray_pixel(lua_state, helpers, x + 1, y + 1, gray)?;
                }
                "rgb" => {
                    let offset = (y * width + x) * 3;
                    let rgb = [
                        frame.bytes[offset],
                        frame.bytes[offset + 1],
                        frame.bytes[offset + 2],
                    ];
                    game_lua_push_rgb_pixel(lua_state, helpers, x + 1, y + 1, rgb)?;
                }
                _ => return Err("invalid mode".to_string()),
            }
            game_lua_rawseti(lua_state, helpers, (x + 1) as i64);
        }
        game_lua_rawseti(lua_state, helpers, (y + 1) as i64);
    }
    Ok(1)
}

fn game_lua_push_gray_pixel(
    lua_state: *mut c_void,
    helpers: GameLuaHelpers,
    x: usize,
    y: usize,
    gray: u8,
) -> Result<(), String> {
    game_lua_push_table(lua_state, helpers)?;
    game_lua_push_number(lua_state, x as f64)?;
    game_lua_rawseti(lua_state, helpers, 1);
    game_lua_push_number(lua_state, y as f64)?;
    game_lua_rawseti(lua_state, helpers, 2);
    game_lua_push_number(lua_state, gray as f64)?;
    game_lua_rawseti(lua_state, helpers, 3);
    Ok(())
}

fn game_lua_push_rgb_pixel(
    lua_state: *mut c_void,
    helpers: GameLuaHelpers,
    x: usize,
    y: usize,
    rgb: [u8; 3],
) -> Result<(), String> {
    game_lua_push_table(lua_state, helpers)?;
    game_lua_push_number(lua_state, x as f64)?;
    game_lua_rawseti(lua_state, helpers, 1);
    game_lua_push_number(lua_state, y as f64)?;
    game_lua_rawseti(lua_state, helpers, 2);
    game_lua_push_table(lua_state, helpers)?;
    for (index, value) in rgb.iter().enumerate() {
        game_lua_push_number(lua_state, *value as f64)?;
        game_lua_rawseti(lua_state, helpers, index as i64 + 1);
    }
    game_lua_rawseti(lua_state, helpers, 3);
    Ok(())
}

fn game_lua_arg_count(lua_state: *mut c_void) -> Result<i32, String> {
    if lua_state.is_null() || !memory_range_is_readable(lua_state, 0x28) {
        return Err("Stormworks Lua state is not readable".to_string());
    }
    let stack_top = unsafe { *(lua_state.cast::<usize>().add(2)) };
    let call_base_slot = unsafe { *(lua_state.cast::<usize>().add(4)) };
    if stack_top == 0 || call_base_slot == 0 {
        return Err("Stormworks Lua stack pointers are null".to_string());
    }
    if !memory_range_is_readable(stack_top as *const c_void, 0x10)
        || !memory_range_is_readable(call_base_slot as *const c_void, 0x8)
    {
        return Err("Stormworks Lua stack pointers are not readable".to_string());
    }
    let call_base = unsafe { *(call_base_slot as *const usize) };
    if call_base == 0 || !memory_range_is_readable(call_base as *const c_void, 0x10) {
        return Err("Stormworks Lua call base is not readable".to_string());
    }
    if stack_top < call_base + 0x10 {
        return Err("Stormworks Lua stack range is invalid".to_string());
    }
    Ok(((stack_top as isize - call_base as isize - 0x10) / 0x10) as i32)
}

fn game_lua_stack_top(lua_state: *mut c_void) -> Result<*mut u8, String> {
    if lua_state.is_null() {
        return Err("missing Stormworks Lua state".to_string());
    }
    let stack_top = unsafe { *(lua_state.cast::<*mut u8>().add(2)) };
    if stack_top.is_null() {
        return Err("Stormworks Lua stack top is null".to_string());
    }
    Ok(stack_top)
}

fn game_lua_arg_slot(lua_state: *mut c_void, index: i32) -> Result<*mut u8, String> {
    let arg_count = game_lua_arg_count(lua_state)?;
    if index < 1 || index > arg_count {
        return Err(format!("missing argument {index}"));
    }

    let arg_slot_helper = GAME_LUA_ARG_SLOT.load(Ordering::SeqCst);
    if arg_slot_helper != 0 {
        let arg_slot: GameLuaArgSlotFn = unsafe { std::mem::transmute(arg_slot_helper) };
        let slot = unsafe { arg_slot(lua_state as usize, index) };
        if !slot.is_null() && memory_range_is_readable(slot.cast::<c_void>(), 0x10) {
            return Ok(slot);
        }
    }

    let call_base_slot = unsafe { *(lua_state.cast::<usize>().add(4)) };
    let call_base = unsafe { *(call_base_slot as *const usize) };
    let slot = (call_base + index as usize * 0x10) as *mut u8;
    if !memory_range_is_readable(slot.cast::<c_void>(), 0x10) {
        return Err(format!("argument {index} is not readable"));
    }
    Ok(slot)
}

fn game_lua_arg_tag(lua_state: *mut c_void, index: i32) -> Result<u32, String> {
    let slot = game_lua_arg_slot(lua_state, index)?;
    Ok(unsafe { *(slot.add(8) as *const u32) })
}

fn game_lua_check_u32(lua_state: *mut c_void, index: i32, name: &str) -> Result<u32, String> {
    let slot = game_lua_arg_slot(lua_state, index)?;
    let tag = game_lua_arg_tag(lua_state, index)?;
    let value = match tag {
        3 => unsafe { *(slot as *const f64) },
        0x13 => unsafe { *(slot as *const i64) as f64 },
        _ => return Err(format!("{name} must be a number")),
    };
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 {
        return Err(format!("{name} is out of range"));
    }
    Ok(value as u32)
}

fn game_lua_check_string(lua_state: *mut c_void, index: i32) -> Option<String> {
    let slot = game_lua_arg_slot(lua_state, index).ok()?;
    let tag = game_lua_arg_tag(lua_state, index).ok()?;
    if tag & 0xf != 4 && tag & 0xf != 5 {
        return None;
    }
    let value = unsafe { *(slot as *const usize) };
    if value == 0 {
        return None;
    }
    let ptr = (value + 0x18) as *const c_char;
    unsafe_cstr(ptr)
}

fn game_lua_slot_arg(lua_state: *mut c_void) -> Result<u32, String> {
    if game_lua_arg_count(lua_state)? == 0 {
        Ok(DEFAULT_VIDEO_SLOT)
    } else {
        game_lua_check_u32(lua_state, 1, "slot")
    }
}

fn game_lua_arg_is_string(lua_state: *mut c_void, index: i32) -> bool {
    game_lua_arg_tag(lua_state, index)
        .map(|tag| tag & 0xf == 4 || tag & 0xf == 5)
        .unwrap_or(false)
}

fn game_lua_init_args(lua_state: *mut c_void) -> Result<(u32, u32, u32, String), String> {
    match game_lua_arg_count(lua_state)? {
        0 | 1 => Err("video.init requires width and height".to_string()),
        2 => Ok((
            DEFAULT_VIDEO_SLOT,
            game_lua_check_u32(lua_state, 1, "width")?,
            game_lua_check_u32(lua_state, 2, "height")?,
            "rgb".to_string(),
        )),
        3 if game_lua_arg_is_string(lua_state, 3) => Ok((
            DEFAULT_VIDEO_SLOT,
            game_lua_check_u32(lua_state, 1, "width")?,
            game_lua_check_u32(lua_state, 2, "height")?,
            game_lua_check_string(lua_state, 3).unwrap_or_else(|| "rgb".to_string()),
        )),
        3 => Ok((
            game_lua_check_u32(lua_state, 1, "slot")?,
            game_lua_check_u32(lua_state, 2, "width")?,
            game_lua_check_u32(lua_state, 3, "height")?,
            "rgb".to_string(),
        )),
        _ => Ok((
            game_lua_check_u32(lua_state, 1, "slot")?,
            game_lua_check_u32(lua_state, 2, "width")?,
            game_lua_check_u32(lua_state, 3, "height")?,
            game_lua_check_string(lua_state, 4).unwrap_or_else(|| "rgb".to_string()),
        )),
    }
}

fn game_lua_push_nil(lua_state: *mut c_void) -> Result<(), String> {
    let top = game_lua_stack_top(lua_state)?;
    unsafe {
        *(top.add(8) as *mut u32) = 0;
        *(lua_state.cast::<usize>().add(2)) = top as usize + 0x10;
    }
    Ok(())
}

fn game_lua_push_bool(lua_state: *mut c_void, value: bool) -> Result<(), String> {
    let top = game_lua_stack_top(lua_state)?;
    unsafe {
        *(top as *mut u32) = if value { 1 } else { 0 };
        *(top.add(8) as *mut u32) = 1;
        *(lua_state.cast::<usize>().add(2)) = top as usize + 0x10;
    }
    Ok(())
}

fn game_lua_push_number(lua_state: *mut c_void, value: f64) -> Result<(), String> {
    let top = game_lua_stack_top(lua_state)?;
    unsafe {
        *(top as *mut f64) = value;
        *(top.add(8) as *mut u32) = 3;
        *(lua_state.cast::<usize>().add(2)) = top as usize + 0x10;
    }
    Ok(())
}

fn game_lua_push_string(lua_state: *mut c_void, value: &str) -> Result<(), String> {
    let helpers = game_lua_helpers()?;
    let value = CString::new(value).map_err(|error| format!("Lua string contains nul: {error}"))?;
    unsafe {
        (helpers.push_string)(lua_state as usize, value.as_ptr());
    }
    Ok(())
}

fn game_lua_push_byte_array(lua_state: *mut c_void, bytes: &[u8]) -> Result<(), String> {
    let helpers = game_lua_helpers()?;
    game_lua_push_table(lua_state, helpers)?;
    for (index, byte) in bytes.iter().enumerate() {
        game_lua_push_number(lua_state, *byte as f64)?;
        game_lua_rawseti(lua_state, helpers, index as i64 + 1);
    }
    Ok(())
}

fn game_lua_rawseti(lua_state: *mut c_void, helpers: GameLuaHelpers, index: i64) {
    unsafe {
        (helpers.rawseti)(lua_state as usize, -2, index);
    }
}

fn game_lua_push_table(lua_state: *mut c_void, helpers: GameLuaHelpers) -> Result<(), String> {
    let table = unsafe { (helpers.create_table)(lua_state as usize) };
    if table == 0 {
        return Err("game_lua.create_table returned null".to_string());
    }
    let top = game_lua_stack_top(lua_state)?;
    unsafe {
        *(top as *mut usize) = table;
        *(top.add(8) as *mut u32) = 0x45;
        *((lua_state as *mut u8).add(0x10) as *mut usize) = top as usize + 0x10;
    }
    Ok(())
}

fn push_packed_or_error(
    lua_state: *mut c_void,
    lua: &VideoGetLuaApiV1,
    component: &str,
    slot: u32,
    mode: &str,
) -> Result<i32, String> {
    match packed_frame_for_component_slot(component, slot, mode) {
        Ok(buffer) => {
            push_json_lua_value(lua_state, lua, &buffer)?;
            Ok(1)
        }
        Err(error) => {
            lua_push_nil(lua_state, lua)?;
            lua_push_string(lua_state, lua, &error)?;
            Ok(2)
        }
    }
}

fn lua_guard<F>(lua_state: *mut c_void, action: F) -> i32
where
    F: FnOnce(*mut c_void, &VideoGetLuaApiV1, String) -> Result<i32, String>,
{
    let component = lua_component_from_state(lua_state);
    let result = lua_adapter_api().and_then(|api| action(lua_state, &api, component));
    match result {
        Ok(count) => count,
        Err(error) => {
            record_lua_adapter_error(error.clone());
            if let Ok(api) = lua_adapter_api() {
                let _ = lua_push_nil(lua_state, &api);
                let _ = lua_push_string(lua_state, &api, &error);
                2
            } else {
                0
            }
        }
    }
}

fn lua_adapter_cell() -> &'static Mutex<LuaAdapterState> {
    LUA_ADAPTER.get_or_init(|| {
        Mutex::new(LuaAdapterState {
            api: None,
            hook_api: None,
            registrations: 0,
            hook_registrations: 0,
            hook_original_calls: 0,
            last_error: None,
        })
    })
}

fn lua_adapter_api() -> Result<VideoGetLuaApiV1, String> {
    lua_adapter_cell()
        .lock()
        .map_err(|_| "lua adapter mutex poisoned".to_string())?
        .api
        .ok_or_else(|| "lua api not registered".to_string())
}

fn record_lua_adapter_error(error: String) {
    if let Ok(mut adapter) = lua_adapter_cell().lock() {
        adapter.last_error = Some(error.clone());
    }
    set_last_error(error);
}

fn lua_adapter_status_value() -> serde_json::Value {
    let context_depth = lua_component_context_depth();
    let current_context = current_lua_component_context();
    match lua_adapter_cell().lock() {
        Ok(adapter) => serde_json::json!({
            "registered": adapter.api.is_some(),
            "hook_api_configured": adapter.hook_api.is_some(),
            "registrations": adapter.registrations,
            "hook_registrations": adapter.hook_registrations,
            "hook_original_calls": adapter.hook_original_calls,
            "hook_original_trampolines": lua_registration_original_trampoline_status(),
            "game_lua_helpers": game_lua_helper_status(),
            "component_context_original_trampolines": component_context_original_trampoline_status(),
            "component_context_depth": context_depth,
            "current_component_context": current_context,
            "last_error": adapter.last_error
        }),
        Err(_) => serde_json::json!({
            "registered": false,
            "hook_api_configured": false,
            "registrations": 0,
            "hook_registrations": 0,
            "hook_original_calls": 0,
            "hook_original_trampolines": lua_registration_original_trampoline_status(),
            "game_lua_helpers": game_lua_helper_status(),
            "component_context_original_trampolines": component_context_original_trampoline_status(),
            "component_context_depth": context_depth,
            "current_component_context": current_context,
            "last_error": "lua adapter mutex poisoned"
        }),
    }
}

fn lua_component_from_state(lua_state: *mut c_void) -> String {
    if let Some(component) = current_lua_component_context() {
        return component;
    }
    let Ok(api) = lua_adapter_api() else {
        return DEFAULT_COMPONENT.to_string();
    };
    let Some(component_id) = api.component_id else {
        return format!("lua_state:{:x}", lua_state as usize);
    };
    let mut buffer = vec![0u8; 256];
    let written =
        unsafe { component_id(lua_state, buffer.as_mut_ptr() as *mut c_char, buffer.len()) };
    if written == 0 {
        return format!("lua_state:{:x}", lua_state as usize);
    }
    let len = written.min(buffer.len());
    let end = buffer[..len]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(len);
    let text = String::from_utf8_lossy(&buffer[..end]).trim().to_string();
    if text.is_empty() {
        format!("lua_state:{:x}", lua_state as usize)
    } else {
        text
    }
}

fn current_lua_component_context() -> Option<String> {
    LUA_COMPONENT_CONTEXT.with(|stack| stack.borrow().last().cloned())
}

fn lua_component_context_depth() -> usize {
    LUA_COMPONENT_CONTEXT.with(|stack| stack.borrow().len())
}

fn write_c_string(text: &str, out: *mut c_char, out_len: usize) -> Result<usize, String> {
    if out.is_null() || out_len == 0 {
        return Err("missing output buffer".to_string());
    }
    let bytes = text.as_bytes();
    let count = bytes.len().min(out_len.saturating_sub(1));
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), out as *mut u8, count);
        *out.add(count) = 0;
    }
    Ok(count)
}

fn lua_check_u32(
    lua_state: *mut c_void,
    lua: &VideoGetLuaApiV1,
    index: i32,
    name: &str,
) -> Result<u32, String> {
    let value = unsafe { lua.luaL_checkinteger.unwrap()(lua_state, index) };
    if value < 0 || value > u32::MAX as i64 {
        return Err(format!("{name} is out of range"));
    }
    Ok(value as u32)
}

fn lua_check_string(
    lua_state: *mut c_void,
    lua: &VideoGetLuaApiV1,
    index: i32,
    name: &str,
) -> Result<String, String> {
    let ptr = unsafe { lua.luaL_checkstring.unwrap()(lua_state, index) };
    unsafe_cstr(ptr).ok_or_else(|| format!("{name} must be a string"))
}

fn lua_push_nil(lua_state: *mut c_void, lua: &VideoGetLuaApiV1) -> Result<(), String> {
    unsafe { lua.lua_pushnil.unwrap()(lua_state) };
    Ok(())
}

fn lua_push_bool(
    lua_state: *mut c_void,
    lua: &VideoGetLuaApiV1,
    value: bool,
) -> Result<(), String> {
    unsafe { lua.lua_pushboolean.unwrap()(lua_state, if value { 1 } else { 0 }) };
    Ok(())
}

fn lua_push_i64(lua_state: *mut c_void, lua: &VideoGetLuaApiV1, value: i64) -> Result<(), String> {
    unsafe { lua.lua_pushinteger.unwrap()(lua_state, value) };
    Ok(())
}

fn lua_push_string(
    lua_state: *mut c_void,
    lua: &VideoGetLuaApiV1,
    value: &str,
) -> Result<(), String> {
    let value = CString::new(value).map_err(|error| format!("Lua string contains nul: {error}"))?;
    unsafe { lua.lua_pushstring.unwrap()(lua_state, value.as_ptr()) };
    Ok(())
}

fn push_json_lua_value(
    lua_state: *mut c_void,
    lua: &VideoGetLuaApiV1,
    value: &serde_json::Value,
) -> Result<(), String> {
    match value {
        serde_json::Value::Null => lua_push_nil(lua_state, lua),
        serde_json::Value::Bool(value) => lua_push_bool(lua_state, lua, *value),
        serde_json::Value::Number(value) => {
            lua_push_i64(lua_state, lua, value.as_i64().unwrap_or_default())
        }
        serde_json::Value::String(value) => lua_push_string(lua_state, lua, value),
        serde_json::Value::Array(items) => {
            unsafe { lua.lua_createtable.unwrap()(lua_state, items.len() as i32, 0) };
            for (index, item) in items.iter().enumerate() {
                push_json_lua_value(lua_state, lua, item)?;
                unsafe { lua.lua_rawseti.unwrap()(lua_state, -2, index as i64 + 1) };
            }
            Ok(())
        }
        serde_json::Value::Object(object) => {
            unsafe { lua.lua_createtable.unwrap()(lua_state, 0, object.len() as i32) };
            for (key, item) in object {
                let key = CString::new(key.as_str())
                    .map_err(|error| format!("Lua table key contains nul: {error}"))?;
                push_json_lua_value(lua_state, lua, item)?;
                unsafe { lua.lua_setfield.unwrap()(lua_state, -2, key.as_ptr()) };
            }
            Ok(())
        }
    }
}

fn missing_required_hook_stages(plan: &HookPlan, symbols: &serde_json::Value) -> Vec<String> {
    [
        "lua_api_registration",
        "current_lua_component_context",
        "microprocessor_input_video_node",
        "video_texture_source",
    ]
    .into_iter()
    .chain(hook_plan_uses_texture_upload(plan).then_some("video_texture_upload"))
    .chain(hook_plan_uses_monitor_render(plan).then_some("monitor_render_queue"))
    .chain(hook_plan_uses_additive_monitor_bind(plan).then_some("additive_monitor_bind"))
    .filter(|stage| {
        hook_plan_stage_is_used(plan, stage)
            || matches!(
                *stage,
                "lua_api_registration"
                    | "current_lua_component_context"
                    | "microprocessor_input_video_node"
                    | "video_texture_source"
            )
    })
    .filter(|stage| {
        symbols
            .get(stage)
            .and_then(|group| group.get("value"))
            .and_then(|value| value.as_array())
            .map(|items| items.is_empty())
            .unwrap_or(true)
    })
    .map(str::to_string)
    .collect()
}

fn hook_plan_stage_is_used(plan: &HookPlan, stage: &str) -> bool {
    plan.hooks
        .iter()
        .filter(|hook| hook.enabled)
        .any(|hook| hook.stage == stage)
}

fn detour_cell() -> &'static Mutex<DetourRegistry> {
    DETOURS.get_or_init(|| {
        Mutex::new(DetourRegistry {
            installed: Vec::new(),
            last_error: None,
        })
    })
}

fn detour_engine_available() -> bool {
    cfg!(windows)
}

fn detour_installed_count() -> usize {
    detour_cell()
        .lock()
        .map(|registry| registry.installed.len())
        .unwrap_or(0)
}

fn detour_trampoline_count(registry: &DetourRegistry) -> usize {
    registry
        .installed
        .iter()
        .filter(|detour| detour.trampoline.is_some())
        .count()
}

fn detour_trampoline_bytes_total(registry: &DetourRegistry) -> usize {
    registry
        .installed
        .iter()
        .filter_map(|detour| detour.trampoline.as_ref())
        .map(|trampoline| trampoline.len())
        .sum()
}

fn detour_status_value() -> serde_json::Value {
    match detour_cell().lock() {
        Ok(registry) => {
            let status = DetourStatus {
                engine_ready: detour_engine_available(),
                installed_count: registry.installed.len(),
                installed_labels: registry
                    .installed
                    .iter()
                    .map(|detour| detour.label.clone())
                    .collect(),
                trampoline_count: detour_trampoline_count(&registry),
                trampoline_bytes_total: detour_trampoline_bytes_total(&registry),
                last_error: registry.last_error.clone(),
            };
            serde_json::to_value(status).unwrap_or_else(|_| serde_json::json!({}))
        }
        Err(_) => serde_json::json!({
            "engine_ready": detour_engine_available(),
            "installed_count": 0,
            "installed_labels": [],
            "trampoline_count": 0,
            "trampoline_bytes_total": 0,
            "last_error": "detour registry mutex poisoned"
        }),
    }
}

fn run_detour_self_test() -> Result<serde_json::Value, String> {
    #[cfg(windows)]
    {
        let label = format!(
            "detour_self_test_{}",
            FRAME_ID.fetch_add(1, Ordering::Relaxed)
        );
        DETOUR_SELF_TEST_TRAMPOLINE.store(0, Ordering::SeqCst);
        let before = detour_self_test_target();
        let trampoline = install_absolute_jump_detour_with_trampoline(
            &label,
            detour_self_test_target as *mut c_void,
            detour_self_test_replacement as *const c_void,
        )?;
        DETOUR_SELF_TEST_TRAMPOLINE.store(trampoline as usize, Ordering::SeqCst);
        let during = detour_self_test_target();
        let trampoline_direct = detour_call_self_test_trampoline()?;
        uninstall_absolute_jump_detour(&label)?;
        DETOUR_SELF_TEST_TRAMPOLINE.store(0, Ordering::SeqCst);
        let after = detour_self_test_target();
        if before != 7 || during != 49 || trampoline_direct != 7 || after != 7 {
            return Err(format!(
                "unexpected detour self-test values before={before} during={during} trampoline_direct={trampoline_direct} after={after}"
            ));
        }
        Ok(serde_json::json!({
            "engine_ready": true,
            "installed": true,
            "uninstalled": true,
            "before": before,
            "during": during,
            "trampoline_direct": trampoline_direct,
            "after": after,
            "patch_len": absolute_jump_patch_len(),
            "trampoline_len": trampoline_len(),
            "patch_kind": "mov_rax_imm64_jmp_rax",
            "trampoline_jump_back_kind": "jmp_qword_ptr_rip_relative"
        }))
    }
    #[cfg(not(windows))]
    {
        Err("detour engine is only available on Windows".to_string())
    }
}

#[cfg(windows)]
#[inline(never)]
extern "C" fn detour_self_test_target() -> i32 {
    7
}

#[cfg(windows)]
#[inline(never)]
extern "C" fn detour_self_test_replacement() -> i32 {
    42 + detour_call_self_test_trampoline().unwrap_or(-1000)
}

#[cfg(windows)]
fn detour_call_self_test_trampoline() -> Result<i32, String> {
    let trampoline = DETOUR_SELF_TEST_TRAMPOLINE.load(Ordering::SeqCst);
    if trampoline == 0 {
        return Err("detour self-test trampoline is not set".to_string());
    }
    let original: extern "C" fn() -> i32 = unsafe { std::mem::transmute(trampoline) };
    Ok(original())
}

#[allow(dead_code)]
fn install_absolute_jump_detour(
    label: &str,
    target: *mut c_void,
    replacement: *const c_void,
) -> Result<(), String> {
    install_absolute_jump_detour_len(label, target, replacement, absolute_jump_patch_len())
}

fn install_absolute_jump_detour_len(
    label: &str,
    target: *mut c_void,
    replacement: *const c_void,
    patch_len: usize,
) -> Result<(), String> {
    install_absolute_jump_detour_inner(label, target, replacement, false, patch_len).map(|_| ())
}

fn install_absolute_jump_detour_with_trampoline(
    label: &str,
    target: *mut c_void,
    replacement: *const c_void,
) -> Result<*const c_void, String> {
    install_absolute_jump_detour_with_trampoline_len(
        label,
        target,
        replacement,
        absolute_jump_patch_len(),
    )
}

fn install_absolute_jump_detour_with_trampoline_len(
    label: &str,
    target: *mut c_void,
    replacement: *const c_void,
    patch_len: usize,
) -> Result<*const c_void, String> {
    match install_absolute_jump_detour_inner(label, target, replacement, true, patch_len)? {
        Some(trampoline) => Ok(trampoline),
        None => Err("trampoline allocation was not requested".to_string()),
    }
}

fn install_absolute_jump_detour_inner(
    label: &str,
    target: *mut c_void,
    replacement: *const c_void,
    create_trampoline: bool,
    patch_len: usize,
) -> Result<Option<*const c_void>, String> {
    #[cfg(windows)]
    unsafe {
        if target.is_null() || replacement.is_null() {
            return Err("detour target/replacement must not be null".to_string());
        }
        if patch_len < absolute_jump_patch_len() {
            return Err(format!(
                "detour patch_len {patch_len} is shorter than absolute jump patch {}",
                absolute_jump_patch_len()
            ));
        }
        let target = target as *mut u8;
        let replacement = replacement as u64;
        let mut registry = detour_cell()
            .lock()
            .map_err(|_| "detour registry mutex poisoned".to_string())?;
        if registry
            .installed
            .iter()
            .any(|detour| detour.target == target)
        {
            return Err(format!("detour target already installed: {label}"));
        }
        let original = std::slice::from_raw_parts(target, patch_len).to_vec();
        let trampoline = if create_trampoline {
            Some(allocate_trampoline(
                &original,
                target.add(patch_len) as u64,
            )?)
        } else {
            None
        };
        let trampoline_ptr = trampoline
            .as_ref()
            .map(|trampoline| trampoline.as_ptr() as *const c_void);
        let patch = absolute_jump_patch_bytes(replacement, patch_len);
        if let Err(error) = write_executable_memory(target, &patch) {
            registry.last_error = Some(error.clone());
            return Err(error);
        }
        registry.installed.push(InstalledDetour {
            label: label.to_string(),
            target,
            original,
            trampoline,
        });
        Ok(trampoline_ptr)
    }
    #[cfg(not(windows))]
    {
        let _ = (label, target, replacement, create_trampoline, patch_len);
        Err("detour engine is only available on Windows".to_string())
    }
}

fn uninstall_absolute_jump_detour(label: &str) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        let mut registry = detour_cell()
            .lock()
            .map_err(|_| "detour registry mutex poisoned".to_string())?;
        let Some(index) = registry
            .installed
            .iter()
            .position(|detour| detour.label == label)
        else {
            return Err(format!("detour not installed: {label}"));
        };
        let detour = registry.installed.remove(index);
        if let Err(error) = write_executable_memory(detour.target, &detour.original) {
            registry.last_error = Some(error.clone());
            return Err(error);
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = label;
        Err("detour engine is only available on Windows".to_string())
    }
}

fn absolute_jump_patch_len() -> usize {
    12
}

fn trampoline_len() -> usize {
    absolute_jump_patch_len() + trampoline_jump_back_len()
}

fn absolute_jump_patch(address: u64) -> [u8; 12] {
    let mut patch = [0u8; 12];
    patch[0] = 0x48;
    patch[1] = 0xb8;
    patch[2..10].copy_from_slice(&address.to_le_bytes());
    patch[10] = 0xff;
    patch[11] = 0xe0;
    patch
}

fn absolute_jump_patch_bytes(address: u64, patch_len: usize) -> Vec<u8> {
    let mut patch = vec![0x90u8; patch_len];
    patch[..absolute_jump_patch_len()].copy_from_slice(&absolute_jump_patch(address));
    patch
}

fn trampoline_jump_back_len() -> usize {
    14
}

fn trampoline_jump_back_patch(address: u64) -> [u8; 14] {
    let mut patch = [0u8; 14];
    // FF 25 00 00 00 00 jumps through the following absolute address without
    // clobbering volatile registers that may remain live after copied bytes.
    patch[0] = 0xff;
    patch[1] = 0x25;
    patch[2..6].copy_from_slice(&0u32.to_le_bytes());
    patch[6..14].copy_from_slice(&address.to_le_bytes());
    patch
}

#[cfg(windows)]
unsafe fn allocate_trampoline(
    original: &[u8],
    return_address: u64,
) -> Result<AllocatedTrampoline, String> {
    let len = original.len() + trampoline_jump_back_len();
    let ptr = VirtualAlloc(
        ptr::null(),
        len,
        MEM_COMMIT | MEM_RESERVE,
        PAGE_EXECUTE_READWRITE,
    ) as *mut u8;
    if ptr.is_null() {
        return Err(format!(
            "VirtualAlloc trampoline failed: {}",
            GetLastError()
        ));
    }
    std::ptr::copy_nonoverlapping(original.as_ptr(), ptr, original.len());
    let jump_back = trampoline_jump_back_patch(return_address);
    std::ptr::copy_nonoverlapping(jump_back.as_ptr(), ptr.add(original.len()), jump_back.len());
    if FlushInstructionCache(GetCurrentProcess(), ptr as *const c_void, len) == 0 {
        let _ = VirtualFree(ptr as *mut c_void, 0, MEM_RELEASE);
        return Err(format!(
            "FlushInstructionCache trampoline failed: {}",
            GetLastError()
        ));
    }
    Ok(AllocatedTrampoline { ptr, len })
}

#[cfg(windows)]
unsafe fn write_executable_memory(target: *mut u8, bytes: &[u8]) -> Result<(), String> {
    let mut old_protect = 0u32;
    if VirtualProtect(
        target as *const c_void,
        bytes.len(),
        PAGE_EXECUTE_READWRITE,
        &mut old_protect,
    ) == 0
    {
        return Err(format!("VirtualProtect RWX failed: {}", GetLastError()));
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
    let mut restored = 0u32;
    if VirtualProtect(
        target as *const c_void,
        bytes.len(),
        old_protect,
        &mut restored,
    ) == 0
    {
        return Err(format!("VirtualProtect restore failed: {}", GetLastError()));
    }
    if FlushInstructionCache(GetCurrentProcess(), target as *const c_void, bytes.len()) == 0 {
        return Err(format!("FlushInstructionCache failed: {}", GetLastError()));
    }
    Ok(())
}

#[cfg(windows)]
fn write_pointer_memory(target: *mut usize, value: usize) -> Result<(), String> {
    unsafe {
        let mut old_protect = 0u32;
        if VirtualProtect(
            target.cast::<c_void>(),
            size_of::<usize>(),
            PAGE_EXECUTE_READWRITE,
            &mut old_protect,
        ) == 0
        {
            return Err(format!(
                "VirtualProtect pointer RWX failed: {}",
                GetLastError()
            ));
        }
        ptr::write(target, value);
        let mut restored = 0u32;
        if VirtualProtect(
            target.cast::<c_void>(),
            size_of::<usize>(),
            old_protect,
            &mut restored,
        ) == 0
        {
            return Err(format!(
                "VirtualProtect pointer restore failed: {}",
                GetLastError()
            ));
        }
        if FlushInstructionCache(
            GetCurrentProcess(),
            target.cast::<c_void>(),
            size_of::<usize>(),
        ) == 0
        {
            return Err(format!(
                "FlushInstructionCache pointer failed: {}",
                GetLastError()
            ));
        }
    }
    Ok(())
}

fn normalized_mock_fps(value: u32) -> u32 {
    value.clamp(1, 60)
}

fn ensure_mock_frame_pump_started() {
    if FRAME_PUMP_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let _ = thread::Builder::new()
        .name("stormworks-video-get-frame-pump".to_string())
        .spawn(mock_frame_pump_loop);
}

fn mock_frame_pump_loop() {
    loop {
        let (enabled, fps) = match runtime_cell().lock() {
            Ok(state) => (
                state.configured
                    && state.hook_runtime.runtime_active
                    && state.hook_runtime.mock_frame_pump_active
                    && state.config.mock_render.enabled,
                normalized_mock_fps(state.config.mock_render.max_fps),
            ),
            Err(_) => (false, 60),
        };
        if enabled {
            let _ = refresh_mock_render_slots();
        }
        thread::sleep(Duration::from_millis(1000 / fps as u64));
    }
}

fn refresh_mock_render_slots() -> Result<(), String> {
    let state = request_runtime_state()?;
    if !state.config.mock_render.update_initialized_slots {
        return Ok(());
    }
    let requests = state
        .slots
        .values()
        .map(|slot| (slot.component.clone(), capture_request_from_slot(slot)))
        .collect::<Vec<_>>();
    for (component, request) in requests {
        if request.source != 0 && request.source != capture_source_code("mock_render") {
            continue;
        }
        let frame_id = FRAME_ID.load(Ordering::Relaxed);
        let rgb = mock_render_rgb_frame(
            frame_id,
            &component,
            request.slot,
            request.width,
            request.height,
        );
        let bytes = flatten_rgb_pixels(&rgb);
        push_rgb_frame_for_capture_request_with_source(
            request.component_hash,
            request.slot,
            request.width,
            request.height,
            bytes.as_ptr(),
            bytes.len(),
            1,
            "mock_render",
        )?;
    }
    Ok(())
}

fn mock_render_rgb_frame(
    frame_id: u64,
    component: &str,
    slot: u32,
    width: u32,
    height: u32,
) -> Vec<[u8; 3]> {
    let seed = component.bytes().fold(
        slot.wrapping_mul(31).wrapping_add(frame_id as u32),
        |acc, byte| acc.wrapping_mul(33).wrapping_add(byte as u32),
    );
    let mut rgb = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            rgb.push([
                ((x.wrapping_mul(5).wrapping_add(seed)) & 0xff) as u8,
                ((y.wrapping_mul(7).wrapping_add(seed >> 3)) & 0xff) as u8,
                ((x.wrapping_add(y)
                    .wrapping_mul(3)
                    .wrapping_add(frame_id as u32))
                    & 0xff) as u8,
            ]);
        }
    }
    rgb
}

fn flatten_rgb_pixels(rgb: &[[u8; 3]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(rgb.len() * 3);
    for pixel in rgb {
        bytes.extend_from_slice(pixel);
    }
    bytes
}

fn frame_info_for_slot(slot: u32) -> Result<serde_json::Value, String> {
    frame_info_for_component_slot(DEFAULT_COMPONENT, slot)
}

fn frame_info_for_component_slot(component: &str, slot: u32) -> Result<serde_json::Value, String> {
    with_component_slot(component, slot, |slot| {
        Ok(serde_json::to_value(FrameInfo {
            frame_id: slot.frame_id,
            component: slot.component.clone(),
            slot: slot.slot,
            width: slot.width,
            height: slot.height,
            mode: slot.mode.clone(),
            source: slot
                .latest_frame
                .as_ref()
                .map(|frame| frame.source.clone())
                .unwrap_or_else(|| "none".to_string()),
            ready: is_slot_ready_for_lua(slot),
            connected: slot.connected,
            input_source_handle: slot.input_source_handle,
            input_candidate_source_handle: slot.input_candidate_source_handle,
            input_selected_source_handle: slot.input_selected_source_handle,
            input_resolved_source_handle: slot.input_resolved_source_handle,
            input_upstream_source_handle: slot_upstream_source_handle(slot),
        })
        .unwrap())
    })
}

fn frame_for_slot(slot_id: u32, requested_mode: &str) -> Result<serde_json::Value, String> {
    frame_for_component_slot(DEFAULT_COMPONENT, slot_id, requested_mode)
}

fn frame_size_for_component_slot(component: &str, slot_id: u32) -> Result<(u32, u32), String> {
    let slot = require_slot_for_component(component, slot_id)?;
    Ok((slot.width, slot.height))
}

fn frame_for_component_slot_auto(
    component: &str,
    slot_id: u32,
) -> Result<serde_json::Value, String> {
    let mode = require_slot_for_component(component, slot_id)?.mode;
    frame_for_component_slot(component, slot_id, &mode)
}

fn frame_for_component_slot(
    component: &str,
    slot_id: u32,
    requested_mode: &str,
) -> Result<serde_json::Value, String> {
    let slot = require_slot_for_component(component, slot_id)?;
    if !is_slot_ready_for_lua(&slot) {
        return Err("frame not ready".to_string());
    }
    if !slot.connected {
        return Err("video not connected".to_string());
    }
    if slot.mode != requested_mode {
        return Err(format!(
            "slot {} initialized as {}, not {}",
            slot.slot, slot.mode, requested_mode
        ));
    }
    if let Some(frame) = slot
        .latest_frame
        .as_ref()
        .filter(|frame| frame_source_is_enabled_for_lua(frame.source.as_str()))
    {
        return match requested_mode {
            "gray" => Ok(serde_json::to_value(gray_matrix_from_frame(frame)).unwrap()),
            "rgb" => Ok(serde_json::to_value(rgb_matrix_from_frame(frame)).unwrap()),
            _ => Err("invalid mode".to_string()),
        };
    }
    Err("frame not ready".to_string())
}

fn packed_frame_for_component_slot(
    component: &str,
    slot_id: u32,
    requested_mode: &str,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::to_value(packed_frame_data_for_component_slot(
        component,
        slot_id,
        requested_mode,
    )?)
    .unwrap())
}

fn packed_frame_data_for_component_slot(
    component: &str,
    slot_id: u32,
    requested_mode: &str,
) -> Result<PackedFrame, String> {
    let slot = require_slot_for_component(component, slot_id)?;
    if !is_slot_ready_for_lua(&slot) {
        return Err("frame not ready".to_string());
    }
    if !slot.connected {
        return Err("video not connected".to_string());
    }
    if slot.mode != requested_mode {
        return Err(format!(
            "slot {} initialized as {}, not {}",
            slot.slot, slot.mode, requested_mode
        ));
    }

    let Some(frame) = slot
        .latest_frame
        .as_ref()
        .filter(|frame| frame_source_is_enabled_for_lua(frame.source.as_str()))
    else {
        return Err("frame not ready".to_string());
    };
    let (frame_id, source, stride, format, bytes) = {
        match requested_mode {
            "gray" => (
                frame.frame_id,
                frame.source.clone(),
                1,
                "u8-gray",
                packed_gray_bytes_from_frame(frame),
            ),
            "rgb" => (
                frame.frame_id,
                frame.source.clone(),
                3,
                "u8-rgb",
                packed_rgb_bytes_from_frame(frame),
            ),
            _ => return Err("invalid mode".to_string()),
        }
    };

    Ok(PackedFrame {
        frame_id,
        component: slot.component,
        slot: slot.slot,
        width: slot.width,
        height: slot.height,
        mode: requested_mode.to_string(),
        source,
        format: format.to_string(),
        stride,
        byte_len: bytes.len(),
        bytes,
    })
}

fn with_slot<F>(slot_id: u32, action: F) -> Result<serde_json::Value, String>
where
    F: FnOnce(&SlotState) -> Result<serde_json::Value, String>,
{
    with_component_slot(DEFAULT_COMPONENT, slot_id, action)
}

fn with_component_slot<F>(
    component: &str,
    slot_id: u32,
    action: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(&SlotState) -> Result<serde_json::Value, String>,
{
    let slot = require_slot_for_component(component, slot_id)?;
    action(&slot)
}

fn is_slot_ready_for_lua(slot: &SlotState) -> bool {
    slot.ready
        && slot.connected
        && slot
            .latest_frame
            .as_ref()
            .is_some_and(|frame| frame_source_is_enabled_for_lua(frame.source.as_str()))
}

fn frame_source_is_enabled_for_lua(source: &str) -> bool {
    matches!(
        source,
        "mock_render" | "pushed_rgb" | "texture_source" | "texture_upload" | "monitor_render"
    ) || (source == "source_texture" && source_texture_probe_enabled())
}

fn require_slot_for_component(component: &str, slot_id: u32) -> Result<SlotState, String> {
    if slot_id == 0 {
        return Err("invalid slot".to_string());
    }
    let state = request_runtime_state()?;
    state
        .slots
        .get(&slot_key(component, slot_id))
        .cloned()
        .ok_or_else(|| "not initialized".to_string())
}

const DEFAULT_COMPONENT: &str = "__default__";
const DEFAULT_VIDEO_SLOT: u32 = 1;

fn normalize_component(component: Option<&str>) -> String {
    let component = component.unwrap_or(DEFAULT_COMPONENT).trim();
    if component.is_empty() {
        DEFAULT_COMPONENT.to_string()
    } else {
        component.to_string()
    }
}

fn component_from_ptr(component: *const c_char) -> String {
    normalize_component(unsafe_cstr(component).as_deref())
}

fn slot_key(component: &str, slot: u32) -> SlotKey {
    SlotKey {
        component: normalize_component(Some(component)),
        slot,
    }
}

/// Drop component slots that have stopped executing `video.*` callbacks. Registration ownership
/// is intentionally not used here: multiple component contexts can share one Lua owner, and the
/// owner map only retains the most recently registered context. Callback liveness distinguishes
/// those components while still aging out despawned/reloaded vehicle state.
fn prune_dead_component_video_slots(state: &mut RuntimeState, keep_component: &str) -> usize {
    let dead: Vec<SlotKey> = state
        .slots
        .keys()
        .filter(|key| key.component != keep_component && !component_is_alive(&key.component))
        .cloned()
        .collect();
    let removed = dead.len();
    for key in dead {
        state.slots.remove(&key);
        state
            .video_source_components
            .retain(|_, component| slot_key(component, 0).component != key.component);
    }
    removed
}

fn describe_logic_video_ref(value: u64) -> String {
    if value == 0 {
        return "logic_ref=0".to_string();
    }
    let high = value >> 32;
    let low = logic_video_ref_low(value);
    format!("logic_ref_high=0x{high:x} logic_ref_low={low}")
}

fn logic_video_ref_low(value: u64) -> u64 {
    value & 0xffff_ffff
}

fn slot_key_label(slot: &SlotState) -> String {
    format!("{}:{}", slot.component, slot.slot)
}

fn slot_input_handles(slot: &SlotState) -> Vec<(&'static str, u64)> {
    [
        ("input", slot.input_source_handle),
        ("candidate", slot.input_candidate_source_handle),
        ("selected", slot.input_selected_source_handle),
        ("resolved", slot.input_resolved_source_handle),
        ("upstream", slot.input_upstream_source_handle),
    ]
    .into_iter()
    .filter(|(_, value)| *value != 0)
    .collect()
}

fn slot_matches_input_handle(slot: &SlotState, handle: u64) -> bool {
    handle != 0
        && slot_input_handles(slot)
            .into_iter()
            .any(|(_, value)| value == handle)
}

fn format_slot_input_handles(slot: &SlotState) -> String {
    let handles = slot_input_handles(slot);
    if handles.is_empty() {
        return "none".to_string();
    }
    handles
        .into_iter()
        .map(|(label, value)| format!("{label}={}", format_hex_or_zero(value)))
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Debug, Clone)]
struct MonitorInputSlotMatch {
    key: SlotKey,
    strength: MonitorInputSlotMatchStrength,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum MonitorInputSlotMatchStrength {
    Indirect,
    Exact,
}

impl MonitorInputSlotMatchStrength {
    fn label(self) -> &'static str {
        match self {
            Self::Indirect => "indirect",
            Self::Exact => "exact",
        }
    }
}

#[derive(Debug, Clone)]
struct MonitorInputSlotRelation {
    matches: Vec<MonitorInputSlotMatch>,
    details: Vec<String>,
    input_layout: String,
}

fn monitor_input_slot_relation(
    state: &RuntimeState,
    input_slot_ref: u64,
    effective_input_handle: u64,
) -> MonitorInputSlotRelation {
    monitor_input_slot_relation_with_candidates(
        state,
        [input_slot_ref, effective_input_handle]
            .into_iter()
            .filter(|value| *value != 0)
            .collect(),
    )
}

fn monitor_input_slot_relation_with_candidates(
    state: &RuntimeState,
    mut input_handles: Vec<u64>,
) -> MonitorInputSlotRelation {
    input_handles.retain(|value| *value != 0);
    input_handles.dedup();
    let mut matches = Vec::new();
    let mut details = Vec::new();
    for slot in state.slots.values() {
        let mut reasons = Vec::new();
        let mut strength = None;
        let mut marker_checks = Vec::new();
        let mut slot_has_pointer_relation = false;
        for (input_index, input_handle) in input_handles.iter().copied().enumerate() {
            let input_label = monitor_relation_input_label(input_index);
            if slot_matches_input_handle_or_source_key(slot, input_handle) {
                reasons.push(format!("exact_{input_label}"));
                strength = Some(MonitorInputSlotMatchStrength::Exact);
            }
            if let Some(value) = monitor_input_component_marker(input_handle) {
                let marker_component = format!("component_lua_context:{value:x}");
                if marker_component == slot.component {
                    reasons.push(format!("{input_label}_component_marker_same_lua_output"));
                } else {
                    reasons.push(format!(
                        "{input_label}_component_marker_external={}",
                        format_hex_or_zero(value as u64)
                    ));
                }
                marker_checks.push((input_label, input_handle, value));
            }
            for (label, handle) in slot_input_handles(slot) {
                for path in pointer_graph_contains_u64_paths(
                    input_handle,
                    MONITOR_INPUT_REF_RELATION_SCAN_BYTES,
                    MONITOR_INPUT_REF_NESTED_SCAN_BYTES,
                    handle,
                    2,
                )
                .into_iter()
                .take(3)
                {
                    reasons.push(format!("{input_label}_graph_contains_{label}@{path}"));
                    slot_has_pointer_relation = true;
                    if strength.is_none() {
                        strength = Some(MonitorInputSlotMatchStrength::Indirect);
                    }
                }
                for path in pointer_graph_contains_u64_paths(
                    handle,
                    MONITOR_INPUT_REF_RELATION_SCAN_BYTES,
                    MONITOR_INPUT_REF_NESTED_SCAN_BYTES,
                    input_handle,
                    1,
                )
                .into_iter()
                .take(2)
                {
                    reasons.push(format!("{label}_graph_contains_{input_label}@{path}"));
                    slot_has_pointer_relation = true;
                    if strength.is_none() {
                        strength = Some(MonitorInputSlotMatchStrength::Indirect);
                    }
                }
                if pointer_range_contains_u64_path(
                    input_handle,
                    MONITOR_INPUT_REF_RELATION_SCAN_BYTES,
                    handle,
                )
                .is_some()
                {
                    reasons.push(format!("{input_label}_range_contains_{label}"));
                }
                if pointer_range_contains_u64_path(
                    handle,
                    MONITOR_INPUT_REF_RELATION_SCAN_BYTES,
                    input_handle,
                )
                .is_some()
                {
                    reasons.push(format!("{label}_range_contains_{input_label}"));
                }
            }
        }
        for (input_label, input_handle, marker) in marker_checks {
            let (matched, reason) = external_video_bridge_marker_match_status(
                state,
                slot,
                input_handle,
                marker,
                slot_has_pointer_relation,
            );
            if !reason.is_empty() {
                reasons.push(format!("{input_label}_{reason}"));
            }
            if matched {
                strength = Some(MonitorInputSlotMatchStrength::Exact);
            }
        }
        reasons.sort();
        reasons.dedup();
        if !reasons.is_empty() {
            let key = slot_key(&slot.component, slot.slot);
            let strength = strength.unwrap_or(MonitorInputSlotMatchStrength::Indirect);
            details.push(format!(
                "{}:{}:{}={}",
                slot.component,
                slot.slot,
                strength.label(),
                reasons.join("+")
            ));
            if !matches
                .iter()
                .any(|matched: &MonitorInputSlotMatch| matched.key == key)
            {
                matches.push(MonitorInputSlotMatch { key, strength });
            }
        }
    }
    let input_layout = if input_handles.is_empty() {
        "input_ref=0".to_string()
    } else {
        input_handles
            .into_iter()
            .take(4)
            .enumerate()
            .map(|(index, handle)| {
                monitor_relation_handle_layout_text(monitor_relation_input_label(index), handle)
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    MonitorInputSlotRelation {
        matches,
        details,
        input_layout,
    }
}

fn monitor_relation_input_label(index: usize) -> &'static str {
    match index {
        0 => "input",
        1 => "effective",
        2 => "candidate2",
        3 => "candidate3",
        _ => "candidate",
    }
}

fn monitor_relation_handle_layout_text(label: &str, handle: u64) -> String {
    compact_relation_handle_layout_text(label, handle)
}

fn compact_relation_handle_layout_text(label: &str, handle: u64) -> String {
    if handle == 0 {
        return format!("{label}=0");
    }
    if !pointer_value_looks_process_address(handle)
        || !memory_range_is_readable(handle as *const c_void, size_of::<usize>())
    {
        return format!(
            "{label}={} readable=false decoded={}",
            format_hex_or_zero(handle),
            describe_logic_video_ref(handle)
        );
    }
    let base = handle as usize;
    let vtable = read_usize_field(base, 0).unwrap_or(0) as u64;
    let vtable_static = runtime_to_static_va(vtable)
        .map(format_hex_or_zero)
        .unwrap_or_else(|| "unknown".to_string());
    let fields = [
        0x08usize, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x50, 0x58,
    ]
    .into_iter()
    .filter_map(|offset| {
        read_usize_field(base, offset)
            .map(|value| format!("+0x{offset:x}={}", format_hex_or_zero(value as u64)))
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{label}={} static={} decoded={} fields=[{}]",
        format_hex_or_zero(handle),
        vtable_static,
        describe_logic_video_ref(handle),
        fields
    )
}

#[allow(dead_code)]
fn verbose_monitor_relation_handle_layout_text(label: &str, handle: u64) -> String {
    if video_source_vtable_static(handle) == Some(VTABLE_VEHICLE_LOGIC_SLOT_INPUT_VIDEO) {
        input_video_source_debug_layout_text(label, handle)
    } else {
        format!("{label}:{}", monitor_input_ref_layout_text(handle))
    }
}

fn format_monitor_input_slot_relation(relation: &MonitorInputSlotRelation) -> String {
    if relation.details.is_empty() {
        "no_relation".to_string()
    } else {
        relation.details.join("|")
    }
}

fn external_video_bridge_marker_match_status(
    state: &RuntimeState,
    slot: &SlotState,
    monitor_input_handle: u64,
    marker: usize,
    has_pointer_relation: bool,
) -> (bool, String) {
    if format!("component_lua_context:{marker:x}") == slot.component {
        return (false, String::new());
    }
    let candidates = external_video_bridge_markers_for_slot(state, slot);
    let candidate_count = candidates
        .iter()
        .filter(|candidate| candidate.marker == marker)
        .count();
    if !has_pointer_relation {
        return (
            false,
            format!(
                "external_video_bridge_marker_skip={} reason=no_pointer_relation candidates={}",
                format_hex_or_zero(marker as u64),
                candidate_count
            ),
        );
    }
    let monitor_kind = monitor_input_logic_kind(monitor_input_handle);
    if monitor_kind != Some(LOGIC_KIND_EXTERNAL_VIDEO_INPUT) {
        return (
            false,
            format!(
                "external_video_bridge_marker_skip={} reason=monitor_kind_{:?} candidates={}",
                format_hex_or_zero(marker as u64),
                monitor_kind,
                candidate_count
            ),
        );
    }
    let matches = candidates
        .iter()
        .filter(|candidate| candidate.marker == marker)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return (
            false,
            format!(
                "external_video_bridge_marker_skip={} reason=no_candidate candidates=0",
                format_hex_or_zero(marker as u64)
            ),
        );
    }
    let candidate = *matches[0];
    (
        true,
        format!(
            "external_video_bridge_marker_match={} source={} node={} candidates={}",
            format_hex_or_zero(marker as u64),
            format_hex_or_zero(candidate.source_handle),
            format_hex_or_zero(candidate.node),
            matches.len()
        ),
    )
}

#[derive(Debug, Clone, Copy)]
struct ExternalVideoBridgeMarker {
    marker: usize,
    source_handle: u64,
    node: u64,
}

fn external_video_bridge_markers_for_slot(
    state: &RuntimeState,
    slot: &SlotState,
) -> Vec<ExternalVideoBridgeMarker> {
    let mut markers = Vec::new();
    let script_node =
        lua_script_input_video_node_from_component(&slot.component).map(|value| value as u64);
    let slot_local_kind = script_node.and_then(|node| {
        read_usize_field(node as usize, 0x18).map(|value| logic_video_ref_low(value as u64))
    });
    for (node, handles) in &state.video_node_sources {
        if Some(*node) == script_node {
            continue;
        }
        if slot_local_kind.is_some()
            && read_usize_field(*node as usize, 0x18).map(|value| logic_video_ref_low(value as u64))
                == slot_local_kind
        {
            continue;
        }
        for (_, handle) in handles.handles() {
            if handle == 0
                || output_video_logic_kind(handle) != Some(LOGIC_KIND_EXTERNAL_VIDEO_INPUT)
            {
                continue;
            }
            let Some(marker) = output_video_component_marker(handle) else {
                continue;
            };
            if format!("component_lua_context:{marker:x}") == slot.component {
                continue;
            }
            let candidate = ExternalVideoBridgeMarker {
                marker,
                source_handle: handle,
                node: *node,
            };
            if !markers.iter().any(|existing: &ExternalVideoBridgeMarker| {
                existing.marker == candidate.marker
                    && existing.source_handle == candidate.source_handle
                    && existing.node == candidate.node
            }) {
                markers.push(candidate);
            }
        }
    }
    markers
}

fn compact_logic_ref_field(base: usize, offset: usize) -> String {
    read_usize_field(base, offset)
        .map(|value| {
            let value = value as u64;
            format!(
                "+0x{offset:x}={}({})",
                format_hex_or_zero(value),
                describe_logic_video_ref(value)
            )
        })
        .unwrap_or_else(|| format!("+0x{offset:x}=unreadable"))
}

fn compact_bridge_slot_field_summary(label: &str, handle: u64) -> String {
    if handle == 0 {
        return format!("{label}=0");
    }
    if !pointer_value_looks_process_address(handle)
        || !memory_range_is_readable(handle as *const c_void, size_of::<usize>())
    {
        return format!(
            "{label}={} readable=false decoded={}",
            format_hex_or_zero(handle),
            describe_logic_video_ref(handle)
        );
    }
    let base = handle as usize;
    let vtable_static = video_source_vtable_static(handle)
        .map(format_hex_or_zero)
        .unwrap_or_else(|| "unknown".to_string());
    let fields = [
        0x08usize, 0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x50, 0x58,
    ]
    .into_iter()
    .map(|offset| compact_logic_ref_field(base, offset))
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{label}={} static={} fields=[{}]",
        format_hex_or_zero(handle),
        vtable_static,
        fields
    )
}

fn compact_lua_slot_bridge_summary(slot: &SlotState) -> String {
    let node = lua_script_input_video_node_from_component(&slot.component);
    let node_text = node
        .map(|node| compact_bridge_slot_field_summary("lua_input_node", node as u64))
        .unwrap_or_else(|| "lua_input_node=none".to_string());
    let resolved_text =
        compact_bridge_slot_field_summary("resolved", slot.input_resolved_source_handle);
    let upstream_text =
        compact_bridge_slot_field_summary("upstream", slot_upstream_source_handle(slot));
    format!(
        "slot={}:{} {} {} {} handles=[{}]",
        slot.component,
        slot.slot,
        node_text,
        resolved_text,
        upstream_text,
        format_slot_input_handles(slot)
    )
}

fn monitor_bridge_diagnostic_text(
    state: &RuntimeState,
    input_slot_ref: u64,
    effective_input_handle: u64,
    relation: &MonitorInputSlotRelation,
) -> String {
    let monitor_input = compact_bridge_slot_field_summary("monitor_input", input_slot_ref);
    let monitor_effective =
        compact_bridge_slot_field_summary("monitor_effective", effective_input_handle);
    let slot_summaries = state
        .slots
        .values()
        .map(compact_lua_slot_bridge_summary)
        .collect::<Vec<_>>()
        .join(" | ");
    let marker_summaries = state
        .slots
        .values()
        .map(|slot| {
            format!(
                "{}:{} markers=[{}]",
                slot.component,
                slot.slot,
                external_video_bridge_markers_text(state, slot)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "monitor bridge diagnostic input={} effective={} relation={} {} {} lua_slots=[{}] external_bridge_markers=[{}]",
        format_hex_or_zero(input_slot_ref),
        format_hex_or_zero(effective_input_handle),
        format_monitor_input_slot_relation(relation),
        monitor_input,
        monitor_effective,
        slot_summaries,
        marker_summaries
    )
}

fn external_video_bridge_markers_text(state: &RuntimeState, slot: &SlotState) -> String {
    let markers = external_video_bridge_markers_for_slot(state, slot);
    if markers.is_empty() {
        return "none".to_string();
    }
    markers
        .into_iter()
        .take(8)
        .map(|candidate| {
            format!(
                "{}@{} node={}",
                format_hex_or_zero(candidate.marker as u64),
                format_hex_or_zero(candidate.source_handle),
                format_hex_or_zero(candidate.node)
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn log_monitor_input_relation_diagnostic(
    state: &RuntimeState,
    input_slot_ref: u64,
    effective_input_handle: u64,
    relation: &MonitorInputSlotRelation,
) {
    log_runtime_diagnostic_no_snapshot(
        state,
        &format!(
            "monitor input relation unresolved input={} effective={} relation={} layout={} slots={}",
            format_hex_or_zero(input_slot_ref),
            format_hex_or_zero(effective_input_handle),
            format_monitor_input_slot_relation(relation),
            relation.input_layout,
            describe_slots(state)
        ),
        &MONITOR_INPUT_RELATION_DIAGNOSTIC_COUNT,
        8,
    );
    log_runtime_diagnostic_no_snapshot(
        state,
        &monitor_bridge_diagnostic_text(state, input_slot_ref, effective_input_handle, relation),
        &MONITOR_BRIDGE_DIAGNOSTIC_COUNT,
        16,
    );
}

fn monitor_input_ref_layout_text(input_slot_ref: u64) -> String {
    if input_slot_ref == 0 {
        return "input_ref=0".to_string();
    }
    if !pointer_value_looks_process_address(input_slot_ref)
        || !memory_range_is_readable(
            input_slot_ref as *const c_void,
            MONITOR_INPUT_REF_LAYOUT_BYTES,
        )
    {
        return format!(
            "input_ref={} readable=false",
            format_hex_or_zero(input_slot_ref)
        );
    }
    let base = input_slot_ref as usize;
    let vtable = read_usize_field(base, 0).unwrap_or(0) as u64;
    let vtable_static = runtime_to_static_va(vtable)
        .map(format_hex_or_zero)
        .unwrap_or_else(|| "unknown".to_string());
    let fields = (0..MONITOR_INPUT_REF_LAYOUT_BYTES)
        .step_by(size_of::<usize>())
        .filter_map(|offset| {
            read_usize_field(base, offset)
                .map(|value| format!("+0x{offset:x}={}", format_hex_or_zero(value as u64)))
        })
        .take(16)
        .collect::<Vec<_>>()
        .join(",");
    let nested = monitor_input_ref_nested_layout_text(base);
    format!(
        "input_ref={} vtable={} static={} fields=[{}] nested=[{}]",
        format_hex_or_zero(input_slot_ref),
        format_hex_or_zero(vtable),
        vtable_static,
        fields,
        nested
    )
}

fn monitor_input_ref_nested_layout_text(base: usize) -> String {
    let mut parts = Vec::new();
    let mut seen = BTreeSet::new();
    for offset in (0..MONITOR_INPUT_REF_LAYOUT_BYTES).step_by(size_of::<usize>()) {
        if parts.len() >= 8 {
            break;
        }
        let Some(pointer) = read_usize_field(base, offset).map(|value| value as u64) else {
            continue;
        };
        if !pointer_value_looks_process_address(pointer) || pointer == base as u64 {
            continue;
        }
        if !seen.insert(pointer) {
            continue;
        }
        let Some(summary) = pointer_qword_summary(pointer, 8) else {
            continue;
        };
        parts.push(format!(
            "+0x{offset:x}->{}:{}",
            format_hex_or_zero(pointer),
            summary
        ));
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join("|")
    }
}

fn pointer_qword_summary(pointer: u64, qwords: usize) -> Option<String> {
    if !pointer_value_looks_process_address(pointer) {
        return None;
    }
    let base = pointer as usize;
    if !memory_range_is_readable(base as *const c_void, size_of::<usize>()) {
        return None;
    }
    let fields = (0..qwords.saturating_mul(size_of::<usize>()))
        .step_by(size_of::<usize>())
        .filter_map(|offset| {
            read_usize_field(base, offset)
                .map(|value| format!("+0x{offset:x}={}", format_hex_or_zero(value as u64)))
        })
        .collect::<Vec<_>>();
    if fields.is_empty() {
        None
    } else {
        Some(fields.join(","))
    }
}

fn compact_pointer_layout(label: &str, pointer: usize) -> String {
    if pointer == 0 {
        return format!("{label}=0");
    }
    let readable = memory_range_is_readable(pointer as *const c_void, size_of::<usize>());
    if !readable {
        return format!(
            "{label}={} readable=false",
            format_hex_or_zero(pointer as u64)
        );
    }
    let summary = pointer_qword_summary(pointer as u64, ADDITIVE_BIND_ARGUMENT_LAYOUT_QWORDS)
        .unwrap_or_else(|| "qwords=none".to_string());
    let handle_28 = read_u32_field(pointer, ADDITIVE_MONITOR_TEXTURE_DIRECT_HANDLE_OFFSET)
        .map(|value| format!("0x{value:x}"))
        .unwrap_or_else(|| "unreadable".to_string());
    let handle_48 = read_u32_field(pointer, ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET)
        .map(|value| format!("0x{value:x}"))
        .unwrap_or_else(|| "unreadable".to_string());
    format!(
        "{label}={} +0x28={} +0x48={} qwords=[{}]",
        format_hex_or_zero(pointer as u64),
        handle_28,
        handle_48,
        summary
    )
}

fn compact_float_layout(label: &str, pointer: usize) -> String {
    if pointer == 0 {
        return format!("{label}=0");
    }
    let byte_len = ADDITIVE_BIND_ARGUMENT_LAYOUT_QWORDS.saturating_mul(size_of::<usize>());
    if !memory_range_is_readable(pointer as *const c_void, byte_len) {
        return format!(
            "{label}={} readable=false",
            format_hex_or_zero(pointer as u64)
        );
    }
    let base = pointer as *const u8;
    let mut floats = Vec::new();
    let mut doubles = Vec::new();
    for offset in (0..byte_len).step_by(size_of::<f32>()) {
        let bits = unsafe { read_unaligned_at::<u32>(base, offset) };
        let value = f32::from_bits(bits);
        if value.is_finite() && value.abs() <= 10000.0 {
            floats.push(format!("+0x{offset:x}={value:.6}"));
            if floats.len() >= 16 {
                break;
            }
        }
    }
    for offset in (0..byte_len).step_by(size_of::<f64>()) {
        let bits = unsafe { read_unaligned_at::<u64>(base, offset) };
        let value = f64::from_bits(bits);
        if value.is_finite() && value.abs() <= 10000.0 {
            doubles.push(format!("+0x{offset:x}={value:.6}"));
            if doubles.len() >= 12 {
                break;
            }
        }
    }
    format!(
        "{label}={} f32=[{}] f64=[{}]",
        format_hex_or_zero(pointer as u64),
        if floats.is_empty() {
            "none".to_string()
        } else {
            floats.join(",")
        },
        if doubles.is_empty() {
            "none".to_string()
        } else {
            doubles.join(",")
        }
    )
}

fn describe_monitor_input_slot_relation(state: &RuntimeState, input_slot_ref: u64) -> String {
    if input_slot_ref == 0 {
        return "input_slot_ref=0".to_string();
    }
    let relation = monitor_input_slot_relation(state, input_slot_ref, 0);
    if relation.details.is_empty() {
        format!(
            "no_relation readable={}",
            memory_range_is_readable(input_slot_ref as *const c_void, size_of::<usize>())
        )
    } else {
        relation.details.join("|")
    }
}

fn describe_monitor_input_slot_relation_with_candidates(
    state: &RuntimeState,
    input_handles: Vec<u64>,
) -> String {
    let first = input_handles
        .iter()
        .copied()
        .find(|value| *value != 0)
        .unwrap_or(0);
    if first == 0 {
        return "input_slot_ref=0".to_string();
    }
    let relation = monitor_input_slot_relation_with_candidates(state, input_handles);
    if relation.details.is_empty() {
        format!(
            "no_relation readable={} layout={}",
            memory_range_is_readable(first as *const c_void, size_of::<usize>()),
            relation.input_layout
        )
    } else {
        relation.details.join("|")
    }
}

fn describe_source_relation_to_slots(state: &RuntimeState, source_handle: u64) -> String {
    if source_handle == 0 {
        return "source=0".to_string();
    }
    let matches = state
        .slots
        .values()
        .filter(|slot| slot_matches_input_handle(slot, source_handle))
        .map(|slot| format!("{}:{}", slot.component, slot.slot))
        .collect::<Vec<_>>();
    if !matches.is_empty() {
        return format!("exact_slots={}", matches.join(","));
    }
    format!(
        "no_exact_slot readable={}",
        memory_range_is_readable(source_handle as *const c_void, size_of::<usize>())
    )
}

fn pointer_graph_contains_u64_paths(
    base: u64,
    direct_scan_bytes: usize,
    nested_scan_bytes: usize,
    needle: u64,
    max_depth: usize,
) -> Vec<String> {
    let mut paths = Vec::new();
    let mut visited = BTreeSet::new();
    pointer_graph_contains_u64_paths_inner(
        base,
        direct_scan_bytes,
        nested_scan_bytes,
        needle,
        max_depth,
        "root".to_string(),
        &mut visited,
        &mut paths,
    );
    paths
}

fn pointer_graph_contains_u64_paths_inner(
    base: u64,
    scan_bytes: usize,
    nested_scan_bytes: usize,
    needle: u64,
    depth_remaining: usize,
    path_prefix: String,
    visited: &mut BTreeSet<u64>,
    paths: &mut Vec<String>,
) {
    if paths.len() >= MONITOR_INPUT_REF_NESTED_POINTER_LIMIT {
        return;
    }
    if base == 0 || needle == 0 || !pointer_value_looks_process_address(base) {
        return;
    }
    if !visited.insert(base) {
        return;
    }
    let Some(direct_path) = pointer_range_contains_u64_path(base, scan_bytes, needle) else {
        if depth_remaining == 0 {
            return;
        }
        let mut scanned_pointers = 0usize;
        for offset in (0..scan_bytes).step_by(size_of::<usize>()) {
            if scanned_pointers >= MONITOR_INPUT_REF_NESTED_POINTER_LIMIT {
                break;
            }
            let Some(pointer) = read_usize_field(base as usize, offset).map(|value| value as u64)
            else {
                continue;
            };
            if !pointer_value_looks_process_address(pointer) || pointer == base {
                continue;
            }
            if !memory_range_is_readable(pointer as *const c_void, size_of::<usize>()) {
                continue;
            }
            scanned_pointers += 1;
            pointer_graph_contains_u64_paths_inner(
                pointer,
                nested_scan_bytes,
                nested_scan_bytes,
                needle,
                depth_remaining.saturating_sub(1),
                format!("{path_prefix}+0x{offset:x}->"),
                visited,
                paths,
            );
            if paths.len() >= MONITOR_INPUT_REF_NESTED_POINTER_LIMIT {
                break;
            }
        }
        return;
    };
    paths.push(format!("{path_prefix}{direct_path}"));
}

fn pointer_range_contains_u64_path(base: u64, scan_bytes: usize, needle: u64) -> Option<String> {
    if base == 0 || needle == 0 || !pointer_value_looks_process_address(base) {
        return None;
    }
    for offset in (0..scan_bytes).step_by(size_of::<usize>()) {
        let Some(value) = read_usize_field(base as usize, offset).map(|value| value as u64) else {
            continue;
        };
        if value == needle {
            return Some(format!("+0x{offset:x}"));
        }
    }
    None
}

fn pointer_range_contains_u64(base: u64, scan_bytes: usize, needle: u64) -> bool {
    pointer_range_contains_u64_path(base, scan_bytes, needle).is_some()
}

fn describe_slots(state: &RuntimeState) -> String {
    if state.slots.is_empty() {
        return "none".to_string();
    }
    state
        .slots
        .values()
        .take(8)
        .map(|slot| {
            format!(
                "{}:{}={}x{}:{} connected={} ready={} handles=[{}] upload_tex={} source_tex={} frame={}",
                slot.component,
                slot.slot,
                slot.width,
                slot.height,
                slot.mode,
                slot.connected,
                is_slot_ready_for_lua(slot),
                format_slot_input_handles(slot),
                slot.texture_upload_handle
                    .map(format_hex_or_zero)
                    .unwrap_or_else(|| "none".to_string()),
                slot.source_texture_handle
                    .map(format_hex_or_zero)
                    .unwrap_or_else(|| "none".to_string()),
                slot.latest_frame
                    .as_ref()
                    .map(|frame| frame.source.as_str())
                    .unwrap_or("none")
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn log_runtime_diagnostic(
    state: &RuntimeState,
    message: &str,
    counter: &AtomicUsize,
    max_lines: usize,
) {
    log_runtime_diagnostic_no_snapshot(state, message, counter, max_lines);
}

fn diagnostic_budget_available(counter: &AtomicUsize, max_lines: usize) -> bool {
    verbose_runtime_diagnostics_enabled() && counter.load(Ordering::Relaxed) < max_lines
}

fn verbose_runtime_diagnostics_enabled() -> bool {
    VERBOSE_RUNTIME_DIAGNOSTICS.load(Ordering::Relaxed)
}

fn log_runtime_diagnostic_no_snapshot(
    state: &RuntimeState,
    message: &str,
    counter: &AtomicUsize,
    max_lines: usize,
) {
    if !verbose_runtime_diagnostics_enabled() {
        return;
    }
    let should_log = counter.fetch_add(1, Ordering::Relaxed) < max_lines;
    if should_log {
        if let Some(path) = &state.log_path {
            let _ = append_log(path, message);
        }
    }
}

fn validate_config(config: &VideoGetConfig) -> Result<(), String> {
    if !config.enabled {
        return Err("video_get config disabled the plugin".to_string());
    }
    validate_limit(&config.limits.gray, "gray")?;
    validate_limit(&config.limits.rgb, "rgb")?;
    if config.limits.max_active_slots == 0 {
        return Err("max_active_slots must be >= 1".to_string());
    }
    Ok(())
}

fn validate_limit(limit: &FrameLimit, name: &str) -> Result<(), String> {
    if limit.max_width == 0 || limit.max_height == 0 {
        return Err(format!("{name} max_width/max_height must be >= 1"));
    }
    Ok(())
}

fn validate_frame_size(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
    mode: &str,
) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("width and height must be >= 1".to_string());
    }
    if width > max_width || height > max_height {
        return Err(format!(
            "{mode} frame {width}x{height} exceeds configured limit {max_width}x{max_height}"
        ));
    }
    Ok(())
}

fn signature_keys(symbols: &serde_json::Value) -> Vec<String> {
    let mut keys = symbols
        .as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn signature_summary(symbols: &serde_json::Value) -> serde_json::Value {
    let mut summary = serde_json::Map::new();
    if let Some(object) = symbols.as_object() {
        for (name, value) in object {
            let kind = value
                .get("kind")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let candidate_count = value
                .get("value")
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            summary.insert(
                name.clone(),
                serde_json::json!({
                    "kind": kind,
                    "candidate_count": candidate_count
                }),
            );
        }
    }
    serde_json::Value::Object(summary)
}

fn observation_candidate_count(symbols: &serde_json::Value) -> usize {
    symbols
        .as_object()
        .map(|object| {
            object
                .values()
                .map(|group| {
                    group
                        .get("value")
                        .and_then(|value| value.as_array())
                        .map(Vec::len)
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0)
}

fn load_hook_plan(path: Option<&PathBuf>) -> Result<HookPlan, String> {
    match path {
        Some(path) => read_json::<HookPlan>(path)
            .map_err(|error| format!("loading hook plan {}: {error:#}", path.display())),
        None => Ok(default_hook_plan()),
    }
}

fn default_hook_plan() -> HookPlan {
    HookPlan {
        schema_version: 1,
        source: "default_no_patch_plan".to_string(),
        generated_at: None,
        patching_allowed: false,
        dry_run_only: true,
        required_stage: Some("lua_api_registration".to_string()),
        accepted_stages: Vec::new(),
        lua_api: None,
        game_lua: None,
        hooks: Vec::new(),
    }
}

fn hook_plan_summary_value(plan: &HookPlan, validation: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "schema_version": plan.schema_version,
        "source": plan.source,
        "patching_allowed": plan.patching_allowed,
        "dry_run_only": plan.dry_run_only,
        "required_stage": plan.required_stage,
        "accepted_stage_count": plan.accepted_stages.len(),
        "lua_api_configured": plan.lua_api.is_some(),
        "game_lua_configured": plan.game_lua.is_some(),
        "hook_count": plan.hooks.len(),
        "enabled_hook_count": plan.hooks.iter().filter(|hook| hook.enabled).count(),
        "validation": validation
    })
}

fn validate_hook_plan(plan: &HookPlan, symbols: &serde_json::Value) -> serde_json::Value {
    let mut invalid = Vec::new();
    let mut valid = 0usize;
    for hook in &plan.hooks {
        let stage = symbols.get(&hook.stage);
        let Some(stage) = stage else {
            invalid.push(format!("{}: unknown stage {}", hook.label, hook.stage));
            continue;
        };
        let candidates = stage
            .get("value")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let matched = candidates.iter().any(|candidate| {
            let entry = candidate
                .get("entry")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let va = candidate
                .pointer("/byte_check/va")
                .and_then(|value| value.as_str())
                .unwrap_or(entry);
            entry.eq_ignore_ascii_case(&hook.target_va) || va.eq_ignore_ascii_case(&hook.target_va)
        });
        if matched {
            valid += 1;
        } else {
            invalid.push(format!(
                "{}: target_va {} is not in signature stage {}",
                hook.label, hook.target_va, hook.stage
            ));
        }
    }
    serde_json::json!({
        "valid": invalid.is_empty(),
        "valid_hook_count": valid,
        "invalid_hook_count": invalid.len(),
        "errors": invalid
    })
}

fn hook_install_dry_run(
    context: Option<&PluginRuntimeContext>,
    plan: &HookPlan,
    symbols: &serde_json::Value,
    validation: &serde_json::Value,
) -> serde_json::Value {
    let mut hooks = Vec::new();
    for hook in &plan.hooks {
        hooks.push(hook_install_dry_run_entry(context, hook, symbols));
    }
    serde_json::json!({
        "mode": "dry_run_no_target_memory_writes",
        "hook_count": plan.hooks.len(),
        "process": current_process_context_value(context),
        "lua_api": lua_api_plan_status(context, plan),
        "game_lua": game_lua_plan_status(context, plan),
        "component_context": component_context_plan_status(plan),
        "input_video": input_video_plan_status(plan),
        "texture_source": texture_source_plan_status(plan),
        "texture_upload": texture_upload_plan_status(plan),
        "monitor_render": monitor_render_plan_status(plan),
        "validation": validation,
        "hooks": hooks
    })
}

fn hook_install_dry_run_entry(
    context: Option<&PluginRuntimeContext>,
    hook: &HookPlanEntry,
    symbols: &serde_json::Value,
) -> serde_json::Value {
    let target = resolve_hook_target_address(context, hook);
    let signature_match = hook_signature_match(hook, symbols);
    let replacement = resolve_replacement_symbol(&hook.replacement);
    serde_json::json!({
        "label": hook.label,
        "stage": hook.stage,
        "enabled": hook.enabled,
        "target_va": hook.target_va,
        "preferred_image_base": target.preferred_image_base.map(hex_u64),
        "rva": target.rva.map(hex_u64),
        "runtime_module_base": target.runtime_module_base.map(hex_u64),
        "runtime_address": target.runtime_address.map(hex_u64),
        "signature_match": signature_match,
        "replacement": replacement.to_json(),
        "require_trampoline": hook.require_trampoline,
        "patch_len": hook.patch_len.unwrap_or_else(absolute_jump_patch_len),
        "can_install_if_gate_opens": hook.enabled
            && signature_match
            && target.runtime_address.is_some()
            && replacement.usable_for_patch
            && current_process_matches_context(context)
    })
}

#[derive(Debug, Clone)]
struct HookTargetResolution {
    preferred_image_base: Option<u64>,
    rva: Option<u64>,
    runtime_module_base: Option<u64>,
    runtime_address: Option<u64>,
}

fn resolve_hook_target_address(
    context: Option<&PluginRuntimeContext>,
    hook: &HookPlanEntry,
) -> HookTargetResolution {
    let static_va = parse_hex_u64_local(&hook.target_va);
    let preferred_image_base =
        context.and_then(|context| read_game_image_base(&context.game_exe).ok());
    let rva = static_va.and_then(|va| preferred_image_base.and_then(|base| va.checked_sub(base)));
    let runtime_module_base = current_process_module_base();
    let runtime_address = rva.and_then(|rva| runtime_module_base.map(|base| base + rva));
    HookTargetResolution {
        preferred_image_base,
        rva,
        runtime_module_base,
        runtime_address,
    }
}

fn hook_signature_match(hook: &HookPlanEntry, symbols: &serde_json::Value) -> bool {
    let Some(stage) = symbols.get(&hook.stage) else {
        return false;
    };
    stage
        .get("value")
        .and_then(|value| value.as_array())
        .map(|candidates| {
            candidates.iter().any(|candidate| {
                let entry = candidate
                    .get("entry")
                    .and_then(|value| value.as_str())
                    .unwrap_or("");
                let va = candidate
                    .pointer("/byte_check/va")
                    .and_then(|value| value.as_str())
                    .unwrap_or(entry);
                entry.eq_ignore_ascii_case(&hook.target_va)
                    || va.eq_ignore_ascii_case(&hook.target_va)
            })
        })
        .unwrap_or(false)
}

fn hook_plan_requires_lua_api(plan: &HookPlan) -> bool {
    plan.hooks.iter().filter(|hook| hook.enabled).any(|hook| {
        matches!(
            hook.replacement.as_str(),
            "stormworks_video_get_register_lua_api_hook"
                | "stormworks_video_get_register_lua_api_hook_arg1"
                | "stormworks_video_get_register_lua_api_hook_arg2"
                | "stormworks_video_get_register_lua_api_hook_arg3"
                | "stormworks_video_get_register_lua_api_hook_arg4"
        )
    })
}

fn hook_plan_requires_game_lua(plan: &HookPlan) -> bool {
    plan.hooks
        .iter()
        .filter(|hook| hook.enabled)
        .any(|hook| hook.replacement == "stormworks_video_get_component_lua_init_hook_arg1")
}

fn hook_plan_uses_component_context(plan: &HookPlan) -> bool {
    plan.hooks.iter().filter(|hook| hook.enabled).any(|hook| {
        matches!(
            hook.replacement.as_str(),
            "stormworks_video_get_component_context_hook_arg1"
                | "stormworks_video_get_component_context_hook_arg2"
                | "stormworks_video_get_component_context_hook_arg3"
                | "stormworks_video_get_component_context_hook_arg4"
        )
    })
}

fn hook_plan_uses_input_video(plan: &HookPlan) -> bool {
    plan.hooks.iter().filter(|hook| hook.enabled).any(|hook| {
        matches!(
            hook.replacement.as_str(),
            "stormworks_video_get_input_video_hook_arg1"
                | "stormworks_video_get_input_video_hook_arg2"
                | "stormworks_video_get_input_video_hook_arg3"
                | "stormworks_video_get_input_video_hook_arg4"
                | "stormworks_video_get_input_video_node_update_hook_arg2"
                | "stormworks_video_get_input_video_node_select_hook_arg5"
        )
    })
}

fn hook_plan_uses_texture_source(plan: &HookPlan) -> bool {
    plan.hooks.iter().filter(|hook| hook.enabled).any(|hook| {
        matches!(
            hook.replacement.as_str(),
            "stormworks_video_get_texture_source_hook_arg1"
                | "stormworks_video_get_texture_source_hook_arg2"
                | "stormworks_video_get_texture_source_hook_arg3"
                | "stormworks_video_get_texture_source_hook_arg4"
        )
    })
}

fn hook_plan_uses_texture_upload(plan: &HookPlan) -> bool {
    plan.hooks
        .iter()
        .filter(|hook| hook.enabled)
        .any(|hook| hook.replacement == "stormworks_video_get_texture_upload_hook_arg1")
}

fn hook_plan_uses_monitor_render(plan: &HookPlan) -> bool {
    plan.hooks.iter().filter(|hook| hook.enabled).any(|hook| {
        matches!(
            hook.replacement.as_str(),
            "stormworks_video_get_monitor_render_queue_hook_arg6"
                | "stormworks_video_get_render_queue_alloc_hook_arg1"
                | "stormworks_video_get_render_queue_submit_copy_hook_arg2"
                | "stormworks_video_get_render_target_texture_create_hook_arg3"
                | "stormworks_video_get_renderer_video_pass_hook_arg8"
                | "stormworks_video_get_additive_monitor_bind_hook"
        )
    })
}

fn hook_plan_uses_additive_monitor_bind(plan: &HookPlan) -> bool {
    plan.hooks
        .iter()
        .filter(|hook| hook.enabled)
        .any(|hook| hook.replacement == "stormworks_video_get_additive_monitor_bind_hook")
}

fn hook_plan_uses_experimental_gl_capture(plan: &HookPlan) -> bool {
    plan.hooks.iter().filter(|hook| hook.enabled).any(|hook| {
        matches!(
            hook.replacement.as_str(),
            "stormworks_video_get_render_queue_alloc_hook_arg1"
                | "stormworks_video_get_render_queue_submit_copy_hook_arg2"
                | "stormworks_video_get_render_target_texture_create_hook_arg3"
                | "stormworks_video_get_additive_monitor_bind_hook"
        )
    })
}

fn lua_api_plan_status(
    context: Option<&PluginRuntimeContext>,
    plan: &HookPlan,
) -> serde_json::Value {
    match plan.lua_api.as_ref() {
        Some(lua_api) => match build_lua_api_from_hook_plan(context, lua_api) {
            Ok(_) => serde_json::json!({
                "required": hook_plan_requires_lua_api(plan),
                "configured": true,
                "valid": true,
                "lua_version": lua_api.lua_version
            }),
            Err(error) => serde_json::json!({
                "required": hook_plan_requires_lua_api(plan),
                "configured": true,
                "valid": false,
                "error": error
            }),
        },
        None => serde_json::json!({
            "required": hook_plan_requires_lua_api(plan),
            "configured": false,
            "valid": !hook_plan_requires_lua_api(plan)
        }),
    }
}

fn game_lua_plan_status(
    context: Option<&PluginRuntimeContext>,
    plan: &HookPlan,
) -> serde_json::Value {
    match plan.game_lua.as_ref() {
        Some(game_lua) => match build_game_lua_helpers_from_hook_plan(context, game_lua) {
            Ok(_) => serde_json::json!({
                "required": hook_plan_requires_game_lua(plan),
                "configured": true,
                "valid": true,
                "arg_slot_configured": game_lua.arg_slot.as_deref().map(str::trim).filter(|value| !value.is_empty()).is_some()
            }),
            Err(error) => serde_json::json!({
                "required": hook_plan_requires_game_lua(plan),
                "configured": true,
                "valid": false,
                "error": error
            }),
        },
        None => serde_json::json!({
            "required": hook_plan_requires_game_lua(plan),
            "configured": false,
            "valid": !hook_plan_requires_game_lua(plan)
        }),
    }
}

fn component_context_plan_status(plan: &HookPlan) -> serde_json::Value {
    serde_json::json!({
        "required": hook_plan_uses_component_context(plan),
        "key_format": "component_ptr:<hex>",
        "trampolines": component_context_original_trampoline_status()
    })
}

fn input_video_plan_status(plan: &HookPlan) -> serde_json::Value {
    serde_json::json!({
        "required": hook_plan_uses_input_video(plan),
        "source_handle": "thread_context opaque pointer or Lua script input_video node +0x30 after original",
        "component_context_required": "thread-local component context or registered Lua script input node mapped by node+0x550",
        "slot_binding": "all initialized slots for mapped component until per-slot evidence is accepted",
        "trampolines": input_video_original_trampoline_status()
    })
}

fn texture_source_plan_status(plan: &HookPlan) -> serde_json::Value {
    serde_json::json!({
        "required": hook_plan_uses_texture_source(plan),
        "source_handle": "opaque_pointer_arg",
        "request_match": "VideoGetCaptureRequestV1.input_source_handle",
        "frame_return": "stormworks_video_get_push_rgb_capture_request_direct(component_hash, slot, ...)",
        "trampolines": texture_source_original_trampoline_status()
    })
}

fn texture_upload_plan_status(plan: &HookPlan) -> serde_json::Value {
    serde_json::json!({
        "required": hook_plan_uses_texture_upload(plan),
        "source": "FUN_14020d250 glTexSubImage2D upload context",
        "accepted_formats": [
            "GL_RED/UNSIGNED_BYTE",
            "GL_LUMINANCE/UNSIGNED_BYTE",
            "GL_LUMINANCE_ALPHA/UNSIGNED_BYTE",
            "GL_RG/UNSIGNED_BYTE",
            "GL_RGB/UNSIGNED_BYTE",
            "GL_RGBA/UNSIGNED_BYTE",
            "GL_BGR/UNSIGNED_BYTE",
            "GL_BGRA/UNSIGNED_BYTE"
        ],
        "slot_match": "initialized video slots with matching width and height",
        "trampolines": texture_upload_original_trampoline_status()
    })
}

fn monitor_render_plan_status(plan: &HookPlan) -> serde_json::Value {
    serde_json::json!({
        "required": hook_plan_uses_monitor_render(plan),
        "source": "FUN_140366e90 monitor render queue or FUN_140677a10 additive monitor material bind",
        "monitor_offsets": {
            "active": format!("0x{:x}", MONITOR_ACTIVE_OFFSET),
            "width": format!("0x{:x}", MONITOR_WIDTH_OFFSET),
            "height": format!("0x{:x}", MONITOR_HEIGHT_OFFSET),
            "video_input_slot": format!("0x{:x}", MONITOR_VIDEO_INPUT_SLOT_OFFSET),
            "render_resource_a": format!("0x{:x}", MONITOR_RENDER_RESOURCE_A_OFFSET),
            "render_resource_b": format!("0x{:x}", MONITOR_RENDER_RESOURCE_B_OFFSET)
        },
        "additive_monitor_bind": {
            "required": hook_plan_uses_additive_monitor_bind(plan),
            "stage": "additive_monitor_bind",
            "target": "FUN_140677a10 c_material_additive_monitor bind",
            "texture_video_argument": "param_3",
            "texture_overlay_argument": "param_4",
            "monitor_pointer": "*(param_2 + 0x0), copied from FUN_140366e90 draw item",
            "texture_handle_layouts": [
                "texture_video+0x28",
                "texture_video+0x48",
                "*(texture_video+0x8)+0x28",
                "*(texture_video+0x8)+0x48"
            ],
            "readback": "PBO async readback returns the previous ready frame while scheduling the next one"
        },
        "render_target_texture_create": {
            "target": "FUN_1401afeb0 monitor resource GL texture creation",
            "texture_slot_argument": "param_1",
            "resource_key": "texture_slot - 0x48",
            "binding": "records texture_slot/resource -> GL texture handle; monitor+0x4c8/+0x4d8 wrappers resolve through wrapper+0x8 to this resource"
        },
        "slot_match": "connected Lua video slots whose input source handle has an exact or memory-contained relation to the monitor input ref/effective handle; no single-slot fallback is used",
        "frame_source": "monitor_render",
        "trampolines": {
            "monitor_render_queue": monitor_render_original_trampoline_status(),
            "additive_monitor_bind": additive_monitor_bind_original_trampoline_status()
        }
    })
}

fn build_lua_api_from_hook_plan(
    context: Option<&PluginRuntimeContext>,
    lua_api: &HookPlanLuaApi,
) -> Result<VideoGetLuaApiV1, String> {
    Ok(VideoGetLuaApiV1 {
        size: size_of::<VideoGetLuaApiV1>() as u32,
        lua_version: lua_api.lua_version,
        lua_createtable: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, i32, i32),
        >(
            context,
            "lua_createtable",
            lua_api.lua_createtable.as_deref(),
        )?),
        lua_pushcclosure: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, VideoGetLuaCFunction, i32),
        >(
            context,
            "lua_pushcclosure",
            lua_api.lua_pushcclosure.as_deref(),
        )?),
        lua_setglobal: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, *const c_char),
        >(
            context,
            "lua_setglobal",
            lua_api.lua_setglobal.as_deref(),
        )?),
        lua_setfield: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, i32, *const c_char),
        >(
            context, "lua_setfield", lua_api.lua_setfield.as_deref()
        )?),
        lua_rawseti: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, i32, i64),
        >(
            context, "lua_rawseti", lua_api.lua_rawseti.as_deref()
        )?),
        lua_pushnil: Some(
            resolve_lua_api_function::<unsafe extern "C" fn(*mut c_void)>(
                context,
                "lua_pushnil",
                lua_api.lua_pushnil.as_deref(),
            )?,
        ),
        lua_pushboolean: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, i32),
        >(
            context,
            "lua_pushboolean",
            lua_api.lua_pushboolean.as_deref(),
        )?),
        lua_pushinteger: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, i64),
        >(
            context,
            "lua_pushinteger",
            lua_api.lua_pushinteger.as_deref(),
        )?),
        lua_pushstring: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, *const c_char),
        >(
            context,
            "lua_pushstring",
            lua_api.lua_pushstring.as_deref(),
        )?),
        luaL_checkinteger: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, i32) -> i64,
        >(
            context,
            "luaL_checkinteger",
            lua_api.luaL_checkinteger.as_deref(),
        )?),
        luaL_checkstring: Some(resolve_lua_api_function::<
            unsafe extern "C" fn(*mut c_void, i32) -> *const c_char,
        >(
            context,
            "luaL_checkstring",
            lua_api.luaL_checkstring.as_deref(),
        )?),
        component_id: match lua_api.component_id.as_deref() {
            Some(value) if !value.trim().is_empty() => {
                Some(resolve_lua_api_function::<
                    unsafe extern "C" fn(*mut c_void, *mut c_char, usize) -> usize,
                >(context, "component_id", Some(value))?)
            }
            _ => None,
        },
    })
}

fn resolve_lua_api_function<T>(
    context: Option<&PluginRuntimeContext>,
    name: &str,
    value: Option<&str>,
) -> Result<T, String>
where
    T: Copy,
{
    let raw = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("hook plan lua_api missing {name}"))?;
    let address = resolve_hook_plan_address(context, raw)
        .ok_or_else(|| format!("hook plan lua_api {name} address `{raw}` could not resolve"))?;
    Ok(unsafe { std::mem::transmute_copy::<usize, T>(&(address as usize)) })
}

fn resolve_hook_plan_address(context: Option<&PluginRuntimeContext>, value: &str) -> Option<u64> {
    let parsed = parse_hex_u64_local(value)?;
    if value.trim_start().starts_with("rva:") {
        return current_process_module_base().map(|base| base + parsed);
    }
    let preferred_image_base =
        context.and_then(|context| read_game_image_base(&context.game_exe).ok());
    if let Some(base) = preferred_image_base {
        if parsed >= base {
            if let Some(rva) = parsed.checked_sub(base) {
                return current_process_module_base().map(|runtime_base| runtime_base + rva);
            }
        }
    }
    Some(parsed)
}

fn read_game_image_base(path: &PathBuf) -> Result<u64, String> {
    read_pe_image_base(path)
}

fn current_process_context_value(context: Option<&PluginRuntimeContext>) -> serde_json::Value {
    serde_json::json!({
        "process_id": current_process_id(),
        "context_process_id": context.and_then(|context| context.process_id),
        "process_matches_context": current_process_matches_context(context),
        "process_exe": current_process_exe_path().map(|path| path.display().to_string()),
        "context_game_exe": context.map(|context| context.game_exe.display().to_string()),
        "process_exe_matches_context": context
            .map(|context| current_process_exe_matches_context(&context.game_exe))
            .unwrap_or(false),
        "main_module_base": current_process_module_base().map(hex_u64)
    })
}

fn current_process_matches_context(context: Option<&PluginRuntimeContext>) -> bool {
    let Some(context) = context else {
        return false;
    };
    let pid_matches = match context.process_id {
        Some(pid) => current_process_id() == Some(pid),
        None if context.mode == "replace_dll" => current_process_id().is_some(),
        None => false,
    };
    if !pid_matches {
        return false;
    }
    if context.mode == "replace_dll" {
        return current_process_exe_matches_context(&context.game_exe);
    }
    true
}

fn current_process_exe_matches_context(game_exe: &PathBuf) -> bool {
    let Some(current_exe) = current_process_exe_path() else {
        return false;
    };
    normalized_path_for_compare(&current_exe) == normalized_path_for_compare(game_exe)
}

fn current_process_exe_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

fn normalized_path_for_compare(path: &PathBuf) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.clone())
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn current_process_id() -> Option<u32> {
    Some(unsafe { GetCurrentProcessId() })
}

#[cfg(not(windows))]
fn current_process_id() -> Option<u32> {
    None
}

fn resolve_replacement_symbol(name: &str) -> ReplacementResolution {
    match name {
        "stormworks_video_get_unbound_review_stub" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_unbound_review_stub as *const c_void as u64),
            usable_for_patch: false,
            note: "review stub resolved; not patch-eligible".to_string(),
        },
        "stormworks_video_get_register_lua_api_hook" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_register_lua_api_hook as *const c_void as u64),
            usable_for_patch: true,
            note:
                "registered patch replacement symbol; expects lua_State-compatible first argument"
                    .to_string(),
        },
        "stormworks_video_get_register_lua_api_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_register_lua_api_hook_arg1 as *const c_void as u64),
            usable_for_patch: true,
            note: "registered patch replacement symbol; uses argument 1 as lua_State".to_string(),
        },
        "stormworks_video_get_register_lua_api_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_register_lua_api_hook_arg2 as *const c_void as u64),
            usable_for_patch: true,
            note: "registered patch replacement symbol; uses argument 2 as lua_State".to_string(),
        },
        "stormworks_video_get_register_lua_api_hook_arg3" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_register_lua_api_hook_arg3 as *const c_void as u64),
            usable_for_patch: true,
            note: "registered patch replacement symbol; uses argument 3 as lua_State".to_string(),
        },
        "stormworks_video_get_register_lua_api_hook_arg4" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_register_lua_api_hook_arg4 as *const c_void as u64),
            usable_for_patch: true,
            note: "registered patch replacement symbol; uses argument 4 as lua_State".to_string(),
        },
        "stormworks_video_get_component_lua_init_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_component_lua_init_hook_arg1 as *const c_void as u64),
            usable_for_patch: true,
            note: "component Lua initializer replacement; chains void arg1 context and uses *(arg1+0x8) as the Lua owner".to_string(),
        },
        "stormworks_video_get_component_context_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_component_context_hook_arg1 as *const c_void as u64),
            usable_for_patch: true,
            note: "component-context replacement symbol; uses argument 1 as component pointer"
                .to_string(),
        },
        "stormworks_video_get_component_context_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_component_context_hook_arg2 as *const c_void as u64),
            usable_for_patch: true,
            note: "component-context replacement symbol; uses argument 2 as component pointer"
                .to_string(),
        },
        "stormworks_video_get_component_context_hook_arg3" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_component_context_hook_arg3 as *const c_void as u64),
            usable_for_patch: true,
            note: "component-context replacement symbol; uses argument 3 as component pointer"
                .to_string(),
        },
        "stormworks_video_get_component_context_hook_arg4" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_component_context_hook_arg4 as *const c_void as u64),
            usable_for_patch: true,
            note: "component-context replacement symbol; uses argument 4 as component pointer"
                .to_string(),
        },
        "stormworks_video_get_input_video_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_input_video_hook_arg1 as *const c_void as u64),
            usable_for_patch: true,
            note: "input-video replacement symbol; uses argument 1 as opaque video source pointer"
                .to_string(),
        },
        "stormworks_video_get_input_video_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_input_video_hook_arg2 as *const c_void as u64),
            usable_for_patch: true,
            note: "input-video replacement symbol; uses argument 2 as opaque video source pointer"
                .to_string(),
        },
        "stormworks_video_get_input_video_hook_arg3" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_input_video_hook_arg3 as *const c_void as u64),
            usable_for_patch: true,
            note: "input-video replacement symbol; uses argument 3 as opaque video source pointer"
                .to_string(),
        },
        "stormworks_video_get_input_video_hook_arg4" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_input_video_hook_arg4 as *const c_void as u64),
            usable_for_patch: true,
            note: "input-video replacement symbol; uses argument 4 as opaque video source pointer"
                .to_string(),
        },
        "stormworks_video_get_input_video_node_update_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_input_video_node_update_hook_arg2 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "input-video node update replacement; chains void(node, source_candidate), maps Lua script input_video node to component_lua_context via +0x550, then binds selected_source node+0x28 with resolved_source node+0x30 diagnostics".to_string(),
        },
        "stormworks_video_get_input_video_node_select_hook_arg5" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_input_video_node_select_hook_arg5 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "input-video node selection replacement; chains void(node, source_collection, arg3, arg4, selected_index_ptr), then records node+0x28 selected source and node+0x30 resolved source for Lua script input-video slots".to_string(),
        },
        "stormworks_video_get_video_output_slot_add_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_video_output_slot_add_hook_arg2 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "video output-slot edge-add replacement; chains the original then records a strictly RTTI-validated output_video -> input_video edge".to_string(),
        },
        "stormworks_video_get_video_output_slot_remove_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_video_output_slot_remove_hook_arg2 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "video output-slot edge-remove replacement; chains the original then removes the matching validated graph edge".to_string(),
        },
        "stormworks_video_get_video_output_slot_clear_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_video_output_slot_clear_hook_arg1 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "video output-slot clear replacement; chains the original then removes every cached edge owned by that validated output slot".to_string(),
        },
        "stormworks_video_get_texture_source_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_texture_source_hook_arg1 as *const c_void as u64),
            usable_for_patch: true,
            note: "texture-source replacement symbol; uses argument 1 as opaque source/texture pointer"
                .to_string(),
        },
        "stormworks_video_get_texture_source_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_texture_source_hook_arg2 as *const c_void as u64),
            usable_for_patch: true,
            note: "texture-source replacement symbol; uses argument 2 as opaque source/texture pointer"
                .to_string(),
        },
        "stormworks_video_get_texture_source_hook_arg3" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_texture_source_hook_arg3 as *const c_void as u64),
            usable_for_patch: true,
            note: "texture-source replacement symbol; uses argument 3 as opaque source/texture pointer"
                .to_string(),
        },
        "stormworks_video_get_texture_source_hook_arg4" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_texture_source_hook_arg4 as *const c_void as u64),
            usable_for_patch: true,
            note: "texture-source replacement symbol; uses argument 4 as opaque source/texture pointer"
                .to_string(),
        },
        "stormworks_video_get_texture_upload_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_texture_upload_hook_arg1 as *const c_void as u64),
            usable_for_patch: true,
            note: "texture-upload replacement symbol; chains FUN_14020d250 and copies the CPU-side GL texture upload buffer"
                .to_string(),
        },
        "stormworks_video_get_monitor_render_queue_hook_arg6" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_monitor_render_queue_hook_arg6 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "monitor render queue replacement; chains FUN_140366e90 and probes monitor render resources at +0x4c8/+0x4d8 for matching connected Lua video slots".to_string(),
        },
        "stormworks_video_get_render_queue_alloc_hook_arg1" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_render_queue_alloc_hook_arg1 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "render queue allocator replacement; chains FUN_1408ca480, preserves the returned queue item pointer, and records monitor queue items only while a monitor/submission context is active".to_string(),
        },
        "stormworks_video_get_render_queue_submit_copy_hook_arg2" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_render_queue_submit_copy_hook_arg2 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "render queue submit-copy replacement; chains FUN_140673fb0 and records monitor-shaped 0x180 queue items copied into the render-context queue".to_string(),
        },
        "stormworks_video_get_render_target_texture_create_hook_arg3" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_render_target_texture_create_hook_arg3 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "monitor resource texture create replacement; chains FUN_1401afeb0 and records texture_slot/resource GL texture bindings for monitor render resources".to_string(),
        },
        "stormworks_video_get_renderer_video_pass_hook_arg8" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_renderer_video_pass_hook_arg8 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "renderer video pass replacement; chains FUN_1406d1960 and records the pass context used by c_material_additive_monitor binds".to_string(),
        },
        "stormworks_video_get_additive_monitor_bind_hook" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_additive_monitor_bind_hook as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "additive monitor material bind replacement; chains FUN_140677a10, maps draw_item->monitor, and PBO-readbacks the explicit texture_video argument for matching Lua video slots".to_string(),
        },
        "stormworks_video_get_additive_monitor_video_bind_hook_arg3" => ReplacementResolution {
            name: name.to_string(),
            address: Some(
                stormworks_video_get_additive_monitor_video_bind_hook_arg3 as *const c_void as u64,
            ),
            usable_for_patch: true,
            note: "additive_monitor texture bind replacement (FUN_140688ec0); chains the original, reads the texture_video GL id from arg3+0x48, and glGetTexImage-readbacks it into connected Lua video slots".to_string(),
        },
        #[cfg(test)]
        "stormworks_video_get_test_noarg_detour_hook" => ReplacementResolution {
            name: name.to_string(),
            address: Some(stormworks_video_get_test_noarg_detour_hook as *const c_void as u64),
            usable_for_patch: true,
            note: "test-only no-arg replacement symbol".to_string(),
        },
        _ => ReplacementResolution {
            name: name.to_string(),
            address: None,
            usable_for_patch: false,
            note: "unknown replacement symbol".to_string(),
        },
    }
}

impl ReplacementResolution {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "resolved": self.address.is_some(),
            "address": self.address.map(hex_u64),
            "usable_for_patch": self.usable_for_patch,
            "note": self.note
        })
    }
}

fn evaluate_target_patch_gate(
    config: &VideoGetConfig,
    plan: &HookPlan,
    validation: &serde_json::Value,
    install_dry_run: &serde_json::Value,
) -> serde_json::Value {
    let enabled_hooks = plan
        .hooks
        .iter()
        .filter(|hook| hook.enabled)
        .collect::<Vec<_>>();
    let required_stage = plan
        .required_stage
        .as_deref()
        .unwrap_or("lua_api_registration");
    let required_stage_accepted = plan
        .accepted_stages
        .iter()
        .any(|stage| stage == required_stage);
    let mut blockers = Vec::new();
    if !config.hooking.allow_target_patches {
        blockers.push("hooking.allow_target_patches=false");
    }
    if config.hooking.require_gate_for_target_patches && !plan.patching_allowed {
        blockers.push("hook_plan.patching_allowed=false");
    }
    if config.hooking.require_gate_for_target_patches && !required_stage_accepted {
        blockers.push("required_stage_not_accepted");
    }
    if plan.dry_run_only {
        blockers.push("hook_plan.dry_run_only=true");
    }
    if enabled_hooks.is_empty() {
        blockers.push("no_enabled_hooks");
    }
    if !validation
        .get("valid")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        blockers.push("hook_plan_validation_failed");
    }
    let enabled_hook_blocked_by_dry_run = install_dry_run
        .get("hooks")
        .and_then(|value| value.as_array())
        .map(|hooks| {
            hooks.iter().any(|hook| {
                hook.get("enabled")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
                    && !hook
                        .get("can_install_if_gate_opens")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if enabled_hook_blocked_by_dry_run {
        blockers.push("hook_install_dry_run_failed");
    }
    let lua_api_invalid = install_dry_run
        .get("lua_api")
        .map(|value| {
            value
                .get("required")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                && !value
                    .get("valid")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    if lua_api_invalid {
        blockers.push("hook_plan.lua_api_missing_or_invalid");
    }
    let game_lua_invalid = install_dry_run
        .get("game_lua")
        .map(|value| {
            value
                .get("required")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
                && !value
                    .get("valid")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    if game_lua_invalid {
        blockers.push("hook_plan.game_lua_missing_or_invalid");
    }
    let can_patch = blockers.is_empty();
    serde_json::json!({
        "can_patch": can_patch,
        "target_patch_points_modified": false,
        "required_stage": required_stage,
        "required_stage_accepted": required_stage_accepted,
        "enabled_hook_count": enabled_hooks.len(),
        "blockers": blockers,
        "install_dry_run": install_dry_run,
        "plan": hook_plan_summary_value(plan, validation)
    })
}

fn install_hook_plan_detours(
    context: Option<&PluginRuntimeContext>,
    plan: &HookPlan,
    symbols: &serde_json::Value,
    validation: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !validation
        .get("valid")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return Err("hook plan validation failed".to_string());
    }
    if !current_process_matches_context(context) {
        return Err("current process does not match runtime context process_id".to_string());
    }

    let lua_api_status = lua_api_plan_status(context, plan);
    if hook_plan_requires_lua_api(plan) {
        let lua_api_plan = plan.lua_api.as_ref().ok_or_else(|| {
            "enabled Lua registration hook requires hook plan lua_api".to_string()
        })?;
        let lua_api = build_lua_api_from_hook_plan(context, lua_api_plan)?;
        set_lua_hook_api(&lua_api)?;
    }
    let game_lua_status = game_lua_plan_status(context, plan);
    if hook_plan_requires_game_lua(plan) {
        let game_lua_plan = plan.game_lua.as_ref().ok_or_else(|| {
            "enabled component Lua init hook requires hook plan game_lua".to_string()
        })?;
        let helpers = build_game_lua_helpers_from_hook_plan(context, game_lua_plan)?;
        set_game_lua_helpers(helpers)?;
    }

    let mut hooks = Vec::new();
    let mut installed_count = 0usize;
    for hook in plan.hooks.iter().filter(|hook| hook.enabled) {
        if !hook_signature_match(hook, symbols) {
            return Err(format!(
                "hook {} target {} no longer matches signature stage {}",
                hook.label, hook.target_va, hook.stage
            ));
        }
        let target = resolve_hook_target_address(context, hook);
        let Some(runtime_address) = target.runtime_address else {
            return Err(format!(
                "hook {} target {} could not be resolved to a runtime address",
                hook.label, hook.target_va
            ));
        };
        let replacement = resolve_replacement_symbol(&hook.replacement);
        if !replacement.usable_for_patch {
            return Err(format!(
                "hook {} replacement {} is not patch-eligible",
                hook.label, hook.replacement
            ));
        }
        let Some(replacement_address) = replacement.address else {
            return Err(format!(
                "hook {} replacement {} did not resolve",
                hook.label, hook.replacement
            ));
        };
        let patch_len = hook.patch_len.unwrap_or_else(absolute_jump_patch_len);
        let trampoline = if hook.require_trampoline {
            install_absolute_jump_detour_with_trampoline_len(
                &hook.label,
                runtime_address as *mut c_void,
                replacement_address as *const c_void,
                patch_len,
            )
            .map(|ptr| Some(ptr as u64))?
        } else {
            install_absolute_jump_detour_len(
                &hook.label,
                runtime_address as *mut c_void,
                replacement_address as *const c_void,
                patch_len,
            )?;
            None
        };
        set_lua_registration_original_trampoline(&hook.replacement, trampoline);
        set_component_context_original_trampoline(&hook.replacement, trampoline);
        set_input_video_original_trampoline(&hook.replacement, trampoline);
        set_texture_source_original_trampoline(&hook.replacement, trampoline);
        set_texture_upload_original_trampoline(&hook.replacement, trampoline);
        set_monitor_render_queue_original_trampoline(&hook.replacement, trampoline);
        set_render_queue_alloc_original_trampoline(&hook.replacement, trampoline);
        set_render_queue_submit_copy_original_trampoline(&hook.replacement, trampoline);
        set_render_target_texture_create_original_trampoline(&hook.replacement, trampoline);
        set_renderer_video_pass_original_trampoline(&hook.replacement, trampoline);
        set_additive_monitor_bind_original_trampoline(&hook.replacement, trampoline);
        installed_count += 1;
        hooks.push(serde_json::json!({
            "label": hook.label,
            "stage": hook.stage,
            "target_va": hook.target_va,
            "runtime_address": hex_u64(runtime_address),
            "replacement": replacement.to_json(),
            "require_trampoline": hook.require_trampoline,
            "patch_len": patch_len,
            "trampoline": trampoline.map(hex_u64),
            "installed": true
        }));
    }
    let gl_iat_hooks = if hook_plan_uses_experimental_gl_capture(plan) {
        Some(install_gl_render_iat_hooks(context)?)
    } else {
        None
    };

    Ok(serde_json::json!({
        "attempted": true,
        "installed_count": installed_count,
        "target_patch_points_modified": installed_count > 0,
        "lua_api": lua_api_status,
        "game_lua": game_lua_status,
        "component_context": component_context_plan_status(plan),
        "input_video": input_video_plan_status(plan),
        "texture_source": texture_source_plan_status(plan),
        "texture_upload": texture_upload_plan_status(plan),
        "monitor_render": monitor_render_plan_status(plan),
        "gl_iat_hooks": gl_iat_hooks,
        "hooks": hooks,
        "detours": detour_status_value()
    }))
}

#[cfg(windows)]
fn install_gl_render_iat_hooks(
    context: Option<&PluginRuntimeContext>,
) -> Result<serde_json::Value, String> {
    let wgl = install_import_iat_hook(
        context,
        "wglGetProcAddress",
        STORMWORKS_WGL_GET_PROC_ADDRESS_IAT_VA,
        &WGL_GET_PROC_ADDRESS_IAT_INSTALLED,
        &WGL_GET_PROC_ADDRESS_ORIGINAL,
        stormworks_video_get_wgl_get_proc_address_hook as *const () as usize,
    )?;
    let bind_texture = install_import_iat_hook(
        context,
        "glBindTexture",
        STORMWORKS_GL_BIND_TEXTURE_IAT_VA,
        &GL_BIND_TEXTURE_IAT_INSTALLED,
        &GL_BIND_TEXTURE_ORIGINAL,
        stormworks_video_get_gl_bind_texture_hook as *const () as usize,
    )?;
    Ok(serde_json::json!({
        "installed": true,
        "wgl_get_proc_address": wgl,
        "gl_bind_texture": bind_texture,
        "dynamic_wrappers": [
            "glBindTextureUnit",
            "glBindTextures",
            "glFramebufferTexture2D",
            "glFramebufferTexture",
            "glFramebufferTextureLayer"
        ]
    }))
}

#[cfg(windows)]
fn install_import_iat_hook(
    context: Option<&PluginRuntimeContext>,
    name: &'static str,
    iat_va: u64,
    installed: &AtomicBool,
    original_storage: &AtomicUsize,
    hook: usize,
) -> Result<serde_json::Value, String> {
    if installed.load(Ordering::SeqCst) {
        return Ok(serde_json::json!({
            "installed": true,
            "already_installed": true,
            "iat_va": hex_u64(iat_va),
            "original": hex_u64(original_storage.load(Ordering::SeqCst) as u64)
        }));
    }
    if !current_process_matches_context(context) {
        return Err(format!(
            "current process does not match runtime context for {name} IAT hook"
        ));
    }
    let preferred_image_base = context
        .and_then(|context| read_game_image_base(&context.game_exe).ok())
        .unwrap_or(STORMWORKS_IMAGE_BASE);
    let rva = iat_va
        .checked_sub(preferred_image_base)
        .or_else(|| iat_va.checked_sub(STORMWORKS_IMAGE_BASE))
        .ok_or_else(|| format!("{name} IAT VA is below preferred image base"))?;
    let runtime_base = current_process_module_base()
        .ok_or_else(|| "could not resolve current process module base".to_string())?;
    let slot_address = runtime_base
        .checked_add(rva)
        .ok_or_else(|| format!("{name} IAT runtime address overflow"))?
        as *mut usize;
    if !memory_range_is_readable(slot_address.cast::<c_void>(), size_of::<usize>()) {
        return Err(format!(
            "{name} IAT slot {} is not readable",
            hex_u64(slot_address as u64)
        ));
    }
    let original = unsafe { ptr::read(slot_address) };
    if original == hook {
        installed.store(true, Ordering::SeqCst);
        return Ok(serde_json::json!({
            "installed": true,
            "already_patched": true,
            "iat_va": hex_u64(iat_va),
            "slot_runtime": hex_u64(slot_address as u64),
            "original": hex_u64(original_storage.load(Ordering::SeqCst) as u64)
        }));
    }
    if original == 0 {
        return Err(format!("{name} IAT original pointer is null"));
    }
    original_storage.store(original, Ordering::SeqCst);
    write_pointer_memory(slot_address, hook)?;
    installed.store(true, Ordering::SeqCst);
    if let Ok(state) = request_runtime_state() {
        log_runtime_diagnostic(
            &state,
            &format!(
                "{} IAT hook installed iat_va={} slot_runtime={} original={} hook={}",
                name,
                hex_u64(iat_va),
                hex_u64(slot_address as u64),
                hex_u64(original as u64),
                hex_u64(hook as u64)
            ),
            &ADDITIVE_GL_BIND_DIAGNOSTIC_COUNT,
            96,
        );
    }
    Ok(serde_json::json!({
        "installed": true,
        "iat_va": hex_u64(iat_va),
        "preferred_image_base": hex_u64(preferred_image_base),
        "rva": hex_u64(rva),
        "runtime_module_base": hex_u64(runtime_base),
        "slot_runtime": hex_u64(slot_address as u64),
        "original": hex_u64(original as u64),
        "hook": hex_u64(hook as u64)
    }))
}

#[cfg(not(windows))]
fn install_gl_render_iat_hooks(
    _context: Option<&PluginRuntimeContext>,
) -> Result<serde_json::Value, String> {
    Err("GL render IAT hooks are only available on Windows".to_string())
}

fn observation_plan_from_symbols(symbols: &serde_json::Value) -> serde_json::Value {
    let mut stages = serde_json::Map::new();
    let mut watchlist = Vec::new();
    if let Some(object) = symbols.as_object() {
        let mut groups = object.iter().collect::<Vec<_>>();
        groups.sort_by_key(|(stage, _)| {
            observation_stage_spec(stage)
                .get("priority")
                .and_then(|value| value.as_u64())
                .unwrap_or(u64::MAX)
        });
        for (stage, group) in groups {
            let spec = observation_stage_spec(stage);
            let values = group
                .get("value")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            let candidates = values
                .iter()
                .enumerate()
                .map(|(index, candidate)| {
                    let byte_check = candidate.get("byte_check").cloned().unwrap_or_default();
                    serde_json::json!({
                        "order": index + 1,
                        "entry": candidate.get("entry").and_then(|value| value.as_str()),
                        "va": byte_check.get("va").and_then(|value| value.as_str())
                            .or_else(|| candidate.get("entry").and_then(|value| value.as_str())),
                        "byte_check_size": byte_check.get("size").and_then(|value| value.as_u64()),
                        "expected_bytes": byte_check.get("bytes").and_then(|value| value.as_str()),
                        "reason": candidate.get("reason").and_then(|value| value.as_str()),
                        "observation_kind": "manual_breakpoint_or_runtime_logger",
                        "patching_allowed": false
                    })
                })
                .collect::<Vec<_>>();
            for candidate in &candidates {
                let mut candidate = candidate.clone();
                if let Some(object) = candidate.as_object_mut() {
                    object.insert("stage".to_string(), serde_json::json!(stage));
                    object.insert("priority".to_string(), spec["priority"].clone());
                }
                watchlist.push(candidate);
            }
            stages.insert(
                stage.to_string(),
                serde_json::json!({
                    "priority": spec["priority"].clone(),
                    "phase": spec["phase"].clone(),
                    "questions": spec["questions"].clone(),
                    "record_fields": spec["record_fields"].clone(),
                    "acceptance": spec["acceptance"].clone(),
                    "candidate_count": candidates.len(),
                    "candidates": candidates,
                    "patching_allowed": false
                }),
            );
        }
    }

    serde_json::json!({
        "mode": "runtime_observation_only",
        "candidate_only": true,
        "patching_allowed_by_this_plan": false,
        "starts_game": false,
        "attaches_to_process": false,
        "writes_target_memory": false,
        "stage_count": stages.len(),
        "candidate_count": observation_candidate_count(symbols),
        "next_gate": "collect runtime observations before implementing hooks",
        "stages": stages,
        "candidate_watchlist": watchlist
    })
}

fn observation_stage_spec(stage: &str) -> serde_json::Value {
    match stage {
        "lua_api_registration" => serde_json::json!({
            "priority": 1,
            "phase": "startup/component Lua API registration",
            "questions": [
                "Does the candidate execute when component Lua APIs are registered?",
                "Which argument or stack location appears to hold lua_State or an API registration table?",
                "Which bridge replacement matches the observed Lua owner position: direct, arg1, arg1+0x8, arg2, arg3, or arg4?",
                "Does the call happen once globally or per component Lua VM?",
                "Can a read-only observer distinguish component Lua from addon/server Lua?"
            ],
            "record_fields": [
                "timestamp",
                "thread_id",
                "hit_count",
                "rcx",
                "rdx",
                "r8",
                "r9",
                "rsp",
                "return_address",
                "lua_state_argument",
                "recommended_replacement",
                "lua_api",
                "nearby_ascii_or_utf8_strings"
            ],
            "acceptance": [
                "Candidate hit correlates with Lua API setup, not unrelated UI/help text only.",
                "A stable lua_State or registration owner can be inferred without modifying process state."
            ]
        }),
        "current_lua_component_context" => serde_json::json!({
            "priority": 2,
            "phase": "component Lua onTick/onDraw execution",
            "questions": [
                "Does the candidate execute during component onTick or onDraw?",
                "Which pointer remains stable for the current microprocessor/component instance?",
                "Which bridge replacement matches the observed component pointer position: arg1, arg2, arg3, or arg4?",
                "Can the observer separate different Lua components on the same vehicle?",
                "Does the candidate expose composite input/output ownership or script filename context?"
            ],
            "record_fields": [
                "timestamp",
                "thread_id",
                "hit_count",
                "rcx",
                "rdx",
                "r8",
                "r9",
                "rsp",
                "return_address",
                "component_argument",
                "component_pointer_guess",
                "recommended_replacement",
                "script_path_or_lua_filename"
            ],
            "acceptance": [
                "A component-scoped identity can be observed across multiple ticks.",
                "Two different Lua components produce distinguishable context identities."
            ]
        }),
        "microprocessor_input_video_node" => serde_json::json!({
            "priority": 3,
            "phase": "microprocessor logic node serialization/update",
            "questions": [
                "Does the candidate execute when a microprocessor has a Video Input node?",
                "Do connected and disconnected video inputs change observed fields or branch paths?",
                "Can slot index and type 6 connection ownership be inferred read-only?",
                "Which bridge replacement matches the observed source pointer position: arg1, arg2, arg3, or arg4?",
                "Is the candidate part of serialization only, or also active logic update?"
            ],
            "record_fields": [
                "timestamp",
                "thread_id",
                "hit_count",
                "rcx",
                "rdx",
                "r8",
                "r9",
                "rsp",
                "return_address",
                "input_video_string_reference",
                "possible_slot_index",
                "possible_connection_pointer",
                "source_argument",
                "recommended_replacement"
            ],
            "acceptance": [
                "Connected/disconnected video input produces a repeatable observation difference.",
                "The observer can identify whether the candidate is update-time or save/load-time only."
            ]
        }),
        "video_texture_source" => serde_json::json!({
            "priority": 4,
            "phase": "video texture registration/render source",
            "questions": [
                "Does the candidate execute on the render thread or resource setup thread?",
                "Does it correlate with texture_video usage for monitors or video sources?",
                "Can source texture/FBO ownership be inferred without GL calls from Lua thread?",
                "Which bridge replacement matches the observed source/texture pointer position: arg1, arg2, arg3, or arg4?",
                "Is there a safe render-thread handoff point for future readback?"
            ],
            "record_fields": [
                "timestamp",
                "thread_id",
                "hit_count",
                "rcx",
                "rdx",
                "r8",
                "r9",
                "rsp",
                "return_address",
                "texture_video_string_reference",
                "possible_texture_or_resource_pointer",
                "source_argument",
                "recommended_replacement"
            ],
            "acceptance": [
                "The candidate can be correlated with video texture setup or sampling.",
                "The observer can distinguish render-thread work from Lua VM execution."
            ]
        }),
        "monitor_render_queue" => serde_json::json!({
            "priority": 5,
            "phase": "monitor render queue / camera video readback",
            "questions": [
                "Does the candidate execute when a monitor receives a vehicle video signal?",
                "Can monitor width/height and render resources at monitor+0x4c8 or monitor+0x4d8 be read consistently?",
                "Do GL texture readbacks from those resources match the camera component connected to the monitor?",
                "Can the monitor input slot be mapped back to the Lua component video slot without falling back to the only connected Lua slot?"
            ],
            "record_fields": [
                "timestamp",
                "thread_id",
                "hit_count",
                "monitor",
                "render_context",
                "active_flag",
                "input_slot_handle",
                "width",
                "height",
                "resource_a",
                "resource_b",
                "candidate_texture_handles",
                "updated_slots",
                "source_stats"
            ],
            "acceptance": [
                "The hook fires only after monitor render queue work and before Lua needs the frame.",
                "A nonblank readback reaches the intended connected Lua video slot without using arbitrary source-object texture scans."
            ]
        }),
        _ => serde_json::json!({
            "priority": 99,
            "phase": "unknown candidate stage",
            "questions": [],
            "record_fields": [
                "timestamp",
                "thread_id",
                "hit_count",
                "rcx",
                "rdx",
                "r8",
                "r9",
                "rsp",
                "return_address"
            ],
            "acceptance": []
        }),
    }
}

fn with_runtime<F>(action: F) -> Result<serde_json::Value, String>
where
    F: FnOnce(&RuntimeState) -> Result<serde_json::Value, String>,
{
    let state = request_runtime_state()?;
    action(&state)
}

fn request_runtime_state() -> Result<RuntimeState, String> {
    let mutex = runtime_cell();
    let state = mutex
        .lock()
        .map_err(|_| "runtime mutex poisoned".to_string())?;
    if !state.configured {
        return Err("video_get runtime is not configured".to_string());
    }
    Ok(state.clone())
}

fn runtime_snapshot() -> RuntimeState {
    runtime_cell()
        .lock()
        .map(|state| state.clone())
        .unwrap_or_else(|_| {
            let mut state = default_runtime_state();
            state.last_error = Some("runtime mutex poisoned".to_string());
            state
        })
}

fn set_runtime(state: RuntimeState) {
    if let Ok(mut guard) = runtime_cell().lock() {
        *guard = state;
    }
}

fn set_last_error(error: String) {
    if let Ok(mut state) = runtime_cell().lock() {
        state.last_error = Some(error.clone());
        if let Some(path) = &state.log_path {
            let _ = append_log(path, &format!("error: {error}"));
        }
    }
}

fn runtime_cell() -> &'static Mutex<RuntimeState> {
    RUNTIME.get_or_init(|| Mutex::new(default_runtime_state()))
}

fn default_runtime_state() -> RuntimeState {
    RuntimeState {
        configured: false,
        context: None,
        config: default_video_get_config(),
        hook_runtime: default_hook_runtime_state(),
        signatures_loaded: false,
        signature_symbol_count: 0,
        signature_keys: Vec::new(),
        signature_symbols: serde_json::json!({}),
        signature_summary: serde_json::json!({}),
        byte_check_summary: ByteCheckSummary {
            checked: 0,
            verified: 0,
            failed: 0,
            failures: Vec::new(),
        },
        hook_plan: Some(default_hook_plan()),
        hook_plan_path: None,
        slots: BTreeMap::new(),
        latest_texture_upload_frame: None,
        gl_texture_bindings: BTreeMap::new(),
        video_node_sources: BTreeMap::new(),
        video_source_components: BTreeMap::new(),
        monitor_pbo_readbacks: BTreeMap::new(),
        monitor_gl_bind_events: Vec::new(),
        renderer_video_pass_events: Vec::new(),
        pending_monitor_render_probes: Vec::new(),
        last_error: None,
        log_path: None,
        load_event_path: None,
        runtime_snapshot_path: None,
        runtime_snapshot_jsonl_path: None,
    }
}

fn default_hook_runtime_state() -> HookRuntimeState {
    HookRuntimeState {
        install_attempted: false,
        runtime_active: false,
        detour_engine_ready: detour_engine_available(),
        installed_detour_count: 0,
        lua_registration_adapter: true,
        lua_api_registered: false,
        game_lua_callback_calls: 0,
        game_lua_last_callback: None,
        game_lua_last_component: None,
        mock_frame_pump_active: false,
        real_lua_hook: false,
        real_video_capture: false,
        input_video_bridge_updates: 0,
        texture_source_bridge_frames: 0,
        texture_upload_bridge_frames: 0,
        texture_upload_skipped_bound_slots: 0,
        texture_upload_skipped_small_unbound_slots: 0,
        texture_upload_skipped_fps_slots: 0,
        texture_upload_auto_bound_slots: 0,
        monitor_render_attempts: 0,
        monitor_render_candidates: 0,
        monitor_render_blank_reads: 0,
        monitor_render_read_errors: 0,
        monitor_render_frames: 0,
        monitor_render_skipped_fps_slots: 0,
        additive_monitor_bind_attempts: 0,
        additive_monitor_bind_candidates: 0,
        additive_monitor_bind_blank_reads: 0,
        additive_monitor_bind_read_errors: 0,
        additive_monitor_bind_frames: 0,
        additive_monitor_bind_skipped_fps_slots: 0,
        source_texture_probe_attempts: 0,
        source_texture_probe_candidates: 0,
        source_texture_probe_read_errors: 0,
        source_texture_probe_blank_reads: 0,
        source_texture_probe_frames: 0,
        source_texture_probe_skipped_fps_slots: 0,
        installed_by_mode: None,
        last_install_error: None,
    }
}

fn synthetic_gray_matrix(width: u32, height: u32) -> Vec<Vec<PixelGray>> {
    (1..=height)
        .map(|y| {
            (1..=width)
                .map(|x| PixelGray {
                    x,
                    y,
                    gray: (((x * 3 + y * 5) % 256) as u8),
                })
                .collect()
        })
        .collect()
}

fn synthetic_rgb_matrix(width: u32, height: u32) -> Vec<Vec<PixelRgb>> {
    (1..=height)
        .map(|y| {
            (1..=width)
                .map(|x| PixelRgb {
                    x,
                    y,
                    rgb: [
                        ((x * 5) % 256) as u8,
                        ((y * 7) % 256) as u8,
                        (((x + y) * 3) % 256) as u8,
                    ],
                })
                .collect()
        })
        .collect()
}

fn gray_matrix_from_frame(frame: &FrameBuffer) -> Vec<Vec<PixelGray>> {
    (0..frame.height)
        .map(|y| {
            (0..frame.width)
                .map(|x| {
                    let rgb = frame.rgb[(y * frame.width + x) as usize];
                    PixelGray {
                        x: x + 1,
                        y: y + 1,
                        gray: luma(rgb),
                    }
                })
                .collect()
        })
        .collect()
}

fn rgb_matrix_from_frame(frame: &FrameBuffer) -> Vec<Vec<PixelRgb>> {
    (0..frame.height)
        .map(|y| {
            (0..frame.width)
                .map(|x| PixelRgb {
                    x: x + 1,
                    y: y + 1,
                    rgb: frame.rgb[(y * frame.width + x) as usize],
                })
                .collect()
        })
        .collect()
}

fn packed_gray_bytes_from_frame(frame: &FrameBuffer) -> Vec<u8> {
    frame.rgb.iter().map(|rgb| luma(*rgb)).collect()
}

fn packed_rgb_bytes_from_frame(frame: &FrameBuffer) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(frame.rgb.len() * 3);
    for rgb in &frame.rgb {
        bytes.extend_from_slice(rgb);
    }
    bytes
}

fn luma(rgb: [u8; 3]) -> u8 {
    let value = 77u16 * rgb[0] as u16 + 150u16 * rgb[1] as u16 + 29u16 * rgb[2] as u16;
    (value >> 8) as u8
}

fn json_result(value: Result<serde_json::Value, String>) -> *mut c_char {
    let payload = match value {
        Ok(value) => serde_json::json!({ "ok": true, "value": value }),
        Err(error) => serde_json::json!({ "ok": false, "error": error }),
    };
    match CString::new(payload.to_string()) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

fn unsafe_cstr(value: *const c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(value).to_str().ok().map(str::to_string) }
}

unsafe fn wide_ptr_to_path(value: *const u16) -> PathBuf {
    let mut len = 0usize;
    while *value.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(value, len);
    PathBuf::from(String::from_utf16_lossy(slice))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_runtime_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn gray_matrix_is_y_then_x() {
        let frame = synthetic_gray_matrix(2, 3);
        assert_eq!(frame.len(), 3);
        assert_eq!(frame[0].len(), 2);
        assert_eq!(frame[2][1].x, 2);
        assert_eq!(frame[2][1].y, 3);
    }

    #[test]
    fn frame_limits_reject_oversized_requests() {
        let error = validate_frame_size(161, 90, 160, 90, "gray").unwrap_err();
        assert!(error.contains("exceeds configured limit"));
    }

    #[test]
    fn signature_keys_are_sorted() {
        let value = serde_json::json!({
            "microprocessor_input_video_node": {},
            "lua_api_registration": {},
            "current_lua_component_context": {}
        });
        assert_eq!(
            signature_keys(&value),
            vec![
                "current_lua_component_context",
                "lua_api_registration",
                "microprocessor_input_video_node"
            ]
        );
    }

    #[test]
    fn signature_summary_counts_candidates() {
        let value = serde_json::json!({
            "microprocessor_input_video_node": {
                "kind": "candidate_functions",
                "value": [{"entry": "1"}, {"entry": "2"}]
            }
        });
        assert_eq!(
            signature_summary(&value)
                .get("microprocessor_input_video_node")
                .and_then(|value| value.get("candidate_count"))
                .and_then(|value| value.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn gl_bind_probe_context_is_scoped_to_render_hooks() {
        #[cfg(windows)]
        {
            let _guard = test_runtime_lock();
            assert!(!gl_bind_probe_context_active());
            push_monitor_render_gl_bind_context(MonitorRenderGlBindFrame {
                monitor: 0x1000,
                render_context: 0x2000,
                arg3: 0,
                arg4: 0,
                arg5: 0,
                arg6: 0,
                bind_index: 0,
            });
            assert!(gl_bind_probe_context_active());
            pop_monitor_render_gl_bind_context();
            assert!(!gl_bind_probe_context_active());

            push_additive_monitor_gl_bind_context(AdditiveMonitorGlBindFrame {
                material: 0,
                draw_item: 0,
                texture_video: 0,
                texture_overlay: 0,
                video_texture_object: 0,
                arg6: 0,
                arg7: 0,
                arg8: 0,
                arg10: 0,
                arg13: 0,
                arg16: 0,
                renderer_pass: None,
                bind_index: 0,
            });
            assert!(gl_bind_probe_context_active());
            pop_additive_monitor_gl_bind_context();
            assert!(!gl_bind_probe_context_active());
        }
    }

    #[test]
    fn observation_plan_is_candidate_only() {
        let value = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [
                    {
                        "entry": "1402e6f00",
                        "reason": "test",
                        "byte_check": {
                            "va": "1402e6f00",
                            "size": 16,
                            "bytes": "48 89"
                        }
                    }
                ]
            }
        });
        let plan = observation_plan_from_symbols(&value);
        assert_eq!(
            plan.get("candidate_count").and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            plan.get("patching_allowed_by_this_plan")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            plan.pointer("/stages/lua_api_registration/candidates/0/va")
                .and_then(|value| value.as_str()),
            Some("1402e6f00")
        );
        let record_fields = plan
            .pointer("/stages/lua_api_registration/record_fields")
            .and_then(|value| value.as_array())
            .unwrap();
        assert!(record_fields
            .iter()
            .any(|value| value.as_str() == Some("lua_state_argument")));
        assert!(record_fields
            .iter()
            .any(|value| value.as_str() == Some("recommended_replacement")));
        assert!(record_fields
            .iter()
            .any(|value| value.as_str() == Some("lua_api")));
    }

    #[test]
    fn load_event_jsonl_records_manager_owned_policy() {
        let path = std::env::temp_dir().join(format!(
            "stormworks_video_get_load_event_test_{}_{}.jsonl",
            std::process::id(),
            FRAME_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let value = serde_json::json!({
            "event": "video_get_configured",
            "policy": {
                "manual_dll_injection_required": false,
                "patching_allowed": false,
                "writes_stormworks_directory": false
            }
        });
        append_jsonl(&path, &value).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(
            parsed.get("event").and_then(|value| value.as_str()),
            Some("video_get_configured")
        );
        assert_eq!(
            parsed
                .pointer("/policy/manual_dll_injection_required")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            parsed
                .pointer("/policy/patching_allowed")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn init_respects_slot_limit() {
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.limits.max_active_slots = 1;
        let init = VideoInit {
            slot: 2,
            width: 4,
            height: 4,
            mode: "gray".to_string(),
            component: None,
        };
        let error = validate_init(&init, &state).unwrap_err();
        assert!(error.contains("max_active_slots"));
    }

    #[test]
    fn init_accepts_configured_gray_and_rgb_limits() {
        let mut state = default_runtime_state();
        state.configured = true;

        let gray = VideoInit {
            slot: 1,
            width: state.config.limits.gray.max_width,
            height: state.config.limits.gray.max_height,
            mode: "gray".to_string(),
            component: None,
        };
        validate_init(&gray, &state).unwrap();

        let rgb = VideoInit {
            slot: 2,
            width: state.config.limits.rgb.max_width,
            height: state.config.limits.rgb.max_height,
            mode: "rgb".to_string(),
            component: None,
        };
        validate_init(&rgb, &state).unwrap();
    }

    #[test]
    fn init_rejects_wrong_mode_or_oversized_rgb() {
        let mut state = default_runtime_state();
        state.configured = true;

        let wrong_mode = VideoInit {
            slot: 1,
            width: 4,
            height: 4,
            mode: "color".to_string(),
            component: None,
        };
        let error = validate_init(&wrong_mode, &state).unwrap_err();
        assert!(error.contains("mode must be gray or rgb"));

        let oversized_rgb = VideoInit {
            slot: 1,
            width: state.config.limits.rgb.max_width + 1,
            height: state.config.limits.rgb.max_height,
            mode: "rgb".to_string(),
            component: None,
        };
        let error = validate_init(&oversized_rgb, &state).unwrap_err();
        assert!(error.contains("exceeds configured limit"));
    }

    #[test]
    fn pushed_rgb_frame_exports_rgb_and_gray() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        state.slots.insert(
            slot_key(DEFAULT_COMPONENT, 1),
            SlotState {
                component: DEFAULT_COMPONENT.to_string(),
                slot: 1,
                width: 2,
                height: 1,
                mode: "rgb".to_string(),
                frame_id: 0,
                ready: true,
                connected: true,
                input_source_handle: 0,
                input_candidate_source_handle: 0,
                input_selected_source_handle: 0,
                input_resolved_source_handle: 0,
                input_upstream_source_handle: 0,
                latest_frame: None,
                texture_upload_handle: None,
                source_texture_handle: None,
                last_texture_upload_at: None,
            },
        );
        set_runtime(state);

        let pushed = push_rgb_frame(VideoFrameInput {
            slot: 1,
            width: 2,
            height: 1,
            rgb: vec![[255, 0, 0], [0, 255, 0]],
            connected: Some(true),
            component: None,
        })
        .unwrap();
        assert_eq!(
            pushed.get("source").and_then(|value| value.as_str()),
            Some("pushed_rgb")
        );

        let rgb = frame_for_slot(1, "rgb").unwrap();
        assert_eq!(
            rgb.pointer("/0/0/rgb/0").and_then(|value| value.as_u64()),
            Some(255)
        );
        assert_eq!(
            rgb.pointer("/0/1/rgb/1").and_then(|value| value.as_u64()),
            Some(255)
        );

        let mut state = request_runtime_state().unwrap();
        if let Some(slot) = state.slots.get_mut(&slot_key(DEFAULT_COMPONENT, 1)) {
            slot.mode = "gray".to_string();
        }
        set_runtime(state);
        let gray = frame_for_slot(1, "gray").unwrap();
        assert_eq!(
            gray.pointer("/0/0/gray").and_then(|value| value.as_u64()),
            Some(76)
        );
        assert_eq!(
            gray.pointer("/0/1/gray").and_then(|value| value.as_u64()),
            Some(149)
        );
    }

    #[test]
    fn hook_plan_validation_matches_signature_stage_and_va() {
        let symbols = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [{
                    "entry": "1402e6f00",
                    "byte_check": {"va": "1402e6f00"}
                }]
            }
        });
        let mut plan = default_hook_plan();
        plan.patching_allowed = true;
        plan.dry_run_only = false;
        plan.accepted_stages = vec!["lua_api_registration".to_string()];
        plan.hooks = vec![HookPlanEntry {
            label: "lua_api_registration_1402e6f00".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va: "1402e6f00".to_string(),
            replacement: "stormworks_video_get_register_lua_api_hook".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        assert_eq!(
            validation.get("valid").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            validation
                .get("valid_hook_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );

        let mut config = default_video_get_config();
        config.hooking.allow_target_patches = true;
        let install_dry_run = hook_install_dry_run(None, &plan, &symbols, &validation);
        let gate = evaluate_target_patch_gate(&config, &plan, &validation, &install_dry_run);
        assert_eq!(
            gate.get("can_patch").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            gate.pointer("/install_dry_run/hooks/0/replacement/resolved")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            gate.pointer("/install_dry_run/hooks/0/replacement/usable_for_patch")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            gate.pointer("/install_dry_run/process/process_matches_context")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            gate.pointer("/install_dry_run/hooks/0/can_install_if_gate_opens")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            gate.pointer("/install_dry_run/lua_api/required")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            gate.pointer("/install_dry_run/lua_api/valid")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert!(gate
            .get("blockers")
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("hook_plan.lua_api_missing_or_invalid")));
    }

    #[test]
    fn hook_plan_lua_api_addresses_configure_hook_bridge() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut plan = default_hook_plan();
        plan.accepted_stages = vec!["lua_api_registration".to_string()];
        plan.lua_api = Some(fake_lua_api_plan());
        plan.hooks = vec![HookPlanEntry {
            label: "lua_api_registration_fake".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va: "1402e6f00".to_string(),
            replacement: "stormworks_video_get_register_lua_api_hook_arg2".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];

        let status = lua_api_plan_status(None, &plan);
        assert_eq!(
            status.get("required").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            status.get("configured").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            status.get("valid").and_then(|value| value.as_bool()),
            Some(true)
        );

        let api = build_lua_api_from_hook_plan(None, plan.lua_api.as_ref().unwrap()).unwrap();
        set_lua_hook_api(&api).unwrap();
        let mut lua = FakeLuaState {
            component: "hook_plan_lua_api_component".to_string(),
            ..FakeLuaState::default()
        };
        let lua_ptr = &mut lua as *mut FakeLuaState as *mut c_void;
        assert_eq!(
            stormworks_video_get_register_lua_api_hook_arg2(ptr::null_mut(), lua_ptr),
            1
        );
        assert_eq!(lua.global_name.as_deref(), Some("video"));
        let Some(FakeLuaValue::Table(table)) = lua.global_table.as_ref() else {
            panic!("hook plan lua_api did not register a video table");
        };
        assert!(matches!(table.get("get"), Some(FakeLuaValue::CFunction(_))));
    }

    #[test]
    fn hook_plan_dry_run_resolves_review_stub_but_keeps_it_patch_ineligible() {
        let symbols = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [{"entry": "1402e6f00"}]
            }
        });
        let mut plan = default_hook_plan();
        plan.hooks = vec![HookPlanEntry {
            label: "review".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va: "1402e6f00".to_string(),
            replacement: "stormworks_video_get_unbound_review_stub".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: false,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        let dry_run = hook_install_dry_run(None, &plan, &symbols, &validation);
        assert_eq!(
            dry_run
                .pointer("/hooks/0/replacement/resolved")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            dry_run
                .pointer("/hooks/0/replacement/usable_for_patch")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn hook_plan_validation_blocks_unknown_va() {
        let symbols = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [{"entry": "1402e6f00"}]
            }
        });
        let mut plan = default_hook_plan();
        plan.patching_allowed = true;
        plan.dry_run_only = false;
        plan.accepted_stages = vec!["lua_api_registration".to_string()];
        plan.hooks = vec![HookPlanEntry {
            label: "bad".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va: "140000000".to_string(),
            replacement: "stormworks_video_get_register_lua_api_hook".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        assert_eq!(
            validation.get("valid").and_then(|value| value.as_bool()),
            Some(false)
        );
        let mut config = default_video_get_config();
        config.hooking.allow_target_patches = true;
        let install_dry_run = hook_install_dry_run(None, &plan, &symbols, &validation);
        let gate = evaluate_target_patch_gate(&config, &plan, &validation, &install_dry_run);
        assert_eq!(
            gate.get("can_patch").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert!(gate
            .pointer("/blockers")
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .any(|value| value.as_str() == Some("hook_plan_validation_failed")));
    }

    #[test]
    fn pushed_rgb_frame_rejects_size_mismatch() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        state.slots.insert(
            slot_key(DEFAULT_COMPONENT, 1),
            SlotState {
                component: DEFAULT_COMPONENT.to_string(),
                slot: 1,
                width: 2,
                height: 2,
                mode: "rgb".to_string(),
                frame_id: 0,
                ready: true,
                connected: true,
                input_source_handle: 0,
                input_candidate_source_handle: 0,
                input_selected_source_handle: 0,
                input_resolved_source_handle: 0,
                input_upstream_source_handle: 0,
                latest_frame: None,
                texture_upload_handle: None,
                source_texture_handle: None,
                last_texture_upload_at: None,
            },
        );
        set_runtime(state);

        let error = push_rgb_frame(VideoFrameInput {
            slot: 1,
            width: 2,
            height: 2,
            rgb: vec![[0, 0, 0]; 3],
            connected: None,
            component: None,
        })
        .unwrap_err();
        assert!(error.contains("does not match"));
    }

    #[test]
    fn lua_manifest_and_dispatch_cover_slot_workflow() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let manifest = lua_api_manifest();
        assert_eq!(
            manifest.get("api_table").and_then(|value| value.as_str()),
            Some("video")
        );
        let function_names = manifest
            .get("functions")
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .filter_map(|value| value.get("name").and_then(|value| value.as_str()))
            .collect::<Vec<_>>();
        assert!(function_names.contains(&"init"));
        assert!(function_names.contains(&"getRGB"));
        assert!(function_names.contains(&"getPackedGray"));
        assert!(function_names.contains(&"getPackedRGB"));
        assert!(manifest
            .get("functions")
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .any(|value| value.get("default_slot").and_then(|value| value.as_u64()) == Some(1)));

        let init = dispatch_lua_call(LuaDispatchCall {
            function: "video.init".to_string(),
            args: vec![
                serde_json::json!(1),
                serde_json::json!(2),
                serde_json::json!(1),
                serde_json::json!("rgb"),
            ],
            component: Some("unit".to_string()),
        })
        .unwrap();
        assert_eq!(
            init.pointer("/returns/0").and_then(|value| value.as_bool()),
            Some(true)
        );

        push_rgb_frame(VideoFrameInput {
            slot: 1,
            width: 2,
            height: 1,
            rgb: vec![[1, 2, 3], [250, 251, 252]],
            connected: Some(true),
            component: Some("unit".to_string()),
        })
        .unwrap();

        let rgb = dispatch_lua_call(LuaDispatchCall {
            function: "getRGB".to_string(),
            args: vec![serde_json::json!(1)],
            component: Some("unit".to_string()),
        })
        .unwrap();
        assert_eq!(
            rgb.pointer("/returns/0/0/0/rgb/0")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            rgb.pointer("/returns/0/0/1/rgb/2")
                .and_then(|value| value.as_u64()),
            Some(252)
        );

        let packed_rgb = dispatch_lua_call(LuaDispatchCall {
            function: "getPackedRGB".to_string(),
            args: vec![serde_json::json!(1)],
            component: Some("unit".to_string()),
        })
        .unwrap();
        assert_eq!(
            packed_rgb
                .pointer("/returns/0/format")
                .and_then(|value| value.as_str()),
            Some("u8-rgb")
        );
        assert_eq!(
            packed_rgb
                .pointer("/returns/0/bytes/5")
                .and_then(|value| value.as_u64()),
            Some(252)
        );

        let default_init = dispatch_lua_call(LuaDispatchCall {
            function: "video.init".to_string(),
            args: vec![
                serde_json::json!(2),
                serde_json::json!(1),
                serde_json::json!("rgb"),
            ],
            component: Some("default_unit".to_string()),
        })
        .unwrap();
        assert_eq!(
            default_init
                .pointer("/native/slot")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        push_rgb_frame(VideoFrameInput {
            slot: 1,
            width: 2,
            height: 1,
            rgb: vec![[9, 8, 7], [6, 5, 4]],
            connected: Some(true),
            component: Some("default_unit".to_string()),
        })
        .unwrap();
        let default_rgb = dispatch_lua_call(LuaDispatchCall {
            function: "getRGB".to_string(),
            args: vec![],
            component: Some("default_unit".to_string()),
        })
        .unwrap();
        assert_eq!(
            default_rgb
                .pointer("/returns/0/0/0/rgb/0")
                .and_then(|value| value.as_u64()),
            Some(9)
        );

        dispatch_lua_call(LuaDispatchCall {
            function: "init".to_string(),
            args: vec![
                serde_json::json!(2),
                serde_json::json!(2),
                serde_json::json!(1),
                serde_json::json!("gray"),
            ],
            component: Some("unit".to_string()),
        })
        .unwrap();
        push_rgb_frame(VideoFrameInput {
            slot: 2,
            width: 2,
            height: 1,
            rgb: vec![[255, 0, 0], [0, 255, 0]],
            connected: Some(true),
            component: Some("unit".to_string()),
        })
        .unwrap();
        let packed_gray = dispatch_lua_call(LuaDispatchCall {
            function: "getPackedGray".to_string(),
            args: vec![serde_json::json!(2)],
            component: Some("unit".to_string()),
        })
        .unwrap();
        assert_eq!(
            packed_gray
                .pointer("/returns/0/format")
                .and_then(|value| value.as_str()),
            Some("u8-gray")
        );
        assert_eq!(
            packed_gray
                .pointer("/returns/0/bytes/0")
                .and_then(|value| value.as_u64()),
            Some(76)
        );
        assert_eq!(
            packed_gray
                .pointer("/returns/0/bytes/1")
                .and_then(|value| value.as_u64()),
            Some(149)
        );
    }

    #[test]
    fn direct_hook_abi_pushes_and_writes_packed_buffers() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("direct_unit").unwrap();
        let rgb_mode = CString::new("rgb").unwrap();
        let gray_mode = CString::new("gray").unwrap();
        let bytes = [1u8, 2, 3, 250, 251, 252];

        let init = direct_lua_init(component.as_ptr(), 1, 2, 1, rgb_mode.as_ptr()).unwrap();
        assert_eq!(
            init.pointer("/returns/0").and_then(|value| value.as_bool()),
            Some(true)
        );
        let pushed =
            push_rgb_frame_direct(component.as_ptr(), 1, 2, 1, bytes.as_ptr(), bytes.len(), 1)
                .unwrap();
        assert_eq!(
            pushed.get("source").and_then(|value| value.as_str()),
            Some("pushed_rgb")
        );
        assert_eq!(
            direct_packed_len(component.as_ptr(), 1, "rgb"),
            bytes.len() as i32
        );
        let mut out = [0u8; 6];
        assert_eq!(
            direct_packed_write(component.as_ptr(), 1, "rgb", out.as_mut_ptr(), out.len()),
            bytes.len() as i32
        );
        assert_eq!(out, bytes);

        direct_lua_init(component.as_ptr(), 2, 2, 1, gray_mode.as_ptr()).unwrap();
        push_rgb_frame_direct(component.as_ptr(), 2, 2, 1, bytes.as_ptr(), bytes.len(), 1).unwrap();
        assert_eq!(direct_packed_len(component.as_ptr(), 2, "gray"), 2);
        let mut gray = [0u8; 2];
        assert_eq!(
            direct_packed_write(component.as_ptr(), 2, "gray", gray.as_mut_ptr(), gray.len()),
            2
        );
        assert_eq!(gray, [1, 250]);

        let disconnected = bind_video_input_direct(component.as_ptr(), 1, false, 0).unwrap();
        assert_eq!(
            disconnected
                .get("connected")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(direct_packed_len(component.as_ptr(), 1, "rgb"), -1);
        let rebound = bind_video_input_direct(component.as_ptr(), 1, true, 0xfeed_beef).unwrap();
        assert_eq!(
            rebound
                .get("input_source_handle")
                .and_then(|value| value.as_u64()),
            Some(0xfeed_beef)
        );
        assert_eq!(
            direct_packed_write(component.as_ptr(), 1, "rgb", out.as_mut_ptr(), out.len()),
            bytes.len() as i32
        );

        let mut too_small = [0u8; 1];
        assert_eq!(
            direct_packed_write(
                component.as_ptr(),
                1,
                "rgb",
                too_small.as_mut_ptr(),
                too_small.len()
            ),
            -2
        );
    }

    #[test]
    fn capture_request_abi_enumerates_initialized_slots() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component_a = CString::new("request_component_a").unwrap();
        let component_b = CString::new("request_component_b").unwrap();
        let rgb_mode = CString::new("rgb").unwrap();
        let gray_mode = CString::new("gray").unwrap();
        direct_lua_init(component_a.as_ptr(), 1, 2, 1, rgb_mode.as_ptr()).unwrap();
        direct_lua_init(component_b.as_ptr(), 2, 3, 1, gray_mode.as_ptr()).unwrap();
        bind_video_input_direct(component_a.as_ptr(), 1, true, 0x1111).unwrap();
        bind_video_input_direct(component_b.as_ptr(), 2, false, 0x2222).unwrap();
        let bytes = [1u8, 2, 3, 250, 251, 252];
        push_rgb_frame_direct(
            component_a.as_ptr(),
            1,
            2,
            1,
            bytes.as_ptr(),
            bytes.len(),
            1,
        )
        .unwrap();

        assert_eq!(stormworks_video_get_capture_request_count(), 2);
        let mut one = [VideoGetCaptureRequestV1::default(); 1];
        assert_eq!(
            stormworks_video_get_capture_requests_write(one.as_mut_ptr(), one.len()),
            1
        );
        let mut requests = [VideoGetCaptureRequestV1::default(); 4];
        let written =
            stormworks_video_get_capture_requests_write(requests.as_mut_ptr(), requests.len());
        assert_eq!(written, 2);
        let requests = &requests[..written as usize];
        let a = requests
            .iter()
            .find(|request| request.slot == 1)
            .expect("slot 1 request");
        assert_eq!(a.size, size_of::<VideoGetCaptureRequestV1>() as u32);
        assert_eq!(
            a.component_hash,
            stable_component_hash("request_component_a")
        );
        assert_eq!(a.width, 2);
        assert_eq!(a.height, 1);
        assert_eq!(a.mode, 2);
        assert_eq!(a.ready, 1);
        assert_eq!(a.connected, 1);
        assert_eq!(a.source, 2);
        assert_eq!(a.input_source_handle, 0x1111);
        let b = requests
            .iter()
            .find(|request| request.slot == 2)
            .expect("slot 2 request");
        assert_eq!(
            b.component_hash,
            stable_component_hash("request_component_b")
        );
        assert_eq!(b.width, 3);
        assert_eq!(b.height, 1);
        assert_eq!(b.mode, 1);
        assert_eq!(b.source, 0);
        assert_eq!(b.ready, 0);
        assert_eq!(b.connected, 0);
        assert_eq!(b.input_source_handle, 0);
        assert_eq!(
            stormworks_video_get_capture_requests_write(ptr::null_mut(), 0),
            0
        );
        assert_eq!(
            stormworks_video_get_capture_requests_write(ptr::null_mut(), 1),
            -1
        );
    }

    #[test]
    fn capture_request_hash_ingest_updates_matching_component_slot() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mode = CString::new("rgb").unwrap();
        let component_a = CString::new("capture_hash_a").unwrap();
        let component_b = CString::new("capture_hash_b").unwrap();
        direct_lua_init(component_a.as_ptr(), 2, 1, 1, mode.as_ptr()).unwrap();
        direct_lua_init(component_b.as_ptr(), 2, 1, 1, mode.as_ptr()).unwrap();

        let hash_b = stable_component_hash("capture_hash_b");
        let bytes = [9u8, 8, 7];
        let pushed =
            push_rgb_frame_for_capture_request(hash_b, 2, 1, 1, bytes.as_ptr(), bytes.len(), 1)
                .unwrap();
        assert_eq!(
            pushed.get("component").and_then(|value| value.as_str()),
            Some("capture_hash_b")
        );

        let b = packed_frame_data_for_component_slot("capture_hash_b", 2, "rgb").unwrap();
        assert_eq!(b.bytes, bytes);
        let a_error = packed_frame_data_for_component_slot("capture_hash_a", 2, "rgb").unwrap_err();
        assert_eq!(a_error, "frame not ready");
        assert!(push_rgb_frame_for_capture_request(
            0xdead_beef,
            2,
            1,
            1,
            bytes.as_ptr(),
            bytes.len(),
            1,
        )
        .unwrap_err()
        .contains("not initialized"));
    }

    #[test]
    fn capture_request_hash_ingest_can_preserve_mock_render_source() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mode = CString::new("rgb").unwrap();
        let component = CString::new("capture_mock_hash").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        let hash = stable_component_hash("capture_mock_hash");
        let bytes = [3u8, 4, 5, 6, 7, 8];
        let pushed = push_rgb_frame_for_capture_request_with_source(
            hash,
            1,
            2,
            1,
            bytes.as_ptr(),
            bytes.len(),
            1,
            "mock_render",
        )
        .unwrap();
        assert_eq!(
            pushed.get("source").and_then(|value| value.as_str()),
            Some("mock_render")
        );

        let packed = packed_frame_data_for_component_slot("capture_mock_hash", 1, "rgb").unwrap();
        assert_eq!(packed.source, "mock_render");
        assert_eq!(packed.bytes, bytes);
        let request = capture_request_from_slot(
            runtime_snapshot()
                .slots
                .get(&slot_key("capture_mock_hash", 1))
                .unwrap(),
        );
        assert_eq!(request.source, 1);
    }

    #[test]
    fn lua_getters_wait_for_real_or_pushed_frame_after_init() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("real_frame_gate").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        assert_eq!(
            direct_lua_bool(component.as_ptr(), 1, "isConnected")
                .unwrap()
                .pointer("/returns/0")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            direct_lua_bool(component.as_ptr(), 1, "isReady")
                .unwrap()
                .pointer("/returns/0")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            frame_info_for_component_slot("real_frame_gate", 1)
                .unwrap()
                .get("source")
                .and_then(|value| value.as_str()),
            Some("none")
        );
        assert_eq!(
            frame_for_component_slot("real_frame_gate", 1, "rgb").unwrap_err(),
            "frame not ready"
        );
        assert_eq!(
            packed_frame_data_for_component_slot("real_frame_gate", 1, "rgb").unwrap_err(),
            "frame not ready"
        );

        let bytes = [1u8, 2, 3, 250, 251, 252];
        push_rgb_frame_direct(component.as_ptr(), 1, 2, 1, bytes.as_ptr(), bytes.len(), 1).unwrap();

        assert_eq!(
            direct_lua_bool(component.as_ptr(), 1, "isConnected")
                .unwrap()
                .pointer("/returns/0")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            direct_lua_bool(component.as_ptr(), 1, "isReady")
                .unwrap()
                .pointer("/returns/0")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        let packed = packed_frame_data_for_component_slot("real_frame_gate", 1, "rgb").unwrap();
        assert_eq!(packed.source, "pushed_rgb");
        assert_eq!(packed.bytes, bytes);
    }

    #[test]
    fn capture_request_hash_binding_updates_matching_component_slot() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mode = CString::new("rgb").unwrap();
        let component = CString::new("capture_bind_hash").unwrap();
        direct_lua_init(component.as_ptr(), 4, 2, 1, mode.as_ptr()).unwrap();

        let hash = stable_component_hash("capture_bind_hash");
        let bound = bind_video_input_for_capture_request(hash, 4, true, 0xabc_def).unwrap();
        assert_eq!(
            bound.get("component").and_then(|value| value.as_str()),
            Some("capture_bind_hash")
        );
        assert_eq!(
            bound
                .get("input_source_handle")
                .and_then(|value| value.as_u64()),
            Some(0xabc_def)
        );
        let request = capture_request_from_slot(
            runtime_snapshot()
                .slots
                .get(&slot_key("capture_bind_hash", 4))
                .unwrap(),
        );
        assert_eq!(request.connected, 1);
        assert_eq!(request.input_source_handle, 0xabc_def);

        let disconnected = bind_video_input_for_capture_request(hash, 4, false, 0xabc_def).unwrap();
        assert_eq!(
            disconnected
                .get("connected")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        let request = capture_request_from_slot(
            runtime_snapshot()
                .slots
                .get(&slot_key("capture_bind_hash", 4))
                .unwrap(),
        );
        assert_eq!(request.connected, 0);
        assert_eq!(request.input_source_handle, 0);
    }

    #[derive(Debug, Clone, PartialEq)]
    enum FakeLuaValue {
        Nil,
        Bool(bool),
        Integer(i64),
        String(String),
        Table(BTreeMap<String, FakeLuaValue>),
        CFunction(usize),
    }

    #[derive(Default)]
    struct FakeLuaState {
        args: Vec<FakeLuaValue>,
        stack: Vec<FakeLuaValue>,
        scratch: Vec<CString>,
        global_name: Option<String>,
        global_table: Option<FakeLuaValue>,
        component: String,
    }

    #[derive(Default)]
    struct FakeGameLuaState {
        memory: Box<[u8]>,
        call_base_offset: usize,
        stack_top_offset: usize,
        call_base_slot_offset: usize,
        upvalue_offset: usize,
        next_table_object: usize,
        tables: BTreeMap<usize, BTreeMap<String, FakeLuaValue>>,
        registered_table: Option<String>,
        registered_functions: Vec<(String, VideoGetLuaCFunction)>,
        scratch: Vec<CString>,
        arg_slot_calls: Vec<i32>,
    }

    unsafe extern "C" fn fake_lua_createtable(lua: *mut c_void, _: i32, _: i32) {
        fake_lua(lua)
            .stack
            .push(FakeLuaValue::Table(BTreeMap::new()));
    }

    unsafe extern "C" fn fake_lua_pushcclosure(
        lua: *mut c_void,
        function: VideoGetLuaCFunction,
        _: i32,
    ) {
        fake_lua(lua)
            .stack
            .push(FakeLuaValue::CFunction(function as usize));
    }

    unsafe extern "C" fn fake_lua_setglobal(lua: *mut c_void, name: *const c_char) {
        let lua = fake_lua(lua);
        lua.global_name = unsafe_cstr(name);
        lua.global_table = lua.stack.pop();
    }

    unsafe extern "C" fn fake_lua_setfield(lua: *mut c_void, _: i32, name: *const c_char) {
        let lua = fake_lua(lua);
        let Some(value) = lua.stack.pop() else {
            return;
        };
        let Some(FakeLuaValue::Table(table)) = lua.stack.last_mut() else {
            return;
        };
        table.insert(unsafe_cstr(name).unwrap_or_default(), value);
    }

    unsafe extern "C" fn fake_lua_rawseti(lua: *mut c_void, _: i32, index: i64) {
        let lua = fake_lua(lua);
        let Some(value) = lua.stack.pop() else {
            return;
        };
        let Some(FakeLuaValue::Table(table)) = lua.stack.last_mut() else {
            return;
        };
        table.insert(index.to_string(), value);
    }

    unsafe extern "C" fn fake_lua_pushnil(lua: *mut c_void) {
        fake_lua(lua).stack.push(FakeLuaValue::Nil);
    }

    unsafe extern "C" fn fake_lua_pushboolean(lua: *mut c_void, value: i32) {
        fake_lua(lua).stack.push(FakeLuaValue::Bool(value != 0));
    }

    unsafe extern "C" fn fake_lua_pushinteger(lua: *mut c_void, value: i64) {
        fake_lua(lua).stack.push(FakeLuaValue::Integer(value));
    }

    unsafe extern "C" fn fake_lua_pushstring(lua: *mut c_void, value: *const c_char) {
        fake_lua(lua)
            .stack
            .push(FakeLuaValue::String(unsafe_cstr(value).unwrap_or_default()));
    }

    unsafe extern "C" fn fake_lua_checkinteger(lua: *mut c_void, index: i32) -> i64 {
        match fake_lua(lua).args.get(index as usize - 1) {
            Some(FakeLuaValue::Integer(value)) => *value,
            _ => 0,
        }
    }

    unsafe extern "C" fn fake_lua_checkstring(lua: *mut c_void, index: i32) -> *const c_char {
        let lua = fake_lua(lua);
        let text = match lua.args.get(index as usize - 1) {
            Some(FakeLuaValue::String(value)) => value.clone(),
            _ => String::new(),
        };
        lua.scratch.push(CString::new(text).unwrap());
        lua.scratch.last().unwrap().as_ptr()
    }

    unsafe extern "C" fn fake_component_id(
        lua: *mut c_void,
        out: *mut c_char,
        out_len: usize,
    ) -> usize {
        if out.is_null() || out_len == 0 {
            return 0;
        }
        let component = fake_lua(lua).component.as_bytes();
        let count = component.len().min(out_len.saturating_sub(1));
        ptr::copy_nonoverlapping(component.as_ptr(), out as *mut u8, count);
        *out.add(count) = 0;
        count
    }

    unsafe fn fake_lua<'a>(lua: *mut c_void) -> &'a mut FakeLuaState {
        &mut *(lua as *mut FakeLuaState)
    }

    impl FakeGameLuaState {
        fn new() -> Self {
            FakeGameLuaState {
                memory: vec![0u8; 4096].into_boxed_slice(),
                call_base_offset: 0x200,
                stack_top_offset: 0x210,
                call_base_slot_offset: 0x40,
                upvalue_offset: 0x80,
                next_table_object: 0x1234_0000,
                tables: BTreeMap::new(),
                registered_table: None,
                registered_functions: Vec::new(),
                scratch: Vec::new(),
                arg_slot_calls: Vec::new(),
            }
        }

        fn as_lua_ptr(&mut self) -> *mut c_void {
            self.refresh_pointers();
            let self_ptr = self as *mut FakeGameLuaState as usize;
            self.write_usize(0x00, self_ptr);
            self.memory.as_mut_ptr().cast::<c_void>()
        }

        fn set_args(&mut self, args: &[FakeLuaValue]) {
            let clear_len = (args.len() + 1) * 0x10;
            self.memory[self.call_base_offset..self.call_base_offset + clear_len].fill(0);
            self.stack_top_offset = self.call_base_offset + (args.len() + 1) * 0x10;
            self.refresh_pointers();
            for (index, arg) in args.iter().enumerate() {
                let offset = self.call_base_offset + (index + 1) * 0x10;
                match arg {
                    FakeLuaValue::Integer(value) => {
                        self.memory[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
                        self.write_u32(offset + 8, 0x13);
                    }
                    FakeLuaValue::String(value) => {
                        let c_string = CString::new(value.as_str()).unwrap();
                        let ptr = c_string.as_ptr() as usize;
                        self.scratch.push(c_string);
                        self.write_usize(offset, ptr.saturating_sub(0x18));
                        self.write_u32(offset + 8, 4);
                    }
                    FakeLuaValue::Bool(value) => {
                        self.write_u32(offset, if *value { 1 } else { 0 });
                        self.write_u32(offset + 8, 1);
                    }
                    _ => {
                        self.write_u32(offset + 8, 0);
                    }
                }
            }
        }

        fn set_component_upvalue(&mut self, component_context: usize) {
            self.write_usize(self.upvalue_offset, component_context);
            self.write_u32(self.upvalue_offset + 8, 2);
        }

        fn stack_values(&self, count: usize) -> Vec<FakeLuaValue> {
            let base = self.memory.as_ptr() as usize;
            let args_end = self.call_base_offset + (count + 1) * 0x10;
            let top = self.read_usize(0x10).saturating_sub(base);
            let mut values = Vec::new();
            let mut offset = args_end;
            while offset < top {
                values.push(self.slot_value(offset));
                offset += 0x10;
            }
            values
        }

        fn slot_value(&self, offset: usize) -> FakeLuaValue {
            match self.read_u32(offset + 8) {
                0 => FakeLuaValue::Nil,
                1 => FakeLuaValue::Bool(self.read_u32(offset) != 0),
                3 => FakeLuaValue::Integer(self.read_f64(offset) as i64),
                0x13 => {
                    let value =
                        i64::from_le_bytes(self.memory[offset..offset + 8].try_into().unwrap());
                    FakeLuaValue::Integer(value)
                }
                4 | 5 => {
                    let ptr = self.read_usize(offset).saturating_add(0x18) as *const c_char;
                    FakeLuaValue::String(unsafe_cstr(ptr).unwrap_or_default())
                }
                0x45 => self
                    .tables
                    .get(&self.read_usize(offset))
                    .cloned()
                    .map(FakeLuaValue::Table)
                    .unwrap_or_else(|| FakeLuaValue::Table(BTreeMap::new())),
                _ => FakeLuaValue::Nil,
            }
        }

        fn refresh_pointers(&mut self) {
            let base = self.memory.as_ptr() as usize;
            self.write_usize(0x10, base + self.stack_top_offset);
            self.write_usize(self.call_base_slot_offset, base + self.call_base_offset);
            self.write_usize(0x20, base + self.call_base_slot_offset);
        }

        fn write_usize(&mut self, offset: usize, value: usize) {
            self.memory[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }

        fn read_usize(&self, offset: usize) -> usize {
            usize::from_le_bytes(self.memory[offset..offset + 8].try_into().unwrap())
        }

        fn write_u32(&mut self, offset: usize, value: u32) {
            self.memory[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }

        fn read_u32(&self, offset: usize) -> u32 {
            u32::from_le_bytes(self.memory[offset..offset + 4].try_into().unwrap())
        }

        fn read_f64(&self, offset: usize) -> f64 {
            f64::from_le_bytes(self.memory[offset..offset + 8].try_into().unwrap())
        }
    }

    unsafe extern "C" fn fake_game_lua_register_table(
        component_context: usize,
        table_name_ptr: *const usize,
        pairs: *const GameLuaFunctionPair,
        _: usize,
    ) {
        let owner = *(component_context as *const usize);
        let state = &mut *(owner as *mut FakeGameLuaState);
        let table_name = *(table_name_ptr) as *const c_char;
        state.registered_table = unsafe_cstr(table_name);
        let mut index = 0usize;
        loop {
            let pair = *pairs.add(index);
            if pair.name.is_null() {
                break;
            }
            let Some(function) = pair.function else {
                break;
            };
            state
                .registered_functions
                .push((unsafe_cstr(pair.name).unwrap_or_default(), function));
            index += 1;
        }
    }

    unsafe extern "C" fn fake_game_lua_create_table(lua_state: usize) -> usize {
        let owner = *(lua_state as *const usize);
        let state = &mut *(owner as *mut FakeGameLuaState);
        let object = state.next_table_object;
        state.next_table_object += 0x100;
        state.tables.insert(object, BTreeMap::new());
        object
    }

    unsafe extern "C" fn fake_game_lua_push_string(
        lua_state: usize,
        value: *const c_char,
    ) -> usize {
        let owner = *(lua_state as *const usize);
        let state = &mut *(owner as *mut FakeGameLuaState);
        let text = unsafe_cstr(value).unwrap_or_default();
        state.scratch.push(CString::new(text).unwrap());
        let ptr = state.scratch.last().unwrap().as_ptr() as usize;
        let lua = lua_state as *mut u8;
        let top = *(lua.add(0x10) as *const usize) as *mut u8;
        *(top as *mut usize) = ptr.saturating_sub(0x18);
        *(top.add(8) as *mut u32) = 4;
        *(lua.add(0x10) as *mut usize) = top as usize + 0x10;
        ptr.saturating_sub(0x18)
    }

    unsafe extern "C" fn fake_game_lua_rawseti(lua_state: usize, _: i32, index: i64) {
        let owner = *(lua_state as *const usize);
        let state = &mut *(owner as *mut FakeGameLuaState);
        let lua = lua_state as *mut u8;
        let base = lua as usize;
        let top_abs = *(lua.add(0x10) as *const usize);
        if top_abs < base + 0x20 {
            return;
        }
        let value_offset = top_abs.saturating_sub(base + 0x10);
        let table_offset = top_abs.saturating_sub(base + 0x20);
        let value = state.slot_value(value_offset);
        let table_object = state.read_usize(table_offset);
        if let Some(table) = state.tables.get_mut(&table_object) {
            table.insert(index.to_string(), value);
        }
        *(lua.add(0x10) as *mut usize) = top_abs.saturating_sub(0x10);
    }

    unsafe extern "C" fn fake_game_lua_arg_slot(lua_state: usize, index: i32) -> *mut u8 {
        let owner = *(lua_state as *const usize);
        let state = &mut *(owner as *mut FakeGameLuaState);
        state.arg_slot_calls.push(index);
        let base = lua_state;
        if index == GAME_LUA_FIRST_UPVALUE_INDEX {
            return (base + state.upvalue_offset) as *mut u8;
        }
        if index < 1 {
            return ptr::null_mut();
        }
        (base + state.call_base_offset + index as usize * 0x10) as *mut u8
    }

    fn fake_lua_api() -> VideoGetLuaApiV1 {
        VideoGetLuaApiV1 {
            size: size_of::<VideoGetLuaApiV1>() as u32,
            lua_version: 504,
            lua_createtable: Some(fake_lua_createtable),
            lua_pushcclosure: Some(fake_lua_pushcclosure),
            lua_setglobal: Some(fake_lua_setglobal),
            lua_setfield: Some(fake_lua_setfield),
            lua_rawseti: Some(fake_lua_rawseti),
            lua_pushnil: Some(fake_lua_pushnil),
            lua_pushboolean: Some(fake_lua_pushboolean),
            lua_pushinteger: Some(fake_lua_pushinteger),
            lua_pushstring: Some(fake_lua_pushstring),
            luaL_checkinteger: Some(fake_lua_checkinteger),
            luaL_checkstring: Some(fake_lua_checkstring),
            component_id: Some(fake_component_id),
        }
    }

    fn fake_lua_api_without_component_id() -> VideoGetLuaApiV1 {
        VideoGetLuaApiV1 {
            component_id: None,
            ..fake_lua_api()
        }
    }

    fn reset_lua_adapter_for_test() {
        if let Ok(mut adapter) = lua_adapter_cell().lock() {
            adapter.api = None;
            adapter.hook_api = None;
            adapter.registrations = 0;
            adapter.hook_registrations = 0;
            adapter.hook_original_calls = 0;
            adapter.last_error = None;
        }
        LUA_REGISTRATION_ORIGINAL_DIRECT.store(0, Ordering::SeqCst);
        LUA_REGISTRATION_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
        LUA_REGISTRATION_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
        LUA_REGISTRATION_ORIGINAL_ARG3.store(0, Ordering::SeqCst);
        LUA_REGISTRATION_ORIGINAL_ARG4.store(0, Ordering::SeqCst);
        COMPONENT_LUA_INIT_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
        GAME_LUA_CREATE_TABLE.store(0, Ordering::SeqCst);
        GAME_LUA_PUSH_STRING.store(0, Ordering::SeqCst);
        GAME_LUA_RAWSETI.store(0, Ordering::SeqCst);
        GAME_LUA_REGISTER_TABLE.store(0, Ordering::SeqCst);
        GAME_LUA_ARG_SLOT.store(0, Ordering::SeqCst);
        if let Ok(mut contexts) = game_lua_component_contexts_cell().lock() {
            contexts.clear();
        }
        if let Ok(mut seen) = component_liveness_cell().lock() {
            seen.clear();
        }
        COMPONENT_LUA_REGISTRATION_DIAGNOSTIC_COUNT.store(0, Ordering::SeqCst);
        TEXTURE_UPLOAD_FRAME_LOGGED.store(false, Ordering::SeqCst);
        VIDEO_INIT_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        TEXTURE_UPLOAD_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        TEXTURE_UPLOAD_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        TEXTURE_UPLOAD_NO_SLOT_LOGGED_COUNT.store(0, Ordering::Relaxed);
        TEXTURE_UPLOAD_NO_MATCH_LOGGED_COUNT.store(0, Ordering::Relaxed);
        MONITOR_RENDER_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        MONITOR_RENDER_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        ADDITIVE_MONITOR_BIND_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        ADDITIVE_MONITOR_BIND_WITH_SLOTS_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        ADDITIVE_MONITOR_BIND_SLOT_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        ADDITIVE_MONITOR_BIND_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        ADDITIVE_GL_BIND_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        ADDITIVE_GL_BIND_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        ADDITIVE_GL_BIND_UNIT_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        RENDERER_VIDEO_PASS_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        MONITOR_INPUT_RELATION_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        MONITOR_BRIDGE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        SOURCE_TEXTURE_PROBE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        SOURCE_TEXTURE_CAPTURE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        COMPONENT_CONTEXT_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
        COMPONENT_CONTEXT_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
        COMPONENT_CONTEXT_ORIGINAL_ARG3.store(0, Ordering::SeqCst);
        COMPONENT_CONTEXT_ORIGINAL_ARG4.store(0, Ordering::SeqCst);
        INPUT_VIDEO_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
        INPUT_VIDEO_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
        INPUT_VIDEO_ORIGINAL_ARG3.store(0, Ordering::SeqCst);
        INPUT_VIDEO_ORIGINAL_ARG4.store(0, Ordering::SeqCst);
        INPUT_VIDEO_NODE_UPDATE_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
        INPUT_VIDEO_NODE_SELECT_ORIGINAL_ARG5.store(0, Ordering::SeqCst);
        VIDEO_OUTPUT_SLOT_ADD_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
        VIDEO_OUTPUT_SLOT_REMOVE_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
        VIDEO_OUTPUT_SLOT_CLEAR_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
        VIDEO_LOGIC_EDGE_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        if let Ok(mut edges) = VIDEO_INPUT_TO_OUTPUT_EDGES.lock() {
            edges.clear();
        }
        TEXTURE_UPLOAD_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
        VIDEO_NODE_REGISTRY_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        VIDEO_NODE_INIT_REGISTRY_DIAGNOSTIC_COUNT.store(0, Ordering::Relaxed);
        MONITOR_RENDER_QUEUE_ORIGINAL_ARG6.store(0, Ordering::SeqCst);
        RENDER_QUEUE_ALLOC_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
        RENDER_QUEUE_SUBMIT_COPY_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
        RENDERER_VIDEO_PASS_ORIGINAL_ARG8.store(0, Ordering::SeqCst);
        ADDITIVE_MONITOR_BIND_ORIGINAL.store(0, Ordering::SeqCst);
        LUA_COMPONENT_CONTEXT.with(|stack| stack.borrow_mut().clear());
    }

    fn fake_lua_api_plan() -> HookPlanLuaApi {
        fn fn_addr<T>(function: T) -> String
        where
            T: Copy,
        {
            hex_u64(unsafe { std::mem::transmute_copy::<T, usize>(&function) } as u64)
        }

        HookPlanLuaApi {
            lua_version: 504,
            lua_createtable: Some(fn_addr(
                fake_lua_createtable as unsafe extern "C" fn(*mut c_void, i32, i32),
            )),
            lua_pushcclosure: Some(fn_addr(
                fake_lua_pushcclosure
                    as unsafe extern "C" fn(*mut c_void, VideoGetLuaCFunction, i32),
            )),
            lua_setglobal: Some(fn_addr(
                fake_lua_setglobal as unsafe extern "C" fn(*mut c_void, *const c_char),
            )),
            lua_setfield: Some(fn_addr(
                fake_lua_setfield as unsafe extern "C" fn(*mut c_void, i32, *const c_char),
            )),
            lua_rawseti: Some(fn_addr(
                fake_lua_rawseti as unsafe extern "C" fn(*mut c_void, i32, i64),
            )),
            lua_pushnil: Some(fn_addr(
                fake_lua_pushnil as unsafe extern "C" fn(*mut c_void),
            )),
            lua_pushboolean: Some(fn_addr(
                fake_lua_pushboolean as unsafe extern "C" fn(*mut c_void, i32),
            )),
            lua_pushinteger: Some(fn_addr(
                fake_lua_pushinteger as unsafe extern "C" fn(*mut c_void, i64),
            )),
            lua_pushstring: Some(fn_addr(
                fake_lua_pushstring as unsafe extern "C" fn(*mut c_void, *const c_char),
            )),
            luaL_checkinteger: Some(fn_addr(
                fake_lua_checkinteger as unsafe extern "C" fn(*mut c_void, i32) -> i64,
            )),
            luaL_checkstring: Some(fn_addr(
                fake_lua_checkstring as unsafe extern "C" fn(*mut c_void, i32) -> *const c_char,
            )),
            component_id: Some(fn_addr(
                fake_component_id as unsafe extern "C" fn(*mut c_void, *mut c_char, usize) -> usize,
            )),
        }
    }

    fn fake_game_lua_plan() -> HookPlanGameLua {
        fn fn_addr<T>(function: T) -> String
        where
            T: Copy,
        {
            hex_u64(unsafe { std::mem::transmute_copy::<T, usize>(&function) } as u64)
        }

        HookPlanGameLua {
            create_table: Some(fn_addr(fake_game_lua_create_table as GameLuaCreateTableFn)),
            push_string: Some(fn_addr(fake_game_lua_push_string as GameLuaPushStringFn)),
            rawseti: Some(fn_addr(fake_game_lua_rawseti as GameLuaRawSetIFn)),
            register_table: Some(fn_addr(
                fake_game_lua_register_table as GameLuaRegisterTableFn,
            )),
            arg_slot: Some(fn_addr(fake_game_lua_arg_slot as GameLuaArgSlotFn)),
        }
    }

    #[test]
    fn lua_registration_adapter_builds_video_table_and_callbacks() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mut lua = FakeLuaState {
            component: "fake_component".to_string(),
            ..FakeLuaState::default()
        };
        let api = fake_lua_api();
        let lua_ptr = &mut lua as *mut FakeLuaState as *mut c_void;
        assert_eq!(register_lua_api(lua_ptr, &api), Ok(1));
        assert_eq!(lua.global_name.as_deref(), Some("video"));
        let FakeLuaValue::Table(table) = lua.global_table.as_ref().unwrap() else {
            panic!("video global is not a table");
        };
        assert!(matches!(
            table.get("init"),
            Some(FakeLuaValue::CFunction(_))
        ));
        assert!(matches!(
            table.get("getPackedRGB"),
            Some(FakeLuaValue::CFunction(_))
        ));

        lua.stack.clear();
        lua.args = vec![
            FakeLuaValue::Integer(1),
            FakeLuaValue::Integer(2),
            FakeLuaValue::Integer(1),
            FakeLuaValue::String("rgb".to_string()),
        ];
        assert_eq!(unsafe { video_lua_init(lua_ptr) }, 2);
        assert!(matches!(lua.stack.first(), Some(FakeLuaValue::Bool(true))));

        let bytes = [1u8, 2, 3, 250, 251, 252];
        push_rgb_frame_direct(
            CString::new("fake_component").unwrap().as_ptr(),
            1,
            2,
            1,
            bytes.as_ptr(),
            bytes.len(),
            1,
        )
        .unwrap();

        lua.stack.clear();
        lua.args = vec![FakeLuaValue::Integer(1)];
        assert_eq!(unsafe { video_lua_get_packed_rgb(lua_ptr) }, 1);
        let Some(FakeLuaValue::Table(buffer)) = lua.stack.first() else {
            panic!("packed RGB did not push a table");
        };
        assert_eq!(
            buffer.get("byte_len"),
            Some(&FakeLuaValue::Integer(bytes.len() as i64))
        );
        let Some(FakeLuaValue::Table(byte_table)) = buffer.get("bytes") else {
            panic!("packed RGB bytes are not a table");
        };
        assert_eq!(byte_table.get("1"), Some(&FakeLuaValue::Integer(1)));
        assert_eq!(byte_table.get("6"), Some(&FakeLuaValue::Integer(252)));
    }

    #[test]
    fn lua_component_context_stack_scopes_registered_callbacks() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mut lua = FakeLuaState::default();
        let api = fake_lua_api_without_component_id();
        let lua_ptr = &mut lua as *mut FakeLuaState as *mut c_void;
        assert_eq!(register_lua_api(lua_ptr, &api), Ok(1));

        let component = CString::new("thread_context_component").unwrap();
        assert_eq!(
            stormworks_video_get_enter_lua_component_context(component.as_ptr()),
            1
        );
        let mut buffer = [0i8; 64];
        let written =
            stormworks_video_get_current_lua_component_context_write(buffer.as_mut_ptr(), 64);
        assert_eq!(written, "thread_context_component".len());
        assert_eq!(
            unsafe_cstr(buffer.as_ptr()).as_deref(),
            Some("thread_context_component")
        );

        lua.stack.clear();
        lua.args = vec![
            FakeLuaValue::Integer(2),
            FakeLuaValue::Integer(2),
            FakeLuaValue::Integer(1),
            FakeLuaValue::String("rgb".to_string()),
        ];
        assert_eq!(unsafe { video_lua_init(lua_ptr) }, 2);
        assert!(runtime_snapshot()
            .slots
            .contains_key(&slot_key("thread_context_component", 2)));
        assert!(!runtime_snapshot()
            .slots
            .contains_key(&slot_key("lua_state:0", 2)));

        assert_eq!(stormworks_video_get_leave_lua_component_context(), 1);
        assert_eq!(lua_component_context_depth(), 0);
        assert_eq!(
            stormworks_video_get_current_lua_component_context_write(buffer.as_mut_ptr(), 64),
            0
        );
    }

    #[test]
    fn lua_registration_hook_bridge_registers_video_table_from_saved_api() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let api = fake_lua_api();
        assert_eq!(set_lua_hook_api(&api), Ok(()));
        let mut lua = FakeLuaState {
            component: "hook_component".to_string(),
            ..FakeLuaState::default()
        };
        let lua_ptr = &mut lua as *mut FakeLuaState as *mut c_void;
        assert_eq!(stormworks_video_get_register_lua_api_hook(lua_ptr), 1);
        assert_eq!(lua.global_name.as_deref(), Some("video"));
        assert!(matches!(lua.global_table, Some(FakeLuaValue::Table(_))));
        let status = lua_adapter_status_value();
        assert_eq!(
            status
                .get("hook_api_configured")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            status
                .get("hook_registrations")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(runtime_snapshot().hook_runtime.lua_api_registered, true);
    }

    #[test]
    fn lua_registration_hook_argument_shims_select_lua_state() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let api = fake_lua_api();
        assert_eq!(set_lua_hook_api(&api), Ok(()));
        let mut lua = FakeLuaState {
            component: "shim_component".to_string(),
            ..FakeLuaState::default()
        };
        let lua_ptr = &mut lua as *mut FakeLuaState as *mut c_void;
        let dummy = ptr::null_mut();

        for label in ["arg1", "arg2", "arg3", "arg4"] {
            let result = match label {
                "arg1" => stormworks_video_get_register_lua_api_hook_arg1(lua_ptr),
                "arg2" => stormworks_video_get_register_lua_api_hook_arg2(dummy, lua_ptr),
                "arg3" => stormworks_video_get_register_lua_api_hook_arg3(dummy, dummy, lua_ptr),
                "arg4" => {
                    stormworks_video_get_register_lua_api_hook_arg4(dummy, dummy, dummy, lua_ptr)
                }
                _ => unreachable!(),
            };
            assert_eq!(result, 1, "{label}");
            assert_eq!(lua.global_name.as_deref(), Some("video"), "{label}");
            assert!(
                matches!(lua.global_table, Some(FakeLuaValue::Table(_))),
                "{label}"
            );
            lua.global_name = None;
            lua.global_table = None;
        }
        let status = lua_adapter_status_value();
        assert_eq!(
            status
                .get("hook_registrations")
                .and_then(|value| value.as_u64()),
            Some(4)
        );
    }

    #[test]
    fn hook_runtime_mock_render_updates_initialized_slots() {
        let _guard = test_runtime_lock();
        FRAME_PUMP_ACTIVE.store(false, Ordering::SeqCst);
        let mut state = default_runtime_state();
        state.configured = true;
        state.signature_symbols = serde_json::json!({
            "lua_api_registration": {"value": [{}]},
            "current_lua_component_context": {"value": [{}]},
            "microprocessor_input_video_node": {"value": [{}]},
            "video_texture_source": {"value": [{}]}
        });
        set_runtime(state);

        let component = CString::new("pump_unit").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        let installed = install_hook_runtime(false).unwrap();
        assert_eq!(
            installed
                .pointer("/hook_runtime/runtime_active")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            installed
                .pointer("/target_patch_gate/can_patch")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            installed
                .get("target_patch_points_modified")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert!(installed
            .pointer("/target_patch_gate/blockers")
            .and_then(|value| value.as_array())
            .map(|blockers| !blockers.is_empty())
            .unwrap_or(false));

        refresh_mock_render_slots().unwrap();
        let packed = packed_frame_data_for_component_slot("pump_unit", 1, "rgb").unwrap();
        assert_eq!(packed.byte_len, 6);
        assert!(packed.frame_id > 0);
        assert_eq!(packed.source, "mock_render");
        let request = capture_request_from_slot(
            runtime_snapshot()
                .slots
                .get(&slot_key("pump_unit", 1))
                .unwrap(),
        );
        assert_eq!(request.component_hash, stable_component_hash("pump_unit"));
        assert_eq!(request.slot, 1);
        assert_eq!(request.source, 1);
        assert_eq!(request.connected, 1);
    }

    #[test]
    fn mock_render_does_not_overwrite_pushed_frames() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.update_initialized_slots = true;
        state.slots.insert(
            slot_key(DEFAULT_COMPONENT, 1),
            SlotState {
                component: DEFAULT_COMPONENT.to_string(),
                slot: 1,
                width: 2,
                height: 1,
                mode: "rgb".to_string(),
                frame_id: 0,
                ready: true,
                connected: true,
                input_source_handle: 0,
                input_candidate_source_handle: 0,
                input_selected_source_handle: 0,
                input_resolved_source_handle: 0,
                input_upstream_source_handle: 0,
                latest_frame: None,
                texture_upload_handle: None,
                source_texture_handle: None,
                last_texture_upload_at: None,
            },
        );
        set_runtime(state);

        refresh_mock_render_slots().unwrap();
        let mock = packed_frame_data_for_component_slot(DEFAULT_COMPONENT, 1, "rgb").unwrap();
        assert_eq!(mock.source, "mock_render");

        push_rgb_frame(VideoFrameInput {
            slot: 1,
            width: 2,
            height: 1,
            rgb: vec![[1, 2, 3], [250, 251, 252]],
            connected: Some(true),
            component: None,
        })
        .unwrap();
        let pushed = packed_frame_data_for_component_slot(DEFAULT_COMPONENT, 1, "rgb").unwrap();
        assert_eq!(pushed.source, "pushed_rgb");
        assert_eq!(pushed.bytes, vec![1, 2, 3, 250, 251, 252]);

        refresh_mock_render_slots().unwrap();
        let after_refresh =
            packed_frame_data_for_component_slot(DEFAULT_COMPONENT, 1, "rgb").unwrap();
        assert_eq!(after_refresh.source, "pushed_rgb");
        assert_eq!(after_refresh.bytes, vec![1, 2, 3, 250, 251, 252]);
    }

    #[cfg(windows)]
    #[inline(never)]
    extern "C" fn detour_probe_target() -> i32 {
        7
    }

    #[cfg(windows)]
    #[inline(never)]
    extern "C" fn detour_probe_replacement() -> i32 {
        42
    }

    #[cfg(windows)]
    static DETOUR_PROBE_TRAMPOLINE: AtomicUsize = AtomicUsize::new(0);

    #[cfg(windows)]
    #[inline(never)]
    extern "C" fn detour_probe_replacement_with_trampoline() -> i32 {
        100 + detour_probe_call_trampoline()
    }

    #[cfg(windows)]
    #[inline(never)]
    extern "C" fn lua_registration_chain_probe_target(
        _arg1: *mut c_void,
        _lua_state: *mut c_void,
    ) -> i32 {
        77
    }

    static COMPONENT_LUA_INIT_PROBE_CALLS: AtomicUsize = AtomicUsize::new(0);
    static INPUT_VIDEO_NODE_UPDATE_PROBE_CALLS: AtomicUsize = AtomicUsize::new(0);

    #[inline(never)]
    extern "C" fn component_lua_init_chain_probe_target(context: *mut c_void) {
        assert!(!context.is_null());
        COMPONENT_LUA_INIT_PROBE_CALLS.fetch_add(1, Ordering::SeqCst);
    }

    #[inline(never)]
    extern "C" fn input_video_node_update_chain_probe_target(
        node: *mut c_void,
        source: *mut c_void,
    ) {
        assert!(!node.is_null());
        INPUT_VIDEO_NODE_UPDATE_PROBE_CALLS.fetch_add(1, Ordering::SeqCst);
        unsafe {
            *((node as *mut u8).add(0x30) as *mut usize) = source as usize;
        }
    }

    #[inline(never)]
    extern "C" fn component_context_chain_probe_target(
        _arg1: *mut c_void,
        component: *mut c_void,
    ) -> i32 {
        let expected = format!("component_ptr:{:x}", component as usize);
        assert_eq!(
            current_lua_component_context().as_deref(),
            Some(expected.as_str())
        );
        assert_eq!(lua_component_context_depth(), 1);
        88
    }

    static TEXTURE_UPLOAD_CHAINED_BYTES: [u8; 8] = [90, 91, 92, 250, 93, 94, 95, 251];

    #[inline(never)]
    extern "C" fn texture_upload_rewrites_buffer_probe_target(upload_context: *mut c_void) {
        assert!(!upload_context.is_null());
        unsafe {
            let context = upload_context as *mut FakeTextureUploadContext;
            (*context).data = TEXTURE_UPLOAD_CHAINED_BYTES.as_ptr();
        }
    }

    #[cfg(windows)]
    fn detour_probe_call_trampoline() -> i32 {
        let trampoline = DETOUR_PROBE_TRAMPOLINE.load(Ordering::SeqCst);
        if trampoline == 0 {
            return -1000;
        }
        let original: extern "C" fn() -> i32 = unsafe { std::mem::transmute(trampoline) };
        original()
    }

    #[cfg(windows)]
    unsafe fn alloc_executable_test_code(bytes: &[u8]) -> *mut c_void {
        let ptr = VirtualAlloc(
            ptr::null(),
            bytes.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        ) as *mut u8;
        assert!(!ptr.is_null(), "VirtualAlloc test code failed");
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        assert_ne!(
            FlushInstructionCache(GetCurrentProcess(), ptr as *const c_void, bytes.len()),
            0,
            "FlushInstructionCache test code failed"
        );
        ptr as *mut c_void
    }

    #[cfg(windows)]
    unsafe fn free_executable_test_code(ptr: *mut c_void) {
        if !ptr.is_null() {
            let _ = VirtualFree(ptr, 0, MEM_RELEASE);
        }
    }

    #[cfg(windows)]
    static RAX_LIVE_TARGET_PTR: AtomicUsize = AtomicUsize::new(0);

    #[cfg(windows)]
    extern "C" fn rax_live_replacement() -> i32 {
        let trampoline = DETOUR_PROBE_TRAMPOLINE.load(Ordering::SeqCst);
        if trampoline == 0 {
            return -1000;
        }
        let original: extern "C" fn() -> i32 = unsafe { std::mem::transmute(trampoline) };
        original()
    }

    #[cfg(windows)]
    fn current_process_test_context() -> PluginRuntimeContext {
        PluginRuntimeContext {
            schema_version: 1,
            manager_home: PathBuf::from("."),
            plugin_id: "video_get".to_string(),
            plugin_dir: PathBuf::from("."),
            manifest_path: PathBuf::from("plugin.json"),
            config_path: None,
            signatures_path: PathBuf::from("signatures.json"),
            hook_plan_path: None,
            game_exe: std::env::current_exe().unwrap(),
            game_sha256: "test".to_string(),
            game_build_label: "unit-test".to_string(),
            mode: "unit-test".to_string(),
            process_id: current_process_id(),
            log_dir: PathBuf::from("."),
        }
    }

    #[test]
    #[cfg(windows)]
    fn replace_dll_context_without_pid_matches_current_process_after_configure() {
        let _guard = test_runtime_lock();
        let mut context = current_process_test_context();
        context.mode = "replace_dll".to_string();
        context.process_id = None;
        // replace-DLL bootstraps in-process, but it must still be the configured game executable.
        assert!(current_process_matches_context(Some(&context)));

        let mut state = default_runtime_state();
        state.configured = true;
        state.context = Some(context.clone());
        set_runtime(state);
        let snapshot = runtime_snapshot();
        assert_eq!(
            snapshot
                .context
                .as_ref()
                .and_then(|context| context.process_id),
            None
        );
    }

    #[test]
    #[cfg(windows)]
    fn replace_dll_context_rejects_mismatched_process_exe() {
        let _guard = test_runtime_lock();
        let mut context = current_process_test_context();
        context.mode = "replace_dll".to_string();
        context.process_id = current_process_id();
        context.game_exe = current_process_exe_path()
            .unwrap()
            .with_file_name("not_the_current_process.exe");

        assert!(!current_process_matches_context(Some(&context)));
        let process = current_process_context_value(Some(&context));
        assert_eq!(
            process
                .get("process_exe_matches_context")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            process
                .get("process_matches_context")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
    }

    #[test]
    #[cfg(windows)]
    fn replace_dll_patch_gate_blocks_mismatched_process_exe() {
        let _guard = test_runtime_lock();
        let mut context = current_process_test_context();
        context.mode = "replace_dll".to_string();
        context.process_id = current_process_id();
        context.game_exe = current_process_exe_path()
            .unwrap()
            .with_file_name("not_the_current_process.exe");

        let symbols = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [{"entry": "140000000", "byte_check": {"va": "140000000"}}]
            }
        });
        let mut plan = default_hook_plan();
        plan.patching_allowed = true;
        plan.dry_run_only = false;
        plan.accepted_stages = vec!["lua_api_registration".to_string()];
        plan.hooks = vec![HookPlanEntry {
            label: "unit_replace_dll_mismatched_process".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va: "140000000".to_string(),
            replacement: "stormworks_video_get_unbound_review_stub".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        let mut config = default_video_get_config();
        config.hooking.allow_target_patches = true;
        let dry_run = hook_install_dry_run(Some(&context), &plan, &symbols, &validation);
        let gate = evaluate_target_patch_gate(&config, &plan, &validation, &dry_run);

        assert_eq!(
            dry_run
                .pointer("/process/process_exe_matches_context")
                .and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            gate.get("can_patch").and_then(|value| value.as_bool()),
            Some(false)
        );
        let blockers = gate
            .get("blockers")
            .and_then(|value| value.as_array())
            .unwrap();
        assert!(blockers
            .iter()
            .any(|value| value.as_str() == Some("hook_install_dry_run_failed")));
    }

    #[test]
    #[cfg(windows)]
    fn hook_plan_detour_install_executes_when_gate_opens() {
        let _guard = test_runtime_lock();
        DETOUR_PROBE_TRAMPOLINE.store(0, Ordering::SeqCst);
        assert_eq!(detour_probe_target(), 7);
        let context = current_process_test_context();
        let image_base = read_game_image_base(&context.game_exe).unwrap();
        let runtime_base = current_process_module_base().unwrap();
        let target_rva = (detour_probe_target as *const c_void as u64)
            .checked_sub(runtime_base)
            .unwrap();
        let target_va = hex_u64(image_base + target_rva);
        let symbols = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [{"entry": target_va, "byte_check": {"va": target_va}}]
            }
        });
        let mut plan = default_hook_plan();
        plan.patching_allowed = true;
        plan.dry_run_only = false;
        plan.accepted_stages = vec!["lua_api_registration".to_string()];
        plan.hooks = vec![HookPlanEntry {
            label: "unit_hook_plan_detour_install".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va: target_va.clone(),
            replacement: "stormworks_video_get_test_noarg_detour_hook".to_string(),
            require_trampoline: false,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        let mut config = default_video_get_config();
        config.hooking.allow_target_patches = true;
        let dry_run = hook_install_dry_run(Some(&context), &plan, &symbols, &validation);
        let gate = evaluate_target_patch_gate(&config, &plan, &validation, &dry_run);
        assert_eq!(
            gate.get("can_patch").and_then(|value| value.as_bool()),
            Some(true)
        );
        let install = install_hook_plan_detours(Some(&context), &plan, &symbols, &validation)
            .expect("installing reviewed test detour");
        assert_eq!(
            install
                .get("installed_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(detour_probe_target(), 42);
        uninstall_absolute_jump_detour("unit_hook_plan_detour_install").unwrap();
        assert_eq!(detour_probe_target(), 7);
    }

    #[test]
    #[cfg(windows)]
    fn install_runtime_reports_real_lua_hook_for_component_init_plan() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        COMPONENT_LUA_INIT_PROBE_CALLS.store(0, Ordering::SeqCst);

        let context = current_process_test_context();
        let image_base = read_game_image_base(&context.game_exe).unwrap();
        let runtime_base = current_process_module_base().unwrap();
        let target_rva = (component_lua_init_chain_probe_target as *const c_void as u64)
            .checked_sub(runtime_base)
            .unwrap();
        let target_va = hex_u64(image_base + target_rva);
        let symbols = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [{"entry": target_va, "byte_check": {"va": target_va}}]
            },
            "current_lua_component_context": {"value": [{}]},
            "microprocessor_input_video_node": {"value": [{}]},
            "video_texture_source": {"value": [{}]}
        });
        let mut plan = default_hook_plan();
        plan.patching_allowed = true;
        plan.dry_run_only = false;
        plan.accepted_stages = vec!["lua_api_registration".to_string()];
        plan.game_lua = Some(fake_game_lua_plan());
        plan.hooks = vec![HookPlanEntry {
            label: "unit_replace_dll_component_lua_init".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va,
            replacement: "stormworks_video_get_component_lua_init_hook_arg1".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];

        let mut state = default_runtime_state();
        state.configured = true;
        state.context = Some(context);
        state.config.hooking.allow_target_patches = true;
        state.signature_symbols = symbols;
        state.signature_keys = vec![
            "lua_api_registration".to_string(),
            "current_lua_component_context".to_string(),
            "microprocessor_input_video_node".to_string(),
            "video_texture_source".to_string(),
        ];
        state.signature_symbol_count = state.signature_keys.len();
        state.hook_plan = Some(plan);
        set_runtime(state);

        let installed = install_hook_runtime(false).unwrap();
        assert_eq!(
            installed
                .pointer("/target_patch_gate/can_patch")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            installed
                .get("target_patch_points_modified")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            installed
                .get("real_lua_hook")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            installed
                .pointer("/hook_runtime/real_lua_hook")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(runtime_snapshot().hook_runtime.real_lua_hook, true);

        uninstall_absolute_jump_detour("unit_replace_dll_component_lua_init").unwrap();
        reset_lua_adapter_for_test();
    }

    #[test]
    #[cfg(windows)]
    fn lua_registration_hook_plan_calls_original_then_adds_video_table() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let context = current_process_test_context();
        let image_base = read_game_image_base(&context.game_exe).unwrap();
        let runtime_base = current_process_module_base().unwrap();
        let target_rva = (lua_registration_chain_probe_target as *const c_void as u64)
            .checked_sub(runtime_base)
            .unwrap();
        let target_va = hex_u64(image_base + target_rva);
        let symbols = serde_json::json!({
            "lua_api_registration": {
                "kind": "candidate_functions",
                "value": [{"entry": target_va, "byte_check": {"va": target_va}}]
            }
        });
        let mut plan = default_hook_plan();
        plan.patching_allowed = true;
        plan.dry_run_only = false;
        plan.accepted_stages = vec!["lua_api_registration".to_string()];
        plan.lua_api = Some(fake_lua_api_plan());
        plan.hooks = vec![HookPlanEntry {
            label: "unit_lua_registration_chain".to_string(),
            stage: "lua_api_registration".to_string(),
            target_va: target_va.clone(),
            replacement: "stormworks_video_get_register_lua_api_hook_arg2".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        assert_eq!(
            validation.get("valid").and_then(|value| value.as_bool()),
            Some(true)
        );
        let install = install_hook_plan_detours(Some(&context), &plan, &symbols, &validation)
            .expect("installing chained Lua registration hook");
        assert_eq!(
            install
                .pointer("/lua_api/valid")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            install
                .pointer("/hooks/0/trampoline")
                .and_then(|value| value.as_str())
                .is_some(),
            true
        );

        let status = lua_adapter_status_value();
        assert_eq!(
            status
                .pointer("/hook_original_trampolines/arg2")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        uninstall_absolute_jump_detour("unit_lua_registration_chain").unwrap();
        reset_lua_adapter_for_test();
    }

    #[test]
    fn lua_registration_hook_chain_calls_original_then_adds_video_table() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let api = fake_lua_api();
        assert_eq!(set_lua_hook_api(&api), Ok(()));
        LUA_REGISTRATION_ORIGINAL_ARG2.store(
            lua_registration_chain_probe_target as *const () as usize,
            Ordering::SeqCst,
        );
        let mut lua = FakeLuaState {
            component: "chain_component".to_string(),
            ..FakeLuaState::default()
        };
        let lua_ptr = &mut lua as *mut FakeLuaState as *mut c_void;
        let result = register_lua_api_from_hook_chained(
            lua_ptr,
            LuaHookOriginalCall::Arg2(ptr::null_mut(), lua_ptr),
        )
        .unwrap();
        assert_eq!(result, 77);
        assert_eq!(lua.global_name.as_deref(), Some("video"));
        assert!(matches!(lua.global_table, Some(FakeLuaValue::Table(_))));
        let status = lua_adapter_status_value();
        assert_eq!(
            status
                .get("hook_registrations")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            status
                .get("hook_original_calls")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        reset_lua_adapter_for_test();
    }

    #[test]
    fn component_lua_init_hook_uses_context_plus_8_lua_owner() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        COMPONENT_LUA_INIT_PROBE_CALLS.store(0, Ordering::SeqCst);
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let api = fake_lua_api();
        assert_eq!(set_lua_hook_api(&api), Ok(()));
        COMPONENT_LUA_INIT_ORIGINAL_ARG1.store(
            component_lua_init_chain_probe_target as *const () as usize,
            Ordering::SeqCst,
        );
        let mut lua = FakeLuaState {
            component: "component_lua_init_component".to_string(),
            ..FakeLuaState::default()
        };
        let lua_ptr = &mut lua as *mut FakeLuaState as *mut c_void;
        let mut context_words = [0usize, lua_ptr as usize, 0usize, 0usize];
        let context_ptr = context_words.as_mut_ptr().cast::<c_void>();

        stormworks_video_get_component_lua_init_hook_arg1(context_ptr);
        assert_eq!(COMPONENT_LUA_INIT_PROBE_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(lua.global_name.as_deref(), Some("video"));
        assert!(matches!(lua.global_table, Some(FakeLuaValue::Table(_))));
        let status = lua_adapter_status_value();
        assert_eq!(
            status
                .get("hook_registrations")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            status
                .get("hook_original_calls")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            status
                .pointer("/hook_original_trampolines/component_lua_init_arg1")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        reset_lua_adapter_for_test();
    }

    #[test]
    fn component_lua_init_hook_registers_video_with_game_lua_helpers() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        COMPONENT_LUA_INIT_PROBE_CALLS.store(0, Ordering::SeqCst);
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let helpers = build_game_lua_helpers_from_hook_plan(None, &fake_game_lua_plan()).unwrap();
        set_game_lua_helpers(helpers).unwrap();
        COMPONENT_LUA_INIT_ORIGINAL_ARG1.store(
            component_lua_init_chain_probe_target as *const () as usize,
            Ordering::SeqCst,
        );

        let mut game_lua = FakeGameLuaState::new();
        let game_lua_ptr = game_lua.as_lua_ptr();
        let mut context_words = [
            &mut game_lua as *mut FakeGameLuaState as usize,
            game_lua_ptr as usize,
            0usize,
            0usize,
        ];
        let context_ptr = context_words.as_mut_ptr().cast::<c_void>();

        stormworks_video_get_component_lua_init_hook_arg1(context_ptr);
        assert_eq!(COMPONENT_LUA_INIT_PROBE_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(game_lua.registered_table.as_deref(), Some("video"));
        let component_key = format!("component_lua_context:{:x}", context_ptr as usize);
        assert_eq!(
            game_lua_registered_component_context(game_lua_ptr as usize).as_deref(),
            Some(component_key.as_str())
        );
        assert!(game_lua
            .registered_functions
            .iter()
            .any(|(name, _)| name == "init"));
        assert!(game_lua
            .registered_functions
            .iter()
            .any(|(name, _)| name == "getPackedRGB"));

        let init = game_lua
            .registered_functions
            .iter()
            .find(|(name, _)| name == "init")
            .map(|(_, function)| *function)
            .unwrap();
        game_lua.set_args(&[
            FakeLuaValue::Integer(1),
            FakeLuaValue::Integer(2),
            FakeLuaValue::Integer(1),
            FakeLuaValue::String("rgb".to_string()),
        ]);
        assert_eq!(unsafe { init(game_lua_ptr) }, 2);
        let init_values = game_lua.stack_values(4);
        assert_eq!(init_values.len(), 2);
        assert_eq!(init_values.first(), Some(&FakeLuaValue::Bool(true)));
        assert!(game_lua.arg_slot_calls.contains(&1));
        assert!(game_lua.arg_slot_calls.contains(&2));
        assert!(game_lua.arg_slot_calls.contains(&3));
        assert!(game_lua.arg_slot_calls.contains(&4));
        assert!(game_lua
            .arg_slot_calls
            .contains(&GAME_LUA_FIRST_UPVALUE_INDEX));

        let get_size = game_lua
            .registered_functions
            .iter()
            .find(|(name, _)| name == "getSize")
            .map(|(_, function)| *function)
            .unwrap();
        game_lua.set_args(&[FakeLuaValue::Integer(1)]);
        assert_eq!(unsafe { get_size(game_lua_ptr) }, 2);
        let size_values = game_lua.stack_values(1);
        assert_eq!(size_values.len(), 2);
        assert_eq!(size_values.first(), Some(&FakeLuaValue::Integer(2)));
        assert_eq!(size_values.get(1), Some(&FakeLuaValue::Integer(1)));

        let get_packed_rgb = game_lua
            .registered_functions
            .iter()
            .find(|(name, _)| name == "getPackedRGB")
            .map(|(_, function)| *function)
            .unwrap();
        let bytes = [1u8, 2, 3, 250, 251, 252];
        push_rgb_frame_direct(
            CString::new(component_key.clone()).unwrap().as_ptr(),
            1,
            2,
            1,
            bytes.as_ptr(),
            bytes.len(),
            1,
        )
        .unwrap();
        game_lua.set_args(&[FakeLuaValue::Integer(1)]);
        assert_eq!(unsafe { get_packed_rgb(game_lua_ptr) }, 1);
        let packed_values = game_lua.stack_values(1);
        assert_eq!(packed_values.len(), 1);
        let get_rgb = game_lua
            .registered_functions
            .iter()
            .find(|(name, _)| name == "getRGB")
            .map(|(_, function)| *function)
            .unwrap();
        game_lua.set_args(&[FakeLuaValue::Integer(1)]);
        assert_eq!(unsafe { get_rgb(game_lua_ptr) }, 1);
        let rgb_values = game_lua.stack_values(1);
        assert_eq!(rgb_values.len(), 1);
        let Some(FakeLuaValue::Table(rows)) = rgb_values.first() else {
            panic!("getRGB did not return a table");
        };
        let Some(FakeLuaValue::Table(first_row)) = rows.get("1") else {
            panic!("getRGB row 1 is not a table");
        };
        let Some(FakeLuaValue::Table(first_pixel)) = first_row.get("1") else {
            panic!("getRGB pixel 1 is not a table");
        };
        assert_eq!(first_pixel.get("1"), Some(&FakeLuaValue::Integer(1)));
        assert_eq!(first_pixel.get("2"), Some(&FakeLuaValue::Integer(1)));
        let Some(FakeLuaValue::Table(first_rgb)) = first_pixel.get("3") else {
            panic!("getRGB pixel rgb is not a table");
        };
        assert_eq!(first_rgb.get("1"), Some(&FakeLuaValue::Integer(1)));
        assert_eq!(first_rgb.get("2"), Some(&FakeLuaValue::Integer(2)));
        assert_eq!(first_rgb.get("3"), Some(&FakeLuaValue::Integer(3)));
        let Some(FakeLuaValue::Table(second_pixel)) = first_row.get("2") else {
            panic!("getRGB pixel 2 is not a table");
        };
        let Some(FakeLuaValue::Table(second_rgb)) = second_pixel.get("3") else {
            panic!("getRGB pixel 2 rgb is not a table");
        };
        assert_eq!(second_rgb.get("1"), Some(&FakeLuaValue::Integer(250)));
        assert_eq!(second_rgb.get("2"), Some(&FakeLuaValue::Integer(251)));
        assert_eq!(second_rgb.get("3"), Some(&FakeLuaValue::Integer(252)));

        game_lua.set_args(&[]);
        assert_eq!(unsafe { get_rgb(game_lua_ptr) }, 1);
        let default_rgb_values = game_lua.stack_values(0);
        assert_eq!(default_rgb_values.len(), 1);
        let Some(FakeLuaValue::Table(default_rows)) = default_rgb_values.first() else {
            panic!("default getRGB did not return a table");
        };
        let Some(FakeLuaValue::Table(default_row)) = default_rows.get("1") else {
            panic!("default getRGB row 1 is not a table");
        };
        let Some(FakeLuaValue::Table(default_pixel)) = default_row.get("1") else {
            panic!("default getRGB pixel 1 is not a table");
        };
        let Some(FakeLuaValue::Table(default_rgb)) = default_pixel.get("3") else {
            panic!("default getRGB pixel rgb is not a table");
        };
        assert_eq!(default_rgb.get("1"), Some(&FakeLuaValue::Integer(1)));

        game_lua.set_args(&[
            FakeLuaValue::Integer(2),
            FakeLuaValue::Integer(2),
            FakeLuaValue::Integer(1),
            FakeLuaValue::String("gray".to_string()),
        ]);
        assert_eq!(unsafe { init(game_lua_ptr) }, 2);
        assert_eq!(game_lua.stack_values(4).len(), 2);
        push_rgb_frame_direct(
            CString::new(component_key).unwrap().as_ptr(),
            2,
            2,
            1,
            bytes.as_ptr(),
            bytes.len(),
            1,
        )
        .unwrap();
        let get_gray = game_lua
            .registered_functions
            .iter()
            .find(|(name, _)| name == "getGray")
            .map(|(_, function)| *function)
            .unwrap();
        game_lua.set_args(&[FakeLuaValue::Integer(2)]);
        assert_eq!(unsafe { get_gray(game_lua_ptr) }, 1);
        let gray_values = game_lua.stack_values(1);
        assert_eq!(gray_values.len(), 1);
        let Some(FakeLuaValue::Table(gray_rows)) = gray_values.first() else {
            panic!("getGray did not return a table");
        };
        let Some(FakeLuaValue::Table(gray_row)) = gray_rows.get("1") else {
            panic!("getGray row 1 is not a table");
        };
        let Some(FakeLuaValue::Table(first_gray_pixel)) = gray_row.get("1") else {
            panic!("getGray pixel 1 is not a table");
        };
        assert_eq!(first_gray_pixel.get("1"), Some(&FakeLuaValue::Integer(1)));
        assert_eq!(first_gray_pixel.get("2"), Some(&FakeLuaValue::Integer(1)));
        assert_eq!(first_gray_pixel.get("3"), Some(&FakeLuaValue::Integer(1)));
        let Some(FakeLuaValue::Table(second_gray_pixel)) = gray_row.get("2") else {
            panic!("getGray pixel 2 is not a table");
        };
        assert_eq!(
            second_gray_pixel.get("3"),
            Some(&FakeLuaValue::Integer(250))
        );

        let status = lua_adapter_status_value();
        assert_eq!(
            status
                .pointer("/game_lua_helpers/register_table")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        reset_lua_adapter_for_test();
    }

    #[test]
    fn game_lua_callbacks_prefer_closure_upvalue_component_context() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        COMPONENT_LUA_INIT_PROBE_CALLS.store(0, Ordering::SeqCst);
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let helpers = build_game_lua_helpers_from_hook_plan(None, &fake_game_lua_plan()).unwrap();
        set_game_lua_helpers(helpers).unwrap();
        COMPONENT_LUA_INIT_ORIGINAL_ARG1.store(
            component_lua_init_chain_probe_target as *const () as usize,
            Ordering::SeqCst,
        );

        let mut game_lua = FakeGameLuaState::new();
        let game_lua_ptr = game_lua.as_lua_ptr();
        let mut context_a_words = [
            &mut game_lua as *mut FakeGameLuaState as usize,
            game_lua_ptr as usize,
            0usize,
            0usize,
        ];
        let mut context_b_words = [
            &mut game_lua as *mut FakeGameLuaState as usize,
            game_lua_ptr as usize,
            0usize,
            0usize,
        ];
        let context_a = context_a_words.as_mut_ptr().cast::<c_void>();
        let context_b = context_b_words.as_mut_ptr().cast::<c_void>();

        stormworks_video_get_component_lua_init_hook_arg1(context_a);
        stormworks_video_get_component_lua_init_hook_arg1(context_b);
        assert_eq!(
            game_lua_registered_component_context(game_lua_ptr as usize).as_deref(),
            Some(format!("component_lua_context:{:x}", context_b as usize).as_str())
        );

        let init = game_lua
            .registered_functions
            .iter()
            .find(|(name, _)| name == "init")
            .map(|(_, function)| *function)
            .unwrap();
        let get_size = game_lua
            .registered_functions
            .iter()
            .find(|(name, _)| name == "getSize")
            .map(|(_, function)| *function)
            .unwrap();

        game_lua.set_component_upvalue(context_a as usize);
        game_lua.set_args(&[
            FakeLuaValue::Integer(1),
            FakeLuaValue::Integer(2),
            FakeLuaValue::Integer(1),
            FakeLuaValue::String("rgb".to_string()),
        ]);
        assert_eq!(unsafe { init(game_lua_ptr) }, 2);
        let component_a = format!("component_lua_context:{:x}", context_a as usize);
        assert_eq!(
            frame_size_for_component_slot(&component_a, 1).unwrap(),
            (2, 1)
        );

        game_lua.set_component_upvalue(context_b as usize);
        game_lua.set_args(&[
            FakeLuaValue::Integer(1),
            FakeLuaValue::Integer(3),
            FakeLuaValue::Integer(1),
            FakeLuaValue::String("rgb".to_string()),
        ]);
        assert_eq!(unsafe { init(game_lua_ptr) }, 2);
        let component_b = format!("component_lua_context:{:x}", context_b as usize);
        assert_eq!(
            frame_size_for_component_slot(&component_b, 1).unwrap(),
            (3, 1)
        );

        game_lua.set_component_upvalue(context_a as usize);
        game_lua.set_args(&[FakeLuaValue::Integer(1)]);
        assert_eq!(unsafe { get_size(game_lua_ptr) }, 2);
        let size_values = game_lua.stack_values(1);
        assert_eq!(size_values.first(), Some(&FakeLuaValue::Integer(2)));
        assert_eq!(size_values.get(1), Some(&FakeLuaValue::Integer(1)));

        game_lua.set_component_upvalue(context_b as usize);
        game_lua.set_args(&[FakeLuaValue::Integer(1)]);
        assert_eq!(unsafe { get_size(game_lua_ptr) }, 2);
        let size_values = game_lua.stack_values(1);
        assert_eq!(size_values.first(), Some(&FakeLuaValue::Integer(3)));
        assert_eq!(size_values.get(1), Some(&FakeLuaValue::Integer(1)));

        reset_lua_adapter_for_test();
    }

    #[test]
    fn game_lua_function_pairs_end_with_double_null_sentinel() {
        let pairs = game_lua_function_pairs();
        let last = pairs.last().expect("function pair sentinel missing");
        assert!(last.name.is_null());
        assert!(last.function.is_none());
    }

    #[test]
    fn component_context_hook_chain_wraps_original_call() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        COMPONENT_CONTEXT_ORIGINAL_ARG2.store(
            component_context_chain_probe_target as *const () as usize,
            Ordering::SeqCst,
        );
        let mut component_marker = 0u8;
        let component = &mut component_marker as *mut u8 as *mut c_void;
        let result = component_context_from_hook_chained(ComponentContextHookOriginalCall::Arg2(
            ptr::null_mut(),
            component,
        ))
        .unwrap();
        assert_eq!(result, 88);
        assert_eq!(lua_component_context_depth(), 0);
        assert!(current_lua_component_context().is_none());

        COMPONENT_CONTEXT_ORIGINAL_ARG2.store(0, Ordering::SeqCst);
    }

    #[test]
    fn component_context_hook_argument_shims_select_component_pointer() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut component_marker = 0u8;
        let component = &mut component_marker as *mut u8 as *mut c_void;
        let dummy = ptr::null_mut();

        for label in ["arg1", "arg2", "arg3", "arg4"] {
            let result = match label {
                "arg1" => stormworks_video_get_component_context_hook_arg1(component),
                "arg2" => stormworks_video_get_component_context_hook_arg2(dummy, component),
                "arg3" => stormworks_video_get_component_context_hook_arg3(dummy, dummy, component),
                "arg4" => {
                    stormworks_video_get_component_context_hook_arg4(dummy, dummy, dummy, component)
                }
                _ => unreachable!(),
            };
            assert_eq!(result, 1, "{label}");
            assert_eq!(lua_component_context_depth(), 0, "{label}");
            assert!(current_lua_component_context().is_none(), "{label}");
        }
    }

    #[test]
    fn input_video_hook_argument_shims_bind_current_component_slots() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("input_video_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        let entered = stormworks_video_get_enter_lua_component_context(component.as_ptr());
        assert_eq!(entered, 1);
        let mut source_marker = 0u8;
        let source = &mut source_marker as *mut u8 as *mut c_void;
        let dummy = ptr::null_mut();
        for label in ["arg1", "arg2", "arg3", "arg4"] {
            let result = match label {
                "arg1" => stormworks_video_get_input_video_hook_arg1(source),
                "arg2" => stormworks_video_get_input_video_hook_arg2(dummy, source),
                "arg3" => stormworks_video_get_input_video_hook_arg3(dummy, dummy, source),
                "arg4" => stormworks_video_get_input_video_hook_arg4(dummy, dummy, dummy, source),
                _ => unreachable!(),
            };
            assert_eq!(result, 1, "{label}");
            let slot = require_slot_for_component("input_video_component", 1).unwrap();
            assert_eq!(slot.connected, true, "{label}");
            assert_eq!(slot.input_source_handle, source as u64, "{label}");
        }
        assert_eq!(stormworks_video_get_leave_lua_component_context(), 1);
        assert_eq!(lua_component_context_depth(), 0);
        assert_eq!(
            runtime_snapshot().hook_runtime.input_video_bridge_updates,
            4
        );
    }

    #[test]
    fn input_video_hook_without_context_preserves_original_result() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mut source_marker = 0u8;
        let result = stormworks_video_get_input_video_hook_arg1(
            &mut source_marker as *mut u8 as *mut c_void,
        );
        assert_eq!(result, 1);
        assert_eq!(
            runtime_snapshot().hook_runtime.input_video_bridge_updates,
            0
        );
        assert!(runtime_snapshot()
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("no current component context"));
    }

    #[test]
    fn input_video_hook_maps_lua_script_input_node_to_registered_component_context() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mut component_memory =
            vec![0u8; LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA + 0x100].into_boxed_slice();
        let base = component_memory.as_mut_ptr() as usize;
        let node = base;
        let context = node + LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA;
        let source_handle = 0x1234_5678usize;
        component_memory[0x30..0x38].copy_from_slice(&source_handle.to_le_bytes());
        remember_game_lua_component_context(0xabc, context);

        let component_key = CString::new(format!("component_lua_context:{context:x}")).unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component_key.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        assert_eq!(
            require_slot_for_component(component_key.to_str().unwrap(), 1)
                .unwrap()
                .connected,
            true
        );

        let result =
            stormworks_video_get_input_video_hook_arg2(node as *mut c_void, ptr::null_mut());
        assert_eq!(result, 1);
        let slot = require_slot_for_component(component_key.to_str().unwrap(), 1).unwrap();
        assert_eq!(slot.connected, true);
        assert_eq!(slot.input_source_handle, source_handle as u64);
        assert_eq!(
            runtime_snapshot().hook_runtime.input_video_bridge_updates,
            2
        );
    }

    #[test]
    fn direct_lua_init_reads_existing_lua_script_input_video_node_source() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mut component_memory =
            vec![0u8; LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA + 0x100].into_boxed_slice();
        let node = component_memory.as_mut_ptr() as usize;
        let context = node + LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA;
        let source_handle = 0x2468_ace0usize;
        component_memory[0x30..0x38].copy_from_slice(&source_handle.to_le_bytes());
        remember_game_lua_component_context(0x135, context);

        let component_key = CString::new(format!("component_lua_context:{context:x}")).unwrap();
        let mode = CString::new("rgb").unwrap();
        let init = direct_lua_init(component_key.as_ptr(), 1, 32, 32, mode.as_ptr()).unwrap();

        assert_eq!(
            init.pointer("/native/connected")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            init.pointer("/native/input_source_handle")
                .and_then(|value| value.as_u64()),
            Some(source_handle as u64)
        );
        let slot = require_slot_for_component(component_key.to_str().unwrap(), 1).unwrap();
        assert_eq!(slot.connected, true);
        assert_eq!(slot.input_source_handle, source_handle as u64);
        assert_eq!(
            runtime_snapshot().hook_runtime.input_video_bridge_updates,
            1
        );
    }

    #[test]
    fn input_video_node_update_hook_chains_original_then_binds_registered_lua_node() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        INPUT_VIDEO_NODE_UPDATE_PROBE_CALLS.store(0, Ordering::SeqCst);
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mut component_memory =
            vec![0u8; LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA + 0x100].into_boxed_slice();
        let node = component_memory.as_mut_ptr() as usize;
        let context = node + LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA;
        remember_game_lua_component_context(0xdef, context);

        let component_key = CString::new(format!("component_lua_context:{context:x}")).unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component_key.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        INPUT_VIDEO_NODE_UPDATE_ORIGINAL_ARG2.store(
            input_video_node_update_chain_probe_target as *const () as usize,
            Ordering::SeqCst,
        );

        let selected_source_handle = 0x2468_ace0usize;
        component_memory[0x28..0x30].copy_from_slice(&selected_source_handle.to_le_bytes());
        let resolved_source_handle = 0x9876_5432usize as *mut c_void;
        stormworks_video_get_input_video_node_update_hook_arg2(
            node as *mut c_void,
            resolved_source_handle,
        );

        assert_eq!(
            INPUT_VIDEO_NODE_UPDATE_PROBE_CALLS.load(Ordering::SeqCst),
            1
        );
        let slot = require_slot_for_component(component_key.to_str().unwrap(), 1).unwrap();
        assert_eq!(slot.connected, true);
        assert_eq!(slot.input_source_handle, selected_source_handle as u64);
        assert_eq!(
            slot.input_selected_source_handle,
            selected_source_handle as u64
        );
        assert_eq!(
            slot.input_resolved_source_handle,
            resolved_source_handle as u64
        );
        assert_eq!(
            runtime_snapshot().hook_runtime.input_video_bridge_updates,
            1
        );
        reset_lua_adapter_for_test();
    }

    #[test]
    fn input_video_node_update_hook_marks_slot_disconnected_when_original_clears_node_source() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        INPUT_VIDEO_NODE_UPDATE_PROBE_CALLS.store(0, Ordering::SeqCst);
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let mut component_memory =
            vec![0u8; LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA + 0x100].into_boxed_slice();
        let node = component_memory.as_mut_ptr() as usize;
        let context = node + LUA_SCRIPT_INPUT_VIDEO_NODE_TO_CONTEXT_DELTA;
        component_memory[0x30..0x38].copy_from_slice(&0x1111_2222usize.to_le_bytes());
        remember_game_lua_component_context(0xfed, context);

        let component_key = CString::new(format!("component_lua_context:{context:x}")).unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component_key.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component_key.as_ptr(), 1, true, 0x1111_2222).unwrap();
        INPUT_VIDEO_NODE_UPDATE_ORIGINAL_ARG2.store(
            input_video_node_update_chain_probe_target as *const () as usize,
            Ordering::SeqCst,
        );

        stormworks_video_get_input_video_node_update_hook_arg2(
            node as *mut c_void,
            ptr::null_mut(),
        );

        assert_eq!(
            INPUT_VIDEO_NODE_UPDATE_PROBE_CALLS.load(Ordering::SeqCst),
            1
        );
        let slot = require_slot_for_component(component_key.to_str().unwrap(), 1).unwrap();
        assert_eq!(slot.connected, false);
        assert_eq!(slot.input_source_handle, 0);
        reset_lua_adapter_for_test();
    }

    #[test]
    fn texture_source_hook_pushes_matching_capture_request_frames() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("texture_source_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1234_5678).unwrap();

        let source = 0x1234_5678usize as *mut c_void;
        assert_eq!(
            stormworks_video_get_texture_source_hook_arg2(ptr::null_mut(), source),
            1
        );
        let packed =
            packed_frame_data_for_component_slot("texture_source_component", 1, "rgb").unwrap();
        assert_eq!(packed.source, "texture_source");
        assert_eq!(packed.byte_len, 6);
        let request = capture_request_from_slot(
            runtime_snapshot()
                .slots
                .get(&slot_key("texture_source_component", 1))
                .unwrap(),
        );
        assert_eq!(request.source, 3);
        assert_eq!(
            runtime_snapshot().hook_runtime.texture_source_bridge_frames,
            1
        );
    }

    #[test]
    fn texture_source_hook_without_matching_request_preserves_original_result() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let result = stormworks_video_get_texture_source_hook_arg1(0x9999usize as *mut c_void);
        assert_eq!(result, 1);
        assert_eq!(
            runtime_snapshot().hook_runtime.texture_source_bridge_frames,
            0
        );
        assert!(runtime_snapshot()
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("no connected capture requests"));
    }

    #[repr(C)]
    struct FakeTextureUploadContext {
        pad_00: [u8; 0x08],
        texture_owner: *const FakeTextureOwner,
        data: *const u8,
        pad_18: [u8; 0x10],
        width: u32,
        height: u32,
        format: u32,
        ty: u32,
    }

    #[repr(C)]
    struct FakeTextureOwner {
        pad_00: [u8; 0x08],
        texture: *const FakeTextureObject,
    }

    #[repr(C)]
    struct FakeTextureObject {
        pad_00: [u8; 0x28],
        texture_id: u32,
    }

    fn fake_texture_upload_context(
        bytes: &[u8],
        width: u32,
        height: u32,
        format: u32,
        ty: u32,
    ) -> FakeTextureUploadContext {
        FakeTextureUploadContext {
            pad_00: [0; 0x08],
            texture_owner: ptr::null(),
            data: bytes.as_ptr(),
            pad_18: [0; 0x10],
            width,
            height,
            format,
            ty,
        }
    }

    fn fake_texture_upload_context_with_texture(
        bytes: &[u8],
        width: u32,
        height: u32,
        format: u32,
        ty: u32,
        owner: &FakeTextureOwner,
    ) -> FakeTextureUploadContext {
        FakeTextureUploadContext {
            pad_00: [0; 0x08],
            texture_owner: owner as *const FakeTextureOwner,
            data: bytes.as_ptr(),
            pad_18: [0; 0x10],
            width,
            height,
            format,
            ty,
        }
    }

    #[test]
    fn texture_upload_hook_pushes_matching_initialized_slots() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.min_unbound_texture_upload_width = 1;
        state.config.capture.min_unbound_texture_upload_height = 1;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("texture_upload_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        let bytes = [10u8, 20, 30, 250, 40, 50, 60, 251];
        let mut upload = fake_texture_upload_context(&bytes, 2, 1, 0x1908, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        let packed =
            packed_frame_data_for_component_slot("texture_upload_component", 1, "rgb").unwrap();
        assert_eq!(packed.source, "texture_upload");
        assert_eq!(packed.bytes, vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(runtime_snapshot().hook_runtime.real_video_capture, true);
        assert_eq!(
            runtime_snapshot().hook_runtime.texture_upload_bridge_frames,
            1
        );
    }

    #[test]
    fn texture_upload_hook_uses_entry_snapshot_after_chaining_original() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.min_unbound_texture_upload_width = 1;
        state.config.capture.min_unbound_texture_upload_height = 1;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("texture_upload_snapshot_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        let original = [10u8, 20, 30, 250, 40, 50, 60, 251];
        let mut upload = fake_texture_upload_context(&original, 2, 1, 0x1908, 0x1401);
        TEXTURE_UPLOAD_ORIGINAL_ARG1.store(
            texture_upload_rewrites_buffer_probe_target as *const () as usize,
            Ordering::SeqCst,
        );
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        assert_eq!(upload.data, TEXTURE_UPLOAD_CHAINED_BYTES.as_ptr());
        let packed =
            packed_frame_data_for_component_slot("texture_upload_snapshot_component", 1, "rgb")
                .unwrap();
        assert_eq!(packed.source, "texture_upload");
        assert_eq!(packed.bytes, vec![10, 20, 30, 40, 50, 60]);
        TEXTURE_UPLOAD_ORIGINAL_ARG1.store(0, Ordering::SeqCst);
    }

    #[test]
    fn texture_upload_hook_resizes_upload_to_requested_slot_size() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.min_unbound_texture_upload_width = 1;
        state.config.capture.min_unbound_texture_upload_height = 1;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("texture_upload_resize_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        let bytes = [
            1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        ];
        let mut upload = fake_texture_upload_context(&bytes, 4, 2, 0x1907, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        let packed =
            packed_frame_data_for_component_slot("texture_upload_resize_component", 1, "rgb")
                .unwrap();
        assert_eq!(packed.source, "texture_upload");
        assert_eq!(packed.width, 2);
        assert_eq!(packed.height, 1);
        assert_eq!(packed.bytes, vec![1, 2, 3, 7, 8, 9]);
    }

    #[test]
    fn texture_upload_hook_only_upscales_small_uploads_for_connected_slots() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.min_unbound_texture_upload_width = 1;
        state.config.capture.min_unbound_texture_upload_height = 1;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("texture_upload_no_upscale_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 4, 2, mode.as_ptr()).unwrap();

        let small = [1u8, 2, 3, 4, 5, 6];
        let mut upload = fake_texture_upload_context(&small, 2, 1, 0x1907, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        assert_eq!(
            packed_frame_data_for_component_slot("texture_upload_no_upscale_component", 1, "rgb")
                .unwrap_err(),
            "frame not ready"
        );
        let slot = require_slot_for_component("texture_upload_no_upscale_component", 1).unwrap();
        assert_eq!(slot.connected, false);
        assert!(slot.latest_frame.is_none());

        bind_video_input_direct(component.as_ptr(), 1, true, 0xfeed_beef).unwrap();
        let packed =
            packed_frame_data_for_component_slot("texture_upload_no_upscale_component", 1, "rgb")
                .unwrap();
        assert_eq!(packed.source, "texture_upload");
        assert_eq!(packed.width, 4);
        assert_eq!(packed.height, 2);
        assert_eq!(
            packed.bytes,
            vec![1, 2, 3, 1, 2, 3, 4, 5, 6, 4, 5, 6, 1, 2, 3, 1, 2, 3, 4, 5, 6, 4, 5, 6]
        );

        {
            let mut state = runtime_snapshot();
            let interval = capture_frame_interval(state.config.capture.max_fps);
            state.latest_texture_upload_frame = None;
            state.config.capture.min_unbound_texture_upload_width =
                default_min_unbound_texture_upload_width();
            state.config.capture.min_unbound_texture_upload_height =
                default_min_unbound_texture_upload_height();
            let slot = state
                .slots
                .get_mut(&slot_key("texture_upload_no_upscale_component", 1))
                .unwrap();
            slot.ready = false;
            slot.latest_frame = None;
            slot.texture_upload_handle = None;
            slot.last_texture_upload_at = Some(Instant::now() - interval);
            set_runtime(state);
        }

        let blank = [0u8, 0, 0, 0, 0, 0];
        let mut upload = fake_texture_upload_context(&blank, 2, 1, 0x1907, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );
        let slot = require_slot_for_component("texture_upload_no_upscale_component", 1).unwrap();
        assert!(slot.connected);
        assert!(slot.latest_frame.is_none());
        assert_eq!(slot.texture_upload_handle, None);
        assert_eq!(
            packed_frame_data_for_component_slot("texture_upload_no_upscale_component", 1, "rgb")
                .unwrap_err(),
            "frame not ready"
        );
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_skipped_small_unbound_slots,
            1
        );

        {
            let mut state = runtime_snapshot();
            let interval = capture_frame_interval(state.config.capture.max_fps);
            let slot = state
                .slots
                .get_mut(&slot_key("texture_upload_no_upscale_component", 1))
                .unwrap();
            slot.last_texture_upload_at = Some(Instant::now() - interval);
            set_runtime(state);
        }

        let full = [
            1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        ];
        let mut upload = fake_texture_upload_context(&full, 4, 2, 0x1907, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        let packed =
            packed_frame_data_for_component_slot("texture_upload_no_upscale_component", 1, "rgb")
                .unwrap();
        assert_eq!(packed.source, "texture_upload");
        assert_eq!(
            packed.bytes,
            vec![
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24
            ]
        );
    }

    #[test]
    fn texture_upload_hook_does_not_mark_unbound_slot_connected_from_tiny_upload() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("texture_upload_tiny_unbound_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 2, mode.as_ptr()).unwrap();

        let texture = FakeTextureObject {
            pad_00: [0; 0x28],
            texture_id: 0x4444,
        };
        let owner = FakeTextureOwner {
            pad_00: [0; 0x08],
            texture: &texture as *const FakeTextureObject,
        };
        let bytes = [1u8, 2, 3, 250, 4, 5, 6, 251, 7, 8, 9, 252, 10, 11, 12, 253];
        let mut upload =
            fake_texture_upload_context_with_texture(&bytes, 2, 2, 0x1908, 0x1401, &owner);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        assert_eq!(
            packed_frame_data_for_component_slot("texture_upload_tiny_unbound_component", 1, "rgb")
                .unwrap_err(),
            "frame not ready"
        );
        let slot = require_slot_for_component("texture_upload_tiny_unbound_component", 1).unwrap();
        assert_eq!(slot.connected, false);
        assert_eq!(slot.input_source_handle, 0);
        assert!(slot.latest_frame.is_none());
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_skipped_small_unbound_slots,
            1
        );
    }

    #[test]
    fn texture_upload_hook_caps_updates_to_configured_capture_fps() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.max_fps = 60;
        state.config.capture.min_unbound_texture_upload_width = 1;
        state.config.capture.min_unbound_texture_upload_height = 1;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("texture_upload_fps_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        let first = [1u8, 2, 3, 4, 5, 6];
        let mut upload = fake_texture_upload_context(&first, 2, 1, 0x1907, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );
        let first_frame =
            packed_frame_data_for_component_slot("texture_upload_fps_component", 1, "rgb").unwrap();
        assert_eq!(first_frame.bytes, vec![1, 2, 3, 4, 5, 6]);

        let second = [10u8, 20, 30, 40, 50, 60];
        upload = fake_texture_upload_context(&second, 2, 1, 0x1907, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );
        let still_first =
            packed_frame_data_for_component_slot("texture_upload_fps_component", 1, "rgb").unwrap();
        assert_eq!(still_first.frame_id, first_frame.frame_id);
        assert_eq!(still_first.bytes, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_skipped_fps_slots,
            1
        );

        {
            let mut state = request_runtime_state().unwrap();
            let interval = capture_frame_interval(state.config.capture.max_fps);
            let slot = state
                .slots
                .get_mut(&slot_key("texture_upload_fps_component", 1))
                .unwrap();
            slot.last_texture_upload_at = Some(Instant::now() - interval);
            set_runtime(state);
        }

        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );
        let second_frame =
            packed_frame_data_for_component_slot("texture_upload_fps_component", 1, "rgb").unwrap();
        assert!(second_frame.frame_id > first_frame.frame_id);
        assert_eq!(second_frame.bytes, vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(
            runtime_snapshot().hook_runtime.texture_upload_bridge_frames,
            2
        );
    }

    #[test]
    fn texture_upload_hook_updates_input_connected_slot_without_texture_binding() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.min_unbound_texture_upload_width = 1;
        state.config.capture.min_unbound_texture_upload_height = 1;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let bound_component = CString::new("texture_upload_bound_component").unwrap();
        let fallback_component = CString::new("texture_upload_fallback_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(bound_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        direct_lua_init(fallback_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(bound_component.as_ptr(), 1, true, 0xfeed_beef).unwrap();

        let bytes = [10u8, 20, 30, 250, 40, 50, 60, 251];
        let mut upload = fake_texture_upload_context(&bytes, 2, 1, 0x1908, 0x1401);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        let bound =
            packed_frame_data_for_component_slot("texture_upload_bound_component", 1, "rgb")
                .unwrap();
        assert_eq!(bound.source, "texture_upload");
        assert_eq!(bound.bytes, vec![10, 20, 30, 40, 50, 60]);
        let bound_slot = require_slot_for_component("texture_upload_bound_component", 1).unwrap();
        assert_eq!(bound_slot.input_source_handle, 0xfeed_beef);
        assert_eq!(bound_slot.texture_upload_handle, None);

        let fallback =
            packed_frame_data_for_component_slot("texture_upload_fallback_component", 1, "rgb")
                .unwrap();
        assert_eq!(fallback.source, "texture_upload");
        assert_eq!(fallback.bytes, vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_skipped_bound_slots,
            0
        );
        assert_eq!(
            runtime_snapshot().hook_runtime.texture_upload_bridge_frames,
            2
        );
    }

    #[test]
    fn texture_upload_frame_records_resource_texture_binding() {
        let texture = FakeTextureObject {
            pad_00: [0; 0x28],
            texture_id: 0x4567,
        };
        let owner = FakeTextureOwner {
            pad_00: [0; 0x08],
            texture: &texture as *const FakeTextureObject,
        };
        let bytes = [10u8, 20, 30, 250, 40, 50, 60, 251];
        let mut upload =
            fake_texture_upload_context_with_texture(&bytes, 2, 1, 0x1908, 0x1401, &owner);

        let frame =
            read_texture_upload_frame(&mut upload as *mut FakeTextureUploadContext as *mut c_void)
                .unwrap();
        assert_eq!(frame.destination_texture_handle, Some(0x4567));
        assert_eq!(frame.texture_owner_ptr, Some(&owner as *const _ as u64));
        assert_eq!(
            frame.texture_resource_ptr,
            Some(&texture as *const _ as u64)
        );

        let mut state = default_runtime_state();
        record_texture_upload_resource_binding(&mut state, &frame);
        assert_eq!(
            state
                .gl_texture_bindings
                .get(&(&owner as *const _ as u64))
                .map(|binding| binding.handle),
            Some(0x4567)
        );
        assert_eq!(
            state
                .gl_texture_bindings
                .get(&(&texture as *const _ as u64))
                .map(|binding| binding.handle),
            Some(0x4567)
        );
    }

    #[test]
    fn monitor_render_candidates_use_cached_resource_bindings() {
        let texture = FakeTextureObject {
            pad_00: [0; 0x28],
            texture_id: 0x6789,
        };
        let owner = FakeTextureOwner {
            pad_00: [0; 0x08],
            texture: &texture as *const FakeTextureObject,
        };
        let mut bindings = BTreeMap::new();
        bindings.insert(
            &owner as *const _ as u64,
            GlTextureBinding {
                handle: 0x6789,
                owner_ptr: &owner as *const _ as u64,
                texture_ptr: &texture as *const _ as u64,
                width: 64,
                height: 64,
                last_seen: Instant::now(),
            },
        );
        bindings.insert(
            &texture as *const _ as u64,
            GlTextureBinding {
                handle: 0x678a,
                owner_ptr: &owner as *const _ as u64,
                texture_ptr: &texture as *const _ as u64,
                width: 32,
                height: 32,
                last_seen: Instant::now(),
            },
        );

        let mut seen = BTreeSet::new();
        let mut candidates = Vec::new();
        let mut details = Vec::new();
        collect_monitor_render_resource_candidates(
            0xaaaa,
            &owner as *const _ as usize,
            MONITOR_RENDER_RESOURCE_A_OFFSET,
            64,
            64,
            &bindings,
            &mut seen,
            &mut candidates,
            &mut details,
        );

        assert!(candidates.len() >= 2);
        assert!(candidates.iter().any(|candidate| {
            candidate.handle == 0x6789 && candidate.mapped_from == "resource_wrapper"
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.handle == 0x678a && candidate.mapped_from == "resource_nested_+0x8"
        }));
        assert!(!candidates
            .iter()
            .any(|candidate| candidate.mapped_from == "resource_scan_raw_gl_u32_size_match"));
        assert!(details.iter().any(|detail| detail.contains("mapped=")));
    }

    #[test]
    fn monitor_render_rejects_lua_output_monitor_for_same_component() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let component = "component_lua_context:12345678";
        let key = slot_key(component, 1);
        let mut state = default_runtime_state();
        state.configured = true;
        state.slots.insert(
            key.clone(),
            SlotState {
                component: component.to_string(),
                slot: 1,
                width: 2,
                height: 1,
                mode: "gray".to_string(),
                frame_id: 0,
                ready: false,
                connected: true,
                input_source_handle: 0x1111,
                input_candidate_source_handle: 0x1111,
                input_selected_source_handle: 0,
                input_resolved_source_handle: 0,
                input_upstream_source_handle: 0,
                latest_frame: None,
                texture_upload_handle: None,
                source_texture_handle: None,
                last_texture_upload_at: None,
            },
        );
        state
            .video_source_components
            .insert(0x2222, component.to_string());
        set_runtime(state);

        let inputs = MonitorVideoInputHandles {
            slot_object: 0x2222,
            slot_ref: 0,
            object_handles: InputVideoNodeSourceHandles::default(),
            ref_handles: InputVideoNodeSourceHandles::default(),
        };

        assert!(monitor_render_is_lua_output_for_slots(&[key], inputs).is_some());
    }

    #[test]
    fn texture_upload_hook_prefers_matching_destination_texture_handle() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let bound_component = CString::new("texture_upload_matched_bound_component").unwrap();
        let fallback_component = CString::new("texture_upload_matched_fallback_component").unwrap();
        let other_bound_component = CString::new("texture_upload_matched_other_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(bound_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        direct_lua_init(fallback_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        direct_lua_init(other_bound_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        {
            let mut state = request_runtime_state().unwrap();
            state
                .slots
                .get_mut(&slot_key("texture_upload_matched_bound_component", 1))
                .unwrap()
                .texture_upload_handle = Some(0x1234);
            state
                .slots
                .get_mut(&slot_key("texture_upload_matched_other_component", 1))
                .unwrap()
                .texture_upload_handle = Some(0x7777);
            set_runtime(state);
        }

        let texture = FakeTextureObject {
            pad_00: [0; 0x28],
            texture_id: 0x1234,
        };
        let owner = FakeTextureOwner {
            pad_00: [0; 0x08],
            texture: &texture as *const FakeTextureObject,
        };
        let bytes = [10u8, 20, 30, 250, 40, 50, 60, 251];
        let mut upload =
            fake_texture_upload_context_with_texture(&bytes, 2, 1, 0x1908, 0x1401, &owner);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        let bound = packed_frame_data_for_component_slot(
            "texture_upload_matched_bound_component",
            1,
            "rgb",
        )
        .unwrap();
        assert_eq!(bound.source, "texture_upload");
        assert_eq!(bound.bytes, vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(
            packed_frame_data_for_component_slot(
                "texture_upload_matched_fallback_component",
                1,
                "rgb"
            )
            .unwrap_err(),
            "frame not ready"
        );
        assert_eq!(
            packed_frame_data_for_component_slot(
                "texture_upload_matched_other_component",
                1,
                "rgb"
            )
            .unwrap_err(),
            "frame not ready"
        );
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_skipped_bound_slots,
            2
        );
        assert_eq!(
            runtime_snapshot().hook_runtime.texture_upload_bridge_frames,
            1
        );
    }

    #[test]
    fn texture_upload_hook_lets_connected_unbound_slot_join_matching_destination_texture_handle() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let bound_component = CString::new("texture_upload_join_bound_component").unwrap();
        let connected_component = CString::new("texture_upload_join_connected_component").unwrap();
        let other_bound_component = CString::new("texture_upload_join_other_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(bound_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        direct_lua_init(connected_component.as_ptr(), 1, 4, 2, mode.as_ptr()).unwrap();
        direct_lua_init(other_bound_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(connected_component.as_ptr(), 1, true, 0xfeed_beef).unwrap();
        {
            let mut state = request_runtime_state().unwrap();
            state
                .slots
                .get_mut(&slot_key("texture_upload_join_bound_component", 1))
                .unwrap()
                .texture_upload_handle = Some(0x1234);
            state
                .slots
                .get_mut(&slot_key("texture_upload_join_other_component", 1))
                .unwrap()
                .texture_upload_handle = Some(0x7777);
            set_runtime(state);
        }

        let texture = FakeTextureObject {
            pad_00: [0; 0x28],
            texture_id: 0x1234,
        };
        let owner = FakeTextureOwner {
            pad_00: [0; 0x08],
            texture: &texture as *const FakeTextureObject,
        };
        let bytes = [10u8, 20, 30, 250, 40, 50, 60, 251];
        let mut upload =
            fake_texture_upload_context_with_texture(&bytes, 2, 1, 0x1908, 0x1401, &owner);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        let bound =
            packed_frame_data_for_component_slot("texture_upload_join_bound_component", 1, "rgb")
                .unwrap();
        assert_eq!(bound.source, "texture_upload");
        assert_eq!(bound.bytes, vec![10, 20, 30, 40, 50, 60]);

        let connected = packed_frame_data_for_component_slot(
            "texture_upload_join_connected_component",
            1,
            "rgb",
        )
        .unwrap();
        assert_eq!(connected.source, "texture_upload");
        assert_eq!(connected.width, 4);
        assert_eq!(connected.height, 2);
        assert_eq!(
            connected.bytes,
            vec![
                10, 20, 30, 10, 20, 30, 40, 50, 60, 40, 50, 60, 10, 20, 30, 10, 20, 30, 40, 50, 60,
                40, 50, 60
            ]
        );
        let slot =
            require_slot_for_component("texture_upload_join_connected_component", 1).unwrap();
        assert_eq!(slot.input_source_handle, 0xfeed_beef);
        assert_eq!(slot.texture_upload_handle, Some(0x1234));

        assert_eq!(
            packed_frame_data_for_component_slot("texture_upload_join_other_component", 1, "rgb")
                .unwrap_err(),
            "frame not ready"
        );
        let snapshot = runtime_snapshot();
        assert_eq!(snapshot.hook_runtime.texture_upload_skipped_bound_slots, 1);
        assert_eq!(snapshot.hook_runtime.texture_upload_auto_bound_slots, 1);
        assert_eq!(snapshot.hook_runtime.texture_upload_bridge_frames, 2);
    }

    #[test]
    fn texture_upload_hook_auto_binds_unbound_slot_to_first_destination_texture() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        state.config.capture.max_fps = 60;
        state.config.capture.min_unbound_texture_upload_width = 1;
        state.config.capture.min_unbound_texture_upload_height = 1;
        set_runtime(state);

        let component = CString::new("texture_upload_auto_bind_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();

        let texture_a = FakeTextureObject {
            pad_00: [0; 0x28],
            texture_id: 0x2222,
        };
        let owner_a = FakeTextureOwner {
            pad_00: [0; 0x08],
            texture: &texture_a as *const FakeTextureObject,
        };
        let first = [10u8, 20, 30, 250, 40, 50, 60, 251];
        let mut upload =
            fake_texture_upload_context_with_texture(&first, 2, 1, 0x1908, 0x1401, &owner_a);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );

        let slot = require_slot_for_component("texture_upload_auto_bind_component", 1).unwrap();
        assert_eq!(slot.input_source_handle, 0);
        assert_eq!(slot.texture_upload_handle, Some(0x2222));
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_auto_bound_slots,
            1
        );
        let first_frame =
            packed_frame_data_for_component_slot("texture_upload_auto_bind_component", 1, "rgb")
                .unwrap();
        assert_eq!(first_frame.bytes, vec![10, 20, 30, 40, 50, 60]);

        {
            let mut state = runtime_snapshot();
            let slot = state
                .slots
                .get_mut(&slot_key("texture_upload_auto_bind_component", 1))
                .unwrap();
            slot.last_texture_upload_at =
                Some(Instant::now() - capture_frame_interval(state.config.capture.max_fps));
            set_runtime(state);
        }

        let texture_b = FakeTextureObject {
            pad_00: [0; 0x28],
            texture_id: 0x3333,
        };
        let owner_b = FakeTextureOwner {
            pad_00: [0; 0x08],
            texture: &texture_b as *const FakeTextureObject,
        };
        let other = [80u8, 81, 82, 250, 90, 91, 92, 251];
        upload = fake_texture_upload_context_with_texture(&other, 2, 1, 0x1908, 0x1401, &owner_b);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );
        let still_first =
            packed_frame_data_for_component_slot("texture_upload_auto_bind_component", 1, "rgb")
                .unwrap();
        assert_eq!(still_first.frame_id, first_frame.frame_id);
        assert_eq!(still_first.bytes, vec![10, 20, 30, 40, 50, 60]);
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_skipped_bound_slots,
            1
        );

        {
            let mut state = runtime_snapshot();
            let slot = state
                .slots
                .get_mut(&slot_key("texture_upload_auto_bind_component", 1))
                .unwrap();
            slot.last_texture_upload_at =
                Some(Instant::now() - capture_frame_interval(state.config.capture.max_fps));
            set_runtime(state);
        }

        let next = [1u8, 2, 3, 250, 4, 5, 6, 251];
        upload = fake_texture_upload_context_with_texture(&next, 2, 1, 0x1908, 0x1401, &owner_a);
        stormworks_video_get_texture_upload_hook_arg1(
            &mut upload as *mut FakeTextureUploadContext as *mut c_void,
        );
        let next_frame =
            packed_frame_data_for_component_slot("texture_upload_auto_bind_component", 1, "rgb")
                .unwrap();
        assert!(next_frame.frame_id > first_frame.frame_id);
        assert_eq!(next_frame.bytes, vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(
            runtime_snapshot()
                .hook_runtime
                .texture_upload_auto_bound_slots,
            1
        );
    }

    #[test]
    fn source_texture_candidate_scan_finds_direct_and_nested_texture_handles() {
        #[repr(C)]
        struct FakeSourceNested {
            pad_00: [u8; 0x18],
            texture: u32,
        }

        #[repr(C)]
        struct FakeSourceObject {
            pad_00: [u8; 0x20],
            direct_texture: u32,
            pad_24: [u8; 0x04],
            nested: *const FakeSourceNested,
        }

        let nested = FakeSourceNested {
            pad_00: [0; 0x18],
            texture: 0x456,
        };
        let source = FakeSourceObject {
            pad_00: [0; 0x20],
            direct_texture: 0x123,
            pad_24: [0; 0x04],
            nested: &nested as *const FakeSourceNested,
        };
        let candidates = collect_source_texture_candidates(&source as *const _ as u64, None);
        assert!(candidates.iter().any(|candidate| {
            candidate.handle == 0x123
                && candidate.source_offset == 0x20
                && candidate.pointer_offset.is_none()
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.handle == 0x456
                && candidate.source_offset == 0x18
                && candidate.pointer_offset == Some(0x28)
        }));
        assert!(matches!(
            candidates.first(),
            Some(candidate) if candidate.handle == 0x456
        ));
    }

    #[test]
    fn source_texture_candidate_scan_skips_low_direct_texture_handles() {
        #[repr(C)]
        struct FakeSourceObject {
            pad_00: [u8; 0x0c],
            low_direct_texture: u32,
            pad_10: [u8; 0x10],
            direct_texture: u32,
        }

        let source = FakeSourceObject {
            pad_00: [0; 0x0c],
            low_direct_texture: 0x1cb,
            pad_10: [0; 0x10],
            direct_texture: 0x123,
        };
        let candidates = collect_source_texture_candidates(&source as *const _ as u64, None);
        assert!(!candidates.iter().any(|candidate| candidate.handle == 0x1cb));
        assert!(candidates.iter().any(|candidate| {
            candidate.handle == 0x123
                && candidate.source_offset == 0x20
                && candidate.pointer_offset.is_none()
        }));
    }

    #[test]
    fn source_texture_probe_slots_include_ready_source_texture_for_refresh() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.source_texture_probe_enabled = true;
        state.config.capture.source_texture_probe_unsafe_confirm = true;
        state.slots.insert(
            slot_key("refresh_component", 1),
            SlotState {
                component: "refresh_component".to_string(),
                slot: 1,
                width: 2,
                height: 1,
                mode: "rgb".to_string(),
                frame_id: 7,
                ready: true,
                connected: true,
                input_source_handle: 0x1111_2222,
                input_candidate_source_handle: 0x1111_2222,
                input_selected_source_handle: 0x1111_2222,
                input_resolved_source_handle: 0,
                input_upstream_source_handle: 0,
                latest_frame: Some(FrameBuffer {
                    frame_id: 7,
                    width: 2,
                    height: 1,
                    source: "source_texture".to_string(),
                    rgb: vec![[1, 2, 3], [4, 5, 6]],
                }),
                texture_upload_handle: None,
                source_texture_handle: Some(0x4321),
                last_texture_upload_at: None,
            },
        );
        set_runtime(state);

        let slots = source_texture_probe_slots().unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].component, "refresh_component");
        assert_eq!(slots[0].source_texture_handle, Some(0x4321));
    }

    #[test]
    fn source_texture_probe_slots_are_disabled_by_default() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        state.slots.insert(
            slot_key("disabled_probe_component", 1),
            SlotState {
                component: "disabled_probe_component".to_string(),
                slot: 1,
                width: 2,
                height: 1,
                mode: "rgb".to_string(),
                frame_id: 7,
                ready: false,
                connected: true,
                input_source_handle: 0x1111_2222,
                input_candidate_source_handle: 0x1111_2222,
                input_selected_source_handle: 0x1111_2222,
                input_resolved_source_handle: 0,
                input_upstream_source_handle: 0,
                latest_frame: None,
                texture_upload_handle: None,
                source_texture_handle: None,
                last_texture_upload_at: None,
            },
        );
        set_runtime(state);

        assert!(!source_texture_probe_enabled());
        assert!(source_texture_probe_slots().unwrap().is_empty());
    }

    #[test]
    fn source_texture_probe_requires_unsafe_confirmation() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.capture.source_texture_probe_enabled = true;
        state.config.capture.source_texture_probe_unsafe_confirm = false;
        set_runtime(state);

        assert!(!source_texture_probe_enabled());
    }

    #[test]
    fn disabled_source_texture_frame_is_not_lua_ready() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        let key = slot_key("disabled_source_texture_component", 1);
        state.slots.insert(
            key.clone(),
            SlotState {
                component: "disabled_source_texture_component".to_string(),
                slot: 1,
                width: 2,
                height: 1,
                mode: "rgb".to_string(),
                frame_id: 9,
                ready: true,
                connected: true,
                input_source_handle: 0x1111_2222,
                input_candidate_source_handle: 0x1111_2222,
                input_selected_source_handle: 0x1111_2222,
                input_resolved_source_handle: 0,
                input_upstream_source_handle: 0,
                latest_frame: Some(FrameBuffer {
                    frame_id: 9,
                    width: 2,
                    height: 1,
                    source: "source_texture".to_string(),
                    rgb: vec![[1, 2, 3], [4, 5, 6]],
                }),
                texture_upload_handle: None,
                source_texture_handle: Some(0x66),
                last_texture_upload_at: None,
            },
        );
        set_runtime(state);

        let slot = require_slot_for_component("disabled_source_texture_component", 1).unwrap();
        assert!(!is_slot_ready_for_lua(&slot));
        assert_eq!(capture_request_from_slot(&slot).ready, 0);
        assert_eq!(capture_request_from_slot(&slot).source, 0);
        assert_eq!(
            packed_frame_data_for_component_slot("disabled_source_texture_component", 1, "rgb")
                .unwrap_err(),
            "frame not ready"
        );
    }

    #[test]
    fn source_texture_readback_updates_slot_and_capture_request_source() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        state.config.capture.source_texture_probe_enabled = true;
        state.config.capture.source_texture_probe_unsafe_confirm = true;
        set_runtime(state);

        let component = CString::new("source_texture_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111_2222).unwrap();

        let key = slot_key("source_texture_component", 1);
        let readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: 0x4321,
                source_handle: 0x1111_2222,
                source_offset: 0x20,
                pointer_offset: None,
            },
            width: 4,
            height: 1,
            rgb: vec![[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12]],
        };
        let update = apply_source_texture_readback_to_slot(&key, readback).unwrap();
        assert!(update.updated);
        assert_eq!(update.stats.nonzero_pixels, 4);

        let packed =
            packed_frame_data_for_component_slot("source_texture_component", 1, "rgb").unwrap();
        assert_eq!(packed.source, "source_texture");
        assert_eq!(packed.width, 2);
        assert_eq!(packed.height, 1);
        assert_eq!(packed.bytes, vec![1, 2, 3, 7, 8, 9]);

        let snapshot = runtime_snapshot();
        let slot = snapshot.slots.get(&key).unwrap();
        assert_eq!(slot.texture_upload_handle, None);
        assert_eq!(slot.source_texture_handle, Some(0x4321));
        assert_eq!(slot.input_source_handle, 0x1111_2222);
        assert!(snapshot.hook_runtime.real_video_capture);
        assert_eq!(snapshot.hook_runtime.source_texture_probe_frames, 1);
        assert_eq!(capture_request_from_slot(slot).source, 5);
    }

    #[test]
    fn monitor_render_probe_slots_requires_relation_without_single_slot_fallback() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("monitor_fallback_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111).unwrap();

        let slots = monitor_render_probe_slots(0x2222, 0x2222).unwrap();
        assert!(slots.is_empty());

        let slots = monitor_render_probe_slots(0x2222, 0x1111).unwrap();
        assert_eq!(slots, vec![slot_key("monitor_fallback_component", 1)]);

        let second_component = CString::new("monitor_fallback_component_2").unwrap();
        direct_lua_init(second_component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(second_component.as_ptr(), 1, true, 0x3333).unwrap();

        assert!(monitor_render_probe_slots(0x2222, 0x2222)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn monitor_render_probe_slots_keep_refreshing_exact_ready_slot() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("monitor_refresh_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111).unwrap();
        {
            let mut state = request_runtime_state().unwrap();
            let slot = state
                .slots
                .get_mut(&slot_key("monitor_refresh_component", 1))
                .unwrap();
            slot.ready = true;
            slot.latest_frame = Some(FrameBuffer {
                frame_id: 1,
                width: 2,
                height: 1,
                source: "monitor_render".to_string(),
                rgb: vec![[1, 2, 3], [4, 5, 6]],
            });
            set_runtime(state);
        }

        let slots = monitor_render_probe_slots(0x1111, 0x1111).unwrap();
        assert_eq!(slots, vec![slot_key("monitor_refresh_component", 1)]);
    }

    #[test]
    fn monitor_render_probe_slots_match_nested_monitor_input_reference() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("monitor_nested_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111_2222).unwrap();

        let mut nested = vec![0usize; 0x80 / size_of::<usize>()];
        nested[0x20 / size_of::<usize>()] = 0x1111_2222;
        let mut input_ref = vec![0usize; 0x80 / size_of::<usize>()];
        input_ref[0x10 / size_of::<usize>()] = nested.as_ptr() as usize;

        let slots = monitor_render_probe_slots(input_ref.as_ptr() as u64, 0).unwrap();
        assert!(slots.is_empty());

        let relation =
            monitor_input_slot_relation(&runtime_snapshot(), input_ref.as_ptr() as u64, 0);
        assert!(
            format_monitor_input_slot_relation(&relation)
                .contains("indirect=input_graph_contains_")
                && format_monitor_input_slot_relation(&relation).contains("root+0x10->+0x20"),
            "{}",
            format_monitor_input_slot_relation(&relation)
        );
    }

    #[test]
    fn monitor_render_probe_slots_accept_exact_monitor_input_reference() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let component = CString::new("monitor_exact_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111_2222).unwrap();

        let slots = monitor_render_probe_slots(0x1111_2222, 0).unwrap();
        assert_eq!(slots, vec![slot_key("monitor_exact_component", 1)]);

        let relation = monitor_input_slot_relation(&runtime_snapshot(), 0x1111_2222, 0);
        assert!(
            format_monitor_input_slot_relation(&relation).contains("exact=exact_input"),
            "{}",
            format_monitor_input_slot_relation(&relation)
        );
    }

    #[test]
    fn monitor_render_resource_scan_checks_nested_resource_object() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let texture_bindings = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut candidates = Vec::new();
        let mut details = Vec::new();

        let mut nested = vec![0u8; MONITOR_RESOURCE_SCAN_BYTES];
        let nested_ptr = nested.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned((nested_ptr + 0x20) as *mut u32, 0x42);
        }
        let mut wrapper = vec![0u8; MONITOR_RESOURCE_SCAN_BYTES];
        let wrapper_ptr = wrapper.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned((wrapper_ptr + 0x08) as *mut usize, nested_ptr);
        }

        collect_monitor_render_resource_candidates(
            0xaaaa,
            wrapper_ptr,
            MONITOR_RENDER_RESOURCE_A_OFFSET,
            96,
            32,
            &texture_bindings,
            &mut seen,
            &mut candidates,
            &mut details,
        );

        assert!(details.iter().any(|detail| detail.contains(&format!(
            "resource@0x{:x}={}",
            MONITOR_RENDER_RESOURCE_A_OFFSET,
            format_hex_or_zero(nested_ptr as u64)
        ))));
    }

    #[cfg(windows)]
    #[test]
    fn monitor_render_rejects_untrusted_raw_gl_asset_candidate() {
        let trusted_cache_candidate = MonitorRenderResourceCandidate {
            handle: 0x434,
            monitor: 0xaaaa,
            resource: 0xbbbb,
            resource_offset: 0x48,
            monitor_resource_offset: MONITOR_RENDER_RESOURCE_B_OFFSET,
            mapped_key: 0xcccc,
            mapped_from: "resource_scan_ptr",
            mapped_width: 96,
            mapped_height: 32,
            binding_owner_ptr: 0xcccc,
            binding_texture_ptr: 0xdddd,
            binding_age_ms: 10,
        };
        assert!(monitor_render_candidate_can_update_lua(
            &trusted_cache_candidate,
            96,
            32
        ));
        assert!(
            monitor_render_candidate_readback_skip_reason(&trusted_cache_candidate, 96, 32)
                .is_none()
        );

        let raw_asset_candidate = MonitorRenderResourceCandidate {
            mapped_from: "resource_scan_raw_gl_u32",
            mapped_width: 128,
            mapped_height: 128,
            ..trusted_cache_candidate
        };
        assert!(!monitor_render_candidate_can_update_lua(
            &raw_asset_candidate,
            96,
            32
        ));
        assert_eq!(
            monitor_render_candidate_readback_skip_reason(&raw_asset_candidate, 96, 32),
            Some("mapped_size_mismatch")
        );

        let raw_size_match_candidate = MonitorRenderResourceCandidate {
            mapped_from: "resource_scan_raw_gl_u32_size_match",
            mapped_width: 96,
            mapped_height: 32,
            ..trusted_cache_candidate
        };
        assert!(!monitor_render_candidate_can_update_lua(
            &raw_size_match_candidate,
            96,
            32
        ));
        assert_eq!(
            monitor_render_candidate_readback_skip_reason(&raw_size_match_candidate, 96, 32),
            None
        );

        let cached_size_mismatch_candidate = MonitorRenderResourceCandidate {
            mapped_from: "resource_scan_ptr",
            mapped_width: 288,
            mapped_height: 96,
            ..trusted_cache_candidate
        };
        assert!(!monitor_render_candidate_can_update_lua(
            &cached_size_mismatch_candidate,
            96,
            32
        ));
        assert_eq!(
            monitor_render_candidate_readback_skip_reason(&cached_size_mismatch_candidate, 96, 32),
            Some("mapped_size_mismatch")
        );

        let supersampled_monitor_candidate = MonitorRenderResourceCandidate {
            mapped_from: "resource_scan_ptr",
            mapped_width: 192,
            mapped_height: 64,
            ..trusted_cache_candidate
        };
        assert!(monitor_render_candidate_can_update_lua(
            &supersampled_monitor_candidate,
            96,
            32
        ));
        assert!(monitor_render_candidate_readback_skip_reason(
            &supersampled_monitor_candidate,
            96,
            32
        )
        .is_none());
    }

    #[cfg(windows)]
    #[test]
    fn renderer_pass_target_candidates_are_trusted_monitor_sized_bindings() {
        let mut target = vec![0u8; RENDERER_PASS_TARGET_SCAN_BYTES];
        let mut nested = vec![0u8; RENDERER_PASS_TARGET_NESTED_SCAN_BYTES];
        let target_ptr = target.as_mut_ptr() as usize;
        let nested_ptr = nested.as_mut_ptr() as usize;
        let target_binding_key = 0x1111_2222usize;
        let nested_binding_key = 0x3333_4444usize;
        unsafe {
            ptr::write_unaligned(target.as_mut_ptr() as *mut usize, target_binding_key);
            ptr::write_unaligned(target.as_mut_ptr().add(0x10) as *mut usize, nested_ptr);
            ptr::write_unaligned(
                nested.as_mut_ptr().add(0x18) as *mut usize,
                nested_binding_key,
            );
        }
        let mut bindings = BTreeMap::new();
        bindings.insert(
            target_binding_key as u64,
            GlTextureBinding {
                handle: 0x701,
                owner_ptr: target_binding_key as u64,
                texture_ptr: 0xaaaa,
                width: 96,
                height: 32,
                last_seen: Instant::now(),
            },
        );
        bindings.insert(
            nested_binding_key as u64,
            GlTextureBinding {
                handle: 0x702,
                owner_ptr: nested_binding_key as u64,
                texture_ptr: 0xbbbb,
                width: 96,
                height: 32,
                last_seen: Instant::now(),
            },
        );
        let event = RendererVideoPassEvent {
            renderer: 0,
            render_context: 0,
            scene_state: 0,
            command: 0,
            frame_a: 0,
            frame_b: 0,
            frame_c: 0,
            frame_a_texture: 0,
            frame_b_texture: 0,
            frame_c_texture: 0,
            render_target_primary: target_ptr,
            render_target_secondary: 0,
            render_target_video: 0,
            queue_item: 0,
            queue_item_from: "unit_test",
            queue_item_score: 0,
            queue_monitor: 0xaaaa,
            queue_width: 96,
            queue_height: 32,
            queue_resource_a_ref: 0,
            queue_resource_b_ref: 0,
            queue_resource_a_value: 0,
            queue_resource_b_value: 0,
            queue_monitor_input_slot_object: 0,
            queue_monitor_input_slot_ref: 0,
            queue_monitor_effective_handle: 0,
            queue_monitor_input_relation: String::new(),
            command_flags_0xc8: 0,
            command_flags_0xd8: 0,
            command_flags_0xdc: 0,
            object_relation: String::new(),
            source_relation: String::new(),
            slots: String::new(),
            observed_at: Instant::now(),
        };
        let mut seen = BTreeSet::new();
        let mut candidates = Vec::new();
        let mut details = Vec::new();
        collect_renderer_pass_target_candidates(
            &event,
            0xaaaa,
            96,
            32,
            &bindings,
            &mut seen,
            &mut candidates,
            &mut details,
        );

        assert!(candidates.iter().any(|candidate| {
            candidate.handle == 0x701
                && candidate.mapped_from == "renderer_pass_target_direct"
                && monitor_render_candidate_can_update_lua(candidate, 96, 32)
        }));
        assert!(candidates.iter().any(|candidate| {
            candidate.handle == 0x702
                && candidate.mapped_from == "renderer_pass_target_nested"
                && monitor_render_candidate_can_update_lua(candidate, 96, 32)
        }));
        assert!(details
            .iter()
            .any(|detail| detail.contains("renderer_pass_target_candidates=2")));
    }

    #[test]
    fn monitor_render_readback_updates_slot_and_capture_request_source() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("monitor_render_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111_2222).unwrap();

        let key = slot_key("monitor_render_component", 1);
        let mut monitor = vec![0u8; MONITOR_ACTIVE_OFFSET + size_of::<usize>()];
        let monitor_ptr = monitor.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned((monitor_ptr + MONITOR_ACTIVE_OFFSET) as *mut u8, 1);
            ptr::write_unaligned((monitor_ptr + MONITOR_WIDTH_OFFSET) as *mut u32, 4);
            ptr::write_unaligned((monitor_ptr + MONITOR_HEIGHT_OFFSET) as *mut u32, 1);
        }
        let candidate = MonitorRenderResourceCandidate {
            handle: 0x5678,
            monitor: monitor_ptr,
            resource: 0xbbbb,
            resource_offset: 0x28,
            monitor_resource_offset: 0x4c8,
            mapped_key: 0xbbbb,
            mapped_from: "resource_scan_ptr",
            mapped_width: 4,
            mapped_height: 1,
            binding_owner_ptr: 0xbbbb,
            binding_texture_ptr: 0xcccc,
            binding_age_ms: 0,
        };
        let readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: 0x5678,
                source_handle: 0xbbbb,
                source_offset: 0x28,
                pointer_offset: None,
            },
            width: 4,
            height: 1,
            rgb: vec![[10, 20, 30], [40, 50, 60], [70, 80, 90], [100, 110, 120]],
        };
        let update =
            apply_monitor_render_readback_to_slots(&[key.clone()], candidate, readback).unwrap();
        assert!(update.updated);
        assert_eq!(update.updated_slots, 1);
        assert_eq!(update.stats.nonzero_pixels, 4);

        let packed =
            packed_frame_data_for_component_slot("monitor_render_component", 1, "rgb").unwrap();
        assert_eq!(packed.source, "monitor_render");
        assert_eq!(packed.width, 2);
        assert_eq!(packed.height, 1);
        assert_eq!(packed.bytes, vec![10, 20, 30, 70, 80, 90]);

        let snapshot = runtime_snapshot();
        let slot = snapshot.slots.get(&key).unwrap();
        assert_eq!(slot.source_texture_handle, Some(0x5678));
        assert!(snapshot.hook_runtime.real_video_capture);
        assert_eq!(snapshot.hook_runtime.monitor_render_frames, 1);
        assert_eq!(capture_request_from_slot(slot).source, 6);
    }

    #[test]
    fn monitor_render_readback_rejects_shape_mismatch_at_final_write() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("monitor_render_shape_guard").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111_2222).unwrap();

        let key = slot_key("monitor_render_shape_guard", 1);
        let mut monitor = vec![0u8; MONITOR_ACTIVE_OFFSET + size_of::<usize>()];
        let monitor_ptr = monitor.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned((monitor_ptr + MONITOR_ACTIVE_OFFSET) as *mut u8, 1);
            ptr::write_unaligned((monitor_ptr + MONITOR_WIDTH_OFFSET) as *mut u32, 4);
            ptr::write_unaligned((monitor_ptr + MONITOR_HEIGHT_OFFSET) as *mut u32, 1);
        }
        let candidate = MonitorRenderResourceCandidate {
            handle: 0x5678,
            monitor: monitor_ptr,
            resource: 0xbbbb,
            resource_offset: 0x28,
            monitor_resource_offset: 0x4c8,
            mapped_key: 0xbbbb,
            mapped_from: "resource_scan_raw_gl_u32_size_match",
            mapped_width: 4,
            mapped_height: 1,
            binding_owner_ptr: 0,
            binding_texture_ptr: 0,
            binding_age_ms: 0,
        };
        let readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: 0x5678,
                source_handle: 0xbbbb,
                source_offset: 0x28,
                pointer_offset: None,
            },
            width: 128,
            height: 128,
            rgb: vec![[10, 20, 30]; 128 * 128],
        };
        let update = apply_monitor_render_readback_to_slots(&[key], candidate, readback).unwrap();
        assert!(!update.updated);
        assert_eq!(update.updated_slots, 0);
        assert!(
            packed_frame_data_for_component_slot("monitor_render_shape_guard", 1, "rgb").is_err()
        );

        let snapshot = runtime_snapshot();
        assert_eq!(snapshot.hook_runtime.monitor_render_frames, 0);
    }

    #[test]
    fn additive_monitor_readback_updates_slot_and_runtime_counters() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("additive_monitor_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111_2222).unwrap();

        let key = slot_key("additive_monitor_component", 1);
        let candidate = AdditiveMonitorTextureCandidate {
            handle: 0x6789,
            monitor: 0xaaaa,
            draw_item: 0xbbbb,
            texture_arg: 0xcccc,
            texture_object: 0xdddd,
            handle_offset: 0x28,
            pointer_offset: Some(0x08),
            mapped_from: "texture_video_arg+0x48",
        };
        let readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: 0x6789,
                source_handle: 0x1111_2222,
                source_offset: 0x28,
                pointer_offset: Some(0x08),
            },
            width: 2,
            height: 2,
            rgb: vec![[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12]],
        };
        let update =
            apply_additive_monitor_readback_to_slots(&[key.clone()], candidate, readback).unwrap();
        assert!(update.updated);
        assert_eq!(update.stats.nonzero_pixels, 4);

        let packed =
            packed_frame_data_for_component_slot("additive_monitor_component", 1, "rgb").unwrap();
        assert_eq!(packed.source, "monitor_render");
        assert_eq!(packed.width, 2);
        assert_eq!(packed.height, 1);
        assert_eq!(packed.bytes, vec![1, 2, 3, 4, 5, 6]);

        let snapshot = runtime_snapshot();
        let slot = snapshot.slots.get(&key).unwrap();
        assert_eq!(slot.source_texture_handle, Some(0x6789));
        assert!(snapshot.hook_runtime.real_video_capture);
        assert_eq!(snapshot.hook_runtime.monitor_render_frames, 1);
        assert_eq!(snapshot.hook_runtime.additive_monitor_bind_frames, 1);
        assert_eq!(capture_request_from_slot(slot).source, 6);
    }

    #[test]
    fn additive_monitor_rejects_non_video_arg_candidates_for_lua_updates() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("additive_reject_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111_2222).unwrap();

        let key = slot_key("additive_reject_component", 1);
        let candidate = AdditiveMonitorTextureCandidate {
            handle: 0x53,
            monitor: 0xaaaa,
            draw_item: 0xbbbb,
            texture_arg: 0xcccc,
            texture_object: 0xdddd,
            handle_offset: ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
            pointer_offset: None,
            mapped_from: "arg5_video_texture_object+0x48",
        };
        let readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: 0x53,
                source_handle: 0x1111_2222,
                source_offset: ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
                pointer_offset: None,
            },
            width: 2,
            height: 2,
            rgb: vec![[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12]],
        };
        let update =
            apply_additive_monitor_readback_to_slots(&[key.clone()], candidate, readback).unwrap();
        assert!(!update.updated);
        assert_eq!(update.updated_slots, 0);

        let snapshot = runtime_snapshot();
        let slot = snapshot.slots.get(&key).unwrap();
        assert!(slot.latest_frame.is_none());
        assert_eq!(slot.source_texture_handle, None);
        assert!(!snapshot.hook_runtime.real_video_capture);
        assert_eq!(snapshot.hook_runtime.monitor_render_frames, 0);
    }

    #[test]
    fn additive_monitor_bind_probe_requires_relation_and_ignores_recent_capture() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let mode = CString::new("rgb").unwrap();
        let old_component = CString::new("additive_reloaded_old_component").unwrap();
        direct_lua_init(old_component.as_ptr(), 1, 64, 36, mode.as_ptr()).unwrap();
        bind_video_input_direct(old_component.as_ptr(), 1, true, 0x1111).unwrap();
        let old_key = slot_key("additive_reloaded_old_component", 1);
        let old_candidate = AdditiveMonitorTextureCandidate {
            handle: 0x53,
            monitor: 0,
            draw_item: 0xaaaa,
            texture_arg: 0xbbbb,
            texture_object: 0xcccc,
            handle_offset: 0x48,
            pointer_offset: None,
            mapped_from: "texture_video_arg+0x48",
        };
        let old_readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: 0x53,
                source_handle: 0x1111,
                source_offset: 0x48,
                pointer_offset: None,
            },
            width: 2,
            height: 2,
            rgb: vec![[1, 1, 1], [2, 2, 2], [3, 3, 3], [4, 4, 4]],
        };
        apply_additive_monitor_readback_to_slots(&[old_key.clone()], old_candidate, old_readback)
            .unwrap();

        let new_component = CString::new("additive_reloaded_new_component").unwrap();
        direct_lua_init(new_component.as_ptr(), 1, 64, 64, mode.as_ptr()).unwrap();
        bind_video_input_direct(new_component.as_ptr(), 1, true, 0x2222).unwrap();
        let new_key = slot_key("additive_reloaded_new_component", 1);

        let slots = additive_monitor_bind_probe_slots(0, 0).unwrap();
        assert!(slots.is_empty());

        let slots = additive_monitor_bind_probe_slots(0, 0x2222).unwrap();
        assert_eq!(slots, vec![new_key.clone()]);

        let new_candidate = AdditiveMonitorTextureCandidate {
            handle: 0x87,
            monitor: 0,
            draw_item: 0xaaaa,
            texture_arg: 0xdddd,
            texture_object: 0xeeee,
            handle_offset: 0x48,
            pointer_offset: None,
            mapped_from: "texture_video_arg+0x48",
        };
        let new_readback = SourceTextureReadback {
            candidate: SourceTextureCandidate {
                handle: 0x87,
                source_handle: 0x2222,
                source_offset: 0x48,
                pointer_offset: None,
            },
            width: 2,
            height: 2,
            rgb: vec![[9, 9, 9], [8, 8, 8], [7, 7, 7], [6, 6, 6]],
        };
        apply_additive_monitor_readback_to_slots(&[new_key.clone()], new_candidate, new_readback)
            .unwrap();
        {
            let mut state = request_runtime_state().unwrap();
            let now = Instant::now();
            state
                .slots
                .get_mut(&old_key)
                .unwrap()
                .last_texture_upload_at = Some(now - Duration::from_secs(1));
            state
                .slots
                .get_mut(&new_key)
                .unwrap()
                .last_texture_upload_at = Some(now);
            set_runtime(state);
        }

        let slots = additive_monitor_bind_probe_slots(0, 0).unwrap();
        assert!(slots.is_empty());

        let slots = additive_monitor_bind_probe_slots(0, 0x2222).unwrap();
        assert_eq!(slots, vec![new_key]);
    }

    #[test]
    fn additive_monitor_bind_probe_ignores_unmapped_monitor_texture() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        state.config.mock_render.enabled = false;
        set_runtime(state);

        let component = CString::new("additive_unmapped_monitor_component").unwrap();
        let mode = CString::new("rgb").unwrap();
        direct_lua_init(component.as_ptr(), 1, 2, 1, mode.as_ptr()).unwrap();
        bind_video_input_direct(component.as_ptr(), 1, true, 0x1111).unwrap();

        let mut video_texture_object = vec![0u8; 0x60];
        let base = video_texture_object.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned(
                (base + ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET) as *mut u32,
                0x6789,
            );
        }

        let updated =
            probe_additive_monitor_bind_texture_windows(0, 0, 0, 0, base, 0, 0, 0).unwrap();
        assert_eq!(updated, 0);
        assert!(packed_frame_data_for_component_slot(
            "additive_unmapped_monitor_component",
            1,
            "rgb"
        )
        .is_err());
    }

    #[test]
    fn additive_monitor_draw_item_uses_direct_monitor_pointer() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut monitor = vec![0u8; MONITOR_ACTIVE_OFFSET + size_of::<usize>()];
        let monitor_ptr = monitor.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned((monitor_ptr + MONITOR_ACTIVE_OFFSET) as *mut u8, 1);
            ptr::write_unaligned((monitor_ptr + MONITOR_WIDTH_OFFSET) as *mut u32, 64);
            ptr::write_unaligned((monitor_ptr + MONITOR_HEIGHT_OFFSET) as *mut u32, 64);
        }
        let mut draw_item = vec![0u8; ADDITIVE_MONITOR_DRAW_ITEM_LAYOUT_BYTES];
        let draw_item_ptr = draw_item.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned(draw_item_ptr as *mut usize, monitor_ptr);
        }

        assert_eq!(
            monitor_from_additive_draw_item(draw_item_ptr),
            Some(monitor_ptr)
        );
        let layout = additive_monitor_draw_item_layout(draw_item_ptr);
        assert!(layout.contains("plausible_monitors=+0x0->"));
    }

    #[test]
    fn additive_monitor_candidates_include_arg5_video_texture_object() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut video_texture_object = vec![0u8; 0x60];
        let base = video_texture_object.as_mut_ptr() as usize;
        unsafe {
            ptr::write_unaligned(
                (base + ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET) as *mut u32,
                0x6789,
            );
        }

        let candidates = collect_additive_monitor_texture_candidates(0, 0xbbbb, 0, base);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].handle, 0x6789);
        assert_eq!(candidates[0].mapped_from, "arg5_video_texture_object+0x48");
    }

    #[test]
    fn additive_monitor_unit3_candidate_replaces_same_handle_memory_candidate() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let memory_candidate = AdditiveMonitorTextureCandidate {
            handle: 0x0f,
            monitor: 0,
            draw_item: 0xaaaa,
            texture_arg: 0,
            texture_object: 0xbbbb,
            handle_offset: ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
            pointer_offset: None,
            mapped_from: "arg5_video_texture_object+0x48",
        };
        let unit_candidate = AdditiveMonitorTextureCandidate {
            mapped_from: "gl_bound_unit3_after_additive_bind",
            ..memory_candidate
        };
        let mut candidates = vec![memory_candidate];
        upsert_additive_monitor_bound_unit_candidate(&mut candidates, unit_candidate);
        candidates
            .retain(|candidate| candidate.mapped_from == "gl_bound_unit3_after_additive_bind");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].handle, 0x0f);
        assert_eq!(
            candidates[0].mapped_from,
            "gl_bound_unit3_after_additive_bind"
        );
    }

    #[test]
    fn additive_gl_bind_candidate_prefers_live_unit3_over_memory_candidate() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let memory_candidate = AdditiveMonitorTextureCandidate {
            handle: 0x53,
            monitor: 0xaaaa,
            draw_item: 0xbbbb,
            texture_arg: 0xcccc,
            texture_object: 0xdddd,
            handle_offset: ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
            pointer_offset: None,
            mapped_from: "arg5_video_texture_object+0x48",
        };
        let live_candidate = AdditiveMonitorTextureCandidate {
            mapped_from: "gl_bind_inside_additive_unit3",
            ..memory_candidate
        };
        let mut candidates = vec![memory_candidate, live_candidate];
        candidates.sort_by_key(additive_gl_bind_candidate_rank);
        let mut seen = BTreeSet::new();
        let candidates = candidates
            .into_iter()
            .filter(|candidate| candidate.handle != 0 && seen.insert(candidate.handle))
            .collect::<Vec<_>>();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].handle, 0x53);
        assert_eq!(candidates[0].mapped_from, "gl_bind_inside_additive_unit3");
        assert!(additive_monitor_candidate_can_update_lua(&candidates[0]));
    }

    #[test]
    fn additive_blank_frame_rejects_unconfirmed_unit_textures() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let base_candidate = AdditiveMonitorTextureCandidate {
            handle: 0x53,
            monitor: 0xaaaa,
            draw_item: 0xbbbb,
            texture_arg: 0xcccc,
            texture_object: 0xdddd,
            handle_offset: ADDITIVE_MONITOR_TEXTURE_FALLBACK_HANDLE_OFFSET,
            pointer_offset: None,
            mapped_from: "arg5_video_texture_object+0x48",
        };
        let live_bind_candidate = AdditiveMonitorTextureCandidate {
            mapped_from: "gl_bind_inside_additive_unit3",
            ..base_candidate
        };
        let small_video_unit_candidate = AdditiveMonitorTextureCandidate {
            mapped_from: "gl_bind_inside_additive_unit4",
            ..base_candidate
        };
        let texture_arg_candidate = AdditiveMonitorTextureCandidate {
            mapped_from: "texture_video_arg+0x48",
            ..base_candidate
        };

        assert!(!additive_monitor_blank_candidate_can_update_lua(
            &base_candidate
        ));
        assert!(!additive_monitor_blank_readback_can_update_lua(
            &live_bind_candidate,
            1280,
            720
        ));
        assert!(!additive_monitor_blank_readback_can_update_lua(
            &small_video_unit_candidate,
            256,
            256
        ));
        assert!(!additive_monitor_blank_readback_can_update_lua(
            &small_video_unit_candidate,
            1280,
            720
        ));
        assert!(additive_monitor_blank_readback_can_update_lua(
            &texture_arg_candidate,
            1280,
            720
        ));
    }

    #[test]
    fn texture_upload_rgb_conversion_accepts_gray_rgb_and_rgba() {
        let gray = [8u8, 9];
        let rgb = rgb_from_gl_upload(gray.as_ptr(), 2, 1, 0x1903, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[8, 8, 8], [9, 9, 9]]);

        let luminance = [6u8, 7];
        let rgb = rgb_from_gl_upload(luminance.as_ptr(), 2, 1, 0x1909, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[6, 6, 6], [7, 7, 7]]);

        let luminance_alpha = [6u8, 250, 7, 251];
        let rgb = rgb_from_gl_upload(luminance_alpha.as_ptr(), 2, 1, 0x190a, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[6, 6, 6], [7, 7, 7]]);

        let rg = [8u8, 80, 9, 90];
        let rgb = rgb_from_gl_upload(rg.as_ptr(), 2, 1, 0x8227, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[8, 8, 8], [9, 9, 9]]);

        let packed = [1u8, 2, 3, 4, 5, 6];
        let rgb = rgb_from_gl_upload(packed.as_ptr(), 2, 1, 0x1907, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[1, 2, 3], [4, 5, 6]]);

        let rgba = [1u8, 2, 3, 250, 4, 5, 6, 251];
        let rgb = rgb_from_gl_upload(rgba.as_ptr(), 2, 1, 0x1908, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[1, 2, 3], [4, 5, 6]]);

        let bgr = [3u8, 2, 1, 6, 5, 4];
        let rgb = rgb_from_gl_upload(bgr.as_ptr(), 2, 1, 0x80e0, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[1, 2, 3], [4, 5, 6]]);

        let bgra = [3u8, 2, 1, 250, 6, 5, 4, 251];
        let rgb = rgb_from_gl_upload(bgra.as_ptr(), 2, 1, 0x80e1, 0x1401).unwrap();
        assert_eq!(rgb.rgb, vec![[1, 2, 3], [4, 5, 6]]);
    }

    #[test]
    fn texture_upload_rgb_conversion_skips_default_unpack_padding() {
        let padded_rgb = [
            1u8, 2, 3, 4, 5, 6, 250, 251, 10, 20, 30, 40, 50, 60, 252, 253,
        ];
        let rgb = rgb_from_gl_upload(padded_rgb.as_ptr(), 2, 2, 0x1907, 0x1401).unwrap();
        assert_eq!(
            rgb.rgb,
            vec![[1, 2, 3], [4, 5, 6], [10, 20, 30], [40, 50, 60]]
        );

        let padded_gray = [7u8, 8, 9, 250, 10, 11, 12, 251];
        let gray = rgb_from_gl_upload(padded_gray.as_ptr(), 3, 2, 0x1903, 0x1401).unwrap();
        assert_eq!(
            gray.rgb,
            vec![
                [7, 7, 7],
                [8, 8, 8],
                [9, 9, 9],
                [10, 10, 10],
                [11, 11, 11],
                [12, 12, 12]
            ]
        );

        let padded_rg = [
            7u8, 70, 8, 80, 9, 90, 250, 251, 10, 100, 11, 110, 12, 120, 252, 253,
        ];
        let rg = rgb_from_gl_upload(padded_rg.as_ptr(), 3, 2, 0x8227, 0x1401).unwrap();
        assert_eq!(
            rg.rgb,
            vec![
                [7, 7, 7],
                [8, 8, 8],
                [9, 9, 9],
                [10, 10, 10],
                [11, 11, 11],
                [12, 12, 12]
            ]
        );
    }

    #[test]
    #[cfg(windows)]
    fn component_context_hook_plan_calls_original_inside_context() {
        let _guard = test_runtime_lock();
        reset_lua_adapter_for_test();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let context = current_process_test_context();
        let image_base = read_game_image_base(&context.game_exe).unwrap();
        let runtime_base = current_process_module_base().unwrap();
        let target_rva = (component_context_chain_probe_target as *const c_void as u64)
            .checked_sub(runtime_base)
            .unwrap();
        let target_va = hex_u64(image_base + target_rva);
        let symbols = serde_json::json!({
            "current_lua_component_context": {
                "kind": "candidate_functions",
                "value": [{"entry": target_va, "byte_check": {"va": target_va}}]
            }
        });
        let mut plan = default_hook_plan();
        plan.patching_allowed = true;
        plan.dry_run_only = false;
        plan.required_stage = Some("current_lua_component_context".to_string());
        plan.accepted_stages = vec!["current_lua_component_context".to_string()];
        plan.hooks = vec![HookPlanEntry {
            label: "unit_component_context_chain".to_string(),
            stage: "current_lua_component_context".to_string(),
            target_va: target_va.clone(),
            replacement: "stormworks_video_get_component_context_hook_arg2".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        assert_eq!(
            validation.get("valid").and_then(|value| value.as_bool()),
            Some(true)
        );
        let install = install_hook_plan_detours(Some(&context), &plan, &symbols, &validation)
            .expect("installing chained component context hook");
        assert_eq!(
            install
                .pointer("/component_context/required")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            install
                .pointer("/component_context/trampolines/arg2")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        assert_eq!(lua_component_context_depth(), 0);
        uninstall_absolute_jump_detour("unit_component_context_chain").unwrap();
        reset_lua_adapter_for_test();
    }

    #[test]
    #[cfg(windows)]
    fn input_video_hook_plan_resolves_patch_replacement() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let context = current_process_test_context();
        let image_base = read_game_image_base(&context.game_exe).unwrap();
        let runtime_base = current_process_module_base().unwrap();
        let target_rva = (detour_probe_target as *const c_void as u64)
            .checked_sub(runtime_base)
            .unwrap();
        let target_va = hex_u64(image_base + target_rva);
        let symbols = serde_json::json!({
            "microprocessor_input_video_node": {
                "kind": "candidate_functions",
                "value": [{"entry": target_va, "byte_check": {"va": target_va}}]
            }
        });
        let mut plan = default_hook_plan();
        plan.required_stage = Some("microprocessor_input_video_node".to_string());
        plan.accepted_stages = vec!["microprocessor_input_video_node".to_string()];
        plan.hooks = vec![HookPlanEntry {
            label: "unit_input_video_bridge".to_string(),
            stage: "microprocessor_input_video_node".to_string(),
            target_va,
            replacement: "stormworks_video_get_input_video_hook_arg2".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        assert_eq!(
            validation.get("valid").and_then(|value| value.as_bool()),
            Some(true)
        );
        let dry_run = hook_install_dry_run(Some(&context), &plan, &symbols, &validation);
        assert_eq!(
            dry_run
                .pointer("/input_video/required")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            dry_run
                .pointer("/hooks/0/replacement/usable_for_patch")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    #[cfg(windows)]
    fn texture_source_hook_plan_resolves_patch_replacement() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        let context = current_process_test_context();
        let image_base = read_game_image_base(&context.game_exe).unwrap();
        let runtime_base = current_process_module_base().unwrap();
        let target_rva = (detour_probe_target as *const c_void as u64)
            .checked_sub(runtime_base)
            .unwrap();
        let target_va = hex_u64(image_base + target_rva);
        let symbols = serde_json::json!({
            "video_texture_source": {
                "kind": "candidate_functions",
                "value": [{"entry": target_va, "byte_check": {"va": target_va}}]
            }
        });
        let mut plan = default_hook_plan();
        plan.required_stage = Some("video_texture_source".to_string());
        plan.accepted_stages = vec!["video_texture_source".to_string()];
        plan.hooks = vec![HookPlanEntry {
            label: "unit_texture_source_bridge".to_string(),
            stage: "video_texture_source".to_string(),
            target_va,
            replacement: "stormworks_video_get_texture_source_hook_arg2".to_string(),
            require_trampoline: true,
            patch_len: None,
            enabled: true,
        }];
        let validation = validate_hook_plan(&plan, &symbols);
        assert_eq!(
            validation.get("valid").and_then(|value| value.as_bool()),
            Some(true)
        );
        let dry_run = hook_install_dry_run(Some(&context), &plan, &symbols, &validation);
        assert_eq!(
            dry_run
                .pointer("/texture_source/required")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            dry_run
                .pointer("/hooks/0/replacement/usable_for_patch")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
    }

    #[test]
    #[cfg(windows)]
    fn absolute_jump_detour_installs_and_uninstalls() {
        let _guard = test_runtime_lock();
        assert_eq!(detour_probe_target(), 7);
        install_absolute_jump_detour(
            "unit_detour_probe",
            detour_probe_target as *mut c_void,
            detour_probe_replacement as *const c_void,
        )
        .unwrap();
        assert_eq!(detour_probe_target(), 42);
        let status = detour_status_value();
        assert_eq!(
            status
                .get("installed_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        uninstall_absolute_jump_detour("unit_detour_probe").unwrap();
        assert_eq!(detour_probe_target(), 7);
    }

    #[test]
    #[cfg(windows)]
    fn absolute_jump_detour_trampoline_calls_original() {
        let _guard = test_runtime_lock();
        DETOUR_PROBE_TRAMPOLINE.store(0, Ordering::SeqCst);
        assert_eq!(detour_probe_target(), 7);
        let trampoline = install_absolute_jump_detour_with_trampoline(
            "unit_detour_probe_trampoline",
            detour_probe_target as *mut c_void,
            detour_probe_replacement_with_trampoline as *const c_void,
        )
        .unwrap();
        DETOUR_PROBE_TRAMPOLINE.store(trampoline as usize, Ordering::SeqCst);
        assert_eq!(detour_probe_target(), 107);
        assert_eq!(detour_probe_call_trampoline(), 7);
        let status = detour_status_value();
        assert_eq!(
            status
                .get("trampoline_count")
                .and_then(|value| value.as_u64()),
            Some(1)
        );
        assert_eq!(
            status
                .get("trampoline_bytes_total")
                .and_then(|value| value.as_u64()),
            Some(26)
        );
        uninstall_absolute_jump_detour("unit_detour_probe_trampoline").unwrap();
        DETOUR_PROBE_TRAMPOLINE.store(0, Ordering::SeqCst);
        assert_eq!(detour_probe_target(), 7);
    }

    #[test]
    #[cfg(windows)]
    fn trampoline_jump_back_preserves_rax_live_value() {
        let _guard = test_runtime_lock();
        DETOUR_PROBE_TRAMPOLINE.store(0, Ordering::SeqCst);
        let value: u64 = 0x1122_3344_5566_7788;
        RAX_LIVE_TARGET_PTR.store((&value as *const u64) as usize, Ordering::SeqCst);

        let mut code = Vec::new();
        // mov rax, imm64
        code.extend_from_slice(&[0x48, 0xb8]);
        code.extend_from_slice(&(&RAX_LIVE_TARGET_PTR as *const AtomicUsize as u64).to_le_bytes());
        // mov rax, qword ptr [rax]
        code.extend_from_slice(&[0x48, 0x8b, 0x00]);
        // nop * 5, included in the copied 18-byte patch window.
        code.extend_from_slice(&[0x90; 5]);
        // mov eax, dword ptr [rax]
        code.extend_from_slice(&[0x8b, 0x00]);
        code.push(0xc3);

        let target = unsafe { alloc_executable_test_code(&code) };
        let label = "unit_rax_live_trampoline";
        let original: extern "C" fn() -> i32 = unsafe { std::mem::transmute(target) };
        assert_eq!(original(), 0x5566_7788u32 as i32);

        let trampoline = install_absolute_jump_detour_with_trampoline_len(
            label,
            target,
            rax_live_replacement as *const c_void,
            18,
        )
        .unwrap();
        DETOUR_PROBE_TRAMPOLINE.store(trampoline as usize, Ordering::SeqCst);
        assert_eq!(original(), 0x5566_7788u32 as i32);
        assert_eq!(detour_probe_call_trampoline(), 0x5566_7788u32 as i32);

        uninstall_absolute_jump_detour(label).unwrap();
        DETOUR_PROBE_TRAMPOLINE.store(0, Ordering::SeqCst);
        assert_eq!(original(), 0x5566_7788u32 as i32);
        unsafe { free_executable_test_code(target) };
    }

    #[test]
    fn lua_dispatch_scopes_slots_by_component() {
        let _guard = test_runtime_lock();
        let mut state = default_runtime_state();
        state.configured = true;
        set_runtime(state);

        for component in ["component_a", "component_b"] {
            dispatch_lua_call(LuaDispatchCall {
                function: "init".to_string(),
                args: vec![
                    serde_json::json!(1),
                    serde_json::json!(1),
                    serde_json::json!(1),
                    serde_json::json!("rgb"),
                ],
                component: Some(component.to_string()),
            })
            .unwrap();
        }

        push_rgb_frame(VideoFrameInput {
            slot: 1,
            width: 1,
            height: 1,
            rgb: vec![[10, 20, 30]],
            connected: Some(true),
            component: Some("component_a".to_string()),
        })
        .unwrap();
        push_rgb_frame(VideoFrameInput {
            slot: 1,
            width: 1,
            height: 1,
            rgb: vec![[200, 210, 220]],
            connected: Some(true),
            component: Some("component_b".to_string()),
        })
        .unwrap();

        let a = dispatch_lua_call(LuaDispatchCall {
            function: "getRGB".to_string(),
            args: vec![serde_json::json!(1)],
            component: Some("component_a".to_string()),
        })
        .unwrap();
        let b = dispatch_lua_call(LuaDispatchCall {
            function: "getRGB".to_string(),
            args: vec![serde_json::json!(1)],
            component: Some("component_b".to_string()),
        })
        .unwrap();

        assert_eq!(
            a.pointer("/returns/0/0/0/rgb/0")
                .and_then(|value| value.as_u64()),
            Some(10)
        );
        assert_eq!(
            b.pointer("/returns/0/0/0/rgb/0")
                .and_then(|value| value.as_u64()),
            Some(200)
        );

        let a_packed = dispatch_lua_call(LuaDispatchCall {
            function: "getPackedRGB".to_string(),
            args: vec![serde_json::json!(1)],
            component: Some("component_a".to_string()),
        })
        .unwrap();
        let b_packed = dispatch_lua_call(LuaDispatchCall {
            function: "getPackedRGB".to_string(),
            args: vec![serde_json::json!(1)],
            component: Some("component_b".to_string()),
        })
        .unwrap();
        assert_eq!(
            a_packed
                .pointer("/returns/0/bytes/0")
                .and_then(|value| value.as_u64()),
            Some(10)
        );
        assert_eq!(
            b_packed
                .pointer("/returns/0/bytes/0")
                .and_then(|value| value.as_u64()),
            Some(200)
        );
    }

    #[test]
    fn video_logic_graph_joins_monitor_and_lua_inputs_by_camera_output() {
        let mut edges = BTreeMap::new();
        let camera_a_output = 0x1000usize;
        let camera_b_output = 0x2000usize;
        let monitor_a_input = 0x3000usize;
        let lua_a_input = 0x4000usize;
        let lua_b_input = 0x5000usize;

        edges.insert(monitor_a_input, camera_a_output);
        edges.insert(lua_a_input, camera_a_output);
        edges.insert(lua_b_input, camera_b_output);

        assert!(graph_inputs_share_output(
            &edges,
            monitor_a_input,
            lua_a_input
        ));
        assert!(!graph_inputs_share_output(
            &edges,
            monitor_a_input,
            lua_b_input
        ));

        edges.insert(lua_a_input, camera_b_output);
        assert!(!graph_inputs_share_output(
            &edges,
            monitor_a_input,
            lua_a_input
        ));
        edges.remove(&monitor_a_input);
        assert_eq!(graph_output_for_input(&edges, monitor_a_input), None);
    }

    #[test]
    fn video_logic_graph_hook_replacements_are_patch_eligible() {
        for replacement in [
            "stormworks_video_get_video_output_slot_add_hook_arg2",
            "stormworks_video_get_video_output_slot_remove_hook_arg2",
            "stormworks_video_get_video_output_slot_clear_hook_arg1",
        ] {
            let resolved = resolve_replacement_symbol(replacement);
            assert!(resolved.address.is_some(), "{replacement}");
            assert!(resolved.usable_for_patch, "{replacement}");
        }
    }
}
