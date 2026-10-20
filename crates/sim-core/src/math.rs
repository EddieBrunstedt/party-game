//! Transcendental math for simulation code. `std`'s versions call the platform's C math library,
//! whose results differ between Windows and Linux; `libm` is the same pure-Rust code everywhere.
//! Basic arithmetic and `sqrt` are exact IEEE operations and need no wrapper.

pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

pub fn powf(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}

pub fn exp(x: f32) -> f32 {
    libm::expf(x)
}
