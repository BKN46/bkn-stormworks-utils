pub fn run_hook_i32<F, E>(name: &'static str, action: F, set_error: E) -> i32
where
    F: FnOnce() -> Result<i32, String>,
    E: Fn(String),
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            set_error(error);
            -1
        }
        Err(payload) => {
            set_error(format!(
                "hook panic: {name}: {}",
                panic_payload_message(payload.as_ref())
            ));
            -1
        }
    }
}

pub fn run_hook_void<F, E>(name: &'static str, action: F, set_error: E)
where
    F: FnOnce() -> Result<(), String>,
    E: Fn(String),
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => set_error(error),
        Err(payload) => set_error(format!(
            "hook panic: {name}: {}",
            panic_payload_message(payload.as_ref())
        )),
    }
}

pub fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_string()
    }
}
