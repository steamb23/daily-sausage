//! Tray icon: a little sausage that fills up with today's budget usage.
//! Rendered in plain Rust (signed distance field + supersampling), no deps.

use crate::format::Level;

fn fill_rgb(level: Level) -> [f32; 3] {
    match level {
        Level::Ok => [0.25, 0.73, 0.31],   // #3FB950
        Level::Warn => [0.82, 0.60, 0.13], // #D29922
        Level::Over => [0.97, 0.32, 0.29], // #F85149
        Level::Idle => [0.55, 0.58, 0.62], // #8B949E
    }
}

/// Distance from `p` to the capsule between `a` and `b` with radius `r`
/// (negative inside).
fn capsule(px: f32, py: f32, ax: f32, bx: f32, cy: f32, r: f32) -> f32 {
    let t = ((px - ax) / (bx - ax)).clamp(0.0, 1.0);
    let qx = ax + t * (bx - ax);
    ((px - qx).powi(2) + (py - cy).powi(2)).sqrt() - r
}

fn circle(px: f32, py: f32, cx: f32, cy: f32, r: f32) -> f32 {
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r
}

/// Renders a `size`×`size` RGBA image (straight alpha).
///
/// `ratio` is today's used share of the budget (`None` = no data); the sausage
/// fills from the left and turns fully red at 100% or more.
pub fn render_tray(size: u32, ratio: Option<f64>, level: Level) -> Vec<u8> {
    render_tray_rect(size, size, ratio, level)
}

/// Same as [`render_tray`] on a `width`×`height` canvas, such as a wide 2:1
/// image for the macOS menu bar.
pub fn render_tray_rect(width: u32, height: u32, ratio: Option<f64>, level: Level) -> Vec<u8> {
    let (w, h) = (width as f32, height as f32);
    let s = w.min(h);
    // Geometry in pixels.
    let cy = h * 0.5;
    let r = (w * 0.25).min(h * 0.4);
    let left = w * 0.06;
    let right = w * 0.94;
    let ax = left + r;
    let bx = right - r;
    let width_px = right - left;
    let outline = (s * 0.075).max(1.2);
    let halo = (s * 0.045).max(0.8);
    let nub_r = (s * 0.055).max(1.0);

    let ratio = ratio.map(|x| x.max(0.0) as f32);
    let fill_to = match ratio {
        Some(x) if x >= 1.0 => f32::INFINITY,
        Some(x) => left + x * width_px,
        None => f32::NEG_INFINITY,
    };
    let fill = fill_rgb(level);
    let ink = if level == Level::Idle {
        [0.36, 0.38, 0.42]
    } else {
        [0.29, 0.17, 0.12]
    };
    let empty = [1.0, 1.0, 1.0, 0.35];

    const SS: u32 = 4;
    let mut out = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        for x in 0..width {
            let mut acc = [0f32; 4]; // premultiplied
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = x as f32 + (sx as f32 + 0.5) / SS as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SS as f32;
                    let d = capsule(px, py, ax, bx, cy, r);
                    let dn = circle(px, py, left - nub_r * 0.6, cy, nub_r).min(circle(
                        px,
                        py,
                        right + nub_r * 0.6,
                        cy,
                        nub_r,
                    ));
                    let (rgb, a) = if d < -outline {
                        if px <= fill_to {
                            (fill, 1.0)
                        } else {
                            ([empty[0], empty[1], empty[2]], empty[3])
                        }
                    } else if d < 0.0 || dn < 0.0 {
                        (ink, 1.0)
                    } else if d < halo || dn < halo {
                        ([1.0, 1.0, 1.0], 0.55)
                    } else {
                        ([0.0; 3], 0.0)
                    };
                    acc[0] += rgb[0] * a;
                    acc[1] += rgb[1] * a;
                    acc[2] += rgb[2] * a;
                    acc[3] += a;
                }
            }
            let n = (SS * SS) as f32;
            let a = acc[3] / n;
            let i = ((y * width + x) * 4) as usize;
            if a > 0.0 {
                for c in 0..3 {
                    out[i + c] = ((acc[c] / n / a).clamp(0.0, 1.0) * 255.0).round() as u8;
                }
                out[i + 3] = (a * 255.0).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn fills_from_left() {
        let size = 32;
        let img = render_tray(size, Some(0.5), Level::Ok);
        assert_eq!(img.len(), (size * size * 4) as usize);
        let left = px(&img, size, 10, 16);
        let right = px(&img, size, 22, 16);
        assert_eq!(left[3], 255);
        assert!(left[1] > left[0], "green on the left: {left:?}");
        assert!(right[3] < 200, "empty part is translucent: {right:?}");
        assert_eq!(px(&img, size, 0, 0)[3], 0, "corner transparent");
    }

    #[test]
    fn over_budget_is_fully_red() {
        let size = 32;
        let img = render_tray(size, Some(1.4), Level::Over);
        let right = px(&img, size, 22, 16);
        assert_eq!(right[3], 255);
        assert!(right[0] > 200 && right[1] < 120, "{right:?}");
    }

    #[test]
    fn wide_canvas() {
        let img = render_tray_rect(64, 32, Some(0.5), Level::Ok);
        assert_eq!(img.len(), 64 * 32 * 4);
        let i = ((16 * 64 + 16) * 4) as usize;
        assert_eq!(img[i + 3], 255);
    }

    #[test]
    fn no_data_is_empty() {
        let size = 32;
        let img = render_tray(size, None, Level::Idle);
        assert!(px(&img, size, 10, 16)[3] < 200);
    }
}
