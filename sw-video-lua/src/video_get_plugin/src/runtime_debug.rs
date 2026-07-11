use std::{collections::BTreeMap, sync::atomic::Ordering, thread, time::Duration};

use super::*;
use crate::{
    preview::{sanitize_filename_part, write_frame_preview_bmp},
    time_utils::{elapsed_millis_u64, unix_time_millis},
};

pub(super) fn ensure_runtime_debug_snapshot_started() {
    if RUNTIME_DEBUG_SNAPSHOT_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let _ = thread::Builder::new()
        .name("stormworks-video-get-runtime-heartbeat".to_string())
        .spawn(runtime_debug_snapshot_loop);
}

fn runtime_debug_snapshot_loop() {
    loop {
        let state = runtime_snapshot();
        if state.configured {
            maybe_write_runtime_debug_heartbeat(&state);
        }
        thread::sleep(Duration::from_millis(RUNTIME_DEBUG_HEARTBEAT_INTERVAL_MS));
    }
}

pub(super) fn maybe_write_runtime_debug_snapshot(state: &RuntimeState, reason: &str, force: bool) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        maybe_write_runtime_debug_snapshot_inner(state, reason, force)
    }));
    if let Err(payload) = result {
        let count = RUNTIME_DEBUG_SNAPSHOT_PANIC_COUNT.fetch_add(1, Ordering::Relaxed);
        if count < 4 {
            if let Some(path) = &state.log_path {
                let _ = append_log(
                    path,
                    &format!(
                        "runtime debug snapshot panic: {}",
                        panic_payload_message(payload.as_ref())
                    ),
                );
            }
        }
    }
}

fn maybe_write_runtime_debug_snapshot_inner(state: &RuntimeState, reason: &str, force: bool) {
    let Some(path) = &state.runtime_snapshot_path else {
        return;
    };
    let now_ms = unix_time_millis();
    if !force
        && !claim_runtime_debug_snapshot_write(
            &RUNTIME_DEBUG_SNAPSHOT_LAST_WRITE_MS,
            now_ms,
            RUNTIME_DEBUG_SNAPSHOT_INTERVAL_MS,
        )
    {
        return;
    }
    if force {
        RUNTIME_DEBUG_SNAPSHOT_LAST_WRITE_MS.store(now_ms, Ordering::SeqCst);
        RUNTIME_DEBUG_SNAPSHOT_LAST_JSONL_MS.store(now_ms, Ordering::SeqCst);
    }

    let value = runtime_debug_snapshot_value(state, reason, now_ms);
    let _ = write_json_pretty(path, &value);
    write_runtime_frame_previews(state, now_ms);

    let should_append_jsonl = force
        || claim_runtime_debug_snapshot_write(
            &RUNTIME_DEBUG_SNAPSHOT_LAST_JSONL_MS,
            now_ms,
            RUNTIME_DEBUG_SNAPSHOT_JSONL_INTERVAL_MS,
        );
    if should_append_jsonl {
        if let Some(path) = &state.runtime_snapshot_jsonl_path {
            let _ = append_jsonl(path, &value);
        }
    }
}

fn maybe_write_runtime_debug_heartbeat(state: &RuntimeState) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        maybe_write_runtime_debug_heartbeat_inner(state)
    }));
    if let Err(payload) = result {
        let count = RUNTIME_DEBUG_SNAPSHOT_PANIC_COUNT.fetch_add(1, Ordering::Relaxed);
        if count < 4 {
            if let Some(path) = &state.log_path {
                let _ = append_log(
                    path,
                    &format!(
                        "runtime debug heartbeat panic: {}",
                        panic_payload_message(payload.as_ref())
                    ),
                );
            }
        }
    }
}

fn maybe_write_runtime_debug_heartbeat_inner(state: &RuntimeState) {
    let Some(snapshot_path) = &state.runtime_snapshot_path else {
        return;
    };
    let Some(log_dir) = snapshot_path.parent() else {
        return;
    };
    let now_ms = unix_time_millis();
    let heartbeat_path = log_dir.join("video_get_runtime_heartbeat.json");
    let value = runtime_debug_heartbeat_value(state, now_ms);
    let _ = write_json_pretty(&heartbeat_path, &value);
}

