//! Timeline playback, marks, scrubbing and speed.

use super::*;

impl VenturiApp {
    /// Pauses while the playhead is held and resumes on release.
    pub(crate) fn hold_playback_while_scrubbing(&mut self, held: bool) {
        if held {
            if self.is_timeline_playing() {
                self.toggle_playback();
                self.resume_after_scrub = true;
            }
        } else if std::mem::take(&mut self.resume_after_scrub) && !self.is_timeline_playing() {
            self.toggle_playback();
        }
    }

    pub(crate) fn is_timeline_playing(&self) -> bool {
        self.timeline_audio
            .as_ref()
            .is_some_and(TimelineAudio::is_playing)
    }

    pub(crate) fn timeline_audio(&mut self) -> &mut TimelineAudio {
        self.timeline_audio.get_or_insert_with(TimelineAudio::new)
    }

    pub(crate) fn sync_timeline_audio(&mut self) {
        let fps = self.browse_fps();
        let browsed = self
            .browsing_item()
            .map(|item| (item.path.clone(), item.meta.clone()));
        let Some(audio) = &mut self.timeline_audio else {
            return;
        };
        audio.set_budget_bytes(self.settings.audio_budget_bytes);
        if self.browsing.is_some() {
            if let Some((path, meta)) = browsed {
                let duration_secs = meta.duration_frames as f64 / meta.fps.as_f64().max(1e-9);
                audio.sync_media(&path, self.browse_audio_streams, fps, duration_secs);
            }
        } else if let Some(timeline_id) = self.timeline_id {
            audio.sync(
                &self.session.project,
                timeline_id,
                self.session.history.generation(),
            );
        } else {
            audio.clear_media();
        }
    }

    pub(crate) fn mark_at_playhead(&mut self, is_in: bool) {
        let Some(target) = self.mark_target() else {
            return;
        };
        if target.total <= 0 {
            return;
        }
        if is_in {
            target.marks.set_in(target.playhead, target.total);
        } else {
            target.marks.set_out(target.playhead, target.total);
        }
    }

