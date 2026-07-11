use serde::{Deserialize, Serialize};
use std::ffi::{c_char, c_void};

pub type VideoGetLuaCFunction = unsafe extern "C" fn(*mut c_void) -> i32;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VideoGetLuaApiV1 {
    pub size: u32,
    pub lua_version: u32,
    pub lua_createtable: Option<unsafe extern "C" fn(*mut c_void, i32, i32)>,
    pub lua_pushcclosure: Option<unsafe extern "C" fn(*mut c_void, VideoGetLuaCFunction, i32)>,
    pub lua_setglobal: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub lua_setfield: Option<unsafe extern "C" fn(*mut c_void, i32, *const c_char)>,
    pub lua_rawseti: Option<unsafe extern "C" fn(*mut c_void, i32, i64)>,
    pub lua_pushnil: Option<unsafe extern "C" fn(*mut c_void)>,
    pub lua_pushboolean: Option<unsafe extern "C" fn(*mut c_void, i32)>,
    pub lua_pushinteger: Option<unsafe extern "C" fn(*mut c_void, i64)>,
    pub lua_pushstring: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub luaL_checkinteger: Option<unsafe extern "C" fn(*mut c_void, i32) -> i64>,
    pub luaL_checkstring: Option<unsafe extern "C" fn(*mut c_void, i32) -> *const c_char>,
    pub component_id: Option<unsafe extern "C" fn(*mut c_void, *mut c_char, usize) -> usize>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VideoGetCaptureRequestV1 {
    pub size: u32,
    pub component_hash: u64,
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub mode: u32,
    pub ready: u32,
    pub connected: u32,
    pub frame_id: u64,
    pub source: u32,
    pub input_source_handle: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInit {
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub mode: String,
    #[serde(default)]
    pub component: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrameInput {
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<[u8; 3]>,
    #[serde(default)]
    pub connected: Option<bool>,
    #[serde(default)]
    pub component: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuaDispatchCall {
    pub function: String,
    #[serde(default)]
    pub args: Vec<serde_json::Value>,
    #[serde(default)]
    pub component: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PixelGray {
    pub x: u32,
    pub y: u32,
    pub gray: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PixelRgb {
    pub x: u32,
    pub y: u32,
    pub rgb: [u8; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameInfo {
    pub frame_id: u64,
    pub component: String,
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub mode: String,
    pub source: String,
    pub ready: bool,
    pub connected: bool,
    pub input_source_handle: u64,
    pub input_candidate_source_handle: u64,
    pub input_selected_source_handle: u64,
    pub input_resolved_source_handle: u64,
    pub input_upstream_source_handle: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackedFrame {
    pub frame_id: u64,
    pub component: String,
    pub slot: u32,
    pub width: u32,
    pub height: u32,
    pub mode: String,
    pub source: String,
    pub format: String,
    pub stride: u32,
    pub byte_len: usize,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoGetConfig {
    pub enabled: bool,
    pub limits: VideoGetLimits,
    #[serde(default)]
    pub hooking: HookingConfig,
    #[serde(default)]
    pub capture: CaptureConfig,
    #[serde(default)]
    pub mock_render: MockRenderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoGetLimits {
    pub gray: FrameLimit,
    pub rgb: FrameLimit,
    pub max_active_slots: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameLimit {
    pub max_width: u32,
    pub max_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookingConfig {
    #[serde(default = "default_true")]
    pub auto_install: bool,
    #[serde(default = "default_true")]
    pub fail_closed: bool,
    #[serde(default = "default_true")]
    pub require_gate_for_target_patches: bool,
    #[serde(default)]
    pub allow_target_patches: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    #[serde(default = "default_capture_fps")]
    pub max_fps: u32,
    #[serde(default = "default_min_unbound_texture_upload_width")]
    pub min_unbound_texture_upload_width: u32,
    #[serde(default = "default_min_unbound_texture_upload_height")]
    pub min_unbound_texture_upload_height: u32,
    #[serde(default)]
    pub source_texture_probe_enabled: bool,
    #[serde(default)]
    pub source_texture_probe_unsafe_confirm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPlan {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub patching_allowed: bool,
    #[serde(default)]
    pub dry_run_only: bool,
    #[serde(default)]
    pub required_stage: Option<String>,
    #[serde(default)]
    pub accepted_stages: Vec<String>,
    #[serde(default)]
    pub lua_api: Option<HookPlanLuaApi>,
    #[serde(default)]
    pub game_lua: Option<HookPlanGameLua>,
    #[serde(default)]
    pub hooks: Vec<HookPlanEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookPlanLuaApi {
    #[serde(default)]
    pub lua_version: u32,
    #[serde(default)]
    pub lua_createtable: Option<String>,
    #[serde(default)]
    pub lua_pushcclosure: Option<String>,
    #[serde(default)]
    pub lua_setglobal: Option<String>,
    #[serde(default)]
    pub lua_setfield: Option<String>,
    #[serde(default)]
    pub lua_rawseti: Option<String>,
    #[serde(default)]
    pub lua_pushnil: Option<String>,
    #[serde(default)]
    pub lua_pushboolean: Option<String>,
    #[serde(default)]
    pub lua_pushinteger: Option<String>,
    #[serde(default)]
    pub lua_pushstring: Option<String>,
    #[serde(default)]
    pub luaL_checkinteger: Option<String>,
    #[serde(default)]
    pub luaL_checkstring: Option<String>,
    #[serde(default)]
    pub component_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookPlanGameLua {
    #[serde(default)]
    pub create_table: Option<String>,
    #[serde(default)]
    pub push_string: Option<String>,
    #[serde(default)]
    pub rawseti: Option<String>,
    #[serde(default)]
    pub register_table: Option<String>,
    #[serde(default)]
    pub arg_slot: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookPlanEntry {
    pub label: String,
    pub stage: String,
    pub target_va: String,
    pub replacement: String,
    #[serde(default = "default_true")]
    pub require_trampoline: bool,
    #[serde(default)]
    pub patch_len: Option<usize>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockRenderConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_mock_render_fps")]
    pub max_fps: u32,
    #[serde(default = "default_true")]
    pub update_initialized_slots: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureFile {
    pub game_sha256: String,
    pub symbols: serde_json::Value,
}

pub fn default_video_get_config() -> VideoGetConfig {
    VideoGetConfig {
        enabled: true,
        limits: VideoGetLimits {
            gray: FrameLimit {
                max_width: 160,
                max_height: 90,
            },
            rgb: FrameLimit {
                max_width: 64,
                max_height: 64,
            },
            max_active_slots: 4,
        },
        hooking: HookingConfig::default(),
        capture: CaptureConfig::default(),
        mock_render: MockRenderConfig::default(),
    }
}

impl Default for HookingConfig {
    fn default() -> Self {
        Self {
            auto_install: true,
            fail_closed: true,
            require_gate_for_target_patches: true,
            allow_target_patches: false,
        }
    }
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            max_fps: default_capture_fps(),
            min_unbound_texture_upload_width: default_min_unbound_texture_upload_width(),
            min_unbound_texture_upload_height: default_min_unbound_texture_upload_height(),
            source_texture_probe_enabled: false,
            source_texture_probe_unsafe_confirm: false,
        }
    }
}

impl Default for MockRenderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_fps: default_mock_render_fps(),
            update_initialized_slots: true,
        }
    }
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_capture_fps() -> u32 {
    60
}

pub(crate) fn default_min_unbound_texture_upload_width() -> u32 {
    16
}

pub(crate) fn default_min_unbound_texture_upload_height() -> u32 {
    16
}

pub(crate) fn default_mock_render_fps() -> u32 {
    60
}
