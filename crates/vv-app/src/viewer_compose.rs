//! What the viewer composes: isolated clips, scopes, layers.

use super::*;

impl VenturiApp {
    /// Whether the auto balance can measure `targets`: a single clip, under
    /// the playhead, since it is composed there.
    pub(crate) fn can_auto_balance(&self, targets: &[PanelTarget]) -> bool {
        matches!(targets, [t] if self.isolation_at_playhead(t).is_some())
    }

    /// The isolation composing `target` by itself, if the playhead is on it.
    pub(crate) fn isolation_at_playhead(&self, target: &PanelTarget) -> Option<Isolation> {
        let clip = self.session.project.timelines[target.timeline]
            .clip(target.track_index, target.clip_id)?;
        clip.contains(self.timeline_state.playhead)
            .then_some(Isolation {
                track_index: target.track_index,
                keep_below: clip.is_adjustment(),
            })
    }

    /// Adds to `section` the wheel moves balancing `target` at the playhead,
    /// composed apart from what covers it (see `Isolation`); nothing while
    /// its frames are not decoded.
    pub(crate) fn balance_isolated(
        &mut self,
        section: &mut grade_panel::GradeSectionResponse,
        grade: &vv_core::GradeValue,
        target: PanelTarget,
    ) {
        if let Some(pixels) = self.isolated_pixels(target) {
            grade_panel::balance_edits(section, grade, &pixels);
        }
    }

    /// RGBA of `target` at the playhead, composed apart from what covers it.
    pub(crate) fn isolated_pixels(&mut self, target: PanelTarget) -> Option<Vec<u8>> {
        let isolation = self.isolation_at_playhead(&target)?;
        let layers = self.timeline_video_layers(Some(isolation))?;
        let output = self.viewer_output(&layers);
        let render_layers: Vec<vv_render::Layer> = layers
            .iter()
            .map(frame_provider::OwnedLayer::as_render)
            .collect();
        let texture = self
            .compositor
            .render_layers_to_texture(&render_layers, output);
        Some(
            self.compositor
                .read_rgba_texture(&texture, texture.width(), texture.height()),
        )
    }

