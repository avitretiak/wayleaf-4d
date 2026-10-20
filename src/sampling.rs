use crate::{AppError, Config, Result};
use std::time::Duration;
mod letterbox;
#[cfg(test)]
mod temporal_tests;
#[cfg(test)]
mod spatial_tests;
use letterbox::LetterboxDetector;

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
    /// Physical edge for live panel zones; `None` for synthetic/coarse zones.
    pub edge: Option<&'static str>,
    pub x0: f32,
    pub x1: f32,
    pub y0: f32,
    pub y1: f32,
}

fn spatial_weight_table(zones: &[Zone]) -> Vec<Vec<(usize, f32)>> {
    let mut table = Vec::with_capacity(zones.len());
    for (index, zone) in zones.iter().enumerate() {
        let mut neighbors = Vec::new();
        for (other_index, other) in zones.iter().enumerate() {
            if index == other_index { continue; }
            let dx = (zone.x0 + zone.x1 - other.x0 - other.x1) * 0.5;
            let dy = (zone.y0 + zone.y1 - other.y0 - other.y1) * 0.5;
            let distance_squared = dx * dx + dy * dy;
            if distance_squared < 0.35 * 0.35 {
                let distance = distance_squared.sqrt();
                neighbors.push((other_index, (1.0 - distance / 0.35).powi(2)));
            }
        }
        table.push(neighbors);
    }
    table
}

#[derive(Clone, Debug)]
pub struct PanelPosition {
    pub id: u16,
    pub x: i64,
    pub y: i64,
}

