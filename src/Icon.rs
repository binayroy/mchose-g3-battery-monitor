// SPDX-License-Identifier: GPL-3.0-or-later
//! Tray icon drawing (design taken unchanged from the original Rust app).

use tray_icon::Icon;

/// Dynamically generates a clean 32x32 battery icon with fill percentage.
/// `None` draws the warning triangle (battery data unavailable).
pub fn generate_battery_icon(percentage: Option<u8>) -> Icon {
    let width = 32usize;
    let height = 32usize;
    let mut rgba = vec![0u8; width * height * 4];

    // Warning icon when battery data is unavailable.
    if percentage.is_none() {
        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 4;

                // Upward-pointing warning triangle.
                let dx = (x as i32 - 16).abs();
                let half_width = (y as i32 - 5) * 12 / 22;
                let in_triangle = (5..=27).contains(&y) && dx <= half_width;

                // Black exclamation mark.
                let exclamation_stem = (x == 15 || x == 16) && (11..=19).contains(&y);
                let exclamation_dot = (x == 15 || x == 16) && (22..=23).contains(&y);

                if in_triangle {
                    rgba[idx] = 255;
                    rgba[idx + 1] = 190;
                    rgba[idx + 2] = 35;
                    rgba[idx + 3] = 255;
                }

                if exclamation_stem || exclamation_dot {
                    rgba[idx] = 25;
                    rgba[idx + 1] = 25;
                    rgba[idx + 2] = 25;
                    rgba[idx + 3] = 255;
                }
            }
        }

        return Icon::from_rgba(rgba, width as u32, height as u32).unwrap();
    }

    // Normal horizontal battery icon.
    let pct = percentage.unwrap_or(0).min(100);

    // Outline color: red below 20%, gray otherwise.
    let (outline_r, outline_g, outline_b) = if pct < 20 {
        (230, 50, 50)
    } else {
        (220, 220, 220)
    };

    // Two-pixel-thick battery body outline.
    // Body spans x=4..25 and y=8..24.
    // Interior fill is inset to x=6..23 and y=10..22.
    // Terminal is on the right.
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) * 4;

            let is_border =
                // Left and right sides, 2 pixels thick.
                ((4..=5).contains(&x) && (8..=24).contains(&y))
                || ((24..=25).contains(&x) && (8..=24).contains(&y))
                // Top and bottom, 2 pixels thick.
                || ((8..=9).contains(&y) && (6..=23).contains(&x))
                || ((23..=24).contains(&y) && (6..=23).contains(&x));

            let is_terminal = (26..=27).contains(&x) && (14..=18).contains(&y);

            // Fill grows from left to right inside the outline.
            let fill_width = 18 * pct as usize / 100;
            let is_fill = x >= 6 && x < 6 + fill_width && (10..=22).contains(&y);

            if is_border || is_terminal {
                rgba[idx] = outline_r;
                rgba[idx + 1] = outline_g;
                rgba[idx + 2] = outline_b;
                rgba[idx + 3] = 255;
            } else if is_fill {
                if pct < 20 {
                    // Red: very low.
                    rgba[idx] = 230;
                    rgba[idx + 1] = 50;
                    rgba[idx + 2] = 50;
                } else if pct < 50 {
                    // Yellow: low.
                    rgba[idx] = 240;
                    rgba[idx + 1] = 200;
                    rgba[idx + 2] = 50;
                } else {
                    // Green: medium to high.
                    rgba[idx] = 50;
                    rgba[idx + 1] = 205;
                    rgba[idx + 2] = 50;
                }

                rgba[idx + 3] = 255;
            } else {
                // Transparent background.
                rgba[idx + 3] = 0;
            }
        }
    }

    Icon::from_rgba(rgba, width as u32, height as u32).unwrap()
}