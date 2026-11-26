//! Creating, opening and nesting timelines.

use super::*;

impl VenturiApp {
    pub(crate) fn ensure_timeline(&mut self) -> TimelineId {
        self.ensure_timeline_with(vv_core::Rational::new(25, 1), (1920, 1080), true, None)
    }

    /// Like `ensure_timeline`, but a new timeline takes its format from the
    /// first media (see `first_media`; media with video only).
    pub(crate) fn ensure_timeline_for(
        &mut self,
        meta: &vv_core::MediaMeta,
        rotation: u16,
        import: Option<vv_session::JobId>,
    ) -> TimelineId {
        let format = first_media::project_format(meta, rotation);
        if let Some(id) = self.timeline_id
            && !self.has_video_media(id)
        {
            self.set_project_fps(format.fps);
            self.set_project_resolution(format.resolution);
        }
        self.ensure_timeline_with(format.fps, format.resolution, true, import)
    }

    /// Whether the format still waits for a first video: titles and audio
    /// alone do not set it.
    pub(crate) fn has_video_media(&self, timeline_id: TimelineId) -> bool {
        let project = &self.session.project;
        project.timelines[timeline_id]
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .any(|clip| match clip.source {
                vv_core::ClipSource::Media(id) => {
                    project.media_pool.get(id).is_some_and(|m| m.meta.has_video)
                }
                _ => false,
            })
    }

    /// Clockwise turn recorded in the media's file, probed once per path.
    pub(crate) fn media_rotation(&mut self, media_id: MediaId) -> u16 {
        let Some(item) = self.session.project.media_pool.get(media_id) else {
            return 0;
        };
        if item.compound.is_some() || !item.meta.has_video || item.meta.is_image() {
            return 0;
        }
        let path = item.path.clone();
        *self
            .media_rotation
            .entry(path.clone())
            .or_insert_with(|| first_media::probe_rotation(&path))
    }

    /// Turns a clip upright when its file carries a rotation the engine
    /// does not apply, through the clip's own transform.
    pub(crate) fn upright_clip(
        &mut self,
        timeline_id: TimelineId,
        clip: (usize, ClipId),
        media_id: MediaId,
        meta: &vv_core::MediaMeta,
    ) {
        let rotation = self.media_rotation(media_id);
        if rotation == 0 {
            return;
        }
        let canvas = self.session.project.timelines[timeline_id].resolution;
        let (turn, zoom) = first_media::rotation_fit((meta.width, meta.height), canvas, rotation);
        for (param, value) in [
            (vv_core::TransformParam::Rotation, turn),
            (vv_core::TransformParam::ZoomX, zoom),
            (vv_core::TransformParam::ZoomY, zoom),
        ] {
            self.session
                .apply(Box::new(vv_core::set_clip_transform_param(
                    timeline_id,
                    clip.0,
                    clip.1,
                    param,
                    value,
                )));
        }
    }

    /// Like `ensure_timeline`, but without a video track: a drop of audio only must
    /// not create an empty one.
    pub(crate) fn ensure_timeline_audio_only(
        &mut self,
        import: Option<vv_session::JobId>,
    ) -> TimelineId {
        self.ensure_timeline_with(vv_core::Rational::new(25, 1), (1920, 1080), false, import)
    }

    /// A new timeline is part of the undo step of `import`, if given, else
    /// a step of its own (or of the caller's group).
    pub(crate) fn ensure_timeline_with(
        &mut self,
        fps: vv_core::Rational,
        resolution: (u32, u32),
        include_video_track: bool,
        import: Option<vv_session::JobId>,
    ) -> TimelineId {
        if let Some(id) = self.timeline_id {
            return id;
        }
        let mut tracks = Vec::new();
        if include_video_track {
            tracks.push(Track::new_main());
        }
        tracks.push(Track::new(TrackKind::Audio));
        let timeline = vv_core::Timeline {
            name: "Timeline 1".into(),
            fps,
            resolution,
            tracks,
            markers: Vec::new(),
            master: Default::default(),
        };
        let mut add = vv_core::AddEntities::new(vv_core::CommandLabel::NewTimeline);
        let (id, _) = add.timeline(&mut self.session.project, timeline, None);
        match import {
            Some(job) => self.session.join_import(job, Box::new(add)),
            None => self.session.apply(Box::new(add)),
        }
        self.timeline_id = Some(id);
        self.spawn_render_ahead_if_needed(id);
        id
    }

