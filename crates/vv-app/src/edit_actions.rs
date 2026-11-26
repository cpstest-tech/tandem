//! Keyboard editing: three-point edits, nudge, match frame, close gaps.

use vv_core::edit::EditMode;
use vv_core::{ClipId, ClipSource, FrameIdx, MediaId};

use crate::{Browsed, VenturiApp, timeline_ui};

impl VenturiApp {
    /// The media an edit takes, in media frames: the viewer's source between
    /// its marks, else the media selected in the pool, whole.
    fn edit_source(&self) -> Option<(MediaId, FrameIdx, FrameIdx)> {
        if let Some(Browsed::Media(id)) = &self.browsing {
            let (source_in, source_out) = self.browse_marks.resolve(self.browse_total_frames());
            return Some((*id, source_in, source_out));
        }
        let id = *self.media_pool_state.selected.iter().next()?;
        let item = self.session.project.media_pool.get(id)?;
        let drag = timeline_ui::MediaDrag::whole(id, &item.meta);
        Some((id, drag.source_in, drag.source_out))
    }

    pub(crate) fn three_point_edit(&mut self, mode: EditMode) {
        let Some((media, source_in, source_out)) = self.edit_source() else {
            return;
        };
        let Some(meta) = self
            .session
            .project
            .media_pool
            .get(media)
            .map(|i| i.meta.clone())
        else {
            return;
        };
        let Some(timeline_id) = self.timeline_id else {
            // No timeline yet: the first media makes one, as a drop does.
            let set = timeline_ui::MediaDragSet {
                items: vec![timeline_ui::MediaDrag {
                    source_in,
                    source_out,
                    ..timeline_ui::MediaDrag::whole(media, &meta)
                }],
            };
            self.add_media_set_to_timeline_at(&set, 0, timeline_ui::MediaDropTarget::Default);
            return;
        };
        if self
            .session
            .project
            .would_create_a_cycle(media, timeline_id)
        {
            return;
        }
        let group = self.session.history.begin_group();
        let video = meta.has_video;
        let audio = meta.has_audio;
        let Some(mut tracks) = self.resolve_drop_tracks(
            timeline_id,
            timeline_ui::MediaDropTarget::Default,
            video,
            audio,
        ) else {
            self.session.history.end_group(group);
            return;
        };
        if let Some(main) = self.session.project.timelines[timeline_id].main_track_index()
            && video
            && !self.session.project.timelines[timeline_id].is_locked(main)
        {
            tracks.video = Some(main);
        }
        let inserted = vv_core::edit::three_point_edit(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            vv_core::edit::MediaInsert {
                media_id: media,
                source_in,
                source_out,
                video,
                audio,
            },
            self.timeline_state.playhead,
            mode,
            tracks,
        );
        if let Some(clips) = &inserted
            && let Some(&first) = clips.first()
        {
            if video {
                self.upright_clip(timeline_id, first, media, &meta);
            }
            let tl = &self.session.project.timelines[timeline_id];
            if let Some(end) = tl.clip(first.0, first.1).map(|c| c.timeline_end()) {
                self.timeline_state.playhead = end;
            }
            self.timeline_state.set_single_selection(Some(first));
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
    }

    /// Moves the selected clips by `delta` frames, never before frame 0.
    pub(crate) fn nudge_selected(&mut self, delta: FrameIdx) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let selected: Vec<(usize, ClipId)> = self
            .timeline_state
            .selected
            .iter()
            .copied()
            .filter(|&(t, _)| !tl.is_locked(t))
            .collect();
        let earliest = selected
            .iter()
            .filter_map(|&(t, id)| tl.clip(t, id).map(|c| c.timeline_start))
            .min();
        let Some(earliest) = earliest else {
            return;
        };
        let delta = delta.max(-earliest);
        if delta == 0 {
            return;
        }
        let moves = selected
            .iter()
            .filter_map(|&(t, id)| tl.clip(t, id).map(|c| (id, t, t, c.timeline_start + delta)))
            .collect();
        vv_core::edit::move_clips(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            moves,
        );
    }

    /// Opens in the viewer the source of the clip under the playhead, at the
    /// frame the timeline shows.
    pub(crate) fn match_frame(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let playhead = self.timeline_state.playhead;
        let tl = &self.session.project.timelines[timeline_id];
        let picked = self
            .timeline_state
            .selected
            .iter()
            .filter_map(|&(t, id)| tl.clip(t, id))
            .find(|c| c.contains(playhead) && matches!(c.source, ClipSource::Media(_)))
            .or_else(|| tl.active_video_clip_at(playhead).map(|(_, c)| c));
        let Some(clip) = picked else {
            return;
        };
        let ClipSource::Media(media) = clip.source else {
            return;
        };
        if self
            .session
            .project
            .media_pool
            .get(media)
            .is_none_or(|i| i.compound.is_some())
        {
            return;
        }
        let frame = clip.picture_frame_at(playhead);
        let browsed = Browsed::Media(media);
        self.preview(&browsed);
        self.active_clip = None;
        self.browsing = Some(browsed);
        self.seek_browse(frame);
    }

    /// The selected clips take the edit source instead, keeping place,
    /// length and effects.
    pub(crate) fn replace_selected_clips(&mut self) {
        let (Some(timeline_id), Some((media, source_in, _))) =
            (self.timeline_id, self.edit_source())
        else {
            return;
        };
        if self
            .session
            .project
            .would_create_a_cycle(media, timeline_id)
        {
            return;
        }
        let tl = &self.session.project.timelines[timeline_id];
        let targets: Vec<(usize, ClipId)> = self
            .timeline_state
            .selected
            .iter()
            .copied()
            .filter(|&(t, _)| !tl.is_locked(t))
            .collect();
        if targets.is_empty() {
            return;
        }
        let group = self.session.history.begin_group();
        for clip in targets {
            vv_core::edit::replace_clip_media(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                clip,
                media,
                source_in,
            );
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::ReplaceClip);
    }

    /// Deletes the In–Out stretch on every unlocked track; `ripple` closes it.
    pub(crate) fn delete_marked_range(&mut self, ripple: bool) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let marks = self.timeline_state.export_marks;
        if marks.mark_in.is_none() && marks.mark_out.is_none() {
            return;
        }
        let total = self.session.project.timelines[timeline_id].total_frames();
        let (start, end) = marks.resolve(total);
        if end <= start {
            return;
        }
        let mode = if ripple {
            vv_core::edit::RangeDelete::Ripple
        } else {
            vv_core::edit::RangeDelete::Lift { tracks: None }
        };
        vv_core::edit::delete_ranges(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            &[(start, end)],
            &mode,
        );
        self.timeline_state.selected.clear();
        if ripple {
            self.timeline_state.export_marks.clear_all();
            self.timeline_state.playhead = start;
        }
    }

    pub(crate) fn close_all_gaps(&mut self) {
        if let Some(timeline_id) = self.timeline_id {
            vv_core::edit::close_all_gaps(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
            );
        }
    }
}

#[cfg(test)]
#[path = "tests/edit_actions.rs"]
mod tests;