pub fn make_zones(panels: &[PanelPosition], cfg: &Config) -> Result<Vec<Zone>> {
    let n = panels.len();
    if !(1..=40).contains(&n) {
        return Err(AppError::new("unsupported Nanoleaf segment count"));
    }
    if !(5..=20).contains(&cfg.edge_depth) {
        return Err(AppError::new("edge depth must be in 5..20 percent"));
    }
    let border = cfg.edge_depth as f32 / 100.0;
    let min_x = panels.iter().map(|p| p.x).min().unwrap();
    let max_x = panels.iter().map(|p| p.x).max().unwrap();
    let min_y = panels.iter().map(|p| p.y).min().unwrap();
    let max_y = panels.iter().map(|p| p.y).max().unwrap();
    let dx = ((max_x as i128 - min_x as i128).max(1)) as f64;
    let dy = ((max_y as i128 - min_y as i128).max(1)) as f64;

    // Retain the calibrated rectangles only for the exact ordered NL69 layout.
    const REFERENCE: [(i64, i64); 40] = [
        (350,0),(300,0),(250,0),(200,0),(150,0),(100,0),(50,0),(0,0),
        (0,50),(0,100),(0,150),(0,200),(0,250),(0,300),(0,350),(0,400),
        (50,400),(100,400),(150,400),(200,400),(250,400),(300,400),(350,400),
        (400,400),(450,400),(500,400),(550,400),(600,400),(650,400),
        (650,350),(650,300),(650,250),(650,200),(650,150),(650,100),(650,50),
        (600,50),(550,50),(500,50),(450,50),
    ];
    let calibrated = n == 40 && panels.iter().zip(REFERENCE).all(|(p, xy)| (p.x,p.y) == xy);
    let mut zones = Vec::with_capacity(n);
    if calibrated {
        let corner = border * 1.5;
        for (i, panel) in panels.iter().enumerate() {
            let (x,y) = (panel.x as f32, panel.y as f32);
            let edge = if i == 7 { "bottom" } else if i == 15 { "left" }
                else if i == 28 { "top" } else if i == 35 { "right" }
                else if x == 0.0 { "left" } else if y >= 390.0 { "top" }
                else if x >= 640.0 { "right" } else { "bottom" };
            let (x0,x1,y0,y1) = if i == 7 {
                (0.0,corner,1.0-corner,1.0)
            } else if i == 15 {
                (0.0,corner,0.0,corner)
            } else if i == 28 {
                (1.0-corner,1.0,0.0,corner)
            } else if i == 35 {
                (1.0-corner,1.0,1.0-corner,1.0)
            } else if x == 0.0 {
                let cy = 1.0-y/400.0;
                let span = (50.0/400.0)*0.7;
                (0.0,border,(cy-span).max(0.0),(cy+span).min(1.0))
            } else if y >= 390.0 {
                let cx = x/650.0;
                let span = (50.0/650.0)*0.7;
                ((cx-span).max(0.0),(cx+span).min(1.0),0.0,border)
            } else if x >= 640.0 {
                let cy = 1.0-y/400.0;
                let span = (50.0/400.0)*0.7;
                (1.0-border,1.0,(cy-span).max(0.0),(cy+span).min(1.0))
            } else {
                let cx = x/650.0;
                let span = (50.0/650.0)*0.7;
                ((cx-span).max(0.0),(cx+span).min(1.0),1.0-border,1.0)
            };
            zones.push(Zone { id: panel.id, edge: Some(edge), x0,x1,y0,y1 });
        }
    } else {
        // Incoming route segments assign corners to the side just traversed.
        // Independent runs on a side are fitted into contiguous spans weighted
        // by extrapolated live-coordinate extents, not nominal panel counts.
        let edge_for = |p: &PanelPosition, sx: i128, sy: i128| -> usize {
            let dx_step=sx.unsigned_abs();
            let dy_step=sy.unsigned_abs();
            let vertical = if (dx_step == 0 && dy_step == 0) || dx_step == dy_step {
                let distances = [
                    (p.y as i128-min_y as i128).unsigned_abs(),
                    (p.x as i128-min_x as i128).unsigned_abs(),
                    (max_y as i128-p.y as i128).unsigned_abs(),
                    (max_x as i128-p.x as i128).unsigned_abs(),
                ];
                return (0..4).min_by_key(|&j| distances[j]).unwrap();
            } else { dx_step < dy_step };
            if vertical {
                if p.x as i128-min_x as i128 <= max_x as i128-p.x as i128 { 1 } else { 3 }
            } else if p.y as i128-min_y as i128 <= max_y as i128-p.y as i128 { 0 } else { 2 }
        };
        let mut sides=[0usize;40];
        for i in 0..n {
            let p=&panels[i];
            let prev = &panels[i.saturating_sub(1)];
            let mut sx = p.x as i128 - prev.x as i128;
            let mut sy = p.y as i128 - prev.y as i128;
            // A strip is open: the first panel has no incoming segment.
            // Use its outgoing step, never a fabricated last-to-first link.
            if sx == 0 && sy == 0 && i + 1 < n {
                let next = &panels[i + 1];
                sx = next.x as i128 - p.x as i128;
                sy = next.y as i128 - p.y as i128;
            }
            sides[i]=edge_for(p,sx,sy);
        }
        let tangent=|i:usize, side:usize| -> f32 {
            if side==0 || side==2 {
                ((panels[i].x as i128-min_x as i128) as f64/dx) as f32
            } else { ((max_y as i128-panels[i].y as i128) as f64/dy) as f32 }
        };
        let mut runs=[(0usize,0usize,0usize,0.0f32);40];
        let mut run_count=0usize;
        let mut totals=[0.0f32;4];
        let mut first=0;
        while first<n {
            let side=sides[first];
            let mut end=first+1;
            while end<n && sides[end]==side { end+=1; }
            let weight=if end-first==1 { 1.0 } else {
                let a=tangent(first,side);
                let b=tangent(first+1,side);
                let z=tangent(end-1,side);
                let y=tangent(end-2,side);
                let start_outer=a+(a-b)*0.5;
                let end_outer=z+(z-y)*0.5;
                let extent=(end_outer-start_outer).abs();
                if extent <= f32::EPSILON { 1.0 } else { extent }
            };
            runs[run_count]=(side,first,end,weight);
            run_count+=1;
            totals[side]+=weight;
            first=end;
        }
        let mut intervals=[(0.0f32,1.0f32);40];
        let mut cursors=[0.0f32;4];
        for run_index in 0..run_count {
            let (side,first,end,weight)=runs[run_index];
            let start=cursors[side];
            let finish=if totals[side]>0.0 { start+weight/totals[side] } else { 1.0 };
            let c0=tangent(first,side);
            let c1=tangent((first+1).min(end-1),side);
            let cl=tangent(end-1,side);
            let cp=tangent(if end-first>1 {end-2} else {first},side);
            let outer_start=if end-first>1 { c0+(c0-c1)*0.5 } else { c0-0.5 };
            let outer_end=if end-first>1 { cl+(cl-cp)*0.5 } else { cl+0.5 };
            if (outer_end-outer_start).abs() <= f32::EPSILON {
                let count=(end-first) as f32;
                for i in first..end {
                    let lo=start+(i-first) as f32/count*(finish-start);
                    let hi=start+(i-first+1) as f32/count*(finish-start);
                    intervals[i]=(lo,hi);
                }
                cursors[side]=finish;
                continue;
            }
            let outer_low=outer_start.min(outer_end);
            let outer_high=outer_start.max(outer_end);
            let map=|i:usize| {
                start+(tangent(i,side)-outer_low)/(outer_high-outer_low).max(f32::EPSILON)*(finish-start)
            };
            for i in first..end {
                let center=map(i);
                let before=if i>first {
                    (map(i-1)+center)*0.5
                } else if first+1<end {
                    center+(center-map(first+1))*0.5
                } else { start };
                let after=if i+1<end {
                    (center+map(i+1))*0.5
                } else if end-first>1 {
                    center+(center-map(end-2))*0.5
                } else { finish };
                intervals[i]=(before.min(after).clamp(start,finish),before.max(after).clamp(start,finish));
            }
            cursors[side]=finish;
        }
        for i in 0..n {
            let (a,b)=intervals[i];
            let (x0,x1,y0,y1)=match sides[i] {
                0 => (a,b,1.0-border,1.0),
                1 => (0.0,border,a,b),
                2 => (a,b,0.0,border),
                _ => (1.0-border,1.0,a,b),
            };
            zones.push(Zone { id:panels[i].id, edge:Some(["bottom","left","top","right"][sides[i]]), x0,x1,y0,y1 });
        }
    }

    // Alignment permutes references only; physical sampling rectangles never move.
    let corner_anchor = |left: bool, bottom: bool| {
        panels.iter().enumerate()
            .filter(|(_, p)| if left { p.x == min_x } else { p.x == max_x })
            .min_by_key(|(_, p)| if bottom { p.y.abs_diff(min_y) } else { p.y.abs_diff(max_y) })
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let start = match cfg.start_corner.as_str() {
        "bottom_center" => 0,
        "bottom_left" if calibrated => 7,
        "bottom_left" => corner_anchor(true, true),
        "top_left" if calibrated => 15,
        "top_left" => corner_anchor(true, false),
        "top_right" if calibrated => 28,
        "top_right" => corner_anchor(false, false),
        "bottom_right" if calibrated => 35,
        "bottom_right" => corner_anchor(false, true),
        _ => return Err(AppError::new("invalid start corner")),
    };
    let offset = cfg.perimeter_offset.rem_euclid(n as i32) as usize;
    let direction: isize = if cfg.reverse_direction { -1 } else { 1 };
    for (i, zone) in zones.iter_mut().enumerate() {
        let source = (start as isize + offset as isize + direction*i as isize).rem_euclid(n as isize) as usize;
        zone.id = panels[source].id;
    }
    Ok(zones)
}

pub fn zones_for_mode(mode: &str, physical: &[Zone], edge_depth: i32) -> Result<Vec<Zone>> {
    let border = edge_depth as f32 / 100.0;
    match mode {
        "1D" => Ok(Vec::new()),
        "2D" => Ok(vec![Zone {
            id: 0,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        }]),
        "3D" => {
            let mut zones = Vec::with_capacity(10);
            for i in 0..3 {
                zones.push(Zone {
                    id: zones.len() as u16,
                    edge: Some("top"),
                    x0: i as f32 / 3.0,
                    x1: (i + 1) as f32 / 3.0,
                    y0: 0.0,
                    y1: border,
                });
            }
            for i in 0..2 {
                zones.push(Zone {
                    id: zones.len() as u16,
                    edge: Some("right"),
                    x0: 1.0 - border,
                    x1: 1.0,
                    y0: i as f32 / 2.0,
                    y1: (i + 1) as f32 / 2.0,
                });
            }
            for i in 0..3 {
                zones.push(Zone {
                    id: zones.len() as u16,
                    edge: Some("bottom"),
                    x0: i as f32 / 3.0,
                    x1: (i + 1) as f32 / 3.0,
                    y0: 1.0 - border,
                    y1: 1.0,
                });
            }
            for i in 0..2 {
                zones.push(Zone {
                    id: zones.len() as u16,
                    edge: Some("left"),
                    x0: 0.0,
                    x1: border,
                    y0: i as f32 / 2.0,
                    y1: (i + 1) as f32 / 2.0,
                });
            }
            Ok(zones)
        }
        "4D" => Ok(physical.to_vec()),
        _ => Err(AppError::new("unsupported sampling mode")),
    }
}

pub fn colors_for_mode(
    mode: &str,
    physical: &[Zone],
    sample_zones: &[Zone],
    sampled: &[Color],
    static_color: &str,
    brightness: f32,
    output: &mut Vec<Color>,
) -> Result<()> {
    if sample_zones.len() != sampled.len() {
        return Err(AppError::new("invalid mode sampling dimensions"));
    }
    output.resize(physical.len(), Color::default());
    match mode {
        "1D" => {
            let hex = static_color
                .strip_prefix('#')
                .filter(|hex| hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .ok_or_else(|| AppError::new("invalid static color"))?;
            let channel = |start| {
                u8::from_str_radix(&hex[start..start + 2], 16)
                    .map(f32::from)
                    .map_err(|_| AppError::new("invalid static color"))
            };
            output.fill(Color {
                r: (channel(0)? * brightness).clamp(0.0, 255.0),
                g: (channel(2)? * brightness).clamp(0.0, 255.0),
                b: (channel(4)? * brightness).clamp(0.0, 255.0),
            });
        }
        "2D" if sampled.len() == 1 => output.fill(sampled[0]),
        "2D" => return Err(AppError::new("2D mode requires one sampled color")),
        "3D" if sampled.len() == 10 => {
            for (color, zone) in output.iter_mut().zip(physical) {
                let center = ((zone.x0 + zone.x1) * 0.5, (zone.y0 + zone.y1) * 0.5);
                let closest = sample_zones
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        let distance = |zone: &Zone| {
                            let x = (zone.x0 + zone.x1) * 0.5 - center.0;
                            let y = (zone.y0 + zone.y1) * 0.5 - center.1;
                            x * x + y * y
                        };
                        distance(a).total_cmp(&distance(b))
                    })
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                *color = sampled[closest];
            }
        }
        "3D" => return Err(AppError::new("3D mode requires ten sampled colors")),
        "4D" if physical.len() == sampled.len() => output.copy_from_slice(sampled),
        "4D" => return Err(AppError::new("4D mode requires one color per panel")),
        _ => return Err(AppError::new("unsupported sampling mode")),
    }
    Ok(())
}

