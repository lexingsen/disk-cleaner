//! Squarified treemap layout (Bruls, Huizing, van Wijk).
//!
//! Results are absolute rectangles in the units passed in (we lay out in
//! logical pixels), which lets the treemap nest child blocks inside their
//! parent's frame with the same aspect ratio as the real container.

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

struct Work {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn ratio(row: &[f64], sum: f64, side: f64) -> f64 {
    if row.is_empty() || sum <= 0.0 || side <= 0.0 {
        return f64::INFINITY;
    }
    let mx = row.iter().cloned().fold(0.0f64, f64::max);
    let mn = row.iter().cloned().fold(f64::INFINITY, f64::min);
    let s2 = sum * sum;
    let w2 = side * side;
    (w2 * mx / s2).max(s2 / (w2 * mn))
}

/// Lay `values` (already sorted descending) out inside the rectangle
/// `(x, y, w, h)`. Returned rectangles are in the same coordinates as the
/// input rectangle, in the same order as `values`.
pub fn squarify_in(values: &[f64], x: f64, y: f64, w: f64, h: f64) -> Vec<Rect> {
    let mut out = vec![Rect::default(); values.len()];
    let total: f64 = values.iter().filter(|v| **v > 0.0).sum();
    if total <= 0.0 || w <= 0.0 || h <= 0.0 {
        return out;
    }
    let scale = (w * h) / total;
    let areas: Vec<f64> = values.iter().map(|v| v.max(0.0) * scale).collect();
    let mut rect = Work { x, y, w, h };

    let mut i = 0;
    while i < areas.len() {
        if areas[i] <= 0.0 {
            i += 1;
            continue;
        }
        let short = rect.w.min(rect.h);
        let mut j = i + 1;
        let mut sum = areas[i];
        let mut worst = ratio(&areas[i..i + 1], sum, short);
        while j < areas.len() {
            let next = ratio(&areas[i..=j], sum + areas[j], short);
            if next > worst {
                break;
            }
            worst = next;
            sum += areas[j];
            j += 1;
        }
        if rect.w >= rect.h {
            let cw = sum / rect.h.max(1e-9);
            let mut yy = rect.y;
            for k in i..j {
                let hh = if cw > 0.0 { areas[k] / cw } else { 0.0 };
                out[k] = Rect {
                    x: rect.x as f32,
                    y: yy as f32,
                    w: cw as f32,
                    h: hh as f32,
                };
                yy += hh;
            }
            rect.x += cw;
            rect.w = (rect.w - cw).max(0.0);
        } else {
            let ch = sum / rect.w.max(1e-9);
            let mut xx = rect.x;
            for k in i..j {
                let ww = if ch > 0.0 { areas[k] / ch } else { 0.0 };
                out[k] = Rect {
                    x: xx as f32,
                    y: rect.y as f32,
                    w: ww as f32,
                    h: ch as f32,
                };
                xx += ww;
            }
            rect.y += ch;
            rect.h = (rect.h - ch).max(0.0);
        }
        i = j;
    }
    out
}