    /// The frame the viewer composes `layers` into: at the resolution of the
    /// largest decoded frame, widened to the aspect of the timeline so the
    /// bars show without upscaling, and capped by the preview resolution. With SolidColor clips only, at the
    /// timeline's resolution; empty, a tiny black frame of its aspect.
    pub(crate) fn viewer_output(
        &self,
        layers: &[frame_provider::OwnedLayer],
    ) -> vv_render::OutputFrame {
        let video_size = layers
            .iter()
            .filter_map(|l| match &l.content {
                frame_provider::OwnedContent::Video { frame, .. } => {
                    Some((frame.width, frame.height))
                }
                _ => None,
            })
            .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)));
        let timeline_size = self
            .timeline_id
            .map(|id| self.session.project.timelines[id].resolution);
        match video_size.or(timeline_size.filter(|_| !layers.is_empty())) {
            Some(size) => {
                let timeline_size = timeline_size.unwrap_or(size);
                let (out_w, out_h) = vv_render::fit_output_size(size, timeline_size);
                vv_render::OutputFrame::scaled(out_w, out_h, timeline_size)
                    .reduced(self.settings.preview_resolution.divisor())
            }
            None => {
                let (w, h) = timeline_size.unwrap_or((16, 9));
                let step = (w.max(h) / 64).max(1);
                vv_render::OutputFrame::scaled((w / step).max(1), (h / step).max(1), (w, h))
            }
        }
    }

    /// Measures the viewer's frame for the Color window's scope slots, shown
    /// on the next frame. Only what changed (frame, scope or size) is redrawn.
    pub(crate) fn update_scopes(&mut self, ctx: &egui::Context) {
        if !self.color_editor_shown() {
            self.scope_frame = None;
            return;
        }
        let source = self.scope_source();
        self.scope_view.has_source = source.is_some();
        let (Some(render_state), Some(source)) = (self.egui_render_state.clone(), source) else {
            return;
        };
        let scopes = self.scopes.get_or_insert_with(|| {
            vv_render::Scopes::new(
                std::sync::Arc::new(render_state.device.clone()),
                std::sync::Arc::new(render_state.queue.clone()),
            )
        });
        for slot in 0..2 {
            let size = self.scope_view.sizes[slot];
            if size.0 == 0 || size.1 == 0 {
                continue;
            }
            let kind = self.settings.panels.color_scopes[slot];
            let drawn = Some((kind, size, self.scope_generation));
            if self.scope_view.drawn[slot] == drawn {
                continue;
            }
            let texture = scopes.render(&source, kind, slot, size);
            // `Scopes` redraws a slot's texture in place while its size holds.
            let registered = &mut self.scope_view.textures[slot];
            if registered.as_ref().is_none_or(|(_, t)| *t != texture) {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut renderer = render_state.renderer.write();
                let id = match registered {
                    Some((id, _)) => {
                        renderer.update_egui_texture_from_wgpu_texture(
                            &render_state.device,
                            &view,
                            wgpu::FilterMode::Linear,
                            *id,
                        );
                        *id
                    }
                    None => renderer.register_native_texture(
                        &render_state.device,
                        &view,
                        wgpu::FilterMode::Linear,
                    ),
                };
                *registered = Some((id, texture));
            }
            self.scope_view.drawn[slot] = drawn;
            ctx.request_repaint();
        }
    }

    /// What the scopes measure: `scope_target` composed by itself at the
    /// playhead, recomposed only when its stack changes. While its frames are
    /// not decoded, the last composition.
    pub(crate) fn scope_source(&mut self) -> Option<wgpu::Texture> {
        let Some(isolation) = self
            .scope_target
            .and_then(|t| self.isolation_at_playhead(&t))
        else {
            self.scope_frame = None;
            return None;
        };
        let Some(layers) = self.timeline_video_layers(Some(isolation)) else {
            return self.scope_frame.as_ref().map(|(.., t)| (**t).clone());
        };
        let output = self.viewer_output(&layers);
        if let Some((shown, shown_output, texture)) = &self.scope_frame
            && *shown_output == output
            && frame_provider::renders_same(shown, &layers)
        {
            return Some((**texture).clone());
        }
        let render_layers: Vec<vv_render::Layer> = layers
            .iter()
            .map(frame_provider::OwnedLayer::as_render)
            .collect();
        // Owned: a pooled output would be drawn over by the viewer's next
        // frame of the same size.
        let texture = self
            .compositor
            .render_layers_to_owned_texture_transparent(&render_layers, output);
        drop(render_layers);
        let source = (*texture).clone();
        self.scope_frame = Some((layers, output, texture));
        self.scope_generation += 1;
        Some(source)
    }

    /// Undo, redo and opening a project change it too: checked before each
    /// composition rather than where it is set.
    pub(crate) fn sync_compositor_precision(&mut self) {
        let precision = self.session.project.precision;
        if self.compositor.precision() != precision {
            self.compositor.set_precision(precision);
            self.viewer_content = None;
            self.scope_frame = None;
        }
    }

    /// Composes `layers` and shows the resulting texture in the viewer, without
    /// readback. It does nothing without a shared device (tests).
    pub(crate) fn show_composited(
        &mut self,
        layers: Vec<frame_provider::OwnedLayer>,
        output: vv_render::OutputFrame,
    ) {
        self.sync_compositor_precision();
        let unchanged = self.video_texture_id.is_some()
            && self
                .viewer_content
                .as_ref()
                .is_some_and(|(shown, shown_output)| {
                    *shown_output == output && frame_provider::renders_same(shown, &layers)
                });
        if unchanged {
            self.last_viewer_frame_kind = Some(ViewerFrameKind::Video);
            return;
        }
        let Some(render_state) = self.egui_render_state.clone() else {
            return;
        };
        let render_layers: Vec<vv_render::Layer> = layers
            .iter()
            .map(frame_provider::OwnedLayer::as_render)
            .collect();
        let texture = self
            .compositor
            .render_layers_to_texture(&render_layers, output);
        drop(render_layers);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = render_state.renderer.write();
        match self.video_texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &render_state.device,
                &view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.video_texture_id = Some(renderer.register_native_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        drop(renderer);
        let (width, height) = output.timeline_size;
        self.video_display_size = Some(egui::vec2(width as f32, height as f32));
        self.last_viewer_frame_kind = Some(ViewerFrameKind::Video);
        // Not kept with a compound texture: never reusable, and holding it
        // would keep it out of the scratch pool.
        let reusable = !layers
            .iter()
            .any(|l| matches!(l.content, frame_provider::OwnedContent::Texture { .. }));
        self.viewer_content = reusable.then_some((layers, output));
    }

    /// The layers to compose at the playhead, from bottom to top. `None` if a
    /// media frame is not ready: the frame already shown is kept, composing
    /// without that layer would flash black where the clip should be. Once the
    /// buffer says it is caught up the missing frame will never arrive (media
    /// that does not decode): the layer is dropped instead of freezing the
    /// preview. During a crossing transition both halves must be ready.
    /// With `isolation`, only the tracks it shows.
    pub(crate) fn timeline_video_layers(
        &mut self,
        isolation: Option<Isolation>,
    ) -> Option<Vec<frame_provider::OwnedLayer>> {
        self.sync_compositor_precision();
        let timeline = &self.session.project.timelines[self.timeline_id?];
        // With keyframes standing in, a clip with none inside it has nothing
        // to show yet: the frame shown stays rather than going black.
        let still_filling = self
            .render_ahead
            .as_ref()
            .is_some_and(|r| !r.is_caught_up() || r.is_standing_in());
        let render_ahead = self.render_ahead.as_mut()?;
        // Left over by another composition: the caller reads it for this one.
        render_ahead.take_served_keyframe();
        let playhead = self.timeline_state.playhead;
        let clips = timeline.active_video_clips_at(playhead);
        let mut groups = Vec::with_capacity(clips.len());
        for &(track_index, clip) in &clips {
            if isolation.is_some_and(|i| !i.shows(track_index)) {
                continue;
            }
            let frame = playhead.max(clip.timeline_start);
            let involved = match timeline.tracks[track_index].crossing_at(frame) {
                Some((left, right, crossing)) if left.id == clip.id || right.id == clip.id => {
                    // A Smooth Cut or asset transition merges both sides into one layer.
                    if crossing.transition.merges_sides() {
                        1
                    } else {
                        2
                    }
                }
                _ => 1,
            };
            let mut provider = frame_provider::GpuCompounds::new(render_ahead, &self.compositor)
                .with_preview_divisor(self.settings.preview_resolution.divisor());
            let track_layers = frame_provider::track_layers_at(
                &self.session.project,
                timeline,
                track_index,
                clip,
                frame,
                timeline.resolution,
                &mut provider,
            )
            .ok()?;
            if track_layers.len() < involved {
                if std::env::var("VV_DEBUG_RENDER_AHEAD").is_ok()
                    && let Some((media_id, source_frame)) =
                        frame_provider::media_source_frame(clip, frame)
                {
                    eprintln!(
                        "[viewer] MISSING-LAYER playhead={playhead} track={track_index} media={media_id:?} source_frame={source_frame} caught_up={} cached_ranges={:?}",
                        !still_filling,
                        render_ahead.cached_ranges_for(media_id),
                    );
                }
                if still_filling {
                    return None;
                }
            }
            groups.push((clip.effects.matte, track_layers));
        }
        let mut provider = frame_provider::GpuCompounds::new(render_ahead, &self.compositor)
            .with_preview_divisor(self.settings.preview_resolution.divisor());
        Some(frame_provider::stack_layers(
            groups,
            &mut provider,
            timeline.resolution,
        ))
    }

    #[cfg(test)]
    pub(crate) fn active_clip_effects(&self) -> Option<&vv_core::EffectStack> {
        let (track_index, clip_id) = self.active_clip?;
        let timeline_id = self.timeline_id?;
        self.session.project.timelines[timeline_id]
            .clip(track_index, clip_id)
            .map(|c| &c.effects)
    }

    /// The viewer's split view during a slip: `None` when no slip is in
    /// progress, `Some(None)` while its frames decode (the frame already shown
    /// stays).
    pub(crate) fn slip_split_layers(&mut self) -> Option<Option<Vec<frame_provider::OwnedLayer>>> {
        let Some(preview) = self.timeline_state.slip_preview() else {
            self.slip_viewer = None;
            return None;
        };
        let item = self.session.project.media_pool.get(preview.media_id)?;
        // A compound clip renders through its nested timeline, which the
        // single-media buffer does not have.
        if item.compound.is_some() || !item.meta.has_video {
            return None;
        }
        let source_size = (item.meta.width, item.meta.height);
        let timeline_size = self.session.project.timelines[self.timeline_id?].resolution;
        if self
            .slip_viewer
            .as_ref()
            .is_none_or(|v| v.media_id() != preview.media_id)
        {
            // Only the frame under each end matters: a small slice of the budget.
            let spawn = || {
                self.spawn_media_render_ahead(
                    self.session.project.media_pool[preview.media_id].clone(),
                    self.settings.cache_budget_bytes / 8,
                    0.0,
                    0.0,
                )
            };
            self.slip_viewer = Some(slip_viewer::SlipViewer::new(
                preview.media_id,
                [spawn(), spawn()],
            ));
        }
        let frames = self.slip_viewer.as_mut()?.frames(preview);
        Some(frames.map(|f| slip_viewer::split_layers(f, source_size, timeline_size)))
    }

    /// The frame of the media pool preview at its playhead, if already
    /// decoded.
    pub(crate) fn browsing_video_frame(&mut self) -> Option<std::sync::Arc<vv_media::FrameYuv420>> {
        let (render_ahead, media_id) = self.browsing_render_ahead.as_ref()?;
        render_ahead.set_target(self.browse_playhead);
        render_ahead.get_frame(*media_id, self.browse_playhead)
    }
}
