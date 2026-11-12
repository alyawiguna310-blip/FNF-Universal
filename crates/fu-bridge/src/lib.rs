//! Minimal native ABI foundation. Flutter bindings are not wired yet.

#[no_mangle]
pub extern "C" fn fu_core_version() -> u32 { 1 }
