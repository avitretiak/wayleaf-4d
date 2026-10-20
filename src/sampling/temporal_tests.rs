use super::{Color, Sampler, Zone};
use crate::Config;
use std::time::Duration;

const WIDTH: usize = 32;
const HEIGHT: usize = 24;

fn frame(rgb: [u8; 3]) -> Vec<u8> {
    let mut pixels = vec![0; WIDTH * HEIGHT * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[..3].copy_from_slice(&rgb);
        pixel[3] = 255;
    }
    pixels
}

fn cfg() -> Config {
    Config {
        brightness: 1.0,
        smooth: 1.0,
        max_step: 255,
        black_threshold: 0.0,
        saturation_boost: 0.0,
        peak_mix: 0,
        center_bleed: 0,
        feather_depth: 0,
        letterbox: false,
        fps: 10,
        strict_black: false,
        ..Config::default()
    }
}

fn zone(id: u16) -> Zone {
    Zone {
        id,
        edge: None,
        x0: 0.0,
        x1: 1.0,
        y0: 0.0,
        y1: 1.0,
    }
}

fn step(sampler: &mut Sampler, data: &[u8], config: &Config, elapsed: Duration) -> Color {
    sampler
        .sample(data, WIDTH * 4, WIDTH, HEIGHT, config, elapsed)
        .unwrap()[0]
}

#[test]
fn colored_transition_limits_each_channel_and_reaches_endpoint() {
    let mut config = cfg();
    config.smooth = 0.35;
    config.max_step = 12;
    let warm = frame([255, 80, 0]);
    let mut sampler = Sampler::new(vec![zone(1)]);
    let first = step(&mut sampler, &warm, &config, Duration::from_millis(100));
    assert_eq!(
        first,
        Color {
            r: 36.0,
            g: 11.0,
            b: 0.0
        }
    );
    for _ in 0..100 {
        step(&mut sampler, &warm, &config, Duration::from_millis(100));
    }
    assert_eq!(
        sampler.out()[0],
        Color {
            r: 255.0,
            g: 80.0,
            b: 0.0
        }
    );
}

#[test]
fn equal_wall_clock_response_is_stable_across_frame_rates() {
    let data = frame([150, 90, 40]);
    let mut results = Vec::new();
    for hz in [1u64, 5, 10] {
        let mut config = cfg();
        config.smooth = 0.05;
        config.fps = hz as i32;
        let mut sampler = Sampler::new(vec![zone(1)]);
        for _ in 0..hz {
            step(
                &mut sampler,
                &data,
                &config,
                Duration::from_secs_f64(1.0 / hz as f64),
            );
        }
        results.push(sampler.out()[0]);
    }
    for channel in [|c: Color| c.r, |c: Color| c.g, |c: Color| c.b] {
        let values: Vec<f32> = results.iter().copied().map(channel).collect();
        assert!(
            values.iter().all(|v| (*v - values[0]).abs() <= 1.0),
            "{values:?}"
        );
        assert!(
            values.iter().all(|v| *v
                < channel(Color {
                    r: 150.0,
                    g: 90.0,
                    b: 40.0
                })),
            "{values:?}"
        );
    }
}

#[test]
fn long_gap_is_capped_and_preserves_previous_color() {
    let mut sampler = Sampler::new(vec![zone(1)]);
    let mut config = cfg();
    config.max_step = 12;
    config.smooth = 0.35;
    let red = frame([200, 30, 10]);
    for _ in 0..20 {
        step(&mut sampler, &red, &config, Duration::from_millis(100));
    }
    let prior = sampler.out()[0];
    let blue = frame([10, 20, 220]);
    let next = step(&mut sampler, &blue, &config, Duration::from_secs(5));
    let delta = (next.r - prior.r)
        .abs()
        .max((next.g - prior.g).abs())
        .max((next.b - prior.b).abs());
    assert!(
        delta <= 37.0,
        "gap exceeded the capped response step: {prior:?} -> {next:?}"
    );
    assert!(next.r > 0.0, "long gap discarded previous red: {next:?}");
    assert!(
        next.b > prior.b,
        "new blue did not increase: {prior:?} -> {next:?}"
    );
}

