//! What the timeline bar, its context menu and the shortcuts ask the app
//! to do: one entry point, so every path takes the same steps.

use super::*;
use timeline_ui::{TimelineCommand, TimelineTool};
use vv_core::FrameIdx;
use vv_core::workflow::{self, Follow, Side};

/// J pressed: the playhead walks backwards at `tier`× the normal speed (the
/// player decodes forward only, so it is scrubbed, not played).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ReverseShuttle {
    tier: u8,
    owed_frames: f64,
}

const FRAME_JUMP: FrameIdx = 10;
const MAX_SHUTTLE_TIER: u8 = 8;

impl VenturiApp {
    pub(crate) fn follow_options(&self) -> Follow {
        Follow {
            magnet: self.timeline_state.magnet,
            linkage: self.timeline_state.linkage,
        }
    }

    /// Runs what the timeline queued during the frame.
    pub(crate) fn run_timeline_commands(&mut self, ctx: &egui::Context) {
        for command in std::mem::take(&mut self.timeline_state.commands) {
            self.run_timeline_command(ctx, command);
        }
    }

    pub(crate) fn run_timeline_command(&mut self, ctx: &egui::Context, command: TimelineCommand) {
        match command {
            TimelineCommand::AutoCut => self.open_auto_cut(ctx),
            TimelineCommand::AutoZoom => {
                self.reveal_block(Block::Properties);
                self.clip_tab = properties_tabs::ClipTab::Video;
                properties_tabs::select_video_sub_tab(ctx, properties_tabs::VideoSubTab::AutoZoom);
            }
            TimelineCommand::Reframe => {
                self.reveal_block(Block::Properties);
                self.clip_tab = properties_tabs::ClipTab::Video;
                properties_tabs::select_video_sub_tab(ctx, properties_tabs::VideoSubTab::Reframe);
            }
            TimelineCommand::Dub => self.open_dub(),
            TimelineCommand::Voiceover => self.open_voiceover(),
            TimelineCommand::BeatSync => self.beat.open = !self.beat.open,
            TimelineCommand::Undo => self.undo(),
            TimelineCommand::Redo => self.redo(),
            TimelineCommand::SplitAtPlayhead => self.split_at_playhead(),
            TimelineCommand::DeleteLeft => self.delete_around_playhead(Side::Left),
            TimelineCommand::DeleteRight => self.delete_around_playhead(Side::Right),
            TimelineCommand::Delete => self.delete_selected(),
            TimelineCommand::Copy => {
                if self.copy_selected_clips() {
                    self.publish_clipboard(ctx);
                }
            }
            TimelineCommand::Cut => {
                if self.copy_selected_clips() {
                    self.publish_clipboard(ctx);
                    self.delete_selected();
                }
            }
            TimelineCommand::Paste => self.paste_clipboard_at_playhead(),
            TimelineCommand::Duplicate => self.duplicate_selected(),
            TimelineCommand::ToggleDisabled => self.toggle_disabled_selected(),
            TimelineCommand::FreezeFrame => self.freeze_frame_at_playhead(),
            TimelineCommand::Reverse => self.reverse_selected(),
            TimelineCommand::Mirror => self.mirror_selected(),
            TimelineCommand::ExtractAudio => self.extract_selected_audio(),
            TimelineCommand::Group => self.group_selected(),
        }
    }

    pub(crate) fn set_timeline_tool(&mut self, tool: TimelineTool) {
        self.timeline_state.tool = tool;
    }