fn claim_runtime_debug_snapshot_write(
    last_write: &AtomicU64,
    now_ms: u64,
    interval_ms: u64,
) -> bool {
    let mut last = last_write.load(Ordering::SeqCst);
    loop {
        if now_ms.saturating_sub(last) < interval_ms {
            return false;
        }
        match last_write.compare_exchange(last, now_ms, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => return true,
            Err(observed) => last = observed,
        }
    }
}

fn runtime_debug_snapshot_value(
    state: &RuntimeState,
    reason: &str,
    now_ms: u64,
) -> serde_json::Value {
    let context = state
        .context
        .as_ref()
        .map(|context| {
            serde_json::json!({
                "mode": context.mode,
                "process_id": context.process_id,
                "current_process_id": current_process_id(),
                "game_build_label": context.game_build_label,
                "game_sha256": context.game_sha256,
                "game_exe": context.game_exe.display().to_string(),
                "plugin_dir": context.plugin_dir.display().to_string(),
                "config_path": context.config_path.as_ref().map(|path| path.display().to_string()),
                "signatures_path": context.signatures_path.display().to_string(),
                "hook_plan_path": context.hook_plan_path.as_ref().map(|path| path.display().to_string()),
                "log_dir": context.log_dir.display().to_string()
            })
        })
        .unwrap_or_else(|| serde_json::json!(null));
    let slot_values = state
        .slots
        .values()
        .map(runtime_debug_slot_value)
        .collect::<Vec<_>>();
    let capture_requests = state
        .slots
        .values()
        .map(|slot| runtime_debug_capture_request_value(capture_request_from_slot(slot)))
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema": "stormworks_video_get_runtime_debug_v1",
        "timestamp": log_timestamp(),
        "timestamp_unix_ms": now_ms,
        "reason": truncate_debug_reason(reason),
        "plugin": "video_get",
        "configured": state.configured,
        "context": context,
        "paths": {
            "log": state.log_path.as_ref().map(|path| path.display().to_string()),
            "load_events": state.load_event_path.as_ref().map(|path| path.display().to_string()),
            "runtime_snapshot": state.runtime_snapshot_path.as_ref().map(|path| path.display().to_string()),
            "runtime_snapshots_jsonl": state.runtime_snapshot_jsonl_path.as_ref().map(|path| path.display().to_string())
        },
        "summary": {
            "slots": state.slots.len(),
            "connected_slots": state.slots.values().filter(|slot| slot.connected).count(),
            "ready_slots": state.slots.values().filter(|slot| is_slot_ready_for_lua(slot)).count(),
            "framed_slots": state.slots.values().filter(|slot| slot.latest_frame.is_some()).count(),
            "frame_sources": runtime_debug_frame_source_counts(state),
            "gl_texture_binding_count": state.gl_texture_bindings.len(),
            "video_node_source_count": state.video_node_sources.len(),
            "monitor_pbo_readback_count": state.monitor_pbo_readbacks.len(),
            "monitor_gl_bind_event_count": state.monitor_gl_bind_events.len(),
            "renderer_video_pass_event_count": state.renderer_video_pass_events.len(),
            "pending_monitor_render_probe_count": state.pending_monitor_render_probes.len()
        },
        "hook_runtime": hook_runtime_status_value(&state.hook_runtime),
        "lua_adapter": lua_adapter_status_value(),
        "detours": detour_status_value(),
        "gl_bind_texture_iat": gl_bind_texture_iat_status_value(),
        "config": {
            "capture_max_fps": state.config.capture.max_fps,
            "source_texture_probe_enabled": state.config.capture.source_texture_probe_enabled,
            "source_texture_probe_unsafe_confirm": state.config.capture.source_texture_probe_unsafe_confirm,
            "mock_render_enabled": state.config.mock_render.enabled,
            "mock_render_max_fps": state.config.mock_render.max_fps,
            "limits": {
                "gray": {
                    "max_width": state.config.limits.gray.max_width,
                    "max_height": state.config.limits.gray.max_height
                },
                "rgb": {
                    "max_width": state.config.limits.rgb.max_width,
                    "max_height": state.config.limits.rgb.max_height
                },
                "max_active_slots": state.config.limits.max_active_slots
            }
        },
        "slots": slot_values,
        "capture_requests": capture_requests,
        "video_node_sources": runtime_debug_video_node_sources_value(state),
        "latest_texture_upload_frame": state.latest_texture_upload_frame.as_ref().map(runtime_debug_texture_upload_frame_value),
        "gl_texture_bindings_recent": runtime_debug_gl_texture_bindings_value(state),
        "monitor_pbo_readbacks": runtime_debug_monitor_pbo_readbacks_value(state),
        "monitor_gl_bind_events_recent": runtime_debug_monitor_gl_bind_events_value(state),
        "renderer_video_pass_events_recent": runtime_debug_renderer_video_pass_events_value(state),
        "pending_monitor_render_probes": runtime_debug_pending_monitor_render_probes_value(state),
        "describe_slots": describe_slots(state),
        "last_error": state.last_error
    })
}

