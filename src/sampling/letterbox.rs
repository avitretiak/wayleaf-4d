use super::{luma, pixel, Rect};
use std::time::Duration;

const DARK_LUMA: f32 = 12.0 / 255.0;
const BRIGHT_LUMA: f32 = 24.0 / 255.0;
const DARK_OCCUPANCY: f32 = 0.82;
const EVIDENCE_OCCUPANCY: f32 = 0.10;
const MIN_BORDER: usize = 4;
const MAX_BORDER_FRACTION: usize = 3;
const CANDIDATE_MIN_TIME: Duration = Duration::from_millis(500);
const CANDIDATE_MIN_OBSERVATIONS: u8 = 2;
const CROP_TOLERANCE_FRACTION: f32 = 0.0125;

#[derive(Clone, Copy, Debug, PartialEq)]
struct AxisCrop {
    start: usize,
    end: usize,
}

impl AxisCrop {
    fn full(size: usize) -> Self {
        Self {
            start: 0,
            end: size,
        }
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    x: AxisCrop,
    y: AxisCrop,
    elapsed: Duration,
    observations: u8,
}

pub(super) struct LetterboxDetector {
    width: usize,
    height: usize,
    current_x: AxisCrop,
    current_y: AxisCrop,
    candidate: Option<Candidate>,
}

impl LetterboxDetector {
    pub(super) fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            current_x: AxisCrop::full(0),
            current_y: AxisCrop::full(0),
            candidate: None,
        }
    }

    pub(super) fn reset(&mut self) {
        self.width = 0;
        self.height = 0;
        self.current_x = AxisCrop::full(0);
        self.current_y = AxisCrop::full(0);
        self.candidate = None;
    }

    pub(super) fn update(
        &mut self,
        data: &[u8],
        stride: usize,
        width: usize,
        height: usize,
        elapsed: Duration,
    ) -> Rect {
        if width == 0 || height == 0 || stride < width.saturating_mul(4) {
            self.reset();
            return Rect::default();
        }
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.current_x = AxisCrop::full(width);
            self.current_y = AxisCrop::full(height);
            self.candidate = None;
        }

        let mut x = detect_axis(data, stride, width, height, false).unwrap_or(self.current_x);
        let mut y = detect_axis(data, stride, width, height, true).unwrap_or(self.current_y);
        let changed = x != self.current_x || y != self.current_y;
        let full_frame = x == AxisCrop::full(width) && y == AxisCrop::full(height);
        if changed && !full_frame && !common_picture_aspect(x, y) {
            x = self.current_x;
            y = self.current_y;
        }
        if x == self.current_x && y == self.current_y {
            self.candidate = None;
        } else {
            let tolerance_x = pixel_tolerance(width);
            let tolerance_y = pixel_tolerance(height);
            let matches = self
                .candidate
                .is_some_and(|c| near(c.x, x, tolerance_x) && near(c.y, y, tolerance_y));
            if matches {
                let candidate = self.candidate.as_mut().unwrap();
                candidate.x = x;
                candidate.y = y;
                candidate.elapsed = candidate.elapsed.saturating_add(elapsed);
                candidate.observations = candidate.observations.saturating_add(1);
            } else {
                self.candidate = Some(Candidate {
                    x,
                    y,
                    elapsed,
                    observations: 1,
                });
            }
            if let Some(candidate) = self.candidate {
                if candidate.elapsed >= CANDIDATE_MIN_TIME
                    && candidate.observations >= CANDIDATE_MIN_OBSERVATIONS
                {
                    self.current_x = candidate.x;
                    self.current_y = candidate.y;
                    self.candidate = None;
                }
            }
        }

        Rect {
            x0: self.current_x.start as f32 / width as f32,
            x1: self.current_x.end as f32 / width as f32,
            y0: self.current_y.start as f32 / height as f32,
            y1: self.current_y.end as f32 / height as f32,
        }
    }
}

