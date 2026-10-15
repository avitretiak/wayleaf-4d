use crate::{AppError, Config, Result};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub id: u16,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

#[derive(Clone, Debug)]
pub struct PanelPosition {
    pub id: u16,
    pub x: i64,
    pub y: i64,
}

pub fn make_zones(panels: &[PanelPosition], cfg: &Config) -> Result<Vec<Zone>> {
    let n = panels.len();
    if !(4..=40).contains(&n) {
        return Err(AppError::new("unsupported Nanoleaf segment count"));
    }
    if !(5..=20).contains(&cfg.edge_depth) {
        return Err(AppError::new("edge depth must be in 5..20 percent"));
    }

    let border = cfg.edge_depth as f32 / 100.0;
    let corner = border * 1.5;
    let mut zones = vec![
        Zone {
            id: 0,
            x0: 0.0,
            x1: 0.0,
            y0: 0.0,
            y1: 0.0
        };
        n
    ];
    if n == 40 {
        for (i, panel) in panels.iter().enumerate() {
            let (x, y) = if i <= 7 {
                (350.0 - 50.0 * i as f32, 0.0)
            } else if i <= 15 {
                (0.0, 50.0 * (i - 7) as f32)
            } else if i <= 28 {
                (50.0 * (i - 15) as f32, 400.0)
            } else if i <= 35 {
                (650.0, 400.0 - 50.0 * (i - 28) as f32)
            } else {
                (600.0 - 50.0 * (i - 36) as f32, 50.0)
            };
            let (x0, x1, y0, y1) = if i == 7 {
                (0.0, corner, 1.0 - corner, 1.0)
            } else if i == 15 {
                (0.0, corner, 0.0, corner)
            } else if i == 28 {
                (1.0 - corner, 1.0, 0.0, corner)
            } else if i == 35 {
                (1.0 - corner, 1.0, 1.0 - corner, 1.0)
            } else if x == 0.0 {
                let cy = 1.0 - y / 400.0;
                let span = (50.0 / 400.0) * 0.7;
                (0.0, border, (cy - span).max(0.0), (cy + span).min(1.0))
            } else if y >= 390.0 {
                let cx = x / 650.0;
                let span = (50.0 / 650.0) * 0.7;
                ((cx - span).max(0.0), (cx + span).min(1.0), 0.0, border)
            } else if x >= 640.0 {
                let cy = 1.0 - y / 400.0;
                let span = (50.0 / 400.0) * 0.7;
                (
                    1.0 - border,
                    1.0,
                    (cy - span).max(0.0),
                    (cy + span).min(1.0),
                )
            } else {
                let cx = x / 650.0;
                let span = (50.0 / 650.0) * 0.7;
                (
                    (cx - span).max(0.0),
                    (cx + span).min(1.0),
                    1.0 - border,
                    1.0,
                )
            };
            zones[i] = Zone {
                id: panel.id,
                x0,
                x1,
                y0,
                y1,
            };
        }
    } else if n == 34 {
        let min_x = panels.iter().map(|p| p.x).min().unwrap();
        let max_x = panels.iter().map(|p| p.x).max().unwrap();
        let min_y = panels.iter().map(|p| p.y).min().unwrap();
        let max_y = panels.iter().map(|p| p.y).max().unwrap();
        if min_x == max_x || min_y == max_y {
            return Err(AppError::new("34-segment layout has no coordinate extent"));
        }
        let tangent = |i: usize, vertical: bool| -> f32 {
            if vertical {
                1.0 - ((panels[i].y as i128 - min_y as i128) as f32)
                    / ((max_y as i128 - min_y as i128) as f32)
            } else {
                ((panels[i].x as i128 - min_x as i128) as f32)
                    / ((max_x as i128 - min_x as i128) as f32)
            }
        };
        let interval = |first: usize, end: usize, i: usize, vertical: bool| {
            let center = tangent(i, vertical);
            let a = if i == first {
                center + (center - tangent(i + 1, vertical)) * 0.5
            } else {
                (tangent(i - 1, vertical) + center) * 0.5
            };
            let b = if i + 1 == end {
                center + (center - tangent(i - 1, vertical)) * 0.5
            } else {
                (center + tangent(i + 1, vertical)) * 0.5
            };
            (a.min(b).max(0.0), a.max(b).min(1.0))
        };
        for i in 0..6 {
            let (x0, x1) = interval(0, 6, i, false);
            zones[i] = Zone {
                id: panels[i].id,
                x0,
                x1,
                y0: 1.0 - border,
                y1: 1.0,
            };
        }
        for i in 6..12 {
            let (y0, y1) = interval(6, 12, i, true);
            zones[i] = Zone {
                id: panels[i].id,
                x0: 0.0,
                x1: border,
                y0,
                y1,
            };
        }
        for i in 12..21 {
            let (x0, x1) = interval(12, 21, i, false);
            zones[i] = Zone {
                id: panels[i].id,
                x0,
                x1,
                y0: 0.0,
                y1: border,
            };
        }
        for i in 21..27 {
            let (y0, y1) = interval(21, 27, i, true);
            zones[i] = Zone {
                id: panels[i].id,
                x0: 1.0 - border,
                x1: 1.0,
                y0,
                y1,
            };
        }
        for i in 27..34 {
            let (x0, x1) = interval(27, 34, i, false);
            zones[i] = Zone {
                id: panels[i].id,
                x0,
                x1,
                y0: 1.0 - border,
                y1: 1.0,
            };
        }
    } else {
        let left = ((9.0 / 50.0 * n as f64).round() as i32).max(1);
        let top = ((16.0 / 50.0 * n as f64).round() as i32).max(1);
        let right = ((9.0 / 50.0 * n as f64).round() as i32).max(1);
        let bottom = (n as i32 - left - top - right).max(1);
        let mut index = 0usize;
        let bottom_left = bottom / 2;
        for i in 0..bottom_left {
            let a = 0.5 - i as f32 * 0.5 / bottom_left as f32;
            let b = 0.5 - (i + 1) as f32 * 0.5 / bottom_left as f32;
            zones[index] = Zone {
                id: panels[index].id,
                x0: a.min(b),
                x1: a.max(b),
                y0: 1.0 - border,
                y1: 1.0,
            };
            index += 1;
        }
        for i in 0..left {
            let a = 1.0 - i as f32 / left as f32;
            let b = 1.0 - (i + 1) as f32 / left as f32;
            zones[index] = Zone {
                id: panels[index].id,
                x0: 0.0,
                x1: border,
                y0: a.min(b),
                y1: a.max(b),
            };
            index += 1;
        }
        for i in 0..top {
            let a = i as f32 / top as f32;
            let b = (i + 1) as f32 / top as f32;
            zones[index] = Zone {
                id: panels[index].id,
                x0: a,
                x1: b,
                y0: 0.0,
                y1: border,
            };
            index += 1;
        }
        for i in 0..right {
            let a = i as f32 / right as f32;
            let b = (i + 1) as f32 / right as f32;
            zones[index] = Zone {
                id: panels[index].id,
                x0: 1.0 - border,
                x1: 1.0,
                y0: a,
                y1: b,
            };
            index += 1;
        }
        let bottom_right = bottom - bottom_left;
        for i in 0..bottom_right {
            let a = 1.0 - i as f32 * 0.5 / bottom_right as f32;
            let b = 1.0 - (i + 1) as f32 * 0.5 / bottom_right as f32;
            zones[index] = Zone {
                id: panels[index].id,
                x0: a.min(b),
                x1: a.max(b),
                y0: 1.0 - border,
                y1: 1.0,
            };
            index += 1;
        }
    }

    Ok(zones)
}

