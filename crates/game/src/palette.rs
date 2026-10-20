use bevy::prelude::*;
use sim_core::Seat;

/// Player colours. Always paired with the player number and marker shape, never alone.
pub fn seat_color(seat: Seat) -> LinearRgba {
    match seat.0 {
        0 => LinearRgba::rgb(1.0, 0.55, 0.12),
        1 => LinearRgba::rgb(0.12, 0.8, 1.0),
        2 => LinearRgba::rgb(1.0, 0.22, 0.7),
        _ => LinearRgba::rgb(0.5, 1.0, 0.18),
    }
}

pub fn seat_label(seat: Seat) -> String {
    format!("P{}", seat.0 + 1)
}

pub const NIGHT_SKY: Color = Color::srgb(0.012, 0.016, 0.04);
pub const LANTERN_WARM: LinearRgba = LinearRgba::rgb(1.0, 0.48, 0.14);
pub const MOONLIGHT: Color = Color::srgb(0.55, 0.65, 1.0);
