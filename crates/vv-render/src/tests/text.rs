use super::*;

#[test]
fn imported_wide_font_shapes_with_its_own_face() {
    register_font(
        include_bytes!("../../../vv-app/assets/fonts/capcut/tmpsequel100wide-75.ttf").to_vec(),
    );
    let params = TitleParams {
        font_family: "Sequel 100 Wide".into(),
        font_weight: 40,
        content: vv_core::Keyframed::constant("abcde ABCDE".into()),
        ..Default::default()
    };
    let (attrs, align) = text_attrs(&params);
    with_fonts(|fonts, _| {
        let mut buffer = Buffer::new(fonts, Metrics::new(100.0, 120.0));
        shape_block(fonts, &mut buffer, &params, &attrs, align);
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let face = fonts.db().face(glyph.font_id).unwrap();
                assert!(
                    face.families
                        .iter()
                        .any(|(family, _)| family == &params.font_family),
                    "fallback: {:?}",
                    face.families
                );
            }
        }
    });
}

#[test]
fn styled_underlays_follow_edits_to_the_title_content() {
    let mut title = TitleParams {
        content: vv_core::Keyframed::constant("MMMMMM".into()),
        size: 60.0,
        ..Default::default()
    };
    title.style.underlays.push(vv_core::TextUnderlay {
        color: Rgba::WHITE,
        offset: [4.0, -6.0],
        style: vv_core::TextStyle {
            strokes: vec![vv_core::TextStroke {
                width: 8.0,
                ..Default::default()
            }],
            ..Default::default()
        },
    });
    let before = render_title(&title, (640, 360), (640, 360)).masks();
    title.content.default = "I".into();
    let after = render_title(&title, (640, 360), (640, 360)).masks();
    assert_eq!(before.len(), after.len());
    for ((before, _), (after, _)) in before.iter().zip(&after) {
        assert!(after.mask.width < before.mask.width / 2);
    }
}

#[test]
fn prewarm_queue_deduplicates_animation_frames_and_bounds_pending_work() {
    let mut queue = PrewarmQueue::default();
    let title = TitleParams {
        anim_state: Some(vv_core::AnimState::default()),
        ..Default::default()
    };
    for frame in 0..100 {
        let mut animated = title.clone();
        animated.anim_state.as_mut().unwrap().t_loop = frame as f32;
        queue.push((animated, (2160, 3840), (2160, 3840)));
    }
    assert_eq!(queue.pending.len(), 1);
    queue.active = queue.pending.pop_front();
    queue.push((title.clone(), (2160, 3840), (2160, 3840)));
    assert!(queue.pending.is_empty());
    for width in 1..100 {
        queue.push((title.clone(), (2160, 3840), (width, 100)));
    }
    assert_eq!(queue.pending.len(), CACHE_LEN);
    assert_eq!(queue.pending.back().unwrap().2, (99, 100));
}

#[test]
fn background_prewarm_populates_the_caches_used_by_playback() {
    let size = (320, 180);
    let mut title = TitleParams {
        content: vv_core::Keyframed::constant("Background prewarm test".into()),
        ..Default::default()
    };
    for animated in [false, true] {
        if animated {
            title.anim.enter = vv_core::text_anim_presets::in_preset("fade");
            title = title.animated(0.0, 3.0);
        }
        prewarm(&title, size, size);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let ready = if animated {
                crate::text_anim_render::cache()
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(key, cell)| {
                        key.0.display_text() == title.display_text() && cell.get().is_some()
                    })
            } else {
                render_cache()
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(key, _)| key.0 == title)
            };
            if ready {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "prewarm worker did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if animated {
            let before = crate::text_anim_render::build(&title, size, size);
            let later = title.clone().animated(0.5, 3.0);
            let after = crate::text_anim_render::build(&later, size, size);
            assert!(Arc::ptr_eq(&before, &after));
        } else {
            let before = render_cache()
                .lock()
                .unwrap()
                .iter()
                .find(|(key, _)| key.0 == title)
                .unwrap()
                .1
                .clone();
            assert!(Arc::ptr_eq(&before, &render_title(&title, size, size)));
        }
    }
}