#[inline]
fn pixel(data: &[u8], stride: usize, x: usize, y: usize) -> Color {
    let p = y * stride + x * 4;
    Color {
        r: data[p] as f32,
        g: data[p + 1] as f32,
        b: data[p + 2] as f32,
    }
}

#[inline]
fn luma(c: Color) -> f32 {
    (0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b) / 255.0
}

#[inline]
fn saturation(c: Color) -> f32 {
    let hi = c.r.max(c.g).max(c.b);
    let lo = c.r.min(c.g).min(c.b);
    if hi > 0.001 {
        (hi - lo) / hi
    } else {
        0.0
    }
}

#[inline]
fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
    }
}

fn boost_saturation(c: Color, factor: f32) -> Color {
    let hi = c.r.max(c.g).max(c.b);
    let lo = c.r.min(c.g).min(c.b);
    if hi <= 0.0 || factor <= 1.0 {
        return c;
    }
    let delta = hi - lo;
    let mut h = 0.0;
    let mut s = delta / hi;
    let v = hi;
    if delta > 0.0 {
        if hi == c.r {
            h = ((c.g - c.b) / delta) % 6.0;
        } else if hi == c.g {
            h = (c.b - c.r) / delta + 2.0;
        } else {
            h = (c.r - c.g) / delta + 4.0;
        }
        h *= 60.0;
        if h < 0.0 {
            h += 360.0;
        }
    }
    s = (s * factor).min(1.0);
    let chroma = v * s;
    let x = chroma * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = v - chroma;
    let o = if h < 60.0 {
        Color {
            r: chroma,
            g: x,
            b: 0.0,
        }
    } else if h < 120.0 {
        Color {
            r: x,
            g: chroma,
            b: 0.0,
        }
    } else if h < 180.0 {
        Color {
            r: 0.0,
            g: chroma,
            b: x,
        }
    } else if h < 240.0 {
        Color {
            r: 0.0,
            g: x,
            b: chroma,
        }
    } else if h < 300.0 {
        Color {
            r: x,
            g: 0.0,
            b: chroma,
        }
    } else {
        Color {
            r: chroma,
            g: 0.0,
            b: x,
        }
    };
    Color {
        r: o.r + m,
        g: o.g + m,
        b: o.b + m,
    }
}