    /// New timeline of the project: like the initial one it also shows up in
    /// the media pool as a compound clip (see `ensure_timeline_with`).
    pub(crate) fn create_timeline(
        &mut self,
        name: String,
        fps: vv_core::Rational,
        resolution: (u32, u32),
    ) -> TimelineId {
        self.session.create_timeline(vv_core::Timeline {
            name,
            fps,
            resolution,
            tracks: vec![Track::new_main(), Track::new(TrackKind::Audio)],
            markers: Vec::new(),
            master: Default::default(),
        })
    }

    /// Opens a timeline as the top level one: the breadcrumb of the compound
    /// clips one was inside no longer applies.
    pub(crate) fn open_timeline(&mut self, timeline_id: TimelineId) {
        if self.timeline_id == Some(timeline_id) {
            return;
        }
        self.store_view();
        self.timeline_stack.clear();
        self.entered_from.clear();
        self.switch_to_timeline(timeline_id);
        self.restore_playhead();
    }

    /// Records the active timeline and its playhead in `project.view`; from
    /// inside compound clips, the root of the breadcrumb.
    pub(crate) fn store_view(&mut self) {
        let Some(current) = self.timeline_id else {
            return;
        };
        let (timeline, playhead) = match self.timeline_stack.first() {
            Some(&root) => (root, self.playhead_at_stack_level(0)),
            None => (current, self.timeline_state.playhead),
        };
        let project = &mut self.session.project;
        project.view.active_timeline = Some(timeline);
        project.view.playheads.insert(timeline, playhead);
        project
            .view
            .playheads
            .retain(|id, _| project.timelines.contains_key(*id));
    }

    /// Puts the playhead of the active timeline back where `project.view`
    /// left it.
    pub(crate) fn restore_playhead(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let Some(&frame) = self.session.project.view.playheads.get(&timeline_id) else {
            return;
        };
        let end = self.session.project.timelines[timeline_id].total_frames();
        self.timeline_state.playhead = frame.clamp(0, (end - 1).max(0));
        self.timeline_state.reveal_playhead = true;
    }

    /// Spawns `render_ahead` the first time a timeline exists (one
    /// per session: it is not recreated afterwards, only updated via
    /// `sync_render_ahead`/`RenderAhead::update_project`).
    pub(crate) fn spawn_render_ahead_if_needed(&mut self, timeline_id: TimelineId) {
        if self.render_ahead.is_none() {
            self.render_ahead = Some(render_ahead::RenderAhead::spawn(
                self.session.project.clone(),
                timeline_id,
                self.settings.cache_budget_bytes,
                self.settings.proxy(),
                self.settings.lookahead_secs,
                self.settings.behind_secs,
            ));
            self.apply_preview_resolution();
            self.render_ahead_generation = self.session.history.generation();
        }
    }