    pub(crate) fn mark_target(&mut self) -> Option<MarkTarget<'_>> {
        if self.browsing.is_some() {
            if self.preview_error.is_some() {
                None
            } else {
                let total = self.browse_total_frames();
                let playhead = self.browse_playhead;
                Some(MarkTarget {
                    marks: &mut self.browse_marks,
                    total,
                    playhead,
                })
            }
        } else if let Some(timeline_id) = self.timeline_id {
            let total = self.session.project.timelines[timeline_id].total_frames();
            let playhead = self.timeline_state.playhead;
            Some(MarkTarget {
                marks: &mut self.timeline_state.export_marks,
                total,
                playhead,
            })
        } else {
            None
        }
    }

    pub(crate) fn clear_in_point(&mut self) {
        if let Some(target) = self.mark_target() {
            target.marks.clear_in();
        }
    }

    pub(crate) fn clear_out_point(&mut self) {
        if let Some(target) = self.mark_target() {
            target.marks.clear_out();
        }
    }

    pub(crate) fn clear_in_and_out(&mut self) {
        if let Some(target) = self.mark_target() {
            target.marks.clear_all();
        }
    }

    /// Left/right arrows: one frame back/forward, holding them down
    /// scrolls at `ARROW_HOLD_SPEED`. It pauses if playing.
    /// Returns `true` while an arrow is held.
    pub(crate) fn step_playhead_with_arrows(
        &mut self,
        direction: Option<FrameIdx>,
        time: f64,
    ) -> bool {
        let Some(direction) = direction else {
            self.arrow_hold = None;
            return false;
        };
        let Some(timeline_id) = self.timeline_id else {
            return false;
        };
        if self
            .arrow_hold
            .as_ref()
            .is_none_or(|h| h.direction != direction)
        {
            if self.is_timeline_playing() {
                self.toggle_playback();
            }
            self.arrow_hold = Some(ArrowHold {
                direction,
                pressed_at: time,
                start_frame: self.timeline_state.playhead,
            });
        }
        let hold = self.arrow_hold.as_ref().expect("set above");
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        let target = arrow_hold_target(hold, time - hold.pressed_at, fps);
        if target != self.timeline_state.playhead {
            self.timeline_state.playhead = target;
            self.ensure_active_clip_matches_playhead(true);
            self.sync_selection_to_playhead();
            self.play_scrub_audio();
        }
        true
    }

    pub(crate) fn play_scrub_audio(&mut self) {
        if !self.scrub_audio || self.browsing.is_some() {
            return;
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        let playhead = self.timeline_state.playhead;
        self.timeline_audio();
        self.sync_timeline_audio();
        let audio = self.timeline_audio();
        audio.seek_frame(playhead, fps);
        audio.play_scrub_snippet();
    }

    /// Play/pause of the timeline from the playhead, with no need for a selection:
    /// gaps included (they play silence), stopping only past the end of the
    /// content.
    pub(crate) fn toggle_playback(&mut self) {
        if self.browsing.is_some() {
            self.toggle_browse_playback();
            return;
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if self.is_timeline_playing() {
            self.timeline_audio().pause();
            // The space bar always pauses at 1x: a later "a"
            // restarts at normal speed.
            self.reset_playback_speed_to_normal();
            self.finish_take();
            return;
        }
        let timeline = &self.session.project.timelines[timeline_id];
        let (fps, end) = (timeline.fps.as_f64(), timeline.total_frames());
        let mut playhead = self.timeline_state.playhead;
        if self.timeline_state.loop_playback {
            let (from, to) = self.timeline_state.export_marks.resolve(end);
            if playhead < from || playhead >= to {
                playhead = from;
                self.timeline_state.playhead = from;
            }
        }
        if playhead >= end || !self.prepare_take() {
            return;
        }
        self.cue_timeline_playback(playhead, fps);
        self.start_take(playhead);
        self.timeline_audio().play();
    }

    pub(crate) fn cue_timeline_playback(&mut self, playhead: FrameIdx, fps: f64) {
        self.timeline_audio();
        self.sync_timeline_audio();
        self.timeline_audio().seek_frame(playhead, fps);
        self.last_synced_playhead = playhead;
        self.active_clip = self.active_video_clip_at(playhead);
    }

    /// During playback the playhead follows the mixer clock; at the end of the
    /// content it stops.
    pub(crate) fn drive_playback(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if !self.is_timeline_playing() {
            return;
        }
        let timeline = &self.session.project.timelines[timeline_id];
        let (fps, end) = (timeline.fps.as_f64(), timeline.total_frames());
        let mut frame = self.timeline_audio().position_frame(fps);
        if self.timeline_state.loop_playback {
            let (from, to) = self.timeline_state.export_marks.resolve(end);
            if to > from && frame >= to {
                self.timeline_audio().seek_frame(from, fps);
                self.last_synced_playhead = from;
                frame = from;
            }
        }
        if frame >= end {
            frame = end;
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
            self.timeline_audio().seek_frame(end, fps);
        }
        self.timeline_state.playhead = frame;
        self.active_clip = self.active_video_clip_at(frame);
    }

    /// "a" key: when stopped like the space bar (1x), during playback it
    /// accelerates 2x -> 4x -> 8x. Only the space bar pauses.
    pub(crate) fn handle_fast_playback_key(&mut self) {
        if self.browsing.is_some() {
            if !self.is_timeline_playing() {
                self.toggle_browse_playback();
            }
            return;
        }
        if !self.is_timeline_playing() {
            self.toggle_playback();
            return;
        }
        let next_speed = match SpeedTier::from_multiplier(self.playback_speed) {
            Some(tier) => tier.next().tempo(),
            None => SpeedTier::X2.tempo(),
        };
        self.request_playback_speed(next_speed);
    }

    pub(crate) fn reset_playback_speed_to_normal(&mut self) {
        self.request_playback_speed(1.0);
    }

    /// 1x immediately; the fast speeds when their stretched audio
    /// is ready (see `TimelineAudio::request_speed`).
    pub(crate) fn request_playback_speed(&mut self, speed: f64) {
        match &mut self.timeline_audio {
            Some(audio) => {
                audio.request_speed(speed);
                self.playback_speed = audio.speed();
            }
            None => self.playback_speed = speed,
        }
    }
}