fn runtime_debug_heartbeat_value(state: &RuntimeState, now_ms: u64) -> serde_json::Value {
    let context = state
        .context
        .as_ref()
        .map(|context| {
            serde_json::json!({
                "mode": context.mode,
                "process_id": context.process_id,
                "current_process_id": current_process_id(),
                "game_build_label": context.game_build_label,
                "game_sha256": context.game_sha256,
                "game_exe": context.game_exe.display().to_string(),
                "plugin_dir": context.plugin_dir.display().to_string(),
                "config_path": context.config_path.as_ref().map(|path| path.display().to_string()),
                "log_dir": context.log_dir.display().to_string()
            })
        })
        .unwrap_or_else(|| serde_json::json!(null));
    serde_json::json!({
        "schema": "stormworks_video_get_runtime_heartbeat_v1",
        "timestamp": log_timestamp(),
        "timestamp_unix_ms": now_ms,
        "reason": "heartbeat",
        "plugin": "video_get",
        "configured": state.configured,
        "context": context,
        "summary": runtime_debug_summary_value(state),
        "hook_runtime": hook_runtime_status_value(&state.hook_runtime),
        "config": {
            "capture_max_fps": state.config.capture.max_fps,
            "source_texture_probe_enabled": state.config.capture.source_texture_probe_enabled,
            "source_texture_probe_unsafe_confirm": state.config.capture.source_texture_probe_unsafe_confirm,
            "mock_render_enabled": state.config.mock_render.enabled,
            "mock_render_max_fps": state.config.mock_render.max_fps,
            "limits": {
                "gray": {
                    "max_width": state.config.limits.gray.max_width,
                    "max_height": state.config.limits.gray.max_height
                },
                "rgb": {
                    "max_width": state.config.limits.rgb.max_width,
                    "max_height": state.config.limits.rgb.max_height
                },
                "max_active_slots": state.config.limits.max_active_slots
            }
        },
        "describe_slots": describe_slots(state),
        "last_error": state.last_error
    })
}

fn runtime_debug_summary_value(state: &RuntimeState) -> serde_json::Value {
    serde_json::json!({
        "slots": state.slots.len(),
        "connected_slots": state.slots.values().filter(|slot| slot.connected).count(),
        "ready_slots": state.slots.values().filter(|slot| is_slot_ready_for_lua(slot)).count(),
        "framed_slots": state.slots.values().filter(|slot| slot.latest_frame.is_some()).count(),
        "frame_sources": runtime_debug_frame_source_counts(state),
        "gl_texture_binding_count": state.gl_texture_bindings.len(),
        "video_node_source_count": state.video_node_sources.len(),
        "monitor_pbo_readback_count": state.monitor_pbo_readbacks.len(),
        "monitor_gl_bind_event_count": state.monitor_gl_bind_events.len(),
        "renderer_video_pass_event_count": state.renderer_video_pass_events.len(),
        "pending_monitor_render_probe_count": state.pending_monitor_render_probes.len()
    })
}

fn runtime_debug_slot_value(slot: &SlotState) -> serde_json::Value {
    serde_json::json!({
        "component": slot.component,
        "component_hash": hex_u64(stable_component_hash(&slot.component)),
        "slot": slot.slot,
        "width": slot.width,
        "height": slot.height,
        "mode": slot.mode,
        "frame_id": slot.frame_id,
        "connected": slot.connected,
        "ready": is_slot_ready_for_lua(slot),
        "input_source_handle": format_hex_or_zero(slot.input_source_handle),
        "input_candidate_source_handle": format_hex_or_zero(slot.input_candidate_source_handle),
        "input_selected_source_handle": format_hex_or_zero(slot.input_selected_source_handle),
        "input_resolved_source_handle": format_hex_or_zero(slot.input_resolved_source_handle),
        "input_upstream_source_handle": format_hex_or_zero(slot_upstream_source_handle(slot)),
        "texture_upload_handle": slot.texture_upload_handle.map(format_hex_or_zero),
        "source_texture_handle": slot.source_texture_handle.map(format_hex_or_zero),
        "last_texture_upload_age_ms": slot.last_texture_upload_at.map(elapsed_millis_u64),
        "latest_frame": slot.latest_frame.as_ref().map(runtime_debug_frame_value)
    })
}