#[test]
fn threshold_requires_confirmation_and_hysteresis_to_wake() {
    let mut config = cfg();
    config.black_threshold = 0.01;
    let mut sampler = Sampler::new(vec![zone(1)]);
    let warm = frame([80, 80, 80]);
    for _ in 0..20 {
        step(&mut sampler, &warm, &config, Duration::from_millis(100));
    }
    let prior = sampler.out()[0];
    let black = frame([0, 0, 0]);
    let first = step(&mut sampler, &black, &config, Duration::from_millis(100));
    assert_eq!(
        first, prior,
        "one low sample should hold output during confirmation"
    );
    let confirmed = step(&mut sampler, &black, &config, Duration::from_millis(100));
    assert!(confirmed.r < prior.r && confirmed.g < prior.g && confirmed.b < prior.b);
    assert!(
        confirmed.r > 0.0,
        "fade should not finish on its first packet: {confirmed:?}"
    );
    let mut faded = confirmed;
    for _ in 0..3 {
        faded = step(&mut sampler, &black, &config, Duration::from_millis(100));
    }
    assert_eq!(faded, Color::default());

    let between = frame([3, 3, 3]);
    for _ in 0..5 {
        let held = step(&mut sampler, &between, &config, Duration::from_millis(100));
        assert_eq!(
            held,
            Color::default(),
            "input in hysteresis band woke output: {held:?}"
        );
    }
    let wake = step(
        &mut sampler,
        &frame([5, 5, 5]),
        &config,
        Duration::from_millis(100),
    );
    assert!(
        wake.r > 0.0 && wake.g > 0.0 && wake.b > 0.0,
        "above-band input did not wake output: {wake:?}"
    );
}

#[test]
fn zero_threshold_black_and_zero_brightness_wake_cleanly() {
    let mut config = cfg();
    config.smooth = 0.05;
    let mut sampler = Sampler::new(vec![zone(1)]);
    let bright = frame([200, 100, 50]);
    let black = frame([0, 0, 0]);
    step(&mut sampler, &bright, &config, Duration::from_millis(100));
    let prior = sampler.out()[0];
    let zero = step(&mut sampler, &black, &config, Duration::from_millis(100));
    assert!(
        zero.r > 0.0 && zero.r < prior.r,
        "zero-threshold black should fade: {prior:?} -> {zero:?}"
    );
    for _ in 0..3 {
        step(&mut sampler, &black, &config, Duration::from_millis(100));
    }
    assert_eq!(sampler.out()[0], Color::default());
    step(&mut sampler, &bright, &config, Duration::from_millis(100));
    assert!(sampler.out()[0].r > 0.0);

    let before_darkening = sampler.out()[0];
    config.brightness = 0.0;
    let darkened = step(&mut sampler, &bright, &config, Duration::from_millis(100));
    assert!(
        darkened.r > 0.0 && darkened.r < before_darkening.r,
        "zero brightness should fade: {before_darkening:?} -> {darkened:?}"
    );
    for _ in 0..3 {
        step(&mut sampler, &bright, &config, Duration::from_millis(100));
    }
    assert_eq!(sampler.out()[0], Color::default());
    config.brightness = 1.0;
    let restored = step(&mut sampler, &bright, &config, Duration::from_millis(100));
    assert!(
        restored.r > 0.0 && restored.g > 0.0 && restored.b > 0.0,
        "restored brightness failed to wake: {restored:?}"
    );
}

#[test]
fn strict_black_is_immediate_and_disabling_it_does_not_restore_fade() {
    let mut config = cfg();
    config.max_step = 1;
    config.strict_black = true;
    let mut sampler = Sampler::new(vec![zone(1)]);
    let warm = frame([200, 100, 50]);
    step(&mut sampler, &warm, &config, Duration::from_millis(100));
    assert!(sampler.out()[0].r > 0.0);
    let black = frame([0, 0, 0]);
    assert_eq!(
        step(&mut sampler, &black, &config, Duration::from_millis(100)),
        Color::default()
    );
    config.strict_black = false;
    for _ in 0..5 {
        assert_eq!(
            step(&mut sampler, &black, &config, Duration::from_millis(100)),
            Color::default()
        );
    }
}

