//! What a gesture asks for once released, applied to the project.

use super::*;

pub(super) fn apply_pending_action(
    project: &mut Project,
    history: &mut History,
    state: &mut TimelineState,
    timeline_id: TimelineId,
    action: PendingAction,
) {
    match action {
        PendingAction::Marker(change) => {
            markers::apply_change(project, history, timeline_id, state.playhead, change);
        }
        PendingAction::Move {
            new_video_tracks,
            new_audio_tracks,
            moves,
            duplicate,
        } => {
            // Creating in increasing depth order is enough: both for
            // video and for audio, the depth-th created ends up
            // by itself on the right row (see `EffectiveTrack::New`).
            let mut video_tracks = Vec::with_capacity(new_video_tracks);
            for _ in 0..new_video_tracks {
                video_tracks.push(add_track(project, history, timeline_id, TrackKind::Video));
            }
            let mut audio_tracks = Vec::with_capacity(new_audio_tracks);
            for _ in 0..new_audio_tracks {
                audio_tracks.push(add_track(project, history, timeline_id, TrackKind::Audio));
            }
            let moves: Vec<(ClipId, usize, usize, FrameIdx)> = moves
                .into_iter()
                .map(|(id, from_track, dest, start)| {
                    let to_track = match dest {
                        EffectiveTrack::Existing(track) => track,
                        // The kind of the new track is the starting one.
                        EffectiveTrack::New(depth) => {
                            match project.timelines[timeline_id].tracks[from_track].kind {
                                TrackKind::Video => video_tracks[depth - 1],
                                TrackKind::Audio => audio_tracks[depth - 1],
                            }
                        }
                    };
                    (id, from_track, to_track, start)
                })
                .collect();
            if duplicate {
                duplicate_clips(project, history, state, timeline_id, &moves);
                return;
            }
            let onto_ripple = ripple_track(state)
                .is_some_and(|main| moves.iter().any(|&(_, _, to, _)| to == main));
            if onto_ripple {
                vv_core::edit::move_clips_over(project, history, timeline_id, moves);
            } else {
                vv_core::edit::move_clips(project, history, timeline_id, moves);
            }
        }
        PendingAction::Slip { clips, delta } => {
            vv_core::edit::slip_clips(project, history, timeline_id, &clips, delta);
        }
        PendingAction::Blade { clips, frame } => {
            let only: BTreeSet<ClipKey> = clips.into_iter().collect();
            vv_core::edit::split_clips(project, history, timeline_id, frame, Some(&only));
        }
        PendingAction::Trim {
            trims,
            mut overwritten,
        } => {
            if let Some(ripple) = ripple_track(state) {
                overwritten.retain(|&(track, _, _)| track != ripple);
            }
            let ripple_moves = if state.tool == TimelineTool::Ripple {
                overwritten.clear();
                ripple_trim_moves(&project.timelines[timeline_id], &trims)
            } else {
                Vec::new()
            };
            // The stretch gained by lengthening overwrites what was there; the trimmed
            // clips stay out, or they would cut themselves.
            let exclude: Vec<(usize, ClipId)> = trims
                .iter()
                .map(|&(clip_id, track_index, _, _)| (track_index, clip_id))
                .collect();
            let mut commands: Vec<Box<dyn vv_core::Command>> = Vec::new();
            vv_core::make_room_for_ranges(
                project,
                timeline_id,
                &overwritten,
                &exclude,
                &mut commands,
            );
            commands.extend(
                trims
                    .into_iter()
                    .map(|(clip_id, track_index, edge, new_value)| {
                        Box::new(vv_core::TrimClip::new(
                            timeline_id,
                            track_index,
                            clip_id,
                            edge,
                            new_value,
                        )) as Box<dyn vv_core::Command>
                    }),
            );
            if !ripple_moves.is_empty() {
                commands.push(Box::new(vv_core::MoveClips::new(timeline_id, ripple_moves)));
            }
            history.do_command(
                project,
                Box::new(vv_core::CompositeCommand::new(
                    vv_core::CommandLabel::TrimClips,
                    commands,
                )),
            );
        }
        PendingAction::SetFade {
            track_index,
            clip_id,
            edge,
            new_value,
        } => {
            history.do_command(
                project,
                Box::new(vv_core::SetClipFade::new(
                    timeline_id,
                    track_index,
                    clip_id,
                    edge,
                    new_value,
                )),
            );
        }
        PendingAction::SetGain {
            track_index,
            clip_id,
            new_value,
        } => {
            history.do_command(
                project,
                Box::new(vv_core::set_clip_gain(
                    timeline_id,
                    track_index,
                    clip_id,
                    new_value,
                )),
            );
        }
        PendingAction::ApplyFilter {
            track_index,
            clip_id,
            filter,
        } => {
            // If it is already present (dropping it again) it is only re-enabled,
            // instead of being duplicated at the end.
            let new_filters = project.timelines[timeline_id]
                .clip(track_index, clip_id)
                .map(|clip| {
                    let mut filters = clip.effects.filters.clone();
                    let own = match filters.iter().position(|f| f.kind == filter.kind) {
                        Some(pos) => &mut filters[pos],
                        None => {
                            filters.push(vv_core::ClipFilter::new(filter.kind));
                            filters.last_mut().unwrap()
                        }
                    };
                    own.enabled = true;
                    if let Some(preset) = filter.preset {
                        own.grade.apply_preset(preset);
                    }
                    filters
                });
            if let Some(filters) = new_filters {
                history.do_command(
                    project,
                    Box::new(vv_core::set_clip_filters(
                        timeline_id,
                        track_index,
                        clip_id,
                        filters,
                    )),
                );
            }
        }
        PendingAction::ApplyFx { targets, id } => {
            if let Some(asset) = vv_render::assets::library().get(&id) {
                crate::fx_library::apply_asset_to_clips(
                    project,
                    history,
                    timeline_id,
                    &targets,
                    asset,
                );
            }
        }
        PendingAction::ApplyAssetTransition {
            track_index,
            left,
            right,
            id,
        } => {
            let library = vv_render::assets::library();
            let applied = library.get(&id).is_some_and(|asset| {
                vv_core::fx::apply_asset_transition(
                    project,
                    history,
                    timeline_id,
                    track_index,
                    (left, right),
                    vv_core::AssetFx::new(asset.id.clone(), asset.default_params()),
                    asset.default_secs,
                )
            });
            if applied {
                select_crossing(state, track_index, left);
            } else {
                state.toast = Some(t!("fx.no_adjacent_pair").into_owned());
            }
        }
        PendingAction::ApplyTransition {
            track_index,
            clip_id,
            edge,
            kind,
        } => {
            // A Smooth Cut only bridges a few frames, as in Resolve.
            let seconds = match kind {
                vv_core::TransitionKind::SmoothCut => 0.16,
                _ => 0.45,
            };
            let default_duration =
                (project.timelines[timeline_id].fps.as_f64() * seconds).round() as FrameIdx;
            let transition = vv_core::Transition {
                kind,
                duration: default_duration.max(1),
                direction: vv_core::PushDirection::Right,
                ease: vv_core::Transition::default_ease(kind),
                curve: 0.5,
                asset: None,
            };
            let neighbor = adjacent_clip(
                &project.timelines[timeline_id].tracks[track_index],
                clip_id,
                edge,
            );
            if let Some(neighbor_id) = neighbor {
                let (left_id, right_id) = match edge {
                    FadeEdge::In => (neighbor_id, clip_id),
                    FadeEdge::Out => (clip_id, neighbor_id),
                };
                apply_new_crossing(
                    project,
                    history,
                    state,
                    timeline_id,
                    track_index,
                    left_id,
                    right_id,
                    transition,
                );
            } else if let Some(clip) = project.timelines[timeline_id].clip(track_index, clip_id) {
                let mut transition = transition;
                transition.duration = transition.duration.clamp(1, clip.timeline_len.max(1));
                history.do_command(
                    project,
                    Box::new(vv_core::set_clip_transition(
                        timeline_id,
                        track_index,
                        clip_id,
                        edge,
                        Some(transition),
                    )),
                );
                state.selected.clear();
                state.selection_anchor = None;
                state.selected_gap = None;
                state.selected_transition =
                    Some(TransitionSelection::Edge((track_index, clip_id), edge));
            }
        }
        PendingAction::SetTransitionDuration {
            track_index,
            clip_id,
            edge,
            new_value,
        } => {
            if let Some(clip) = project.timelines[timeline_id].clip(track_index, clip_id) {
                let mut transition = match edge {
                    FadeEdge::In => clip.effects.transition_in.clone(),
                    FadeEdge::Out => clip.effects.transition_out.clone(),
                };
                if let Some(t) = &mut transition {
                    t.duration = new_value.clamp(1, clip.timeline_len.max(1));
                }
                if let Some(transition) = transition {
                    history.do_command(
                        project,
                        Box::new(vv_core::set_clip_transition(
                            timeline_id,
                            track_index,
                            clip_id,
                            edge,
                            Some(transition),
                        )),
                    );
                }
            }
        }
        PendingAction::SetCrossingDuration {
            track_index,
            left_clip,
            new_value,
        } => {
            let track = &project.timelines[timeline_id].tracks[track_index];
            if let Some(mut crossing) = track.crossing_from(left_clip).cloned() {
                let max_duration = match (
                    track.clip(crossing.left_clip),
                    track.clip(crossing.right_clip),
                ) {
                    (Some(left), Some(right)) => {
                        (2 * left.timeline_len.min(right.timeline_len)).max(1)
                    }
                    _ => new_value.max(1),
                };
                crossing.transition.duration = new_value.clamp(1, max_duration);
                history.do_command(
                    project,
                    Box::new(vv_core::SetCrossTransition::new(
                        timeline_id,
                        track_index,
                        left_clip,
                        Some(crossing),
                    )),
                );
            }
        }
        PendingAction::DuplicateTransition {
            track_index,
            clip_id,
            edge,
            transition,
        } => {
            let neighbor = adjacent_clip(
                &project.timelines[timeline_id].tracks[track_index],
                clip_id,
                edge,
            );
            if let Some(neighbor_id) = neighbor {
                let (left_id, right_id) = match edge {
                    FadeEdge::In => (neighbor_id, clip_id),
                    FadeEdge::Out => (clip_id, neighbor_id),
                };
                apply_new_crossing(
                    project,
                    history,
                    state,
                    timeline_id,
                    track_index,
                    left_id,
                    right_id,
                    transition,
                );
            } else if let Some(clip) = project.timelines[timeline_id].clip(track_index, clip_id) {
                let mut transition = transition;
                transition.duration = transition.duration.clamp(1, clip.timeline_len.max(1));
                history.do_command(
                    project,
                    Box::new(vv_core::set_clip_transition(
                        timeline_id,
                        track_index,
                        clip_id,
                        edge,
                        Some(transition),
                    )),
                );
                state.selected.clear();
                state.selection_anchor = None;
                state.selected_gap = None;
                state.selected_transition =
                    Some(TransitionSelection::Edge((track_index, clip_id), edge));
            }
        }
        PendingAction::Unlink(track_index, clip_id) => {
            history.do_command(
                project,
                Box::new(vv_core::UnlinkClip::new(timeline_id, track_index, clip_id)),
            );
        }
        PendingAction::Link(targets) => {
            history.do_command(
                project,
                Box::new(vv_core::LinkClips::new(timeline_id, targets)),
            );
        }
        PendingAction::SetTrackFlag(track_index, flag, value) => {
            history.do_command(
                project,
                Box::new(vv_core::SetTrackFlag::new(
                    timeline_id,
                    track_index,
                    flag,
                    value,
                )),
            );
            if flag == TrackFlag::Locked && value {
                state.drop_locked(&project.timelines[timeline_id]);
            }
        }
        PendingAction::AddTrack(kind) => {
            add_track(project, history, timeline_id, kind);
        }
        PendingAction::ArmNeedsSave => state.record_needs_save = true,
        PendingAction::EditCover => state.edit_cover = true,
        PendingAction::RemoveTrack(track_index) => {
            history.do_command(
                project,
                Box::new(vv_core::RemoveTrack::new(timeline_id, track_index)),
            );
            // The track indices of the selection are no longer valid: it is cleared.
            state.clear_selection();
        }
        PendingAction::UngroupCompound((track_index, clip_id)) => {
            match vv_core::fx::ungroup_compound_clip(
                project,
                history,
                timeline_id,
                track_index,
                clip_id,
            ) {
                Ok(created) => {
                    state.clear_selection();
                    state.selected.extend(created);
                }
                Err(vv_core::fx::UngroupError::NotCompound) => {}
                Err(vv_core::fx::UngroupError::CarriesEffects) => {
                    state.toast = Some(t!("fx.ungroup_has_effects").into_owned());
                }
            }
        }
        PendingAction::MakeCompound(clips) => {
            make_compound_clip(project, history, timeline_id, clips);
            state.clear_selection();
        }
        PendingAction::SetDisplayColor(clips, color) => {
            history.do_command(
                project,
                Box::new(vv_core::SetClipsDisplayColor::new(
                    timeline_id,
                    clips,
                    color,
                )),
            );
        }
        PendingAction::SetFreeze { clips, at } => {
            history.do_command(
                project,
                Box::new(vv_core::SetClipFreeze::new(timeline_id, clips, at)),
            );
        }
        // As a trim: the stretch gained overwrites what was there, a shorter
        // clip leaves a gap.
        PendingAction::SetSpeed {
            clips,
            speed,
            pitch_correction,
            resize_to,
        } => {
            let tl = &project.timelines[timeline_id];
            let overwritten: Vec<(usize, FrameIdx, FrameIdx)> = clips
                .iter()
                .filter_map(|&(track_index, clip_id)| {
                    let clip = tl.clip(track_index, clip_id)?;
                    let ClipSource::Media(media_id) = clip.source else {
                        return None;
                    };
                    let media = project.media_pool.get(media_id)?;
                    let new_end = match resize_to {
                        Some((from, to)) if from == clip.timeline_end() => to,
                        _ => {
                            let mut retimed = clip.clone();
                            retimed.unfreeze(media.meta.duration_frames);
                            retimed.set_speed(
                                speed,
                                vv_core::Rational::conform_rate(tl.fps, media.meta.fps),
                            );
                            retimed.timeline_end()
                        }
                    };
                    vv_core::edit::grown_range(clip, track_index, TrimEdge::End, new_end)
                })
                .collect();
            let mut commands: Vec<Box<dyn vv_core::Command>> = Vec::new();
            vv_core::make_room_for_ranges(
                project,
                timeline_id,
                &overwritten,
                &clips,
                &mut commands,
            );
            commands.push(Box::new(vv_core::SetClipSpeed::new(
                timeline_id,
                clips,
                speed,
                pitch_correction,
                match resize_to {
                    Some((from, to)) => vv_core::SpeedFit::ResizeTo { from, to },
                    None => vv_core::SpeedFit::Resize,
                },
            )));
            history.do_command(
                project,
                Box::new(vv_core::CompositeCommand::new(
                    vv_core::CommandLabel::ClipSpeed,
                    commands,
                )),
            );
        }
    }
}
