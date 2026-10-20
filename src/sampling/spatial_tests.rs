use super::{spatial_weight_table, Color, Sampler, Zone};

fn zone(id: u16, x0: f32, x1: f32) -> Zone {
    Zone {
        id,
        edge: None,
        x0,
        x1,
        y0: 0.4,
        y1: 0.6,
    }
}

fn set_colors(sampler: &mut Sampler, colors: &[Color]) {
    sampler.previous.copy_from_slice(colors);
    sampler.write_output();
}

#[test]
fn spatial_weights_use_geometry_and_radius_falloff() {
    let zones = [zone(10, 0.0, 0.1), zone(11, 0.2, 0.3), zone(12, 0.6, 0.7)];
    let weights = spatial_weight_table(&zones);
    assert_eq!(weights[0].len(), 1);
    assert_eq!(weights[0][0].0, 1);
    let distance = 0.2_f32;
    assert!((weights[0][0].1 - (1.0 - distance / 0.35).powi(2)).abs() < 1e-6);
    assert!(weights[2].is_empty());
}

#[test]
fn blending_skips_black_receivers_and_never_feeds_back() {
    let zones = vec![zone(1, 0.0, 0.1), zone(2, 0.2, 0.3), zone(3, 0.4, 0.5)];
    let mut sampler = Sampler::new(zones);
    sampler.set_spatial_blend(true);
    let initial = [
        Color {
            r: 10.0,
            g: 20.0,
            b: 30.0,
        },
        Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        },
        Color {
            r: 90.0,
            g: 100.0,
            b: 110.0,
        },
    ];
    set_colors(&mut sampler, &initial);
    assert_eq!(sampler.out()[1], Color::default());
    // The middle receiver's result is based on immutable temporal source colors.
    sampler.previous[1] = Color {
        r: 30.0,
        g: 40.0,
        b: 50.0,
    };
    sampler.write_output();
    let stable = sampler.out().to_vec();
    sampler.write_output();
    assert_eq!(sampler.out(), stable.as_slice());
    assert_eq!(sampler.previous[0], initial[0]);
    assert_eq!(sampler.previous[2], initial[2]);
    // An exact-black source stays black even when adjacent zones are lit.
    sampler.previous[1] = Color::default();
    sampler.write_output();
    assert_eq!(sampler.out()[1], Color::default());
}

#[test]
fn disabling_blend_restores_unblended_packet_output_and_toggle_resets_anchor() {
    let mut sampler = Sampler::new(vec![zone(1, 0.0, 0.1), zone(2, 0.2, 0.3)]);
    let colors = [
        Color {
            r: 51.0,
            g: 80.0,
            b: 120.0,
        },
        Color {
            r: 200.0,
            g: 140.0,
            b: 70.0,
        },
    ];
    set_colors(&mut sampler, &colors);
    let unblended = sampler.out().to_vec();
    sampler.set_spatial_blend(true);
    set_colors(&mut sampler, &colors);
    assert_ne!(sampler.out(), unblended.as_slice());
    sampler.set_spatial_blend(false);
    assert!(sampler.temporal.iter().all(|state| state.packet == [0; 3]));
    set_colors(&mut sampler, &colors);
    assert_eq!(sampler.out(), unblended.as_slice());
}

#[test]
fn remap_rebuilds_geometry_for_changed_ids_and_keeps_cache_for_same_mapping() {
    let a = zone(1, 0.0, 0.1);
    let b = zone(2, 0.2, 0.3);
    let c = zone(3, 0.6, 0.7);
    let mut sampler = Sampler::new(vec![a.clone(), b.clone(), c.clone()]);
    let original = sampler.spatial_weights.clone();
    sampler.remap(vec![c.clone(), a.clone(), b.clone()]);
    assert_eq!(
        sampler.zones.iter().map(|z| z.id).collect::<Vec<_>>(),
        [3, 1, 2]
    );
    assert_eq!(
        sampler.spatial_weights,
        spatial_weight_table(&sampler.zones)
    );
    let remapped = sampler.spatial_weights.clone();
    sampler.remap(vec![c, a, b]);
    assert_eq!(sampler.spatial_weights, remapped);
    assert_ne!(original, remapped);
}

#[test]
fn black_neighbor_contributes_its_weight_and_darkens_lit_receiver() {
    let mut sampler = Sampler::new(vec![zone(1, 0.0, 0.1), zone(2, 0.2, 0.3)]);
    sampler.set_spatial_blend(true);
    set_colors(
        &mut sampler,
        &[
            Color {
                r: 100.0,
                g: 100.0,
                b: 100.0,
            },
            Color::default(),
        ],
    );
    assert_eq!(
        sampler.out()[0],
        Color {
            r: 80.0,
            g: 80.0,
            b: 80.0
        }
    );
}

#[test]
fn remap_resizes_storage_and_output_is_zone_order_independent() {
    let a = zone(1, 0.0, 0.1);
    let b = zone(2, 0.2, 0.3);
    let c = zone(3, 0.4, 0.5);
    let colors = [
        Color {
            r: 20.0,
            g: 30.0,
            b: 40.0,
        },
        Color {
            r: 90.0,
            g: 100.0,
            b: 110.0,
        },
        Color {
            r: 180.0,
            g: 160.0,
            b: 140.0,
        },
    ];
    let mut first = Sampler::new(vec![a.clone(), b.clone(), c.clone()]);
    first.set_spatial_blend(true);
    set_colors(&mut first, &colors);
    let expected = first.out().to_vec();
    let mut reordered = Sampler::new(vec![c.clone(), a.clone(), b.clone()]);
    reordered.set_spatial_blend(true);
    set_colors(&mut reordered, &[colors[2], colors[0], colors[1]]);
    assert_eq!(reordered.out(), &[expected[2], expected[0], expected[1]]);
    reordered.remap(vec![a.clone()]);
    assert_eq!(reordered.previous.len(), 1);
    assert_eq!(reordered.spatial_weights.len(), 1);
    reordered.remap(vec![c.clone(), a, b]);
    assert_eq!(reordered.previous.len(), 3);
    assert_eq!(reordered.spatial_weights.len(), 3);
}

#[test]
fn sampler_config_changes_adjacent_zone_packet_output_only_for_supported_modes() {
    use std::time::Duration;

    let zones = vec![zone(1, 0.0, 0.25), zone(2, 0.25, 0.5)];
    let mut frame = vec![0; 8 * 4];
    frame[0..4].copy_from_slice(&[200, 0, 0, 255]);
    frame[8..12].copy_from_slice(&[0, 0, 200, 255]);
    let enabled_config = crate::Config {
        mode: "4D".into(),
        spatial_blend: true,
        letterbox: false,
        brightness: 1.0,
        saturation_boost: 0.0,
        black_threshold: 0.0,
        smooth: 1.0,
        max_step: 255,
        ..crate::Config::default()
    };
    let mut enabled = Sampler::new(zones.clone());
    let blended = enabled
        .sample(
            &frame,
            32,
            8,
            1,
            &enabled_config,
            Duration::from_millis(100),
        )
        .unwrap()
        .to_vec();
    let two_d_config = crate::Config {
        mode: "2D".into(),
        ..enabled_config
    };
    let mut two_d = Sampler::new(zones);
    let unblended = two_d
        .sample(&frame, 32, 8, 1, &two_d_config, Duration::from_millis(100))
        .unwrap()
        .to_vec();
    assert_ne!(blended, unblended);
    assert_eq!(two_d.out(), unblended.as_slice());
}