#[test]
fn confirmed_fade_is_bounded_monotonic_and_interruptible() {
    let mut config = cfg();
    config.black_threshold = 0.01;
    config.max_step = 1;
    let mut sampler = Sampler::new(vec![zone(1)]);
    let warm = frame([120, 60, 30]);
    for _ in 0..10 {
        step(&mut sampler, &warm, &config, Duration::from_millis(100));
    }
    let initial = sampler.out()[0];
    let black = frame([0, 0, 0]);
    step(&mut sampler, &black, &config, Duration::from_millis(100));
    let mut prior = step(&mut sampler, &black, &config, Duration::from_millis(100));
    let bound = (initial.r.max(initial.g).max(initial.b) / 3.0).ceil() as usize + 12;
    for _ in 0..bound {
        let current = step(&mut sampler, &black, &config, Duration::from_millis(100));
        let delta = (current.r - prior.r)
            .abs()
            .max((current.g - prior.g).abs())
            .max((current.b - prior.b).abs());
        assert!(
            delta <= 4.0,
            "fade packet step too large: {prior:?} -> {current:?}"
        );
        assert!(
            current.r <= prior.r && current.g <= prior.g && current.b <= prior.b,
            "fade was not monotonic: {prior:?} -> {current:?}"
        );
        prior = current;
    }
    assert_eq!(
        prior,
        Color::default(),
        "fade exceeded its finite bound of {bound} packets"
    );

    for _ in 0..10 {
        step(&mut sampler, &warm, &config, Duration::from_millis(100));
    }
    step(&mut sampler, &black, &config, Duration::from_millis(100));
    let before_interrupt = step(&mut sampler, &black, &config, Duration::from_millis(100));
    let interrupted = step(&mut sampler, &warm, &config, Duration::from_millis(100));
    assert!(
        interrupted.r > before_interrupt.r,
        "valid color did not interrupt fade: {before_interrupt:?} -> {interrupted:?}"
    );
}

#[test]
fn packet_quantization_holds_jitter_tracks_drift_and_preserves_endpoints() {
    let mut config = cfg();
    let uniform = frame([100, 100, 100]);
    let mut sampler = Sampler::new(vec![zone(1)]);
    config.brightness = 0.1049;
    let first = step(&mut sampler, &uniform, &config, Duration::from_millis(100));
    config.brightness = 0.1051;
    let second = step(&mut sampler, &uniform, &config, Duration::from_millis(100));
    assert_eq!(
        first, second,
        "sub-byte input jitter chattered: {first:?} -> {second:?}"
    );
    for index in 0..100 {
        config.brightness = if index % 2 == 0 { 0.1049 } else { 0.1051 };
        let held = step(&mut sampler, &uniform, &config, Duration::from_millis(100));
        assert_eq!(
            held, first,
            "sub-byte input jitter chattered: {first:?} -> {held:?}"
        );
    }

    config.brightness = 0.1049;
    let mut drift_output = first;
    for index in 1050..=1300 {
        config.brightness = index as f32 / 10_000.0;
        drift_output = step(&mut sampler, &uniform, &config, Duration::from_millis(100));
    }
    assert_eq!(
        drift_output,
        Color {
            r: 13.0,
            g: 13.0,
            b: 13.0
        },
        "slow brightness drift did not reach packet 13: {drift_output:?}"
    );

    config.brightness = 0.0;
    let black = frame([0, 0, 0]);
    for _ in 0..10 {
        step(&mut sampler, &black, &config, Duration::from_millis(100));
    }
    assert_eq!(sampler.out()[0], Color::default());
    config.brightness = 1.0;
    let white = frame([255, 255, 255]);
    let endpoint = step(&mut sampler, &white, &config, Duration::from_millis(100));
    assert_eq!(
        endpoint,
        Color {
            r: 255.0,
            g: 255.0,
            b: 255.0
        }
    );
}

#[test]
fn packet_quantization_releases_near_neutral_chroma_across_rgb_permutations() {
    for mask in 1..7 {
        let mut config = cfg();
        config.brightness = 0.1;
        let mut sampler = Sampler::new(vec![zone(1)]);
        let warm =
            std::array::from_fn(|channel| if mask & (1 << channel) != 0 { 110 } else { 100 });
        step(
            &mut sampler,
            &frame(warm),
            &config,
            Duration::from_millis(100),
        );
        config.brightness = 10.49 / 101.0;
        let near_neutral =
            std::array::from_fn(|channel| if mask & (1 << channel) != 0 { 101 } else { 100 });
        let output = step(
            &mut sampler,
            &frame(near_neutral),
            &config,
            Duration::from_millis(100),
        );
        assert!(
            output.r == output.g && output.g == output.b,
            "mask {mask} retained chroma below one quantization step: {output:?}"
        );
    }
}