    /// Q / W.
    fn delete_around_playhead(&mut self, side: Side) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let selected: BTreeSet<(usize, ClipId)> = self.timeline_state.selected.clone();
        let follow = self.follow_options();
        let changed = workflow::delete_side(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            self.timeline_state.playhead,
            side,
            &selected,
            follow,
        );
        if changed {
            self.sync_selection_to_playhead();
        }
    }

    /// Del: removes the selected clips; with the Magnet on, the hole on the
    /// Main Track closes in the same undo step.
    pub(crate) fn delete_clips_with_magnet(
        &mut self,
        timeline_id: vv_core::TimelineId,
        clips: &[(usize, ClipId)],
    ) {
        let follow = self.follow_options();
        let anchors = if follow.magnet && follow.linkage {
            workflow::linkage_anchors(&self.session.project, timeline_id)
        } else {
            Vec::new()
        };
        let mark = self.session.history.begin_group();
        vv_core::edit::delete_clips(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            clips,
        );
        if follow.magnet {
            workflow::compact_main_track(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                &anchors,
            );
        }
        self.session
            .history
            .end_group_as(mark, vv_core::CommandLabel::DeleteClips);
    }

    fn selected_clips(&self) -> Vec<(usize, ClipId)> {
        self.timeline_state.selected.iter().copied().collect()
    }

    fn duplicate_selected(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let selected = self.selected_clips();
        let copies = workflow::duplicate_after(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            &selected,
        );
        if !copies.is_empty() {
            let anchor = copies.first().copied();
            self.timeline_state
                .set_selection(copies.into_iter().collect(), anchor);
        }
    }

    fn freeze_frame_at_playhead(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let selected = self.timeline_state.selected.clone();
        let still = workflow::freeze_frame(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            self.timeline_state.playhead,
            &selected,
        );
        if let Some(still) = still {
            self.timeline_state.set_single_selection(Some(still));
        }
    }

    /// Plays the selected media clips backwards; if all already do, forwards.
    pub(crate) fn reverse_selected(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let targets: Vec<(usize, ClipId, bool)> = self
            .timeline_state
            .selected
            .iter()
            .filter(|&&(t, _)| !tl.is_locked(t))
            .filter_map(|&(t, id)| tl.clip(t, id).map(|c| (t, id, c)))
            .filter(|(_, _, c)| {
                matches!(c.source, vv_core::ClipSource::Media(_)) && c.freeze.is_none()
            })
            .map(|(t, id, c)| (t, id, c.reversed))
            .collect();
        if targets.is_empty() {
            return;
        }
        let reversed = !targets.iter().all(|&(_, _, r)| r);
        let clips = targets.into_iter().map(|(t, id, _)| (t, id)).collect();
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(vv_core::SetClipsReversed::new(timeline_id, clips, reversed)),
        );
    }

    /// Flips the picture horizontally; if every selected clip already is, it
    /// flips them back.
    fn mirror_selected(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let targets: Vec<(usize, ClipId, [bool; 2])> = self
            .timeline_state
            .selected
            .iter()
            .filter(|&&(t, _)| tl.tracks[t].kind == vv_core::TrackKind::Video && !tl.is_locked(t))
            .filter_map(|&(t, id)| tl.clip(t, id).map(|c| (t, id, c.effects.transform.flip)))
            .collect();
        if targets.is_empty() {
            return;
        }
        let mirrored = !targets.iter().all(|&(_, _, flip)| flip[0]);
        let commands: Vec<Box<dyn vv_core::Command>> = targets
            .into_iter()
            .map(|(t, id, flip)| {
                Box::new(vv_core::set_clip_flip(
                    timeline_id,
                    t,
                    id,
                    [mirrored, flip[1]],
                )) as Box<dyn vv_core::Command>
            })
            .collect();
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(vv_core::CompositeCommand::new(
                vv_core::CommandLabel::Flip,
                commands,
            )),
        );
    }

    fn extract_selected_audio(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let selected = self.selected_clips();
        let freed = workflow::extract_audio(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            &selected,
        );
        if !freed.is_empty() {
            let anchor = freed.first().copied();
            self.timeline_state
                .set_selection(freed.into_iter().collect(), anchor);
        }
    }

    /// Ctrl+G: links the selected clips so they move and are edited together.
    fn group_selected(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let selected = self.selected_clips();
        if selected.len() < 2 {
            return;
        }
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(vv_core::LinkClips::new(timeline_id, selected)),
        );
    }

    /// Puts the playhead on `frame` as a click on the ruler would.
    pub(crate) fn jump_playhead_to(&mut self, frame: FrameIdx) {
        if self.timeline_id.is_none() {
            return;
        }
        if self.is_timeline_playing() {
            self.toggle_playback();
        }
        self.reverse_shuttle = None;
        self.timeline_state.playhead = frame.max(0);
        self.timeline_state.reveal_playhead = true;
        self.ensure_active_clip_matches_playhead(true);
        self.sync_selection_to_playhead();
    }

    pub(crate) fn step_playhead_by(&mut self, frames: FrameIdx) {
        self.jump_playhead_to(self.timeline_state.playhead + frames);
        self.play_scrub_audio();
    }

    pub(crate) fn step_playhead_by_ten(&mut self, forward: bool) {
        self.step_playhead_by(if forward { FRAME_JUMP } else { -FRAME_JUMP });
    }

    pub(crate) fn go_to_start(&mut self) {
        self.jump_playhead_to(0);
    }

    pub(crate) fn go_to_end(&mut self) {
        if let Some(timeline_id) = self.timeline_id {
            let end = self.session.project.timelines[timeline_id].total_frames();
            self.jump_playhead_to(end);
        }
    }

    /// ↑ / ↓: the previous or next cut of any track.
    pub(crate) fn jump_to_edit_point(&mut self, forward: bool) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let points = workflow::edit_points(&self.session.project, timeline_id);
        let playhead = self.timeline_state.playhead;
        let target = if forward {
            workflow::next_edit_point(&points, playhead)
        } else {
            workflow::previous_edit_point(&points, playhead)
        };
        if let Some(target) = target {
            self.jump_playhead_to(target);
        }
    }

    /// L: play forward, then 2×, 4×, 8×.
    pub(crate) fn shuttle_forward(&mut self) {
        self.reverse_shuttle = None;
        self.handle_fast_playback_key();
    }

    /// K: pause.
    pub(crate) fn shuttle_pause(&mut self) {
        self.reverse_shuttle = None;
        if self.is_timeline_playing() {
            self.toggle_playback();
        }
    }

    /// J: walk backwards, 2×, 4×, 8× with more presses.
    pub(crate) fn shuttle_back(&mut self) {
        if self.timeline_id.is_none() {
            return;
        }
        if self.is_timeline_playing() {
            self.toggle_playback();
        }
        let tier = match self.reverse_shuttle {
            Some(shuttle) => (shuttle.tier * 2).min(MAX_SHUTTLE_TIER),
            None => 1,
        };
        self.reverse_shuttle = Some(ReverseShuttle {
            tier,
            owed_frames: 0.0,
        });
    }

    /// Advances the J walk by the time since the last frame. Returns whether
    /// it is still running, so the caller keeps repainting.
    pub(crate) fn drive_reverse_shuttle(&mut self, dt: f32) -> bool {
        let Some(mut shuttle) = self.reverse_shuttle else {
            return false;
        };
        let Some(timeline_id) = self.timeline_id else {
            self.reverse_shuttle = None;
            return false;
        };
        if self.is_timeline_playing() || self.timeline_state.gesture_active() {
            self.reverse_shuttle = None;
            return false;
        }
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        shuttle.owed_frames += f64::from(dt) * fps * f64::from(shuttle.tier);
        let whole = shuttle.owed_frames.floor();
        shuttle.owed_frames -= whole;
        let target = (self.timeline_state.playhead - whole as FrameIdx).max(0);
        self.timeline_state.playhead = target;
        self.timeline_state.reveal_playhead = true;
        self.ensure_active_clip_matches_playhead(true);
        self.sync_selection_to_playhead();
        self.reverse_shuttle = (target > 0).then_some(shuttle);
        self.reverse_shuttle.is_some()
    }

    /// The frame under the preview axis, when the player should show it
    /// instead of the playhead's.
    pub(crate) fn preview_axis_frame(&self) -> Option<FrameIdx> {
        if self.is_timeline_playing() || self.timeline_state.gesture_active() {
            return None;
        }
        self.timeline_state
            .hover_frame
            .filter(|&frame| frame != self.timeline_state.playhead)
    }
}
