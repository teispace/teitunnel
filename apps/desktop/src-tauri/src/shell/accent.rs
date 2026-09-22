//! The system accent colour.
//!
//! WKWebView resolves the CSS `AccentColor` keyword to a fixed blue rather than the user's
//! accent (verified on macOS 27, D-023), so the shell reads it from AppKit instead.

/// The accent colour as `#rrggbb`, or `None` where the platform has no accent to follow.
#[cfg(target_os = "macos")]
pub fn accent_color() -> Option<String> {
    use objc2_app_kit::{NSColor, NSColorSpace};

    let color =
        NSColor::controlAccentColor().colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    Some(to_hex(
        color.redComponent(),
        color.greenComponent(),
        color.blueComponent(),
    ))
}

/// The accent colour as `#rrggbb`, or `None` where the platform has no accent to follow.
#[cfg(not(target_os = "macos"))]
pub fn accent_color() -> Option<String> {
    None
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn to_hex(r: f64, g: f64, b: f64) -> String {
    // Components are clamped to 0..=1 first, so the cast can't truncate or wrap.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = |c: f64| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_components_as_hex() {
        assert_eq!(to_hex(0.0, 0.478, 1.0), "#007aff");
        assert_eq!(to_hex(-0.2, 1.4, 0.5), "#00ff80");
    }
}