#[test]
fn animation_frames_do_not_evict_prepared_still_titles() {
    let mut title = TitleParams::default();
    title.anim.enter = vv_core::text_anim_presets::in_preset("fade");
    let still = render_title(&title, (320, 180), (320, 180));
    for frame in 0..CACHE_LEN * 2 {
        render_title(
            &title.clone().animated(frame as f32 / 100.0, 3.0),
            (320, 180),
            (320, 180),
        );
    }
    assert!(Arc::ptr_eq(
        &still,
        &render_title(&title, (320, 180), (320, 180))
    ));
}

fn covered_bounds(mask: &TextMask) -> Option<(u32, u32, u32, u32)> {
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..mask.height {
        for x in 0..mask.width {
            if mask.data[(y * mask.width + x) as usize] > 0 {
                let b = bounds.get_or_insert((x, y, x, y));
                b.0 = b.0.min(x);
                b.1 = b.1.min(y);
                b.2 = b.2.max(x);
                b.3 = b.3.max(y);
            }
        }
    }
    bounds
}

#[test]
fn default_title_is_drawn_around_the_center() {
    let render = dense_title(&TitleParams::default(), (640, 360), (640, 360));
    let (x0, y0, x1, y1) = covered_bounds(render.text()).expect("no pixel drawn");
    let (cx, cy) = ((x0 + x1) / 2, (y0 + y1) / 2);
    assert!((cx as i32 - 320).abs() < 20, "centro x {cx}");
    assert!((cy as i32 - 180).abs() < 30, "centro y {cy}");
}

#[test]
fn left_anchor_starts_the_text_at_the_position() {
    let params = TitleParams {
        anchor: (HAnchor::Left, VAnchor::Middle),
        ..Default::default()
    };
    let render = dense_title(&params, (640, 360), (640, 360));
    let (x0, ..) = covered_bounds(render.text()).unwrap();
    assert!((x0 as i32 - 320).abs() < 12, "start x {x0}");
}

