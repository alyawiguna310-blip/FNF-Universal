use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Measurement { pub elapsed: Duration }

pub fn measure<T>(f: impl FnOnce() -> T) -> (T, Measurement) {
    let start = Instant::now();
    let value = f();
    (value, Measurement { elapsed: start.elapsed() })
}