fn runtime_debug_frame_value(frame: &FrameBuffer) -> serde_json::Value {
    let stats = pixel_stats_from_rgb(&frame.rgb);
    serde_json::json!({
        "frame_id": frame.frame_id,
        "width": frame.width,
        "height": frame.height,
        "source": frame.source,
        "pixel_count": frame.rgb.len(),
        "rgb_hash": hex_u64(rgb_content_hash(&frame.rgb)),
        "stats": pixel_stats_value(&stats)
    })
}

fn write_runtime_frame_previews(state: &RuntimeState, now_ms: u64) {
    let Some(snapshot_path) = &state.runtime_snapshot_path else {
        return;
    };
    let Some(log_dir) = snapshot_path.parent() else {
        return;
    };
    let preview_dir = log_dir.join("frame_previews");
    for slot in state.slots.values() {
        let Some(frame) = &slot.latest_frame else {
            continue;
        };
        if frame.rgb.is_empty() || frame.width == 0 || frame.height == 0 {
            continue;
        }
        let texture = slot
            .source_texture_handle
            .or(slot.texture_upload_handle)
            .map(format_hex_or_zero)
            .unwrap_or_else(|| "none".to_string());
        let path = preview_dir.join(format!(
            "component_{:016x}_slot{}_frame{}_{}_tex_{}_{}ms.bmp",
            stable_component_hash(&slot.component),
            slot.slot,
            frame.frame_id,
            sanitize_filename_part(&frame.source),
            sanitize_filename_part(&texture),
            now_ms
        ));
        let _ = write_frame_preview_bmp(&path, frame.width, frame.height, &frame.rgb, 8);
    }
}

fn runtime_debug_texture_upload_frame_value(frame: &TextureUploadFrame) -> serde_json::Value {
    serde_json::json!({
        "width": frame.width,
        "height": frame.height,
        "format": hex_u64(u64::from(frame.format)),
        "type": hex_u64(u64::from(frame.ty)),
        "data_ptr": format_hex_usize(frame.data_ptr),
        "context_ptr": format_hex_usize(frame.context_ptr),
        "destination_texture_handle": frame.destination_texture_handle.map(format_hex_or_zero),
        "texture_owner_ptr": frame.texture_owner_ptr.map(format_hex_or_zero),
        "texture_resource_ptr": frame.texture_resource_ptr.map(format_hex_or_zero),
        "pixel_sample": rgb_sample_value(&frame.rgb)
    })
}

fn runtime_debug_gl_texture_bindings_value(state: &RuntimeState) -> Vec<serde_json::Value> {
    let mut bindings = state.gl_texture_bindings.iter().collect::<Vec<_>>();
    bindings.sort_by_key(|(_, binding)| binding.last_seen);
    bindings
        .into_iter()
        .take(64)
        .map(|(mapped_key, binding)| {
            serde_json::json!({
                "mapped_key": format_hex_or_zero(*mapped_key),
                "handle": format_hex_or_zero(u64::from(binding.handle)),
                "owner_ptr": format_hex_or_zero(binding.owner_ptr),
                "texture_ptr": format_hex_or_zero(binding.texture_ptr),
                "width": binding.width,
                "height": binding.height,
                "age_ms": elapsed_millis_u64(binding.last_seen)
            })
        })
        .collect()
}

fn runtime_debug_video_node_sources_value(state: &RuntimeState) -> Vec<serde_json::Value> {
    state
        .video_node_sources
        .iter()
        .take(64)
        .map(|(node, source)| {
            serde_json::json!({
                "node": format_hex_or_zero(*node),
                "candidate": format_hex_or_zero(source.candidate),
                "selected": format_hex_or_zero(source.selected),
                "resolved": format_hex_or_zero(source.resolved),
                "upstream": format_hex_or_zero(source.upstream),
                "effective": format_hex_or_zero(source.effective()),
                "node_layout": input_video_source_debug_layout(*node),
                "candidate_layout": input_video_source_debug_layout(source.candidate),
                "selected_layout": input_video_source_debug_layout(source.selected),
                "resolved_layout": input_video_source_debug_layout(source.resolved),
                "upstream_layout": input_video_source_debug_layout(source.upstream)
            })
        })
        .collect()
}