fn detect_axis(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    vertical: bool,
) -> Option<AxisCrop> {
    let size = if vertical { height } else { width };
    let extent = if vertical { width } else { height };
    if size < 2 * MIN_BORDER + 1 || extent == 0 {
        return None;
    }
    let limit = size / MAX_BORDER_FRACTION;
    let mut start = 0;
    while start < limit && line_dark(data, stride, width, height, vertical, start) {
        start += 1;
    }
    let mut end_border = 0;
    while end_border < limit
        && line_dark(data, stride, width, height, vertical, size - 1 - end_border)
    {
        end_border += 1;
    }
    let end = size - end_border;
    if start >= MIN_BORDER && end_border >= MIN_BORDER {
        if start.abs_diff(end_border) > pixel_tolerance(size) {
            return None;
        }
        if has_interior_evidence(data, stride, width, height, vertical, start, end) {
            return Some(AxisCrop { start, end });
        }
        return None;
    }

    // Full-frame evidence must come from both edges as well as the interior;
    // an all-dark image is therefore ambiguous rather than an uncrop signal.
    if start == 0
        && end_border == 0
        && line_bright(data, stride, width, height, vertical, 0)
        && line_bright(data, stride, width, height, vertical, size - 1)
        && has_interior_evidence(data, stride, width, height, vertical, 0, size)
    {
        Some(AxisCrop::full(size))
    } else {
        None
    }
}

fn pixel_tolerance(size: usize) -> usize {
    ((size as f32 * CROP_TOLERANCE_FRACTION).round() as usize).max(2)
}

fn common_picture_aspect(x: AxisCrop, y: AxisCrop) -> bool {
    const FOUR_THREE: f32 = 4.0 / 3.0;
    const FOUR_THREE_TOLERANCE: f32 = 0.04;
    const WIDESCREEN_CINEMA_MIN: f32 = 1.80;
    const WIDESCREEN_CINEMA_MAX: f32 = 2.45;

    let width = x.end.saturating_sub(x.start);
    let height = y.end.saturating_sub(y.start);
    if width == 0 || height == 0 {
        return false;
    }
    let aspect = width as f32 / height as f32;
    (aspect - FOUR_THREE).abs() <= FOUR_THREE_TOLERANCE
        || (WIDESCREEN_CINEMA_MIN..=WIDESCREEN_CINEMA_MAX).contains(&aspect)
}

fn near(a: AxisCrop, b: AxisCrop, tolerance: usize) -> bool {
    a.start.abs_diff(b.start) <= tolerance && a.end.abs_diff(b.end) <= tolerance
}

fn sample_range(size: usize) -> impl Iterator<Item = usize> {
    (0..size).step_by(4)
}

fn line_dark(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    vertical: bool,
    line: usize,
) -> bool {
    let extent = if vertical { width } else { height };
    let mut dark = 0usize;
    let mut count = 0usize;
    for p in sample_range(extent) {
        let (x, y) = if vertical { (p, line) } else { (line, p) };
        dark += usize::from(luma(pixel(data, stride, x, y)) <= DARK_LUMA);
        count += 1;
    }
    count != 0 && dark as f32 / count as f32 >= DARK_OCCUPANCY
}

fn line_bright(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    vertical: bool,
    line: usize,
) -> bool {
    let extent = if vertical { width } else { height };
    let mut bright = 0usize;
    let mut count = 0usize;
    for p in sample_range(extent) {
        let (x, y) = if vertical { (p, line) } else { (line, p) };
        bright += usize::from(luma(pixel(data, stride, x, y)) >= BRIGHT_LUMA);
        count += 1;
    }
    count != 0 && bright as f32 / count as f32 >= EVIDENCE_OCCUPANCY
}