fn row_luma(data: &[u8], stride: usize, width: usize, y: usize) -> f32 {
    let mut sum = 0u32;
    let mut n = 0u32;
    for x in (0..width).step_by(4) {
        sum += (luma(pixel(data, stride, x, y)) * 255.0) as u32;
        n += 1;
    }
    if n != 0 {
        sum as f32 / n as f32
    } else {
        0.0
    }
}

fn col_luma(data: &[u8], stride: usize, height: usize, x: usize) -> f32 {
    let mut sum = 0u32;
    let mut n = 0u32;
    for y in (0..height).step_by(4) {
        sum += (luma(pixel(data, stride, x, y)) * 255.0) as u32;
        n += 1;
    }
    if n != 0 {
        sum as f32 / n as f32
    } else {
        0.0
    }
}

fn detect_active_rect(data: &[u8], stride: usize, width: usize, height: usize) -> Rect {
    let mut top = 0usize;
    let mut bottom = 0usize;
    let mut left = 0usize;
    let mut right = 0usize;
    for y in 0..height / 3 {
        if row_luma(data, stride, width, y) > 12.0 {
            break;
        }
        top = y + 1;
    }
    for i in 0..height / 3 {
        if row_luma(data, stride, width, height - 1 - i) > 12.0 {
            break;
        }
        bottom = i + 1;
    }
    for x in 0..width / 3 {
        if col_luma(data, stride, height, x) > 12.0 {
            break;
        }
        left = x + 1;
    }
    for i in 0..width / 3 {
        if col_luma(data, stride, height, width - 1 - i) > 12.0 {
            break;
        }
        right = i + 1;
    }
    let mut rect = Rect::default();
    if top >= 4 && top.abs_diff(bottom) <= 6 {
        rect.y0 = top as f32 / height as f32;
        rect.y1 = (height - bottom) as f32 / height as f32;
    }
    if left >= 4 && left.abs_diff(right) <= 6 {
        rect.x0 = left as f32 / width as f32;
        rect.x1 = (width - right) as f32 / width as f32;
    }
    rect
}

#[inline]
fn coord(value: f32) -> usize {
    value as usize
}

pub struct Sampler {
    zones: Vec<Zone>,
    previous: Vec<Color>,
    out: Vec<Color>,
    raw: Vec<Color>,
    active: Rect,
    letterbox_enabled: bool,
    frames: u64,
    global: Color,
    scene_cut: bool,
}

impl Sampler {
    pub fn new(zones: Vec<Zone>) -> Self {
        let n = zones.len();
        Self {
            zones,
            previous: vec![Color::default(); n],
            out: vec![Color::default(); n],
            raw: vec![Color::default(); n],
            active: Rect::default(),
            frames: 0,
            global: Color::default(),
            scene_cut: false,
            letterbox_enabled: false,
        }
    }