fn runtime_debug_monitor_pbo_readbacks_value(state: &RuntimeState) -> Vec<serde_json::Value> {
    state
        .monitor_pbo_readbacks
        .iter()
        .take(64)
        .map(|(key, readback)| {
            let pending = readback
                .pending
                .iter()
                .map(|pending| {
                    pending
                        .as_ref()
                        .map(runtime_debug_monitor_pbo_pending_value)
                        .unwrap_or_else(|| serde_json::json!(null))
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "key": format_hex_or_zero(*key),
                "handle": format_hex_or_zero(u64::from(readback.handle)),
                "width": readback.width,
                "height": readback.height,
                "pbos": readback
                    .pbos
                    .iter()
                    .map(|pbo| format_hex_or_zero(u64::from(*pbo)))
                    .collect::<Vec<_>>(),
                "next_index": readback.next_index,
                "pending_count": readback.pending.iter().filter(|pending| pending.is_some()).count(),
                "pending": pending
            })
        })
        .collect()
}

fn runtime_debug_monitor_gl_bind_events_value(state: &RuntimeState) -> Vec<serde_json::Value> {
    state
        .monitor_gl_bind_events
        .iter()
        .rev()
        .take(32)
        .map(|event| {
            serde_json::json!({
                "age_ms": elapsed_millis_u64(event.observed_at),
                "monitor": format_hex_usize(event.monitor),
                "render_context": format_hex_usize(event.render_context),
                "arg3": format_hex_usize(event.arg3),
                "arg4": format_hex_usize(event.arg4),
                "arg5": format_hex_usize(event.arg5),
                "arg6": event.arg6,
                "bind_index": event.bind_index,
                "active_unit": event.active_unit,
                "texture": format_hex_or_zero(u64::from(event.texture)),
                "width": event.width,
                "height": event.height,
                "input_slot_object": format_hex_or_zero(event.input_slot_object),
                "input_slot_ref": format_hex_or_zero(event.input_slot_ref),
                "input_effective_handle": format_hex_or_zero(event.input_effective_handle),
                "input_slot_relation": event.input_slot_relation,
                "source_relation": event.source_relation,
                "slots": event.slots
            })
        })
        .collect()
}

fn runtime_debug_renderer_video_pass_events_value(state: &RuntimeState) -> Vec<serde_json::Value> {
    state
        .renderer_video_pass_events
        .iter()
        .rev()
        .take(32)
        .map(|event| {
            serde_json::json!({
                "age_ms": elapsed_millis_u64(event.observed_at),
                "renderer": format_hex_usize(event.renderer),
                "render_context": format_hex_usize(event.render_context),
                "scene_state": format_hex_usize(event.scene_state),
                "command": format_hex_usize(event.command),
                "frame_a": format_hex_usize(event.frame_a),
                "frame_b": format_hex_usize(event.frame_b),
                "frame_c": format_hex_usize(event.frame_c),
                "frame_a_texture": format_hex_or_zero(u64::from(event.frame_a_texture)),
                "frame_b_texture": format_hex_or_zero(u64::from(event.frame_b_texture)),
                "frame_c_texture": format_hex_or_zero(u64::from(event.frame_c_texture)),
                "render_target_primary": format_hex_usize(event.render_target_primary),
                "render_target_secondary": format_hex_usize(event.render_target_secondary),
                "render_target_video": format_hex_usize(event.render_target_video),
                "queue_item": format_hex_usize(event.queue_item),
                "queue_item_from": event.queue_item_from,
                "queue_item_score": event.queue_item_score,
                "queue_monitor": format_hex_usize(event.queue_monitor),
                "queue_width": event.queue_width,
                "queue_height": event.queue_height,
                "queue_resource_a_ref": format_hex_usize(event.queue_resource_a_ref),
                "queue_resource_b_ref": format_hex_usize(event.queue_resource_b_ref),
                "queue_resource_a_value": format_hex_usize(event.queue_resource_a_value),
                "queue_resource_b_value": format_hex_usize(event.queue_resource_b_value),
                "queue_monitor_input_slot_object": format_hex_or_zero(event.queue_monitor_input_slot_object),
                "queue_monitor_input_slot_ref": format_hex_or_zero(event.queue_monitor_input_slot_ref),
                "queue_monitor_input_ref_decoded": describe_logic_video_ref(event.queue_monitor_input_slot_ref),
                "queue_monitor_effective_handle": format_hex_or_zero(event.queue_monitor_effective_handle),
                "queue_monitor_input_relation": event.queue_monitor_input_relation,
                "command_flags_0xc8": hex_u64(u64::from(event.command_flags_0xc8)),
                "command_flags_0xd8": hex_u64(u64::from(event.command_flags_0xd8)),
                "command_flags_0xdc": hex_u64(u64::from(event.command_flags_0xdc)),
                "object_relation": event.object_relation,
                "source_relation": event.source_relation,
                "slots": event.slots
            })
        })
        .collect()
}