#[test]
fn packet_quantization_does_not_flash_chroma_during_near_neutral_brightness_ramps() {
    let mut config = cfg();
    let mut sampler = Sampler::new(vec![zone(1)]);
    let mut lowest = 255.0f32;
    let mut highest = 0.0f32;
    for level in (50..=200).chain((50..200).rev()) {
        config.brightness = level as f32 / 1000.0;
        let mask = 1 + level % 6;
        let rgb = std::array::from_fn(|channel| if mask & (1 << channel) != 0 { 101 } else { 100 });
        let output = step(
            &mut sampler,
            &frame(rgb),
            &config,
            Duration::from_millis(100),
        );
        assert!(
            output.r == output.g && output.g == output.b,
            "level {level}, source {rgb:?} flashed chroma: {output:?}"
        );
        lowest = lowest.min(output.r);
        highest = highest.max(output.r);
    }
    assert!(
        lowest <= 6.0 && highest >= 19.0,
        "brightness did not track the ramp"
    );
}

#[test]
fn packet_quantization_preserves_resolvable_chroma_and_zero_components() {
    let mut config = cfg();
    let mut sampler = Sampler::new(vec![zone(1)]);
    for (rgb, brightness) in [
        ([110, 100, 110], 0.1),
        ([101, 100, 101], 1.0),
        ([255, 0, 0], 0.0412),
        ([0, 255, 0], 0.0412),
        ([0, 0, 255], 0.0412),
        ([255, 255, 0], 0.0412),
        ([0, 255, 255], 0.0412),
        ([255, 0, 255], 0.0412),
    ] {
        config.brightness = brightness;
        let output = step(
            &mut sampler,
            &frame(rgb),
            &config,
            Duration::from_millis(100),
        );
        let emitted = [output.r, output.g, output.b];
        assert!(
            emitted.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - emitted.iter().copied().fold(f32::INFINITY, f32::min)
                >= 1.0,
            "resolvable color was removed: {rgb:?} -> {output:?}"
        );
        for (source, byte) in rgb.into_iter().zip(emitted) {
            if source == 0 {
                assert_eq!(
                    byte, 0.0,
                    "zero component illuminated: {rgb:?} -> {output:?}"
                );
            }
            assert!((byte - source as f32 * brightness).abs() <= 1.25);
        }
    }
}

#[test]
fn packet_quantization_releases_warm_tint_for_neutral_target() {
    let mut config = cfg();
    config.brightness = 0.1;
    let mut sampler = Sampler::new(vec![zone(1)]);
    let warm = step(
        &mut sampler,
        &frame([110, 100, 110]),
        &config,
        Duration::from_millis(100),
    );
    assert_eq!(
        warm,
        Color {
            r: 11.0,
            g: 10.0,
            b: 11.0
        }
    );

    let neutral = step(
        &mut sampler,
        &frame([105, 105, 105]),
        &config,
        Duration::from_millis(100),
    );
    assert_eq!(
        sampler.raw()[0],
        Color {
            r: 10.5,
            g: 10.5,
            b: 10.5
        }
    );
    assert!(
        neutral.r == neutral.g && neutral.g == neutral.b,
        "neutral update retained channel tint: {neutral:?}"
    );
    assert!(
        (neutral.r == 10.0) || (neutral.r == 11.0),
        "unexpected neutral packet code: {neutral:?}"
    );
}