    pub fn remap(&mut self, next: Vec<Zone>) {
        let changed = next.len() != self.zones.len()
            || next.iter().zip(&self.zones).any(|(a, b)| {
                a.id != b.id || a.x0 != b.x0 || a.x1 != b.x1 || a.y0 != b.y0 || a.y1 != b.y1
            });
        self.zones = next;
        if changed {
            self.previous.resize(self.zones.len(), Color::default());
            self.out.resize(self.zones.len(), Color::default());
            self.raw.resize(self.zones.len(), Color::default());
            self.previous.fill(Color::default());
            self.out.fill(Color::default());
            self.raw.fill(Color::default());
        }
    }

    pub fn active(&self) -> Rect {
        self.active
    }
    pub fn raw(&self) -> &[Color] {
        &self.raw
    }
    pub fn out(&self) -> &[Color] {
        &self.out
    }

    pub fn sample_in_place(
        &mut self,
        data: &[u8],
        stride: usize,
        width: usize,
        height: usize,
        cfg: &Config,
        elapsed: Duration,
    ) -> Result<()> {
        self.sample(data, stride, width, height, cfg, elapsed)
            .map(|_| ())
    }
    pub fn scene_cut(&self) -> bool {
        self.scene_cut
    }

    pub fn sample(
        &mut self,
        data: &[u8],
        stride: usize,
        width: usize,
        height: usize,
        cfg: &Config,
        elapsed: Duration,
    ) -> Result<&[Color]> {
        let row_bytes = width
            .checked_mul(4)
            .ok_or_else(|| AppError::new("capture dimensions overflow"))?;
        let required = height
            .checked_sub(1)
            .and_then(|rows| rows.checked_mul(stride))
            .and_then(|last| last.checked_add(row_bytes));
        if width == 0
            || height == 0
            || stride < row_bytes
            || required.is_none_or(|len| len > data.len())
        {
            return Err(AppError::new("invalid capture buffer dimensions or stride"));
        }
        if cfg.letterbox {
            if self.frames == 0
                || !self.letterbox_enabled
                || self.frames.wrapping_add(1).is_multiple_of(15)
            {
                self.active = detect_active_rect(data, stride, width, height);
            }
            self.letterbox_enabled = true;
        } else {
            self.active = Rect::default();
            self.letterbox_enabled = false;
        }
        self.frames = self.frames.wrapping_add(1);

        let active_width = (self.active.x1 - self.active.x0).max(0.1);
        let active_height = (self.active.y1 - self.active.y0).max(0.1);
        let cx0 = coord((self.active.x0 + 0.25 * active_width) * width as f32).min(width - 1);
        let cx1 =
            coord((self.active.x0 + 0.75 * active_width) * width as f32).clamp(cx0 + 1, width);
        let cy0 = coord((self.active.y0 + 0.25 * active_height) * height as f32).min(height - 1);
        let cy1 =
            coord((self.active.y0 + 0.75 * active_height) * height as f32).clamp(cy0 + 1, height);
        let mut sr = 0.0f64;
        let mut sg = 0.0f64;
        let mut sb = 0.0f64;
        let mut count = 0u64;
        for y in (cy0..cy1).step_by(4) {
            for x in (cx0..cx1).step_by(4) {
                let c = pixel(data, stride, x, y);
                sr += c.r as f64;
                sg += c.g as f64;
                sb += c.b as f64;
                count += 1;
            }
        }
        let current_global = if count != 0 {
            Color {
                r: (sr / count as f64) as f32,
                g: (sg / count as f64) as f32,
                b: (sb / count as f64) as f32,
            }
        } else {
            Color::default()
        };
        self.scene_cut = self.frames > 1 && {
            let dr = ((current_global.r - self.global.r) / 255.0) as f64;
            let dg = ((current_global.g - self.global.g) / 255.0) as f64;
            let db = ((current_global.b - self.global.b) / 255.0) as f64;
            (dr.powi(2) + dg.powi(2) + db.powi(2)).sqrt() > 0.35
        };
        self.global = current_global;

        let feather = cfg.feather_depth as f32 / 100.0;
        let elapsed = elapsed.as_secs_f32().min(0.1);
        let alpha = if self.scene_cut {
            1.0
        } else {
            1.0 - (1.0 - cfg.smooth).powf(elapsed * 30.0)
        };
        let step = cfg.max_step as f32 * elapsed * 30.0;
        let peak_mix = cfg.peak_mix as f32 / 100.0;
        let center_bleed = cfg.center_bleed as f32 / 100.0;
        let sat_boost = cfg.saturation_boost;
        let threshold = cfg.black_threshold;
        let brightness = cfg.brightness;
        let strict_black = cfg.strict_black;
        let midtone_lift = cfg.midtone_lift;
        let fade = |mut t: f32| {
            t = t.clamp(0.0, 1.0);
            1.0 - t * t * (3.0 - 2.0 * t)
        };

        for i in 0..self.zones.len() {
            let z = &self.zones[i];
            let left = z.x0 == 0.0;
            let right = z.x1 == 1.0;
            let top = z.y0 == 0.0;
            let bottom = z.y1 == 1.0;
            let mut sx0 = z.x0;
            let mut sx1 = z.x1;
            let mut sy0 = z.y0;
            let mut sy1 = z.y1;
            if feather > 0.0 {
                if left {
                    sx1 = (sx1 + feather).min(1.0);
                } else if right {
                    sx0 = (sx0 - feather).max(0.0);
                }
                if top {
                    sy1 = (sy1 + feather).min(1.0);
                } else if bottom {
                    sy0 = (sy0 - feather).max(0.0);
                }
            }
            let x0 = coord((self.active.x0 + sx0 * active_width) * width as f32).min(width - 1);
            let x1 =
                coord((self.active.x0 + sx1 * active_width) * width as f32).clamp(x0 + 1, width);
            let y0 = coord((self.active.y0 + sy0 * active_height) * height as f32).min(height - 1);
            let y1 =
                coord((self.active.y0 + sy1 * active_height) * height as f32).clamp(y0 + 1, height);
            let mut r = 0.0f64;
            let mut g = 0.0f64;
            let mut b = 0.0f64;
            let mut total = 0.0f64;
            let mut peak = Color::default();
            let mut peak_luma = 0.0f32;
            for y in (y0..y1).step_by(2) {
                for x in (x0..x1).step_by(2) {
                    let mut spatial = 1.0f32;
                    if feather > 0.0 {
                        let nx = ((x as f32 + 0.5) / width as f32 - self.active.x0) / active_width;
                        let ny =
                            ((y as f32 + 0.5) / height as f32 - self.active.y0) / active_height;
                        if left && nx > z.x1 {
                            spatial *= fade((nx - z.x1) / feather);
                        } else if right && nx < z.x0 {
                            spatial *= fade((z.x0 - nx) / feather);
                        }
                        if top && ny > z.y1 {
                            spatial *= fade((ny - z.y1) / feather);
                        } else if bottom && ny < z.y0 {
                            spatial *= fade((z.y0 - ny) / feather);
                        }
                    }
                    if spatial <= 0.0 {
                        continue;
                    }
                    let c = pixel(data, stride, x, y);
                    let sat = saturation(c);
                    let lum = luma(c);
                    let weight = (1.0 + sat_boost * sat * sat) * spatial;
                    r += (c.r * weight) as f64;
                    g += (c.g * weight) as f64;
                    b += (c.b * weight) as f64;
                    total += weight as f64;
                    let weighted_luma = lum * spatial;
                    if weighted_luma > peak_luma {
                        peak_luma = weighted_luma;
                        peak = Color {
                            r: c.r * spatial,
                            g: c.g * spatial,
                            b: c.b * spatial,
                        };
                    }
                }
            }
            let mut target = if total != 0.0 {
                Color {
                    r: (r / total) as f32,
                    g: (g / total) as f32,
                    b: (b / total) as f32,
                }
            } else {
                Color::default()
            };
            if peak_luma > 0.0 {
                target = mix(target, peak, peak_mix);
            }
            target = mix(target, current_global, center_bleed);
            if threshold > 0.0 {
                let lum = luma(target);
                if lum <= threshold {
                    target = Color::default();
                } else {
                    let ramp = ((lum - threshold) / threshold).clamp(0.0, 1.0);
                    target = Color {
                        r: target.r * ramp,
                        g: target.g * ramp,
                        b: target.b * ramp,
                    };
                }
            }
            target = boost_saturation(target, sat_boost);
            if midtone_lift {
                let lift = |mut x: f32| {
                    x /= 255.0;
                    (x / (x + 0.25) * 1.25).clamp(0.0, 1.0) * 255.0
                };
                target = Color {
                    r: lift(target.r),
                    g: lift(target.g),
                    b: lift(target.b),
                };
            }
            target = Color {
                r: (target.r * brightness).clamp(0.0, 255.0),
                g: (target.g * brightness).clamp(0.0, 255.0),
                b: (target.b * brightness).clamp(0.0, 255.0),
            };
            self.raw[i] = target;
            if strict_black && target.r == 0.0 && target.g == 0.0 && target.b == 0.0 {
                self.previous[i] = Color::default();
                self.out[i] = Color::default();
                continue;
            }
            let prev = self.previous[i];
            let candidate = mix(prev, target, alpha);
            let bounded = |a: f32, b: f32| a + (b - a).clamp(-step, step);
            let next = Color {
                r: bounded(prev.r, candidate.r),
                g: bounded(prev.g, candidate.g),
                b: bounded(prev.b, candidate.b),
            };
            self.previous[i] = next;
            self.out[i] = next;
        }
        Ok(&self.out)
    }
}

