//! Media, generators and clipboard clips landing on the timeline.

use super::*;

impl VenturiApp {
    /// Drop of an effect from the Effects panel: the generator clip goes on the
    /// video track indicated by `target`, at `start`.
    pub(crate) fn add_generator_to_timeline_at(
        &mut self,
        generator: timeline_ui::Generator,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        if generator == timeline_ui::Generator::Text {
            let template = self.dropped_text_template();
            self.insert_text_template(template.as_deref(), start, target);
            return;
        }
        let group = self.session.history.begin_group();
        let timeline_id = self.ensure_timeline();
        if let Some(tracks) = self.resolve_drop_tracks(timeline_id, target, true, false) {
            // A generator always has video: `resolve_drop_tracks` with
            // `any_video: true` always resolves to `Some`.
            let video_track = tracks
                .video
                .expect("generator: video track always resolved");
            vv_core::edit::insert_generator(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                generator,
                video_track,
                start,
                None,
            );
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
    }

    pub(crate) fn add_drop_to_timeline_at(
        &mut self,
        drag: &timeline_ui::TimelineDrag,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        match drag {
            timeline_ui::TimelineDrag::Media(set) => {
                self.add_media_set_to_timeline_at(set, start, target)
            }
            timeline_ui::TimelineDrag::Generator(g) => {
                self.add_generator_to_timeline_at(*g, start, target)
            }
            timeline_ui::TimelineDrag::Fx(fx) => {
                self.add_fx_element_at(&fx.id, fx.millis, start, target)
            }
        }
    }

    pub(crate) fn insert_clips_overwriting(
        &mut self,
        timeline_id: TimelineId,
        clips: Vec<(usize, vv_core::Clip, Option<u64>)>,
        label: vv_core::CommandLabel,
    ) {
        vv_core::edit::insert_clips(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            clips,
            label,
        );
    }

    /// Appends the media to the video track (and the audio clips alongside).
    #[cfg(test)]
    pub(crate) fn add_media_to_timeline(&mut self, media_id: MediaId) {
        let Some(item) = self.session.project.media_pool.get(media_id) else {
            return;
        };
        let meta = item.meta.clone();
        // If a timeline does not exist yet (e.g. first drag&drop from the media
        // pool), it is created on the fly inheriting fps/resolution from this
        // media.
        let rotation = self.media_rotation(media_id);
        let timeline_id = self.ensure_timeline_for(&meta, rotation, None);
        if self
            .session
            .project
            .would_create_a_cycle(media_id, timeline_id)
        {
            return;
        }
        let video_track = self.session.project.timelines[timeline_id]
            .first_track_index(TrackKind::Video)
            .unwrap_or(0);
        let video_start = track_end(&self.session.project, timeline_id, video_track);
        self.insert_media_clip(
            timeline_id,
            timeline_ui::MediaDrag::whole(media_id, &meta),
            &meta,
            video_start,
            TargetTracks {
                video: Some(video_track),
                extra_audio: None,
            },
        );
    }

    /// Like `add_media_to_timeline`, but places the clip at `start` (position
    /// and `target` from the drag&drop onto the timeline, see `MediaDropTarget`).
    #[cfg(test)]
    pub(crate) fn add_media_to_timeline_at(
        &mut self,
        drag: timeline_ui::MediaDrag,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        self.add_media_set_to_timeline_at(&timeline_ui::MediaDragSet::one(drag), start, target);
    }

    /// Drop of one or more media: appended from `start` in pool order. The
    /// new tracks are created once for the whole drop.
    pub(crate) fn add_media_set_to_timeline_at(
        &mut self,
        set: &timeline_ui::MediaDragSet,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        let drops: Vec<(timeline_ui::MediaDrag, vv_core::MediaMeta)> = set
            .items
            .iter()
            .filter(|d| d.source_len() > 0)
            .filter_map(|d| {
                let item = self.session.project.media_pool.get(d.media_id)?;
                Some((*d, item.meta.clone()))
            })
            .collect();
        if drops.is_empty() {
            return;
        }
        // One drop = one Ctrl+Z, even if inside there are N clips (one
        // per audio stream of each media) plus the timeline and the tracks
        // created on the fly.
        let group = self.session.history.begin_group();
        // fps and resolution from the first *video* media of the set: an audio at the
        // head has no resolution to give the timeline.
        let timeline_id = match drops.iter().find(|(d, meta)| d.takes_video(meta)) {
            Some((first, video_meta)) => {
                let rotation = self.media_rotation(first.media_id);
                self.ensure_timeline_for(video_meta, rotation, None)
            }
            None => self.ensure_timeline_audio_only(None),
        };
        // A drop that would close a cycle (a timeline imported inside
        // itself, directly or through one of its compound clips) is
        // discarded instead of corrupting the project — see
        // `Project::would_create_a_cycle`.
        let drops: Vec<(timeline_ui::MediaDrag, vv_core::MediaMeta)> = drops
            .into_iter()
            .filter(|(d, _)| {
                !self
                    .session
                    .project
                    .would_create_a_cycle(d.media_id, timeline_id)
            })
            .collect();
        if drops.is_empty() {
            self.session.history.end_group(group);
            return;
        }
        let any_video = drops.iter().any(|(d, meta)| d.takes_video(meta));
        let any_audio = drops.iter().any(|(d, meta)| d.takes_audio(meta));
        let Some(tracks) = self.resolve_drop_tracks(timeline_id, target, any_video, any_audio)
        else {
            self.session.history.end_group(group);
            return;
        };

        let timeline_fps = self.session.project.timelines[timeline_id].fps;
        let mut cursor = start;
        let main_track = self.session.project.timelines[timeline_id].main_track_index();
        if self.timeline_state.magnet && tracks.video.is_some() && tracks.video == main_track {
            let project = &self.session.project;
            cursor = vv_core::workflow::main_insert_point(project, timeline_id, start, &[]);
            let anchors = if self.timeline_state.linkage {
                vv_core::workflow::linkage_anchors(project, timeline_id)
            } else {
                Vec::new()
            };
            let total: FrameIdx = drops
                .iter()
                .map(|(drag, meta)| {
                    drag.timeline_len(vv_core::Rational::conform_rate(timeline_fps, meta.fps))
                })
                .sum();
            vv_core::workflow::open_main_gap(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                cursor,
                total,
                &anchors,
            );
        }
        for (drag, meta) in &drops {
            self.insert_media_clip(timeline_id, *drag, meta, cursor, tracks);
            let rate = vv_core::Rational::conform_rate(timeline_fps, meta.fps);
            cursor += drag.timeline_len(rate);
        }
        if self.timeline_state.magnet {
            vv_core::workflow::compact_main_track(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                &[],
            );
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
    }

    /// Creates the tracks required by `target` (once per drop) and
    /// returns where the clips will go.
    pub(crate) fn resolve_drop_tracks(
        &mut self,
        timeline_id: TimelineId,
        target: timeline_ui::MediaDropTarget,
        any_video: bool,
        any_audio: bool,
    ) -> Option<TargetTracks> {
        let video = if !any_video {
            None
        } else {
            Some(match target {
                timeline_ui::MediaDropTarget::NewVideoTrack => timeline_ui::add_track(
                    &mut self.session.project,
                    &mut self.session.history,
                    timeline_id,
                    TrackKind::Video,
                ),
                timeline_ui::MediaDropTarget::Track(track) => {
                    if self.session.project.timelines[timeline_id].is_locked(track) {
                        return None;
                    }
                    track
                }
                _ => match self.session.project.timelines[timeline_id]
                    .first_unlocked_track_index(TrackKind::Video)
                {
                    Some(track) => track,
                    // No free video track: one is created.
                    None => timeline_ui::add_track(
                        &mut self.session.project,
                        &mut self.session.history,
                        timeline_id,
                        TrackKind::Video,
                    ),
                },
            })
        };
        let extra_audio = if target == timeline_ui::MediaDropTarget::NewAudioTrack && any_audio {
            Some(timeline_ui::add_track(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                TrackKind::Audio,
            ))
        } else {
            None
        };
        Some(TargetTracks { video, extra_audio })
    }

    pub(crate) fn insert_media_clip(
        &mut self,
        timeline_id: TimelineId,
        drag: timeline_ui::MediaDrag,
        meta: &vv_core::MediaMeta,
        start: FrameIdx,
        tracks: TargetTracks,
    ) {
        let takes_video = drag.takes_video(meta);
        let inserted = vv_core::edit::insert_media(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            vv_core::edit::MediaInsert {
                media_id: drag.media_id,
                source_in: drag.source_in,
                source_out: drag.source_out,
                video: takes_video,
                audio: drag.takes_audio(meta),
            },
            start,
            tracks,
        );
        // The video clip comes first.
        if takes_video && let Some(&clip) = inserted.as_ref().and_then(|clips| clips.first()) {
            self.upright_clip(timeline_id, clip, drag.media_id, meta);
        }
    }
}
