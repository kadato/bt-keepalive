//! Tray icon pixels, generated in code.
//!
//! Renders the icon with no image dependency at runtime.
//! Output is 64x64 RGBA: a filled circle plus a white Bluetooth rune
//! drawn with thick lines. Deterministic and dependency-free.
//!
//! Canonical palette, shared with `icons/icon.png`, `icons/32x32.png`,
//! `icons/128x128.png`, and `icons/icon.ico`, which all show the
//! playing variant on a transparent background:
//! playing disc (0, 122, 255), paused disc (88, 92, 100),
//! rune (255, 255, 255). No status dots: they vanish at small sizes
//! and make the taskbar, window, and tray disagree.

/// Icon edge length in pixels.
pub const ICON_SIZE: usize = 64;

/// Playing disc color.
pub const PLAYING_DISC: Rgba = (0u8, 122u8, 255u8, 255u8);
/// Paused disc color.
pub const PAUSED_DISC: Rgba = (88u8, 92u8, 100u8, 255u8);
/// Bluetooth rune color.
pub const RUNE: Rgba = (255u8, 255u8, 255u8, 255u8);

/// Render the tray icon. Blue when playing, gray when paused.
#[must_use]
pub fn render_icon(active: bool) -> Vec<u8> {
    let mut px = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    fill_circle(
        &mut px,
        32.0,
        32.0,
        29.0,
        if active { PLAYING_DISC } else { PAUSED_DISC },
    );
    // Bluetooth rune: stem plus crossed diagonals.
    line(&mut px, 32, 14, 32, 50, RUNE, 3);
    line(&mut px, 32, 14, 44, 26, RUNE, 3);
    line(&mut px, 44, 26, 32, 32, RUNE, 3);
    line(&mut px, 32, 32, 44, 38, RUNE, 3);
    line(&mut px, 44, 38, 32, 50, RUNE, 3);
    line(&mut px, 32, 14, 20, 26, RUNE, 3);
    line(&mut px, 20, 26, 32, 32, RUNE, 3);
    line(&mut px, 32, 32, 20, 38, RUNE, 3);
    line(&mut px, 20, 38, 32, 50, RUNE, 3);
    px
}

fn set(px: &mut [u8], x: i32, y: i32, r: u8, g: u8, b: u8, a: u8) {
    if x < 0 || y < 0 || x >= ICON_SIZE as i32 || y >= ICON_SIZE as i32 {
        return;
    }
    let i = (y as usize * ICON_SIZE + x as usize) * 4;
    px[i] = r;
    px[i + 1] = g;
    px[i + 2] = b;
    px[i + 3] = a;
}

/// RGBA color triple plus alpha.
type Rgba = (u8, u8, u8, u8);

fn fill_circle(px: &mut [u8], cx: f64, cy: f64, rad: f64, color: Rgba) {
    let lo = (cx - rad - 1.0).floor() as i32;
    let hi = (cx + rad + 1.0).ceil() as i32;
    for y in lo..=hi {
        for x in lo..=hi {
            let dx = f64::from(x) - cx;
            let dy = f64::from(y) - cy;
            if dx * dx + dy * dy <= rad * rad {
                set(px, x, y, color.0, color.1, color.2, color.3);
            }
        }
    }
}

fn dot(px: &mut [u8], x: i32, y: i32, color: Rgba, rad: i32) {
    for dy in -rad..=rad {
        for dx in -rad..=rad {
            if dx * dx + dy * dy <= rad * rad {
                set(px, x + dx, y + dy, color.0, color.1, color.2, color.3);
            }
        }
    }
}

/// Bresenham line with a round brush.
fn line(px: &mut [u8], x0: i32, y0: i32, x1: i32, y1: i32, color: Rgba, width: i32) {
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let rad = width / 2;
    let mut err = dx + dy;
    loop {
        dot(px, x, y, color, rad);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_size_matches_rgba() {
        assert_eq!(render_icon(true).len(), ICON_SIZE * ICON_SIZE * 4);
        assert_eq!(render_icon(false).len(), ICON_SIZE * ICON_SIZE * 4);
    }

    #[test]
    fn active_and_paused_differ() {
        assert_ne!(render_icon(true), render_icon(false));
    }

    #[test]
    fn rune_marks_the_center() {
        let px = render_icon(true);
        // Rune stem passes through the middle; center pixel must be opaque.
        let i = (32 * ICON_SIZE + 32) * 4;
        assert_eq!(px[i + 3], 255);
        assert!(px[i] > 200 && px[i + 1] > 200 && px[i + 2] > 200);
        // Corners stay transparent.
        assert_eq!(px[3], 0);
    }
}
