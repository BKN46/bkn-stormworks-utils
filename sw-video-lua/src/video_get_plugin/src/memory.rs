use std::{ffi::c_void, mem::size_of, ptr};

#[cfg(windows)]
use windows_sys::Win32::System::{
    LibraryLoader::GetModuleHandleW,
    Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS},
};

pub fn read_u32_field(base: usize, offset: usize) -> Option<u32> {
    let address = base.checked_add(offset)?;
    if !memory_range_is_readable(address as *const c_void, size_of::<u32>()) {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(address as *const u32) })
}

pub fn read_i32_pointer(pointer: usize) -> Option<i32> {
    if pointer == 0 || !memory_range_is_readable(pointer as *const c_void, size_of::<i32>()) {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(pointer as *const i32) })
}

pub fn read_u8_field(base: usize, offset: usize) -> Option<u8> {
    let address = base.checked_add(offset)?;
    if !memory_range_is_readable(address as *const c_void, size_of::<u8>()) {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(address as *const u8) })
}

pub fn read_usize_field(base: usize, offset: usize) -> Option<usize> {
    let address = base.checked_add(offset)?;
    if !memory_range_is_readable(address as *const c_void, size_of::<usize>()) {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(address as *const usize) })
}

pub fn read_pointer_target_usize(pointer: usize) -> Option<usize> {
    if pointer == 0 || !pointer_value_looks_process_address(pointer as u64) {
        return None;
    }
    if !memory_range_is_readable(pointer as *const c_void, size_of::<usize>()) {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(pointer as *const usize) })
}

pub unsafe fn read_unaligned_at<T: Copy>(base: *const u8, offset: usize) -> T {
    ptr::read_unaligned(base.add(offset).cast::<T>())
}

#[cfg(windows)]
pub fn memory_range_is_readable(ptr: *const c_void, len: usize) -> bool {
    if ptr.is_null() || len == 0 {
        return false;
    }
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let queried = unsafe { VirtualQuery(ptr, &mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
    if queried == 0 || info.State != MEM_COMMIT {
        return false;
    }
    if info.Protect & PAGE_NOACCESS != 0 || info.Protect & PAGE_GUARD != 0 {
        return false;
    }
    let start = ptr as usize;
    let region_base = info.BaseAddress as usize;
    let Some(offset) = start.checked_sub(region_base) else {
        return false;
    };
    offset
        .checked_add(len)
        .map(|end| end <= info.RegionSize)
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn memory_range_is_readable(ptr: *const c_void, len: usize) -> bool {
    !ptr.is_null() && len > 0
}

pub fn pointer_value_looks_process_address(value: u64) -> bool {
    value >= 0x10000 && value < 0x0000_8000_0000_0000
}

pub fn format_hex_or_zero(value: u64) -> String {
    if value == 0 {
        "0".to_string()
    } else {
        format!("0x{value:x}")
    }
}

pub fn format_hex_usize(value: usize) -> String {
    if value == 0 {
        "0".to_string()
    } else {
        format!("0x{value:x}")
    }
}

pub fn parse_hex_u64_local(value: &str) -> Option<u64> {
    let value = value.trim();
    let value = value.strip_prefix("rva:").unwrap_or(value);
    u64::from_str_radix(value.trim_start_matches("0x"), 16).ok()
}

pub fn hex_u64(value: u64) -> String {
    format!("0x{value:x}")
}

#[cfg(windows)]
pub fn current_process_module_base() -> Option<u64> {
    let handle = unsafe { GetModuleHandleW(ptr::null()) };
    if handle.is_null() {
        None
    } else {
        Some(handle as u64)
    }
}

#[cfg(not(windows))]
pub fn current_process_module_base() -> Option<u64> {
    None
}