fn runtime_debug_pending_monitor_render_probes_value(
    state: &RuntimeState,
) -> Vec<serde_json::Value> {
    state
        .pending_monitor_render_probes
        .iter()
        .rev()
        .take(32)
        .map(|probe| {
            serde_json::json!({
                "age_ms": elapsed_millis_u64(probe.observed_at),
                "monitor": format_hex_usize(probe.monitor),
                "monitor_width": probe.monitor_width,
                "monitor_height": probe.monitor_height,
                "input_handles": probe.input_handles.iter().map(|value| format_hex_or_zero(*value)).collect::<Vec<_>>(),
                "resource_a": format_hex_usize(probe.resource_a),
                "resource_b": format_hex_usize(probe.resource_b),
                "source": probe.source
            })
        })
        .collect()
}

fn runtime_debug_monitor_pbo_pending_value(pending: &MonitorPboPending) -> serde_json::Value {
    serde_json::json!({
        "pbo": format_hex_or_zero(u64::from(pending.pbo)),
        "width": pending.width,
        "height": pending.height,
        "byte_len": pending.byte_len,
        "sync": format_hex_usize(pending.sync),
        "age_ms": elapsed_millis_u64(pending.submitted_at)
    })
}

fn runtime_debug_capture_request_value(request: VideoGetCaptureRequestV1) -> serde_json::Value {
    serde_json::json!({
        "size": request.size,
        "component_hash": hex_u64(request.component_hash),
        "slot": request.slot,
        "width": request.width,
        "height": request.height,
        "mode": request.mode,
        "ready": request.ready != 0,
        "connected": request.connected != 0,
        "frame_id": request.frame_id,
        "source": request.source,
        "input_source_handle": format_hex_or_zero(request.input_source_handle)
    })
}

fn runtime_debug_frame_source_counts(state: &RuntimeState) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for slot in state.slots.values() {
        let source = slot
            .latest_frame
            .as_ref()
            .map(|frame| frame.source.as_str())
            .unwrap_or("none")
            .to_string();
        *counts.entry(source).or_insert(0) += 1;
    }
    counts
}

fn pixel_stats_value(stats: &PixelStats) -> serde_json::Value {
    serde_json::json!({
        "pixels": stats.pixels,
        "bytes": stats.bytes,
        "nonzero_pixels": stats.nonzero_pixels,
        "nonzero_bytes": stats.nonzero_bytes,
        "min": stats.min,
        "max": stats.max,
        "sample": stats.sample
    })
}

fn rgb_sample_value(rgb: &[[u8; 3]]) -> serde_json::Value {
    let sample = rgb
        .iter()
        .take(8)
        .map(|pixel| vec![pixel[0], pixel[1], pixel[2]])
        .collect::<Vec<_>>();
    let sampled_pixels = rgb.len().min(64);
    let sampled_nonzero_pixels = rgb
        .iter()
        .take(sampled_pixels)
        .filter(|pixel| pixel.iter().any(|channel| *channel != 0))
        .count();
    serde_json::json!({
        "pixel_count": rgb.len(),
        "sampled_pixels": sampled_pixels,
        "sampled_nonzero_pixels": sampled_nonzero_pixels,
        "first_pixels": sample
    })
}

fn truncate_debug_reason(reason: &str) -> String {
    const MAX_CHARS: usize = 180;
    let mut out = reason.chars().take(MAX_CHARS).collect::<String>();
    if reason.chars().count() > MAX_CHARS {
        out.push_str("...");
    }
    out
}
