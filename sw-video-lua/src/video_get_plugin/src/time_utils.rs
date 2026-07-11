use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn elapsed_millis_u64(instant: Instant) -> u64 {
    saturating_u128_to_u64(instant.elapsed().as_millis())
}

pub(crate) fn unix_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| saturating_u128_to_u64(duration.as_millis()))
        .unwrap_or(0)
}

pub(crate) fn saturating_u128_to_u64(value: u128) -> u64 {
    value.min(u128::from(u64::MAX)) as u64
}
