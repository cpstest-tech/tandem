//! The source viewer: previewing a media of the pool or the library.

use super::*;

impl VenturiApp {
    pub(crate) fn preview_media(&mut self, media_id: MediaId) {
        self.preview(&Browsed::Media(media_id));
    }

    /// The pool item or the library asset's media.
    pub(crate) fn browsed_item(&self, browsed: &Browsed) -> Option<&vv_core::MediaItem> {
        match browsed {
            Browsed::Media(id) => self.session.project.media_pool.get(*id),
            Browsed::Asset(id) => self.library.assets.iter().find(|a| a.id == *id)?.item(),
        }
    }

    pub(crate) fn browsing_item(&self) -> Option<&vv_core::MediaItem> {
        self.browsed_item(self.browsing.as_ref()?)
    }

    /// "Raw" preview of a media, not tied to the timeline: it shows the
    /// first frame from a dedicated decode-ahead.
    pub(crate) fn preview(&mut self, browsed: &Browsed) {
        self.browsing_render_ahead = None;
        let Some(item) = self.browsed_item(browsed).cloned() else {
            return;
        };
        self.stop_voiceover();
        let (path, meta) = (item.path.clone(), item.meta.clone());
        if self.is_timeline_playing() {
            self.timeline_audio().pause();
        }
        self.reset_playback_speed_to_normal();
        self.browse_audio_streams = meta.audio_stream_count();
        let has_video = meta.has_video;
        self.preview_meta = Some(meta);
        self.preview_error = None;
        self.browse_playhead = 0;
        self.browse_marks = transport::MarkRange::default();
        if !has_video {
            // A library asset may have none yet; one already on disk is skipped.
            if item.meta.has_audio && item.compound.is_none() {
                let secs = item.meta.duration_frames as f64 / item.meta.fps.as_f64().max(1e-9);
                self.waveform_worker
                    .get_or_insert_with(waveform_worker::WaveformWorker::spawn)
                    .enqueue(
                        path,
                        item.content_hash,
                        item.meta.audio_stream_count(),
                        vv_media::recommended_num_peaks(secs),
                    );
            }
            return;
        }
        // The buffer opens the file on its own and skips the ones that do not
        // open: the error must be seen here.
        if let Err(e) = vv_media::Decoder::open(&path) {
            self.preview_error = Some(e.to_string());
            return;
        }
        self.browsing_render_ahead = Some(self.spawn_browsing_render_ahead(item));
    }

    /// A timeline at the media fps with only it on top: the timeline frames
    /// coincide with the source ones.
    pub(crate) fn spawn_browsing_render_ahead(
        &self,
        item: vv_core::MediaItem,
    ) -> (render_ahead::RenderAhead, MediaId) {
        self.spawn_media_render_ahead(
            item,
            self.settings.cache_budget_bytes,
            self.settings.lookahead_secs,
            self.settings.behind_secs,
        )
    }

    pub(crate) fn spawn_media_render_ahead(
        &self,
        item: vv_core::MediaItem,
        cache_budget_bytes: usize,
        lookahead_secs: f64,
        behind_secs: f64,
    ) -> (render_ahead::RenderAhead, MediaId) {
        let meta = item.meta.clone();
        let mut project = vv_core::Project::default();
        let preview_media = project.media_pool.insert(item);
        let mut track = Track::new(TrackKind::Video);
        track.clips.push(vv_core::Clip::from_source_range(
            project.alloc_clip_id(),
            vv_core::ClipSource::Media(preview_media),
            0,
            meta.duration_frames.max(1),
            0,
            vv_core::Rational::one(),
        ));
        let timeline_id = project.timelines.insert(vv_core::Timeline {
            name: "preview".into(),
            fps: meta.fps,
            resolution: (meta.width, meta.height),
            tracks: vec![track],
            markers: Vec::new(),
            master: Default::default(),
        });
        let render_ahead = render_ahead::RenderAhead::spawn(
            project,
            timeline_id,
            cache_budget_bytes,
            self.settings.proxy(),
            lookahead_secs,
            behind_secs,
        );
        render_ahead.set_preview_divisor(self.settings.preview_resolution.divisor());
        (render_ahead, preview_media)
    }

    pub(crate) fn stop_browsing(&mut self) {
        if self.browsing.is_some() && self.is_timeline_playing() {
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
        }
        self.browsing = None;
        self.browsing_render_ahead = None;
        self.preview_meta = None;
        // The clock stayed at the preview position: force the seek to the
        // timeline playhead.
        self.last_synced_playhead = FrameIdx::MIN;
    }

    /// The viewer shows the waveform of the browsed media.
    pub(crate) fn browsing_audio_only(&self) -> bool {
        self.browsing.is_some() && self.preview_meta.as_ref().is_some_and(|m| !m.has_video)
    }

    pub(crate) fn browse_total_frames(&self) -> FrameIdx {
        self.preview_meta.as_ref().map_or(0, |m| m.duration_frames)
    }

    pub(crate) fn browse_fps(&self) -> f64 {
        self.preview_meta
            .as_ref()
            .map_or(1.0, |m| m.fps.as_f64().max(1e-9))
    }

    /// The mixer clock acts as the playhead for the preview too, as for the
    /// timeline.
    pub(crate) fn toggle_browse_playback(&mut self) {
        if self.is_timeline_playing() {
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
            return;
        }
        let total = self.browse_total_frames();
        if total <= 0 {
            return;
        }
        if self.browse_playhead >= total - 1 {
            self.browse_playhead = 0;
        }
        self.seek_browse(self.browse_playhead);
        self.timeline_audio();
        self.sync_timeline_audio();
        self.timeline_audio().play();
    }

    pub(crate) fn seek_browse(&mut self, frame: FrameIdx) {
        let frame = frame.clamp(0, (self.browse_total_frames() - 1).max(0));
        self.browse_playhead = frame;
        let fps = self.browse_fps();
        if let Some(audio) = &mut self.timeline_audio {
            audio.seek_frame(frame, fps);
        }
        if let Some((render_ahead, _)) = &self.browsing_render_ahead {
            render_ahead.jump_to(frame);
        }
    }

    pub(crate) fn drive_browse_playback(&mut self) {
        if self.browsing.is_none() || !self.is_timeline_playing() {
            return;
        }
        let fps = self.browse_fps();
        let last = (self.browse_total_frames() - 1).max(0);
        let frame = self.timeline_audio().position_frame(fps);
        self.browse_playhead = frame.min(last);
        if frame >= last {
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
            self.timeline_audio().seek_frame(last, fps);
        }
    }
}