fn has_interior_evidence(
    data: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    vertical: bool,
    start: usize,
    end: usize,
) -> bool {
    if end <= start + 2 {
        return false;
    }
    let low = start + (end - start) / 4;
    let high = start + 3 * (end - start) / 4;
    if low >= high {
        return false;
    }
    let mut bright = 0usize;
    let mut count = 0usize;
    for line in (low..high).step_by(4) {
        let extent = if vertical { width } else { height };
        for p in sample_range(extent) {
            let (x, y) = if vertical { (p, line) } else { (line, p) };
            bright += usize::from(luma(pixel(data, stride, x, y)) >= BRIGHT_LUMA);
            count += 1;
        }
    }
    count != 0 && bright as f32 / count as f32 >= EVIDENCE_OCCUPANCY
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize, value: u8) -> Vec<u8> {
        vec![value; width * height * 4]
    }

    fn paint(
        frame: &mut [u8],
        width: usize,
        x0: usize,
        x1: usize,
        y0: usize,
        y1: usize,
        value: u8,
    ) {
        for y in y0..y1 {
            for x in x0..x1 {
                let p = (y * width + x) * 4;
                frame[p..p + 3].fill(value);
                frame[p + 3] = 255;
            }
        }
    }

    fn tick(detector: &mut LetterboxDetector, data: &[u8], w: usize, h: usize, ms: u64) -> Rect {
        detector.update(data, w * 4, w, h, Duration::from_millis(ms))
    }

    fn settled(data: &[u8], w: usize, h: usize, interval: u64) -> Rect {
        let mut d = LetterboxDetector::new();
        tick(&mut d, data, w, h, interval);
        tick(&mut d, data, w, h, interval)
    }

    #[test]
    fn uniform_dark_and_full_black_never_crop() {
        for value in [0, 8, 20] {
            let data = frame(160, 90, value);
            assert_eq!(settled(&data, 160, 90, 500), Rect::default());
        }
    }

    #[test]
    fn stable_dark_borders_with_unusual_active_aspect_do_not_crop() {
        let mut data = frame(160, 90, 0);
        paint(&mut data, 160, 0, 160, 14, 76, 160);
        assert_eq!(settled(&data, 160, 90, 500), Rect::default());
    }

    #[test]
    fn symmetric_bars_are_accepted_after_confidence() {
        let mut data = frame(160, 90, 0);
        paint(&mut data, 160, 0, 160, 11, 79, 160);
        let mut d = LetterboxDetector::new();
        assert_eq!(tick(&mut d, &data, 160, 90, 250), Rect::default());
        let rect = tick(&mut d, &data, 160, 90, 250);
        assert!((rect.y0 - 11.0 / 90.0).abs() < 0.02);
        assert!((rect.y1 - 79.0 / 90.0).abs() < 0.02);
    }

    #[test]
    fn transient_candidate_does_not_flip_immediately() {
        let mut d = LetterboxDetector::new();
        let mut bars = frame(160, 90, 0);
        paint(&mut bars, 160, 0, 160, 11, 79, 160);
        assert_eq!(tick(&mut d, &bars, 160, 90, 100), Rect::default());
        let black = frame(160, 90, 0);
        assert_eq!(tick(&mut d, &black, 160, 90, 100), Rect::default());
    }

    #[test]
    fn subtitles_in_black_bar_preserve_crop_and_dark_scene_holds() {
        let mut d = LetterboxDetector::new();
        let mut bars = frame(160, 90, 0);
        paint(&mut bars, 160, 0, 160, 11, 79, 160);
        tick(&mut d, &bars, 160, 90, 300);
        let crop = tick(&mut d, &bars, 160, 90, 300);
        assert!((crop.y0 - 11.0 / 90.0).abs() < 0.02);

        let mut subtitle = bars.clone();
        paint(&mut subtitle, 160, 60, 100, 80, 84, 255);
        for _ in 0..5 {
            assert_eq!(tick(&mut d, &subtitle, 160, 90, 200), crop);
        }
        assert_eq!(tick(&mut d, &bars, 160, 90, 200), crop);
        assert_eq!(tick(&mut d, &frame(160, 90, 0), 160, 90, 5000), crop);
    }

    #[test]
    fn bad_edge_frame_does_not_replace_stable_crop() {
        let mut d = LetterboxDetector::new();
        let mut bars = frame(160, 90, 0);
        paint(&mut bars, 160, 0, 160, 11, 79, 160);
        tick(&mut d, &bars, 160, 90, 300);
        let crop = tick(&mut d, &bars, 160, 90, 300);
        let mut bad_edge = bars.clone();
        paint(&mut bad_edge, 160, 0, 160, 0, 5, 255);
        assert_eq!(tick(&mut d, &bad_edge, 160, 90, 100), crop);
    }

    #[test]
    fn combined_bars_and_candidate_jitter_preserve_geometry() {
        let mut d = LetterboxDetector::new();
        let mut bars = frame(200, 120, 0);
        paint(&mut bars, 200, 20, 180, 26, 94, 160);
        tick(&mut d, &bars, 200, 120, 300);
        let crop = tick(&mut d, &bars, 200, 120, 300);
        assert!((crop.x0 - 20.0 / 200.0).abs() < 0.02);
        assert!((crop.x1 - 180.0 / 200.0).abs() < 0.02);
        assert!((crop.y0 - 26.0 / 120.0).abs() < 0.02);
        assert!((crop.y1 - 94.0 / 120.0).abs() < 0.02);

        let mut jitter = frame(200, 120, 0);
        paint(&mut jitter, 200, 18, 182, 26, 94, 160);
        for _ in 0..4 {
            assert_eq!(tick(&mut d, &jitter, 200, 120, 100), crop);
        }
    }

    #[test]
    fn dimension_change_discards_pending_candidate() {
        let mut d = LetterboxDetector::new();
        let mut bars = frame(160, 90, 0);
        paint(&mut bars, 160, 0, 160, 11, 79, 160);
        assert_eq!(tick(&mut d, &bars, 160, 90, 300), Rect::default());
        let mut new_bars = frame(200, 120, 0);
        paint(&mut new_bars, 200, 0, 200, 18, 102, 160);
        assert_eq!(tick(&mut d, &new_bars, 200, 120, 300), Rect::default());
        let crop = tick(&mut d, &new_bars, 200, 120, 300);
        assert!((crop.y0 - 18.0 / 120.0).abs() < 0.02);
        assert!((crop.y1 - 102.0 / 120.0).abs() < 0.02);
    }

    #[test]
    fn credible_edges_eventually_uncrop() {
        let mut d = LetterboxDetector::new();
        let mut bars = frame(160, 90, 0);
        paint(&mut bars, 160, 0, 160, 11, 79, 160);
        tick(&mut d, &bars, 160, 90, 300);
        let cropped = tick(&mut d, &bars, 160, 90, 300);
        assert_ne!(cropped, Rect::default());
        let full = frame(160, 90, 100);
        assert_eq!(tick(&mut d, &full, 160, 90, 250), cropped);
        assert_eq!(tick(&mut d, &full, 160, 90, 250), Rect::default());
    }

    #[test]
    fn pillarbox_and_asymmetric_edges_are_handled_independently() {
        let mut pillars = frame(160, 90, 0);
        paint(&mut pillars, 160, 20, 140, 0, 90, 160);
        let rect = settled(&pillars, 160, 90, 300);
        assert!((rect.x0 - 20.0 / 160.0).abs() < 0.02);
        assert_eq!(rect.y0, 0.0);
        let mut asymmetric = frame(160, 90, 0);
        paint(&mut asymmetric, 160, 0, 160, 5, 76, 160);
        assert_eq!(settled(&asymmetric, 160, 90, 300).y0, 0.0);
    }

    #[test]
    fn dimensions_and_reset_return_to_full_frame() {
        let mut d = LetterboxDetector::new();
        let mut bars = frame(160, 90, 0);
        paint(&mut bars, 160, 0, 160, 11, 79, 160);
        tick(&mut d, &bars, 160, 90, 300);
        assert_ne!(tick(&mut d, &bars, 160, 90, 300), Rect::default());
        assert_eq!(
            tick(&mut d, &frame(80, 45, 100), 80, 45, 100),
            Rect::default()
        );
        d.reset();
        assert_eq!(
            tick(&mut d, &frame(80, 45, 100), 80, 45, 100),
            Rect::default()
        );
    }

    #[test]
    fn acceptance_uses_elapsed_time_and_repeated_observations_at_all_rates() {
        for interval in [1000, 200, 100] {
            let mut d = LetterboxDetector::new();
            let mut bars = frame(160, 90, 0);
            paint(&mut bars, 160, 0, 160, 11, 79, 160);
            assert_eq!(tick(&mut d, &bars, 160, 90, interval), Rect::default());
            let required_observations = 500_u64.div_ceil(interval).max(2);
            for observation in 1..required_observations {
                let rect = tick(&mut d, &bars, 160, 90, interval);
                if observation + 1 < required_observations {
                    assert_eq!(rect, Rect::default());
                } else {
                    assert_ne!(rect, Rect::default());
                }
            }
        }
    }
}