pub fn preview_zones_for_mode(
    mode: &str,
    physical: &[Zone],
    sample_zones: &[Zone],
) -> Result<Vec<Zone>> {
    match mode {
        "1D" | "2D" => physical
            .first()
            .cloned()
            .map(|zone| vec![zone])
            .ok_or_else(|| AppError::new("no physical zones for mode preview")),
        "3D" if sample_zones.len() == 10 && !physical.is_empty() => Ok(sample_zones.to_vec()),
        "3D" => Err(AppError::new(
            "3D preview requires ten zones and physical panels",
        )),
        "4D" => Ok(physical.to_vec()),
        _ => Err(AppError::new("unsupported preview mode")),
    }
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
// Chroma is less reliable in dim captures; taper saturation weighting/boost below 15% luma.
const LOW_LIGHT_CHROMA_LUMA: f32 = 0.15;

#[inline]
fn low_light_chroma_strength(luminance: f32) -> f32 {
    let t = (luminance / LOW_LIGHT_CHROMA_LUMA).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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


#[inline]
fn packet_triplet(channels: [f32; 3], previous: [u8; 3]) -> [u8; 3] {
    let channels = channels.map(|value| value.clamp(0.0, 255.0));
    let peak = channels[0].max(channels[1]).max(channels[2]);
    let previous_peak = previous[0].max(previous[1]).max(previous[2]);
    // Hold only the shared intensity anchor, never the preceding chroma.
    let packet_peak = if peak == 0.0 || peak == 255.0 || (peak - previous_peak as f32).abs() > 0.75
    {
        peak.round() as u8
    } else {
        previous_peak
    };
    channels.map(|value| {
        if value == 0.0 || value == 255.0 {
            value as u8
        } else {
            packet_peak.saturating_sub((peak - value).round() as u8)
        }
    })
}

#[inline]
fn coord(value: f32) -> usize {
    value as usize
}

#[derive(Clone, Copy, Default)]
struct ZoneTemporalState {
    packet: [u8; 3],
    black_elapsed: f32,
    confirmed_black: bool,
    fade_start: Color,
    fade_elapsed: f32,
    fade_duration: f32,
    fade_max_step: i32,
}

pub struct Sampler {
    zones: Vec<Zone>,
    previous: Vec<Color>,
    out: Vec<Color>,
    raw: Vec<Color>,
    temporal: Vec<ZoneTemporalState>,
    spatial_weights: Vec<Vec<(usize, f32)>>,
    spatial_enabled: bool,
    active: Rect,
    detector: LetterboxDetector,
    frames: u64,
    global: Color,
    scene_cut: bool,
    dimensions: (usize, usize),
}

impl Sampler {
    fn set_spatial_blend(&mut self, enabled: bool) {
        if self.spatial_enabled != enabled {
            self.spatial_enabled = enabled;
            for state in &mut self.temporal {
                state.packet = [0; 3];
            }
        }
    }
    pub fn new(zones: Vec<Zone>) -> Self {
        let n = zones.len();
        Self {
            spatial_weights: spatial_weight_table(&zones),
            spatial_enabled: false,
            zones,
            previous: vec![Color::default(); n],
            out: vec![Color::default(); n],
            raw: vec![Color::default(); n],
            temporal: vec![ZoneTemporalState::default(); n],
            active: Rect::default(),
            detector: LetterboxDetector::new(),
            frames: 0,
            global: Color::default(),
            scene_cut: false,
            dimensions: (0, 0),
        }
    }

    pub fn remap(&mut self, next: Vec<Zone>) {
        let changed = next.len() != self.zones.len()
            || next.iter().zip(&self.zones).any(|(a, b)| {
                a.id != b.id || a.x0 != b.x0 || a.x1 != b.x1 || a.y0 != b.y0 || a.y1 != b.y1
            });
        if changed {
            self.zones = next;
            self.spatial_weights = spatial_weight_table(&self.zones);
            let n = self.zones.len();
            self.previous.resize(n, Color::default());
            self.out.resize(n, Color::default());
            self.raw.resize(n, Color::default());
            self.temporal.resize(n, ZoneTemporalState::default());
            self.previous.fill(Color::default());
            self.out.fill(Color::default());
            self.raw.fill(Color::default());
            self.temporal.fill(ZoneTemporalState::default());
        } else {
            self.zones = next;
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
        self.sample_inner(data, stride, width, height, cfg, elapsed, true)
    }

    pub fn sample_full_screen(
        &mut self,
        data: &[u8],
        stride: usize,
        width: usize,
        height: usize,
        cfg: &Config,
        elapsed: Duration,
    ) -> Result<&[Color]> {
        self.sample_inner(data, stride, width, height, cfg, elapsed, false)
    }

    fn sample_inner(
        &mut self,
        data: &[u8],
        stride: usize,
        width: usize,
        height: usize,
        cfg: &Config,
        elapsed: Duration,
        use_letterbox: bool,
    ) -> Result<&[Color]> {
        self.set_spatial_blend(
            cfg.spatial_blend && matches!(cfg.mode.as_str(), "3D" | "4D"),
        );
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
        let expected = Duration::from_secs_f32(1.0 / cfg.fps.max(1) as f32);
        let gap_limit = expected.saturating_mul(2).max(Duration::from_millis(500));
        let effective_elapsed = if elapsed > gap_limit {
            elapsed.min(Duration::from_millis(100))
        } else {
            elapsed
        };
        if self.dimensions != (width, height) {
            self.dimensions = (width, height);
            self.detector.reset();
            self.active = Rect::default();
        }
        if use_letterbox && cfg.letterbox {
            self.active = self
                .detector
                .update(data, stride, width, height, effective_elapsed);
        } else {
            self.active = Rect::default();
            self.detector.reset();
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
        let center = Color {
            r: (sr / count as f64) as f32,
            g: (sg / count as f64) as f32,
            b: (sb / count as f64) as f32,
        };
        let mut picture_r = 0.0f64;
        let mut picture_g = 0.0f64;
        let mut picture_b = 0.0f64;
        // The sparse grid includes the captured-frame edges while the dense center sample keeps prior cut sensitivity.
        for row in 0..4 {
            let y = row * (height - 1) / 3;
            for column in 0..4 {
                let x = column * (width - 1) / 3;
                let c = pixel(data, stride, x, y);
                picture_r += c.r as f64;
                picture_g += c.g as f64;
                picture_b += c.b as f64;
            }
        }
        let current_global = Color {
            r: (center.r + (picture_r / 16.0) as f32) * 0.5,
            g: (center.g + (picture_g / 16.0) as f32) * 0.5,
            b: (center.b + (picture_b / 16.0) as f32) * 0.5,
        };
        self.scene_cut = self.frames > 1 && {
            let dr = ((current_global.r - self.global.r) / 255.0) as f64;
            let dg = ((current_global.g - self.global.g) / 255.0) as f64;
            let db = ((current_global.b - self.global.b) / 255.0) as f64;
            (dr.powi(2) + dg.powi(2) + db.powi(2)).sqrt() > 0.35
        };
        self.global = current_global;

        let feather = cfg.feather_depth as f32 / 100.0;
        let elapsed = effective_elapsed.as_secs_f32();
        let alpha = 1.0 - (1.0 - cfg.smooth).powf(elapsed * 30.0);
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
                    let chroma_weight = low_light_chroma_strength(lum);
                    let weight = (1.0 + sat_boost * sat * sat * chroma_weight) * spatial;
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
            target = mix(target, center, center_bleed);
            let gate_luma = luma(target);
            if threshold > 0.0 {
                if gate_luma <= threshold {
                    target = Color::default();
                } else {
                    let ramp = ((gate_luma - threshold) / threshold).clamp(0.0, 1.0);
                    target = Color {
                        r: target.r * ramp,
                        g: target.g * ramp,
                        b: target.b * ramp,
                    };
                }
            }
            let effective_sat_boost =
                1.0 + (sat_boost - 1.0) * low_light_chroma_strength(luma(target));
            target = boost_saturation(target, effective_sat_boost);
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
            let state = &mut self.temporal[i];
            let zero_target = target.r == 0.0 && target.g == 0.0 && target.b == 0.0;
            if strict_black && zero_target {
                self.previous[i] = Color::default();
                self.out[i] = Color::default();
                state.packet = [0; 3];
                state.black_elapsed = 0.0;
                state.confirmed_black = false;
                state.fade_start = Color::default();
                state.fade_elapsed = 0.0;
                state.fade_duration = 0.0;
                continue;
            }
            let near_black = threshold > 0.0 && gate_luma <= threshold;
            let exit_black = threshold > 0.0 && gate_luma > threshold * 1.25;
            let immediate_zero = zero_target && (threshold == 0.0 || gate_luma > threshold);
            if immediate_zero {
                state.black_elapsed = 0.0;
                if !state.confirmed_black {
                    state.confirmed_black = true;
                    state.fade_start = self.previous[i];
                    state.fade_elapsed = 0.0;
                    let max_rgb = state
                        .fade_start
                        .r
                        .max(state.fade_start.g)
                        .max(state.fade_start.b);
                    state.fade_duration = 0.3f32.max(max_rgb / (cfg.max_step.max(1) as f32 * 30.0));
                    state.fade_max_step = cfg.max_step;
                }
            } else if near_black {
                state.black_elapsed += elapsed;
                if state.black_elapsed >= 0.15 && !state.confirmed_black {
                    state.confirmed_black = true;
                    state.fade_start = self.previous[i];
                    state.fade_elapsed = 0.0;
                    let max_rgb = state
                        .fade_start
                        .r
                        .max(state.fade_start.g)
                        .max(state.fade_start.b);
                    state.fade_duration = 0.3f32.max(max_rgb / (cfg.max_step.max(1) as f32 * 30.0));
                    state.fade_max_step = cfg.max_step;
                }
            } else if !zero_target && (threshold == 0.0 || exit_black) {
                state.black_elapsed = 0.0;
                state.confirmed_black = false;
                state.fade_elapsed = 0.0;
                state.fade_duration = 0.0;
                state.fade_max_step = 0;
            } else if !state.confirmed_black {
                state.black_elapsed = 0.0;
            }
            let prev = self.previous[i];
            if state.confirmed_black {
                if cfg.max_step < state.fade_max_step {
                    state.fade_start = self.previous[i];
                    state.fade_elapsed = 0.0;
                    let max_rgb = state
                        .fade_start
                        .r
                        .max(state.fade_start.g)
                        .max(state.fade_start.b);
                    state.fade_duration = 0.3f32.max(max_rgb / (cfg.max_step.max(1) as f32 * 30.0));
                    state.fade_max_step = cfg.max_step;
                }
                state.fade_elapsed += elapsed;
                let t = (state.fade_elapsed / state.fade_duration).clamp(0.0, 1.0);
                self.previous[i] = Color {
                    r: state.fade_start.r * (1.0 - t),
                    g: state.fade_start.g * (1.0 - t),
                    b: state.fade_start.b * (1.0 - t),
                };
                if t >= 1.0 {
                    self.previous[i] = Color::default();
                }
            } else if near_black {
                // Hold the previous float color during entry confirmation.
            } else {
                let delta = Color {
                    r: alpha * (target.r - prev.r),
                    g: alpha * (target.g - prev.g),
                    b: alpha * (target.b - prev.b),
                };
                let max_delta = delta.r.abs().max(delta.g.abs()).max(delta.b.abs());
                let scale = if max_delta > step && max_delta > 0.0 {
                    step / max_delta
                } else {
                    1.0
                };
                let mut response = Color {
                    r: prev.r + delta.r * scale,
                    g: prev.g + delta.g * scale,
                    b: prev.b + delta.b * scale,
                };
                let movement = (target.r - prev.r)
                    .abs()
                    .max((target.g - prev.g).abs())
                    .max((target.b - prev.b).abs());
                let target_scale = target
                    .r
                    .abs()
                    .max(target.g.abs())
                    .max(target.b.abs())
                    .max(1.0);
                if elapsed > 0.0
                    && movement <= 4.0 * f32::EPSILON * target_scale
                    && movement <= step
                {
                    response = target;
                }
                self.previous[i] = response;
            }
        }
        self.write_output();
        Ok(&self.out)
    }

    fn write_output(&mut self) {
        for i in 0..self.zones.len() {
            let source = self.previous[i];
            let output = if self.spatial_enabled
                && (source.r != 0.0 || source.g != 0.0 || source.b != 0.0)
            {
                let mut weighted = Color::default();
                let mut total_weight = 0.0;
                for &(neighbor, weight) in &self.spatial_weights[i] {
                    let color = self.previous[neighbor];
                    weighted.r += color.r * weight;
                    weighted.g += color.g * weight;
                    weighted.b += color.b * weight;
                    total_weight += weight;
                }
                if total_weight > 0.0 {
                    mix(source, Color {
                        r: weighted.r / total_weight,
                        g: weighted.g / total_weight,
                        b: weighted.b / total_weight,
                    }, 0.2)
                } else {
                    source
                }
            } else {
                source
            };
            let mut packet = packet_triplet(
                [output.r, output.g, output.b],
                self.temporal[i].packet,
            );
            if self.temporal[i].confirmed_black {
                for (channel, previous) in packet.iter_mut().zip(self.temporal[i].packet) {
                    *channel = (*channel).min(previous);
                }
            }
            self.temporal[i].packet = packet;
            self.out[i] = Color {
                r: packet[0] as f32,
                g: packet[1] as f32,
                b: packet[2] as f32,
            };
        }
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
    let reference = [
        (350,0),(300,0),(250,0),(200,0),(150,0),(100,0),(50,0),(0,0),
        (0,50),(0,100),(0,150),(0,200),(0,250),(0,300),(0,350),(0,400),
        (50,400),(100,400),(150,400),(200,400),(250,400),(300,400),(350,400),
        (400,400),(450,400),(500,400),(550,400),(600,400),(650,400),
        (650,350),(650,300),(650,250),(650,200),(650,150),(650,100),(650,50),
        (600,50),(550,50),(500,50),(450,50),
    ];
    let panels: Vec<_> = reference.into_iter().enumerate()
        .map(|(i,(x,y))| PanelPosition { id: i as u16 + 100, x, y }).collect();
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
    let zones34 = make_zones(&panels34, &cfg)?;
    let valid_live_geometry = zones34.len() == panels34.len()
        && zones34.iter().zip(&panels34).all(|(zone,panel)| {
            zone.id == panel.id
                && zone.edge.is_some()
                && zone.x0 >= 0.0 && zone.x0 < zone.x1 && zone.x1 <= 1.0
                && zone.y0 >= 0.0 && zone.y0 < zone.y1 && zone.y1 <= 1.0
        });
    if !valid_live_geometry {
        return Err(AppError::new("sampler 34-panel geometry self-test failed"));
    }
    let edge_zones = zones_for_mode("3D", &[], cfg.edge_depth)?;
    if edge_zones.len() != 10
        || edge_zones[0].y0 != 0.0
        || edge_zones[3].x0 != 1.0 - cfg.edge_depth as f32 / 100.0
        || edge_zones[5].y0 != 1.0 - cfg.edge_depth as f32 / 100.0
        || edge_zones[8].x0 != 0.0
    {
        return Err(AppError::new("3D edge-zone geometry self-test failed"));
    }
    let physical_zones: Vec<_> = edge_zones
        .iter()
        .enumerate()
        .map(|(index, zone)| Zone {
            id: index as u16 + 100,
            ..zone.clone()
        })
        .collect();
    let edge_colors: Vec<_> = (0..10)
        .map(|index| Color {
            r: index as f32 * 20.0,
            g: index as f32 * 10.0,
            b: 255.0 - index as f32 * 10.0,
        })
        .collect();
    let mut mode_colors = Vec::new();
    colors_for_mode(
        "3D",
        &physical_zones,
        &edge_zones,
        &edge_colors,
        "#ffffff",
        1.0,
        &mut mode_colors,
    )?;
    let preview_3d = preview_zones_for_mode("3D", &physical_zones, &edge_zones)?;
    if mode_colors != edge_colors || preview_3d != edge_zones {
        return Err(AppError::new(
            "3D preview geometry or physical panel mapping self-test failed",
        ));
    }
    let mapped_panels = [
        Zone {
            id: 211,
            edge: None,
            x0: 0.0,
            x1: 0.1,
            y0: 0.0,
            y1: 0.08,
        },
        Zone {
            id: 34,
            edge: None,
            x0: 0.9,
            x1: 1.0,
            y0: 0.0,
            y1: 0.08,
        },
        Zone {
            id: 12,
            edge: None,
            x0: 0.92,
            x1: 1.0,
            y0: 0.72,
            y1: 0.8,
        },
        Zone {
            id: 3,
            edge: None,
            x0: 0.0,
            x1: 0.08,
            y0: 0.72,
            y1: 0.8,
        },
    ];
    let mut mapped_colors = Vec::new();
    colors_for_mode(
        "3D",
        &mapped_panels,
        &edge_zones,
        &edge_colors,
        "#ffffff",
        1.0,
        &mut mapped_colors,
    )?;
    if mapped_colors
        != vec![
            edge_colors[0],
            edge_colors[2],
            edge_colors[4],
            edge_colors[9],
        ]
    {
        return Err(AppError::new("3D panel-to-edge mapping self-test failed"));
    }
    colors_for_mode(
        "1D",
        &physical_zones,
        &[],
        &[],
        "#01a2FF",
        2.0,
        &mut mode_colors,
    )?;
    let static_color = Color {
        r: 2.0,
        g: 255.0,
        b: 255.0,
    };
    let preview_1d = preview_zones_for_mode("1D", &physical_zones, &[])?;
    if mode_colors.len() != physical_zones.len()
        || mode_colors.iter().any(|color| *color != static_color)
        || preview_1d.len() != 1
        || !physical_zones
            .iter()
            .any(|zone| zone.id == preview_1d[0].id)
    {
        return Err(AppError::new("1D static color output self-test failed"));
    }
    let full_zone = zones_for_mode("2D", &physical_zones, cfg.edge_depth)?;
    let dominant = Color {
        r: 21.0,
        g: 42.0,
        b: 84.0,
    };
    colors_for_mode(
        "2D",
        &physical_zones,
        &full_zone,
        &[dominant],
        "#ffffff",
        1.0,
        &mut mode_colors,
    )?;
    let preview_2d = preview_zones_for_mode("2D", &physical_zones, &full_zone)?;
    if mode_colors.len() != physical_zones.len()
        || mode_colors.iter().any(|color| *color != dominant)
        || preview_2d.len() != 1
    {
        return Err(AppError::new("2D uniform output self-test failed"));
    }
    colors_for_mode(
        "4D",
        &physical_zones,
        &physical_zones,
        &edge_colors,
        "#ffffff",
        1.0,
        &mut mode_colors,
    )?;
    if mode_colors != edge_colors
        || preview_zones_for_mode("4D", &physical_zones, &physical_zones)? != physical_zones
    {
        return Err(AppError::new("4D sampling preservation self-test failed"));
    }
    let w = 32usize;
    let h = 24usize;
    let stride = w * 4 + 8;
    let mut screen_config = cfg.clone();
    screen_config.letterbox = true;
    screen_config.smooth = 1.0;
    screen_config.max_step = 255;
    let mut screen_frame = vec![0u8; stride * h];
    for y in 6..18 {
        for x in 8..24 {
            let pixel = y * stride + x * 4;
            screen_frame[pixel..pixel + 3].copy_from_slice(&[200, 0, 0]);
        }
    }
    let mut full_sampler = Sampler::new(full_zone.clone());
    let full = full_sampler.sample_full_screen(
        &screen_frame,
        stride,
        w,
        h,
        &screen_config,
        Duration::from_millis(34),
    )?[0];
    let mut low_step_config = screen_config.clone();
    low_step_config.max_step = 1;
    let mut low_step_sampler = Sampler::new(full_zone.clone());
    let low_step = low_step_sampler.sample_full_screen(
        &screen_frame,
        stride,
        w,
        h,
        &low_step_config,
        Duration::from_millis(34),
    )?[0];
    if full == low_step || low_step.r <= 0.0 || low_step.r >= full.r * 0.1 {
        return Err(AppError::new("2D maximum-step filtering self-test failed"));
    }
    let mut cropped_sampler = Sampler::new(full_zone);
    for _ in 0..5 {
        cropped_sampler.sample(
            &screen_frame,
            stride,
            w,
            h,
            &screen_config,
            Duration::from_millis(100),
        )?;
    }
    let cropped = cropped_sampler.out()[0];
    if full.r <= 0.0 || full.r >= cropped.r * 0.75 {
        return Err(AppError::new("2D full-screen sampling self-test failed"));
    }
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
        edge: None,
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
        edge: None,
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
        edge: None,
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
        edge: None,
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
    if (stepped.r - expected_step).abs() > 0.5 {
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
    use std::collections::HashSet;

    fn solid_frame(w: usize, h: usize, color: [u8; 3]) -> Vec<u8> {
        let mut data = vec![0; w * h * 4];
        for p in data.chunks_exact_mut(4) {
            p[..3].copy_from_slice(&color);
            p[3] = 255;
        }
        data
    }

    fn reference40() -> Vec<PanelPosition> {
        let xy = [
            (350,0),(300,0),(250,0),(200,0),(150,0),(100,0),(50,0),(0,0),
            (0,50),(0,100),(0,150),(0,200),(0,250),(0,300),(0,350),(0,400),
            (50,400),(100,400),(150,400),(200,400),(250,400),(300,400),(350,400),
            (400,400),(450,400),(500,400),(550,400),(600,400),(650,400),
            (650,350),(650,300),(650,250),(650,200),(650,150),(650,100),(650,50),
            (600,50),(550,50),(500,50),(450,50),
        ];
        xy.into_iter().enumerate().map(|(i,(x,y))| PanelPosition {
            id: i as u16 + 100, x, y,
        }).collect()
    }

    fn route_fixture(bottom_left: usize, left: usize, top: usize, right: usize, bottom_right: usize) -> Vec<PanelPosition> {
        let mut xy = Vec::new();
        // Four explicit corners, then evenly spaced side centers. Route begins
        // on the lower-left bottom run and finishes on lower-right bottom run.
        for i in 0..bottom_left {
            xy.push((450 - (i as i64 + 1) * 450 / (bottom_left as i64 + 1), 0));
        }
        xy.push((0,0));
        for i in 0..left { xy.push((0, (i as i64 + 1) * 400 / (left as i64 + 1))); }
        xy.push((0,400));
        for i in 0..top { xy.push(((i as i64 + 1) * 650 / (top as i64 + 1),400)); }
        xy.push((650,400));
        for i in 0..right { xy.push((650,400 - (i as i64 + 1) * 400 / (right as i64 + 1))); }
        xy.push((650,0));
        for i in 0..bottom_right { xy.push((650 - (i as i64 + 1) * 200 / (bottom_right as i64 + 1),0)); }
        xy.into_iter().enumerate().map(|(i,(x,y))| PanelPosition {
            id: (1000 + i * 3) as u16, x, y,
        }).collect()
    }

    #[test]
    fn mapping_uses_live_geometry_preserves_ids_and_alignment_only_permutates_ids() {
        let ref_panels = reference40();
        let cfg = Config::default();
        let reference_zones = make_zones(&ref_panels, &cfg).unwrap();
        assert_eq!(reference_zones[0].id, ref_panels[0].id);
        assert!((reference_zones[0].y0 - 0.92).abs() < 0.001);
        let panels32: Vec<_> = [
            (450,0),(400,0),(350,0),(300,0),(250,0),(200,0),(150,0),(100,0),(50,0),(0,0),
            (0,50),(0,100),(0,150),(0,200),(0,250),
            (50,250),(100,250),(150,250),(200,250),(250,250),(300,250),(350,250),(400,250),(450,250),(500,250),
            (500,200),(500,150),(500,100),(500,50),(500,0),(450,0),(400,0),
        ].into_iter().enumerate().map(|(i,(x,y))| PanelPosition { id:i as u16,x,y }).collect();
        let zones32 = make_zones(&panels32, &cfg).unwrap();
        let expected_edges: Vec<_> = (0..32).map(|i| match i {
            0..=9 | 30..=31 => "bottom",
            10..=14 => "left",
            15..=24 => "top",
            _ => "right",
        }).collect();
        assert!(zones32.iter().zip(expected_edges.iter()).all(|(zone,expected)| zone.edge==Some(*expected)));
        let edge_counts=zones32.iter().fold([0usize;4],|mut counts,zone| {
            counts[match zone.edge.unwrap() {
                "bottom"=>0,"left"=>1,"top"=>2,_=>3,
            }]+=1;
            counts
        });
        assert_eq!(edge_counts,[12,5,10,5]);
        let mut bottom=zones32.iter().filter(|zone| zone.edge==Some("bottom")).collect::<Vec<_>>();
        bottom.sort_by(|a,b| a.x0.total_cmp(&b.x0));
        assert_eq!(bottom.len(),12);
        assert!(bottom.windows(2).all(|pair| (pair[0].x1-pair[1].x0).abs()<0.001));
        assert!((bottom[0].x0).abs()<0.001 && (bottom[11].x1-1.0).abs()<0.001);
        let deep_cfg=Config { edge_depth:20, ..cfg.clone() };
        let deep=make_zones(&panels32,&deep_cfg).unwrap();
        assert!(deep.iter().zip(expected_edges.iter()).all(|(zone,edge)| zone.edge==Some(*edge)));
        let panels34:Vec<_>=(0..34).map(|i| {
            let (x,y)=if i<7 { (300-50*i as i64,50) }
            else if i<12 { (0,50+50*(i as i64-6)) }
            else if i<21 { (50*(i as i64-11),300) }
            else if i<27 { (450,300-50*(i as i64-20)) }
            else { (450-50*(i as i64-26),0) };
            PanelPosition { id:(200+i*5) as u16,x,y }
        }).collect();
        let zones34=make_zones(&panels34,&cfg).unwrap();
        let edges34:Vec<_>=(0..34).map(|i| match i {
            0..=6 | 27..=33 => "bottom",
            7..=11 => "left",
            12..=20 => "top",
            _ => "right",
        }).collect();
        assert!(zones34.iter().zip(edges34).all(|(zone,edge)| zone.edge==Some(edge)));
        for (n, fixture) in [
            (32, panels32),
            (34, panels34),
            (40, ref_panels.clone()),
        ] {
            assert_eq!(fixture.len(), n);
            let zones = make_zones(&fixture, &cfg).unwrap();
            assert_eq!(zones.len(), n);
            assert!(zones.iter().zip(&fixture).all(|(z,p)| z.id == p.id));
            assert!(zones.iter().all(|z| 0.0 <= z.x0 && z.x0 < z.x1 && z.x1 <= 1.0
                && 0.0 <= z.y0 && z.y0 < z.y1 && z.y1 <= 1.0));
            assert!(zones.iter().all(|z| z.edge.is_some()));
        }
        let generic40 = route_fixture(12,7,11,6,0);
        let live = make_zones(&generic40, &cfg).unwrap();
        assert_eq!(live.len(), 40);
        assert_ne!((live[0].x0,live[0].x1,live[0].y0,live[0].y1),
            (reference_zones[0].x0,reference_zones[0].x1,reference_zones[0].y0,reference_zones[0].y1));
    }

    #[test]
    fn all_supported_counts_have_valid_geometry_and_alignment_permutation() {
        for n in 1..=40 {
            let panels: Vec<_> = (0..n).map(|i| PanelPosition {
                id: (500 + i * 7) as u16,
                x: i as i64 * 3,
                y: i as i64 * 5,
            }).collect();
            let base = make_zones(&panels, &Config::default()).unwrap();
            for corner in ["bottom_center","bottom_left","top_left","top_right","bottom_right"] {
                for reverse in [false,true] {
                    for offset in [-39,-1,0,1,39] {
                        let cfg = Config { start_corner: corner.into(), reverse_direction: reverse,
                            perimeter_offset: offset, ..Config::default() };
                        let zones = make_zones(&panels, &cfg).unwrap();
                        let ids: HashSet<_> = zones.iter().map(|z| z.id).collect();
                        assert_eq!(zones.len(),n);
                        assert_eq!(ids.len(),n);
                        assert!(zones.iter().zip(&base).all(|(a,b)|
                            (a.x0,a.x1,a.y0,a.y1)==(b.x0,b.x1,b.y0,b.y1)));
                        assert!(zones.iter().all(|z| z.x0 >= 0.0 && z.x1 <= 1.0
                            && z.y0 >= 0.0 && z.y1 <= 1.0 && z.x0 < z.x1 && z.y0 < z.y1));
                    }
                }
            }
        }
    }

    #[test]
    fn shortened_strip_has_no_invented_closing_segment() {
        let panels = reference40();
        for count in 1..=panels.len() {
            let zones = make_zones(&panels[..count], &Config::default()).unwrap();
            assert_eq!(zones.len(), count);
            assert_eq!(zones[0].edge, Some("bottom"), "cut to {count} panels");
            assert!(zones.iter().all(|zone| zone.x0 < zone.x1 && zone.y0 < zone.y1));
            assert!(zones.iter().zip(&panels).all(|(zone, panel)| zone.id == panel.id));
        }
    }
    #[test]
    fn alignment_matches_reference_permutations_and_saved_offsets_after_reenumeration() {
        let p32=[
            (450,0),(400,0),(350,0),(300,0),(250,0),(200,0),(150,0),(100,0),(50,0),(0,0),
            (0,50),(0,100),(0,150),(0,200),(0,250),
            (50,250),(100,250),(150,250),(200,250),(250,250),(300,250),(350,250),(400,250),(450,250),(500,250),
            (500,200),(500,150),(500,100),(500,50),(500,0),(450,0),(400,0),
        ];
        let panels32:Vec<_>=p32.into_iter().enumerate().map(|(i,(x,y))| PanelPosition{id:i as u16,x,y}).collect();
        let panels34:Vec<_>=(0..34).map(|i| {
            let (x,y)=if i<7 { (300-50*i as i64,50) }
            else if i<12 { (0,50+50*(i as i64-6)) }
            else if i<21 { (50*(i as i64-11),300) }
            else if i<27 { (450,300-50*(i as i64-20)) }
            else { (450-50*(i as i64-26),0) };
            PanelPosition{id:i as u16,x,y}
        }).collect();
        let panels40=reference40();
        for (panels,anchors) in [
            (&panels32,[0,9,14,24,29]),
            (&panels34,[0,6,11,20,26]),
            (&panels40,[0,7,15,28,35]),
        ] {
            let n=panels.len();
            let offsets:Vec<i32>=if n==40 { vec![-39,-1,0,1,39] } else { vec![-39,-(n as i32),-1,0,1,n as i32,39] };
            for (corner,&start) in ["bottom_center","bottom_left","top_left","top_right","bottom_right"].iter().zip(&anchors) {
                for reverse in [false,true] {
                    for &offset in &offsets {
                        let cfg=Config{start_corner:(*corner).into(),reverse_direction:reverse,
                            perimeter_offset:offset,..Config::default()};
                        let zones=make_zones(panels,&cfg).unwrap();
                        for i in 0..n {
                            let direction=if reverse {-1isize} else {1isize};
                            let expected=(start as isize+offset.rem_euclid(n as i32) as isize
                                +direction*i as isize).rem_euclid(n as isize) as usize;
                            assert_eq!(zones[i].id,panels[expected].id);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn shuffled_panel_ids_follow_position_data_and_alignment_without_moving_rectangles() {
        let mut panels = reference40();
        for (i, panel) in panels.iter_mut().enumerate() {
            panel.id = (39 - i) as u16 * 11;
        }
        let base = make_zones(&panels, &Config::default()).unwrap();
        assert!(base.iter().zip(&panels).all(|(zone,panel)| zone.id == panel.id));
        let aligned = make_zones(&panels, &Config {
            start_corner: "top_right".into(),
            reverse_direction: true,
            perimeter_offset: -39,
            ..Config::default()
        }).unwrap();
        assert!(aligned.iter().zip(&base).all(|(a,b)|
            (a.x0,a.x1,a.y0,a.y1)==(b.x0,b.x1,b.y0,b.y1)));
        assert_eq!(aligned[0].id, panels[29].id);
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
            edge: None,
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
            for x in 16..48 {
                let p = (y * w + x) * 4;
                letterboxed[p..p + 3].copy_from_slice(&[80, 40, 20]);
            }
        }
        sampler
            .sample(&letterboxed, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        assert_eq!(sampler.active(), Rect::default());
        for _ in 0..14 {
            sampler
                .sample(&letterboxed, w * 4, w, h, &cfg, Duration::from_millis(34))
                .unwrap();
        }
        assert!(sampler.active().y0 > 0.0);
        cfg.letterbox = false;
        sampler
            .sample(&data, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        assert_eq!(sampler.active(), Rect::default());
    }

    #[test]
    fn low_light_chroma_noise_is_not_exaggerated() {
        let w = 64;
        let h = 48;
        let zone = Zone {
            id: 1,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        };
        let cfg = Config {
            letterbox: false,
            smooth: 1.0,
            max_step: 255,
            black_threshold: 0.0,
            saturation_boost: 1.25,
            brightness: 1.0,
            peak_mix: 0,
            center_bleed: 0,
            feather_depth: 0,
            ..Config::default()
        };
        let mut outputs = Vec::with_capacity(3);
        for noise_channel in 0..3 {
            let mut frame = solid_frame(w, h, [12, 12, 12]);
            for y in (0..h).step_by(8) {
                for x in (0..w).step_by(8) {
                    let pixel = (y * w + x) * 4;
                    for channel in 0..3 {
                        if channel != noise_channel {
                            frame[pixel + channel] = 0;
                        }
                    }
                }
            }
            let mut sampler = Sampler::new(vec![zone.clone()]);
            let color = sampler
                .sample(&frame, w * 4, w, h, &cfg, Duration::from_millis(34))
                .unwrap()[0];
            let min = color.r.min(color.g).min(color.b);
            let max = color.r.max(color.g).max(color.b);
            assert!(
                max - min < 1.2,
                "low-light chroma noise was exaggerated: {color:?}"
            );
            outputs.push(color);
        }
        for first in 0..outputs.len() {
            for second in first + 1..outputs.len() {
                let delta = (outputs[first].r - outputs[second].r)
                    .abs()
                    .max((outputs[first].g - outputs[second].g).abs())
                    .max((outputs[first].b - outputs[second].b).abs());
                assert!(
                    delta < 1.5,
                    "low-light noise changed hue between channels: {delta}"
                );
            }
        }
    }

    #[test]
    fn black_threshold_suppresses_dim_red_and_preserves_dim_neutral() {
        let w = 64;
        let h = 48;
        let zone = Zone {
            id: 1,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        };
        let cfg = Config {
            letterbox: false,
            smooth: 1.0,
            max_step: 255,
            black_threshold: 0.01,
            saturation_boost: 1.25,
            brightness: 1.0,
            peak_mix: 0,
            center_bleed: 0,
            feather_depth: 0,
            ..Config::default()
        };
        let dim_red = solid_frame(w, h, [8, 0, 0]);
        let dim_gray = solid_frame(w, h, [15, 15, 15]);
        let mut sampler = Sampler::new(vec![zone]);
        assert_eq!(
            sampler
                .sample(&dim_red, w * 4, w, h, &cfg, Duration::from_millis(34))
                .unwrap()[0],
            Color::default()
        );
        assert_eq!(sampler.raw()[0], Color::default());
        assert_eq!(
            sampler
                .sample(&dim_gray, w * 4, w, h, &cfg, Duration::from_millis(34))
                .unwrap()[0],
            Color {
                r: 15.0,
                g: 15.0,
                b: 15.0,
            }
        );
    }

    #[test]
    fn full_screen_scene_cuts_respect_the_maximum_step() {
        let w = 64;
        let h = 48;
        let zone = Zone {
            id: 1,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        };
        let cfg = Config {
            letterbox: false,
            smooth: 1.0,
            max_step: 10,
            black_threshold: 0.0,
            saturation_boost: 0.0,
            brightness: 1.0,
            peak_mix: 0,
            center_bleed: 0,
            feather_depth: 0,
            ..Config::default()
        };
        let black = solid_frame(w, h, [0, 0, 0]);
        let red = solid_frame(w, h, [200, 20, 10]);
        let mut sampler = Sampler::new(vec![zone]);
        sampler
            .sample_full_screen(&black, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        let color = sampler
            .sample_full_screen(&red, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap()[0];
        let step = cfg.max_step as f32 * 0.034 * 30.0;
        assert!(sampler.scene_cut());
        assert!(color.r <= step + 0.001);
        assert!(color.g <= step + 0.001);
        assert!(color.b <= step + 0.001);
    }

    #[test]
    fn scene_cuts_keep_response_smoothing_for_local_zone_changes() {
        let w = 64;
        let h = 48;
        let zone = Zone {
            id: 1,
            edge: None,
            x0: 0.0,
            x1: 0.15,
            y0: 0.4,
            y1: 0.6,
        };
        let mut cfg = Config {
            letterbox: false,
            smooth: 1.0,
            max_step: 255,
            black_threshold: 0.0,
            saturation_boost: 0.0,
            brightness: 1.0,
            peak_mix: 0,
            center_bleed: 0,
            feather_depth: 0,
            ..Config::default()
        };
        let mut first = solid_frame(w, h, [0, 0, 0]);
        for y in 19..29 {
            for x in 0..10 {
                let pixel = (y * w + x) * 4;
                first[pixel..pixel + 3].copy_from_slice(&[40, 60, 80]);
            }
        }
        let mut second = first.clone();
        for y in 19..29 {
            for x in 0..10 {
                let pixel = (y * w + x) * 4;
                second[pixel..pixel + 3].copy_from_slice(&[20, 60, 80]);
            }
        }
        for y in 12..36 {
            for x in 16..48 {
                let pixel = (y * w + x) * 4;
                second[pixel..pixel + 3].fill(255);
            }
        }
        let mut sampler = Sampler::new(vec![zone]);
        sampler
            .sample(&first, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        cfg.smooth = 0.1;
        let color = sampler
            .sample(&second, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap()[0];
        assert!(sampler.scene_cut());
        assert!(
            color.r > 20.0 && color.r < 40.0,
            "scene cut skipped configured smoothing: {color:?}"
        );
    }

    #[test]
    fn detects_cuts_that_change_only_the_picture_perimeter() {
        let w = 64;
        let h = 48;
        let mut sampler = Sampler::new(Vec::new());
        let cfg = Config::default();
        let black = solid_frame(w, h, [0, 0, 0]);
        sampler
            .sample(&black, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        assert!(!sampler.scene_cut());

        let mut perimeter = black;
        for y in 0..h {
            for x in 0..w {
                if x == 0 || x + 1 == w || y == 0 || y + 1 == h {
                    perimeter[(y * w + x) * 4..(y * w + x) * 4 + 3]
                        .copy_from_slice(&[255, 255, 255]);
                }
            }
        }
        sampler
            .sample(&perimeter, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        assert!(sampler.scene_cut());
    }

    #[test]
    fn black_fade_remains_monotonic_when_max_step_changes() {
        let w = 32;
        let h = 24;
        let zone = Zone {
            id: 1,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        };
        let warm = solid_frame(w, h, [200, 80, 20]);
        let black = solid_frame(w, h, [0, 0, 0]);
        let mut cfg = Config {
            fps: 10,
            smooth: 1.0,
            max_step: 255,
            black_threshold: 0.0,
            saturation_boost: 0.0,
            peak_mix: 0,
            center_bleed: 0,
            feather_depth: 0,
            ..Config::default()
        };
        let mut sampler = Sampler::new(vec![zone]);
        sampler
            .sample_full_screen(&warm, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap();
        let before = sampler
            .sample_full_screen(&black, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap()[0];
        cfg.max_step = 1;
        let after = sampler
            .sample_full_screen(&black, w * 4, w, h, &cfg, Duration::from_millis(34))
            .unwrap()[0];
        assert!(after.r <= before.r && after.g <= before.g && after.b <= before.b);
    }
}