pub fn self_test() -> Result<()> {
    let mut cfg = Config {
        smooth: 1.0,
        max_step: 255,
        black_threshold: 0.0,
        saturation_boost: 0.0,
        brightness: 1.0,
        letterbox: false,
        peak_mix: 0,
        center_bleed: 0,
        feather_depth: 0,
        ..Config::default()
    };
    let panels: Vec<_> = (0..40)
        .map(|i| PanelPosition {
            id: i + 100,
            x: 0,
            y: 0,
        })
        .collect();
    let zones = make_zones(&panels, &cfg)?;
    if zones.len() != 40
        || zones
            .iter()
            .zip(&panels)
            .any(|(zone, panel)| zone.id != panel.id)
        || (zones[0].y0 - 0.92).abs() > 0.001
    {
        return Err(AppError::new("sampler 40-panel geometry self-test failed"));
    }
    let w = 32usize;
    let h = 24usize;
    let stride = w * 4 + 8;
    let mut frame = vec![0u8; stride * h];
    for y in 0..h {
        for x in 0..w {
            let p = y * stride + x * 4;
            frame[p] = if x < w / 2 { 220 } else { 20 };
            frame[p + 1] = 20;
            frame[p + 2] = if x < w / 2 { 20 } else { 220 };
            frame[p + 3] = 255;
        }
    }
    let zone = Zone {
        id: 900,
        x0: 0.0,
        x1: 0.5,
        y0: 0.0,
        y1: 1.0,
    };
    let mut sampler = Sampler::new(vec![zone.clone()]);
    let first = sampler.sample(&frame, stride, w, h, &cfg, Duration::from_millis(34))?[0];
    if first.r <= first.b {
        return Err(AppError::new("sampler color self-test failed"));
    }
    cfg.strict_black = true;
    frame.fill(0);
    let black = sampler.sample(&frame, stride, w, h, &cfg, Duration::from_millis(34))?[0];
    if black != Color::default() || sampler.raw()[0] != Color::default() {
        return Err(AppError::new("sampler strict black self-test failed"));
    }
    let edge_zone = Zone {
        id: 901,
        x0: 0.0,
        x1: 0.15,
        y0: 0.4,
        y1: 0.6,
    };
    let mut edge_frame = vec![0u8; stride * h];
    for y in [9usize, 11, 13] {
        let inner = y * stride + 2 * 4;
        edge_frame[inner..inner + 3].copy_from_slice(&[0, 255, 0]);
        let outer = y * stride + 6 * 4;
        edge_frame[outer..outer + 3].copy_from_slice(&[0, 0, 255]);
    }
    let mut edge_cfg = cfg.clone();
    edge_cfg.strict_black = false;
    let mut hard_edge = Sampler::new(vec![edge_zone.clone()]);
    let hard = hard_edge.sample(
        &edge_frame,
        stride,
        w,
        h,
        &edge_cfg,
        Duration::from_millis(34),
    )?[0];
    edge_cfg.feather_depth = 10;
    let mut feathered = Sampler::new(vec![edge_zone]);
    let soft = feathered.sample(
        &edge_frame,
        stride,
        w,
        h,
        &edge_cfg,
        Duration::from_millis(34),
    )?[0];
    if soft.g >= hard.g || soft.b <= hard.b {
        return Err(AppError::new("sampler feather edge self-test failed"));
    }
    let mut blend_cfg = cfg.clone();
    blend_cfg.strict_black = false;
    blend_cfg.center_bleed = 20;
    blend_cfg.peak_mix = 0;
    let large_w = 64usize;
    let large_h = 48usize;
    let large_stride = large_w * 4;
    let corner = Zone {
        id: 902,
        x0: 0.0,
        x1: 0.15,
        y0: 0.0,
        y1: 0.15,
    };
    let mut center_frame = vec![0u8; large_stride * large_h];
    for y in 12..36 {
        for x in 16..48 {
            let p = y * large_stride + x * 4;
            center_frame[p..p + 3].copy_from_slice(&[100, 40, 10]);
        }
    }
    let mut center_sampler = Sampler::new(vec![corner.clone()]);
    let center = center_sampler.sample(
        &center_frame,
        large_stride,
        large_w,
        large_h,
        &blend_cfg,
        Duration::from_millis(34),
    )?[0];
    if (center.r - 20.0).abs() > 0.1 || (center.g - 8.0).abs() > 0.1 || (center.b - 2.0).abs() > 0.1
    {
        return Err(AppError::new("sampler center-blend self-test failed"));
    }

    let mut peak_cfg = blend_cfg.clone();
    peak_cfg.center_bleed = 0;
    let mut peak_frame = vec![0u8; large_stride * large_h];
    peak_frame[..3].copy_from_slice(&[255, 0, 0]);
    let mut peak_sampler = Sampler::new(vec![Zone {
        id: 903,
        x0: 0.0,
        x1: 1.0,
        y0: 0.0,
        y1: 1.0,
    }]);
    peak_cfg.peak_mix = 0;
    let peak_off = peak_sampler.sample(
        &peak_frame,
        large_stride,
        large_w,
        large_h,
        &peak_cfg,
        Duration::from_millis(34),
    )?[0];
    peak_cfg.peak_mix = 15;
    let peak_on = peak_sampler.sample(
        &peak_frame,
        large_stride,
        large_w,
        large_h,
        &peak_cfg,
        Duration::from_millis(34),
    )?[0];
    if peak_off.r >= 1.0 || !(38.0..40.0).contains(&peak_on.r) {
        return Err(AppError::new("sampler peak-mix self-test failed"));
    }

    let mut diagonal_cfg = peak_cfg;
    diagonal_cfg.feather_depth = 10;
    let mut horizontal_frame = vec![0u8; large_stride * large_h];
    let horizontal_pixel = 2 * large_stride + 14 * 4;
    horizontal_frame[horizontal_pixel..horizontal_pixel + 3].fill(255);
    let mut diagonal_frame = vec![0u8; large_stride * large_h];
    let diagonal_pixel = 10 * large_stride + 14 * 4;
    diagonal_frame[diagonal_pixel..diagonal_pixel + 3].fill(255);
    let mut horizontal_sampler = Sampler::new(vec![corner.clone()]);
    let horizontal = horizontal_sampler.sample(
        &horizontal_frame,
        large_stride,
        large_w,
        large_h,
        &diagonal_cfg,
        Duration::from_millis(34),
    )?[0];
    let mut diagonal_sampler = Sampler::new(vec![corner]);
    let diagonal = diagonal_sampler.sample(
        &diagonal_frame,
        large_stride,
        large_w,
        large_h,
        &diagonal_cfg,
        Duration::from_millis(34),
    )?[0];
    if horizontal.r <= diagonal.r * 1.8 || diagonal.r <= 0.0 {
        return Err(AppError::new("sampler diagonal feather self-test failed"));
    }
    let mut temporal_cfg = cfg.clone();
    temporal_cfg.strict_black = false;
    temporal_cfg.smooth = 1.0;
    temporal_cfg.max_step = 10;
    frame.fill(0);
    for y in 0..h {
        for x in 0..w {
            let p = y * stride + x * 4;
            frame[p] = 200;
        }
    }
    let mut temporal = Sampler::new(vec![zone]);
    let stepped = temporal.sample(
        &frame,
        stride,
        w,
        h,
        &temporal_cfg,
        Duration::from_millis(34),
    )?[0];
    let expected_step =
        temporal_cfg.max_step as f32 * Duration::from_millis(34).as_secs_f32().min(0.1) * 30.0;
    if (stepped.r - expected_step).abs() > 0.001 {
        return Err(AppError::new("sampler temporal step self-test failed"));
    }
    let invalid = temporal.sample(
        &frame,
        w * 4 - 1,
        w,
        h,
        &temporal_cfg,
        Duration::from_millis(34),
    );
    if invalid.is_ok() {
        return Err(AppError::new("sampler stride validation self-test failed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_frame(w: usize, h: usize, color: [u8; 3]) -> Vec<u8> {
        let mut data = vec![0; w * h * 4];
        for p in data.chunks_exact_mut(4) {
            p[..3].copy_from_slice(&color);
            p[3] = 255;
        }
        data
    }

    #[test]
    fn calibrated_and_34_panel_mapping_keep_actual_ids() {
        let cfg = Config::default();
        let panels40: Vec<_> = (0..40)
            .map(|i| PanelPosition {
                id: i as u16 + 100,
                x: 0,
                y: 0,
            })
            .collect();
        let zones40 = make_zones(&panels40, &cfg).unwrap();
        assert_eq!(zones40[0].id, 100);
        assert!((zones40[0].y0 - 0.92).abs() < 0.001);
        let panels34: Vec<_> = (0..34)
            .map(|i| {
                let (x, y) = if i < 7 {
                    (300 - 50 * i as i64, 50)
                } else if i < 12 {
                    (0, 50 + 50 * (i as i64 - 6))
                } else if i < 21 {
                    (50 * (i as i64 - 11), 300)
                } else if i < 27 {
                    (450, 300 - 50 * (i as i64 - 20))
                } else {
                    (450 - 50 * (i as i64 - 26), 0)
                };
                PanelPosition { id: i as u16, x, y }
            })
            .collect();
        let zones34 = make_zones(&panels34, &cfg).unwrap();
        assert_eq!(zones34[0].id, 0);
        assert_eq!(zones34[33].id, 33);
        assert!(zones34
            .iter()
            .zip(&panels34)
            .all(|(zone, panel)| zone.id == panel.id));
        assert!(zones34[21].x0 > 0.8 && zones34[27].y0 > 0.8);
    }

    #[test]
    fn samples_edges_black_temporal_and_resets_disabled_crop() {
        let mut cfg = Config {
            letterbox: false,
            smooth: 1.0,
            max_step: 20,
            black_threshold: 0.0,
            saturation_boost: 0.0,
            peak_mix: 0,
            center_bleed: 0,
            feather_depth: 10,
            ..Config::default()
        };
        let w = 64;
        let h = 48;
        let data = solid_frame(w, h, [120, 30, 10]);
        let zone = Zone {
            id: 1,
            x0: 0.0,
            x1: 0.15,
            y0: 0.4,
            y1: 0.6,
        };
        let mut sampler = Sampler::new(vec![zone]);
        let value = sampler
            .sample(&data, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap()[0];
        assert!(value.r > 0.0);
        cfg.strict_black = true;
        let black = solid_frame(w, h, [0, 0, 0]);
        assert_eq!(
            sampler
                .sample(&black, w * 4, w, h, &cfg, Duration::from_millis(34))
                .unwrap()[0],
            Color::default()
        );
        cfg.strict_black = false;
        cfg.letterbox = true;
        let mut letterboxed = solid_frame(w, h, [0, 0, 0]);
        for y in 12..36 {
            for x in 0..w {
                let p = (y * w + x) * 4;
                letterboxed[p..p + 3].copy_from_slice(&[80, 40, 20]);
            }
        }
        sampler
            .sample(&letterboxed, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        assert!(sampler.active().y0 > 0.0);
        cfg.letterbox = false;
        sampler
            .sample(&data, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        assert_eq!(sampler.active(), Rect::default());
    }
}