#[test]
fn background_is_under_the_text_and_contains_it() {
    let params = TitleParams {
        background: vv_core::TitleBackground {
            enabled: true,
            outline_width: 2.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let render = dense_title(&params, (640, 360), (640, 360));
    assert_eq!(render.layers.len(), 3, "background, outline, text");
    let (tx0, ty0, tx1, ty1) = covered_bounds(render.text()).unwrap();
    let (bx0, by0, bx1, by1) = covered_bounds(&render.layers[0].0).unwrap();
    assert!(bx0 < tx0 && by0 < ty0 && bx1 > tx1 && by1 > ty1);
    // The border sits on the perimeter: at the center of the rectangle there is none.
    let ring = &render.layers[1].0;
    let (cx, cy) = ((bx0 + bx1) / 2, (by0 + by1) / 2);
    assert_eq!(ring.data[(cy * ring.width + cx) as usize], 0);
    assert!(ring.data[(cy * ring.width + bx0 + 1) as usize] > 0);
}

#[test]
fn output_smaller_than_timeline_scales_the_text() {
    let full = dense_title(&TitleParams::default(), (640, 360), (640, 360));
    let half = dense_title(&TitleParams::default(), (640, 360), (320, 180));
    let width = |m: &TextMask| {
        let (x0, _, x1, _) = covered_bounds(m).unwrap();
        (x1 - x0) as f32
    };
    let ratio = width(half.text()) / width(full.text());
    assert!((ratio - 0.5).abs() < 0.08, "ratio {ratio}");
}

mod styles {
    use super::*;
    use vv_core::{
        BackgroundShape, Gradient, GradientKind, Rgba, StrokeJoin, TextExtrude, TextFill, TextGlow,
        TextShadow, TextStroke, TitleBackground,
    };

    const SIZE: (u32, u32) = (640, 360);
    const RED: Rgba = Rgba {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    const BLUE: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    };

    fn base() -> TitleParams {
        TitleParams {
            content: vv_core::Keyframed::constant("HI".into()),
            font_weight: 700,
            size: 120.0,
            ..Default::default()
        }
    }

    /// Layers stacked on a black frame, as the compositor does.
    fn composite(render: &DenseRender) -> Vec<[f32; 3]> {
        let (w, h) = (render.layers[0].0.width, render.layers[0].0.height);
        let mut px = vec![[0.0f32; 3]; (w * h) as usize];
        for (mask, c) in &render.layers {
            for (p, &cov) in px.iter_mut().zip(&mask.data) {
                let a = cov as f32 / 255.0 * c.a;
                for (d, s) in p.iter_mut().zip([c.r, c.g, c.b]) {
                    *d += (s - *d) * a;
                }
            }
        }
        px
    }

    fn at(px: &[[f32; 3]], x: u32, y: u32) -> [f32; 3] {
        px[(y * SIZE.0 + x) as usize]
    }

    fn dump(name: &str, render: &DenseRender) {
        let Ok(dir) = std::env::var("VV_TEXT_SHEET_DIR") else {
            return;
        };
        let mut out = format!("P6\n{} {}\n255\n", SIZE.0, SIZE.1).into_bytes();
        let backdrop = [0.13f32; 3];
        let px = composite(render);
        let covered = {
            let mut c = vec![0.0f32; px.len()];
            for (mask, col) in &render.layers {
                for (d, &v) in c.iter_mut().zip(&mask.data) {
                    let a = v as f32 / 255.0 * col.a;
                    *d += (1.0 - *d) * a;
                }
            }
            c
        };
        for (p, a) in px.iter().zip(covered) {
            for (c, b) in p.iter().zip(backdrop) {
                out.push(((c + b * (1.0 - a)).clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
        std::fs::write(format!("{dir}/{name}.ppm"), out).unwrap();
    }

    fn render(name: &str, params: &TitleParams) -> DenseRender {
        let r = dense_title(params, SIZE, SIZE);
        dump(name, &r);
        r
    }

    #[test]
    fn plain_style_is_a_single_text_layer() {
        assert_eq!(render("plain", &base()).layers.len(), 1);
    }

    #[test]
    fn linear_gradient_goes_from_first_to_last_stop() {
        let mut p = base();
        p.style.fill = TextFill::Gradient(Gradient {
            angle: 0.0,
            ..Gradient::two_stops(GradientKind::Linear, RED, BLUE)
        });
        let r = render("gradient_linear", &p);
        let (x0, y0, x1, y1) = covered_bounds(r.text()).unwrap();
        let px = composite(&r);
        let mid_y = (y0 + y1) / 2;
        let left = (x0..x1).find(|&x| at(&px, x, mid_y) != [0.0; 3]).unwrap();
        let right = (x0..x1)
            .rev()
            .find(|&x| at(&px, x, mid_y) != [0.0; 3])
            .unwrap();
        let (l, r_) = (at(&px, left + 2, mid_y), at(&px, right - 2, mid_y));
        assert!(l[0] > 0.7 && l[2] < 0.3, "left {l:?}");
        assert!(r_[2] > 0.7 && r_[0] < 0.3, "right {r_:?}");
    }

    #[test]
    fn gradient_with_three_stops_hits_the_middle_one() {
        let green = Rgba {
            r: 0.0,
            g: 1.0,
            b: 0.0,
            a: 1.0,
        };
        let mut g = Gradient::two_stops(GradientKind::Linear, RED, BLUE);
        g.angle = 0.0;
        g.stops.insert(1, vullo(0.5, green));
        let mut p = base();
        p.size = 200.0;
        p.style.fill = TextFill::Gradient(g);
        let r = render("gradient_3_stops", &p);
        let px = composite(&r);
        let best = px.iter().map(|c| c[1]).fold(0.0, f32::max);
        assert!(best > 0.9, "green stop never reached: {best}");
    }

    fn vullo(pos: f32, color: Rgba) -> vv_core::GradientStop {
        vv_core::GradientStop { pos, color }
    }

    #[test]
    fn radial_gradient_changes_with_the_distance_from_the_center() {
        let mut p = base();
        p.size = 200.0;
        p.style.fill = TextFill::Gradient(Gradient::two_stops(GradientKind::Radial, RED, BLUE));
        let r = render("gradient_radial", &p);
        let (x0, y0, x1, y1) = covered_bounds(r.text()).unwrap();
        let px = composite(&r);
        let center = ((x0 + x1) / 2, (y0 + y1) / 2);
        let near = (x0..x1)
            .map(|x| (x, center.1))
            .filter(|&(x, y)| at(&px, x, y) != [0.0; 3])
            .min_by_key(|&(x, _)| (x as i32 - center.0 as i32).abs())
            .unwrap();
        let far = at(&px, x0 + 3, center.1);
        let nearc = at(&px, near.0, near.1);
        assert!(nearc[0] > far[0], "near {nearc:?} far {far:?}");
    }

    #[test]
    fn gradient_offset_scrolls_the_colors() {
        let mut p = base();
        let mut g = Gradient::two_stops(GradientKind::Linear, RED, BLUE);
        g.angle = 0.0;
        p.style.fill = TextFill::Gradient(g.clone());
        let a = composite(&dense_title(&p, SIZE, SIZE));
        g.offset = vv_core::Keyframed::constant(0.5);
        p.style.fill = TextFill::Gradient(g);
        let b = render("gradient_offset", &p);
        assert_ne!(a, composite(&b));
    }

    #[test]
    fn gradient_offset_follows_its_keyframes() {
        let mut g = Gradient::two_stops(GradientKind::Linear, RED, BLUE);
        let mut offset = vv_core::Keyframed::constant(0.0);
        offset.upsert(0, 0.0, vv_core::Interpolation::Linear);
        offset.upsert(10, 1.0, vv_core::Interpolation::Linear);
        g.offset = offset;
        let mut p = base();
        p.style.fill = TextFill::Gradient(g);
        let (TextFill::Gradient(a), TextFill::Gradient(b)) =
            (p.at(0).style.fill, p.at(5).style.fill)
        else {
            panic!("not a gradient");
        };
        assert_eq!(a.offset.default, 0.0);
        assert!((b.offset.default - 0.5).abs() < 1e-3);
    }

    #[test]
    fn strokes_stack_outwards_and_the_text_stays_on_top() {
        let mut p = base();
        p.style.strokes = vec![
            TextStroke {
                color: BLUE,
                width: 4.0,
                join: StrokeJoin::Round,
            },
            TextStroke {
                color: RED,
                width: 12.0,
                join: StrokeJoin::Round,
            },
        ];
        let r = render("strokes", &p);
        assert_eq!(r.layers.len(), 3);
        let widths: Vec<u32> = r
            .layers
            .iter()
            .map(|(m, _)| {
                let (x0, _, x1, _) = covered_bounds(m).unwrap();
                x1 - x0
            })
            .collect();
        assert!(widths[0] > widths[1] && widths[1] > widths[2], "{widths:?}");
        assert_eq!(r.layers[0].1, RED, "widest below");
    }

    #[test]
    fn miter_join_reaches_further_than_round_at_corners() {
        let area = |join| {
            let mut p = base();
            p.style.strokes = vec![TextStroke {
                color: RED,
                width: 14.0,
                join,
            }];
            let r = dense_title(&p, SIZE, SIZE);
            r.layers[0].0.data.iter().map(|&v| v as u64).sum::<u64>()
        };
        let (round, miter, bevel) = (
            area(StrokeJoin::Round),
            area(StrokeJoin::Miter),
            area(StrokeJoin::Bevel),
        );
        assert!(miter > round, "miter {miter} round {round}");
        assert!(bevel <= miter && bevel >= round * 9 / 10, "bevel {bevel}");
    }

    #[test]
    fn hard_shadow_is_a_displaced_copy_and_blur_softens_it() {
        let mut p = base();
        p.style.shadows = vec![TextShadow {
            offset: [20.0, -20.0],
            blur: 0.0,
            color: BLUE,
            opacity: 100.0,
        }];
        let hard = render("shadow_hard", &p);
        let (tx0, ty0, ..) = covered_bounds(hard.text()).unwrap();
        let (sx0, sy0, ..) = covered_bounds(&hard.layers[0].0).unwrap();
        assert_eq!((sx0 - tx0, sy0 - ty0), (20, 20));
        assert!(
            hard.layers[0]
                .0
                .data
                .iter()
                .all(|&v| v == 0 || v > 100 || v < 255)
        );
        p.style.shadows[0].blur = 16.0;
        let soft = render("shadow_soft", &p);
        let mids = |m: &TextMask| m.data.iter().filter(|&&v| v > 20 && v < 235).count();
        assert!(mids(&soft.layers[0].0) > mids(&hard.layers[0].0) * 2);
    }

    #[test]
    fn several_shadows_make_several_layers() {
        let mut p = base();
        p.style.shadows = vec![TextShadow::default(), TextShadow::default()];
        assert_eq!(render("shadow_double", &p).layers.len(), 3);
    }

    #[test]
    fn glow_extends_beyond_the_text() {
        let mut p = base();
        p.style.glows = vec![
            TextGlow {
                radius: 30.0,
                intensity: 1.5,
                color: BLUE,
            },
            TextGlow {
                radius: 12.0,
                intensity: 1.0,
                color: RED,
            },
        ];
        let r = render("glow", &p);
        assert_eq!(r.layers.len(), 3);
        let (tx0, _, tx1, _) = covered_bounds(r.text()).unwrap();
        let (gx0, _, gx1, _) = covered_bounds(&r.layers[0].0).unwrap();
        assert!(gx0 + 10 < tx0 && gx1 > tx1 + 10);
        let strong = |m: &TextMask| m.data.iter().map(|&v| v as u64).sum::<u64>();
        let more = {
            p.style.glows[0].intensity = 3.0;
            dense_title(&p, SIZE, SIZE)
        };
        assert!(strong(&more.layers[0].0) > strong(&r.layers[0].0));
    }

    #[test]
    fn extrusion_follows_the_angle() {
        let mut p = base();
        p.style.extrude = Some(TextExtrude {
            depth: 24.0,
            angle: -45.0,
            color: BLUE,
        });
        let r = render("extrude", &p);
        let (tx0, ty0, tx1, ty1) = covered_bounds(r.text()).unwrap();
        let (ex0, ey0, ex1, ey1) = covered_bounds(&r.layers[0].0).unwrap();
        assert_eq!((ex0, ey0), (tx0, ty0));
        assert!(
            ex1 >= tx1 + 15 && ey1 >= ty1 + 15,
            "{ex1} {ey1} vs {tx1} {ty1}"
        );
    }

    #[test]
    fn pill_background_is_fully_rounded() {
        let mut p = base();
        p.background = TitleBackground {
            enabled: true,
            shape: BackgroundShape::Pill,
            padding: 0.4,
            ..Default::default()
        };
        p.content = vv_core::Keyframed::constant("PILL SHAPE".into());
        p.size = 60.0;
        let r = render("pill", &p);
        let (x0, y0, x1, y1) = covered_bounds(&r.layers[0].0).unwrap();
        let m = &r.layers[0].0;
        assert_eq!(m.data[(y0 * m.width + x0) as usize], 0, "corner is cut");
        assert!(y1 - y0 < x1 - x0);
    }

    #[test]
    fn background_gradient_and_rotation() {
        let mut p = base();
        p.background = TitleBackground {
            enabled: true,
            gradient: Some(Gradient::two_stops(GradientKind::Linear, RED, BLUE)),
            rotation: 15.0,
            outline_width: 3.0,
            ..Default::default()
        };
        let r = render("background_gradient_rotated", &p);
        assert_eq!(r.layers.len(), 4, "two gradient layers, ring, text");
        let flat = TitleBackground {
            rotation: 0.0,
            ..p.background.clone()
        };
        let (rx0, ry0, rx1, ry1) = covered_bounds(&r.layers[0].0).unwrap();
        p.background = flat;
        let r0 = dense_title(&p, SIZE, SIZE);
        let (fx0, fy0, fx1, fy1) = covered_bounds(&r0.layers[0].0).unwrap();
        assert!(ry1 - ry0 > fy1 - fy0 && rx1 - rx0 > fx1 - fx0 - 1);
    }

    #[test]
    fn everything_together_renders() {
        let mut p = base();
        p.style.fill = TextFill::Gradient(Gradient::two_stops(GradientKind::Linear, RED, BLUE));
        p.style.strokes = vec![TextStroke::default()];
        p.style.shadows = vec![TextShadow::default()];
        p.style.glows = vec![TextGlow::default()];
        p.style.extrude = Some(TextExtrude::default());
        p.background.enabled = true;
        p.background.shape = BackgroundShape::Pill;
        let r = render("combo", &p);
        assert_eq!(r.layers.len(), 7);
    }

    #[test]
    fn bundled_fonts_register_and_change_the_drawing() {
        let anton = include_bytes!("../../../vv-app/assets/fonts/Anton-Regular.ttf").to_vec();
        register_font(anton);
        assert!(font_families().iter().any(|f| f == "Anton"));
        let mut p = base();
        p.font_family = "Anton".into();
        p.font_weight = 400;
        let with_anton = render("font_anton", &p);
        let plain = dense_title(
            &TitleParams {
                font_weight: 400,
                ..base()
            },
            SIZE,
            SIZE,
        );
        assert_ne!(with_anton.text().data, plain.text().data);
    }

    #[test]
    fn variable_font_weight_changes_the_width() {
        register_font(include_bytes!("../../../vv-app/assets/fonts/Montserrat.ttf").to_vec());
        let width = |weight| {
            let p = TitleParams {
                font_family: "Montserrat".into(),
                font_weight: weight,
                ..base()
            };
            let r = dense_title(&p, SIZE, SIZE);
            let (x0, _, x1, _) = covered_bounds(r.text()).unwrap();
            x1 - x0
        };
        assert!(width(900) > width(300), "{} vs {}", width(900), width(300));
    }
}

#[test]
fn the_bubble_shapes_reach_past_their_box_where_they_should() {
    let half = (100.0, 50.0);
    for shape in [
        BackgroundShape::RoundedRect,
        BackgroundShape::Ellipse,
        BackgroundShape::Speech,
        BackgroundShape::Thought,
        BackgroundShape::Burst,
    ] {
        assert!(
            shape_distance(shape, (0.0, 0.0), half, 10.0) < 0.0,
            "{shape:?} centre"
        );
        assert!(
            shape_distance(shape, (0.0, -200.0), half, 10.0) > 0.0,
            "{shape:?} far above"
        );
    }
    // The tail and the bubbles hang below the box, on the left.
    let below_left = (-60.0, 70.0);
    assert!(shape_distance(BackgroundShape::Speech, below_left, half, 10.0) < 0.0);
    assert!(shape_distance(BackgroundShape::RoundedRect, below_left, half, 10.0) > 0.0);
    let bubble = (-55.0, 65.0);
    assert!(shape_distance(BackgroundShape::Thought, bubble, half, 10.0) < 0.0);
    // The burst's spikes reach out where the ellipse does not.
    let spiky = (0..360)
        .map(|deg| {
            let a = (deg as f32).to_radians();
            (115.0 * a.cos(), 57.5 * a.sin())
        })
        .any(|p| shape_distance(BackgroundShape::Burst, p, half, 10.0) < 0.0);
    assert!(spiky);
}

#[test]
fn a_triangle_is_negative_inside_and_positive_outside() {
    let (a, b, c) = ((0.0, 0.0), (10.0, 0.0), (0.0, 10.0));
    assert!(triangle_distance((2.0, 2.0), a, b, c) < 0.0);
    assert!((triangle_distance((-3.0, 0.0), a, b, c) - 3.0).abs() < 1e-4);
}