#[test]
fn packet_quantization_converges_neutral_targets_across_rgb_permutations() {
    let permutations = [
        ([110, 100, 110], [10, 10, 10], 0.049),
        ([100, 110, 100], [10, 10, 10], 0.05),
        ([110, 100, 110], [10, 10, 10], 0.051),
        ([240, 200, 220], [100, 100, 100], 0.1049),
        ([20, 80, 40], [100, 100, 100], 0.105),
        ([110, 100, 110], [100, 100, 100], 0.1051),
        ([240, 200, 220], [250, 250, 250], 0.049),
        ([20, 80, 40], [250, 250, 250], 0.05),
        ([255, 230, 245], [250, 250, 250], 0.051),
    ];
    for (index, (colored, neutral, brightness)) in permutations.into_iter().enumerate() {
        let mut config = cfg();
        config.brightness = brightness;
        config.smooth = 1.0;
        let mut sampler = Sampler::new(vec![zone(1)]);
        step(
            &mut sampler,
            &frame(colored),
            &config,
            Duration::from_millis(100),
        );
        let output = step(
            &mut sampler,
            &frame(neutral),
            &config,
            Duration::from_millis(100),
        );
        assert!(
            output.r == output.g && output.g == output.b,
            "permutation {index}, brightness {brightness}, target {neutral:?} retained tint: {output:?}"
        );
    }
    let mut config = cfg();
    let mut sampler = Sampler::new(vec![zone(1)]);
    config.brightness = 1.016;
    step(
        &mut sampler,
        &frame([251, 250, 251]),
        &config,
        Duration::from_millis(100),
    );
    config.brightness = 1.018;
    let near_white = step(
        &mut sampler,
        &frame([250, 250, 250]),
        &config,
        Duration::from_millis(100),
    );
    assert!(
        near_white.r == near_white.g && near_white.g == near_white.b,
        "near-white neutral target retained tint: {near_white:?}"
    );
}

#[test]
fn packet_quantization_settles_neutral_half_code_with_slow_smoothing() {
    for smooth in [0.05, 0.15] {
        let mut config = cfg();
        config.brightness = 0.1;
        config.smooth = smooth;
        let mut sampler = Sampler::new(vec![zone(1)]);
        for _ in 0..300 {
            step(
                &mut sampler,
                &frame([110, 100, 110]),
                &config,
                Duration::from_millis(100),
            );
        }
        let mut output = Color::default();
        for _ in 0..250 {
            output = step(
                &mut sampler,
                &frame([105, 105, 105]),
                &config,
                Duration::from_millis(100),
            );
        }
        assert!(
            output.r == output.g && output.g == output.b,
            "smooth {smooth} failed to converge neutral packet: {output:?}"
        );
    }
}

#[test]
fn packet_quantization_does_not_retain_reversed_channel_order() {
    let mut config = cfg();
    config.brightness = 0.1;
    let mut sampler = Sampler::new(vec![zone(1)]);
    step(
        &mut sampler,
        &frame([110, 100, 110]),
        &config,
        Duration::from_millis(100),
    );
    let output = step(
        &mut sampler,
        &frame([100, 110, 100]),
        &config,
        Duration::from_millis(100),
    );
    assert!(
        output.r < output.g && output.b < output.g,
        "reversed colored target was not reflected in output: {output:?}"
    );
    let mut endpoint_config = cfg();
    endpoint_config.brightness = 1.0;
    let endpoint = step(
        &mut sampler,
        &frame([0, 128, 255]),
        &endpoint_config,
        Duration::from_millis(100),
    );
    assert_eq!(endpoint.r, 0.0);
    assert_eq!(endpoint.b, 255.0);
}

#[test]
fn remap_changed_zone_identity_and_count_resets_temporal_output() {
    let mut config = cfg();
    config.black_threshold = 0.01;
    config.max_step = 12;
    config.smooth = 0.35;
    let warm = frame([160, 40, 20]);
    let black = frame([0, 0, 0]);
    let new_zones = vec![zone(9), zone(10)];
    let mut sampler = Sampler::new(vec![zone(7)]);
    for _ in 0..10 {
        step(&mut sampler, &warm, &config, Duration::from_millis(100));
    }
    step(&mut sampler, &black, &config, Duration::from_millis(100));
    let fading = step(&mut sampler, &black, &config, Duration::from_millis(100));
    assert!(fading.r > 0.0, "setup did not establish an active fade");
    sampler.remap(new_zones.clone());
    let new_frame = frame([20, 120, 220]);
    let mapped = sampler
        .sample(
            &new_frame,
            WIDTH * 4,
            WIDTH,
            HEIGHT,
            &config,
            Duration::from_millis(100),
        )
        .unwrap();
    let mut fresh = Sampler::new(new_zones);
    let expected = fresh
        .sample(
            &new_frame,
            WIDTH * 4,
            WIDTH,
            HEIGHT,
            &config,
            Duration::from_millis(100),
        )
        .unwrap();
    assert_eq!(mapped.len(), 2);
    assert_eq!(
        mapped, expected,
        "new mapping retained stale fade/packet state: {mapped:?} != {expected:?}"
    );
}
