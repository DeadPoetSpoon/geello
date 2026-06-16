use vello::kurbo::{Cap, Join};
use vello::peniko::Color;

pub fn parse_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        6 => Color::from_rgba8(
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
            255,
        ),
        8 => Color::from_rgba8(
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
            u8::from_str_radix(&hex[6..8], 16).unwrap_or(255),
        ),
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).unwrap_or(0) * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(0) * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(0) * 17;
            Color::from_rgba8(r, g, b, 255)
        }
        _ => Color::from_rgba8(0, 0, 0, 255),
    }
}

pub fn parse_join(s: Option<&str>) -> Join {
    match s {
        Some("Round") => Join::Round,
        Some("Bevel") => Join::Bevel,
        _ => Join::Miter,
    }
}

pub fn parse_cap(s: Option<&str>) -> Cap {
    match s {
        Some("Round") => Cap::Round,
        Some("Square") => Cap::Square,
        _ => Cap::Butt,
    }
}