    /// Opens `nested_id` as if it were the project timeline, with the playhead
    /// on its frame `at`: from the double click on compound clip `clip`
    /// (`timeline_ui::show_timeline`). The whole
    /// existing editing UI stays unchanged (it is already parameterized on
    /// `self.timeline_id`), here only the "which" has to be moved and
    /// where one came from stacked for the breadcrumb.
    pub(crate) fn enter_compound_timeline(
        &mut self,
        nested_id: TimelineId,
        at: FrameIdx,
        clip: Option<ClipId>,
    ) {
        let Some(current) = self.timeline_id else {
            return;
        };
        if current == nested_id || self.timeline_stack.contains(&nested_id) {
            // Already on this level, or already an ancestor on the stack: a
            // residual cycle (see `vv_core::MAX_COMPOUND_DEPTH`) must
            // not make the stack grow indefinitely.
            return;
        }
        self.timeline_stack.push(current);
        self.entered_from.push((clip, self.timeline_state.playhead));
        self.switch_to_timeline(nested_id);
        let end = self.session.project.timelines[nested_id].total_frames();
        self.timeline_state.playhead = at.clamp(0, (end - 1).max(0));
        self.timeline_state.reveal_playhead = true;
    }

    /// Goes back to the timeline at position `index` of `timeline_stack` (0 =
    /// the root): a clicked breadcrumb segment. The playhead lands where the
    /// one inside the compound clips is, level by level.
    pub(crate) fn exit_to_timeline_stack_index(&mut self, index: usize) {
        let Some(target) = self.timeline_stack.get(index).copied() else {
            return;
        };
        let frame = self.playhead_at_stack_level(index);
        self.timeline_stack.truncate(index);
        self.entered_from.truncate(index);
        self.switch_to_timeline(target);
        self.timeline_state.playhead = frame;
        self.timeline_state.reveal_playhead = true;
    }

    /// The playhead brought out of the compound clips to the timeline at
    /// position `index` of `timeline_stack`.
    pub(crate) fn playhead_at_stack_level(&self, index: usize) -> FrameIdx {
        let mut frame = self.timeline_state.playhead;
        for level in (index..self.timeline_stack.len()).rev() {
            let (clip_id, entry_playhead) = self.entered_from[level];
            let parent = &self.session.project.timelines[self.timeline_stack[level]];
            let clip = clip_id.and_then(|id| {
                parent
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .find(|c| c.id == id)
            });
            frame = match clip {
                // A freeze frame shows one frame of it for its whole length.
                Some(clip) if clip.freeze.is_none() => clip
                    .timeline_frame_at(frame)
                    .clamp(clip.timeline_start, clip.timeline_end() - 1),
                _ => entry_playhead,
            };
        }
        frame
    }

    /// The part common to the two functions above: it clears the state
    /// belonging to the level being left (selection, playhead, active clip) and
    /// immediately wakes `render_ahead` on the new timeline, ignoring the
    /// generation gate of `sync_render_ahead` (here the timeline itself
    /// changes, not its content — `sync_render_ahead` would not
    /// notice on its own). The clipboard survives: copying from one
    /// timeline/compound clip and pasting into another must work
    /// (`paste_clipboard_at_playhead` already conforms for different fps).
    pub(crate) fn switch_to_timeline(&mut self, timeline_id: TimelineId) {
        self.stop_voiceover();
        self.timeline_id = Some(timeline_id);
        let clipboard = std::mem::take(&mut self.timeline_state.clipboard);
        self.timeline_state = timeline_ui::TimelineState::default();
        self.timeline_state.clipboard = clipboard;
        self.active_clip = None;
        self.media_pool_state.invalidate_offline();
        if let Some(render_ahead) = &self.render_ahead {
            render_ahead.update_project(&self.session.project, timeline_id);
            self.render_ahead_generation = self.session.history.generation();
        }
    }

    /// Name to show for `id` in the breadcrumb above the timeline: that
    /// of its entry in the media pool (see `MediaItem::compound`), which is
    /// what the user sees elsewhere (e.g. "Compound Clip 2").
    pub(crate) fn timeline_display_name(&self, id: TimelineId) -> String {
        self.session
            .project
            .media_pool
            .values()
            .find(|m| m.compound == Some(id))
            .map(|m| file_label(&m.path))
            .unwrap_or_else(|| self.session.project.timelines[id].name.clone())
    }
}
