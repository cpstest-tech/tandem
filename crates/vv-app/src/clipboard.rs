//! Copy, cut and paste of clips, media and text.

use super::*;

impl VenturiApp {
    /// Copy/cut/paste from the keyboard. A copy also writes the copied clips
    /// or media to the system clipboard as text, so another instance can
    /// paste them; an empty `Paste` may be an image in the clipboard.
    pub(crate) fn handle_clipboard_events(&mut self, ui: &egui::Ui, events: &[egui::Event]) {
        for event in events {
            match event {
                egui::Event::Copy => {
                    let copied = if self.media_pool_state.focused {
                        self.copy_selected_media()
                    } else {
                        self.copy_selected_clips()
                    };
                    if copied {
                        self.publish_clipboard(ui.ctx());
                    }
                }
                // Ctrl+X in a text field cuts the text, not the clips.
                egui::Event::Cut if !ui.ctx().egui_wants_keyboard_input() => {
                    if self.timeline_state.selected.is_empty() {
                        continue;
                    }
                    self.copy_selected_clips();
                    self.publish_clipboard(ui.ctx());
                    self.delete_selected();
                }
                egui::Event::Paste(text)
                    if !ui.ctx().egui_wants_keyboard_input()
                        && self.paste_download_link(ui.ctx(), text) => {}
                egui::Event::Paste(text) if text.is_empty() => {
                    self.request_clipboard_image(ui.ctx())
                }
                egui::Event::Paste(text) => self.paste_text(text),
                _ => {}
            }
        }
    }

    pub(crate) fn clipboard_origin(&self) -> vv_core::clipboard::ClipboardOrigin {
        vv_core::clipboard::ClipboardOrigin {
            instance: self.clipboard_instance,
            epoch: self.session.epoch(),
        }
    }

    pub(crate) fn publish_clipboard(&self, ctx: &egui::Context) {
        ctx.copy_text(self.clipboard_text());
    }

    pub(crate) fn clipboard_text(&self) -> String {
        vv_core::clipboard::ClipboardPayload::new(
            self.clipboard_origin(),
            &self.session.project,
            &self.copied_media,
            &self.timeline_state.clipboard,
        )
        .to_text()
    }

    pub(crate) fn copy_selected_media(&mut self) -> bool {
        if self.media_pool_state.selected.is_empty() {
            return false;
        }
        self.copied_media = self.media_pool_state.selected.iter().copied().collect();
        self.timeline_state.clipboard.clear();
        true
    }

    /// Text from the system clipboard: clips or media copied by Tandem,
    /// maybe another instance or project. Any other text pastes what this
    /// instance copied last.
    pub(crate) fn paste_text(&mut self, text: &str) {
        use vv_core::clipboard::{ClipboardPayload, PayloadError};
        let payload = match ClipboardPayload::from_text(text) {
            None => {
                if !self.media_pool_state.focused {
                    self.paste_clipboard_at_playhead();
                }
                return;
            }
            Some(Err(PayloadError::UnsupportedVersion(version))) => {
                self.project_error =
                    Some(t!("menu.paste_unsupported", version = version).into_owned());
                return;
            }
            Some(Err(PayloadError::Malformed(e))) => {
                self.project_error = Some(t!("menu.paste_malformed", error = e).into_owned());
                return;
            }
            Some(Ok(payload)) => payload,
        };
        let to_pool = self.media_pool_state.focused || !payload.has_clips();
        if payload.origin == self.clipboard_origin() {
            if to_pool {
                let media: Vec<MediaId> = self
                    .copied_media
                    .iter()
                    .copied()
                    .filter(|&id| self.session.project.media_pool.contains_key(id))
                    .collect();
                self.media_pool_state.select_only(media);
            } else {
                self.paste_clipboard_at_playhead();
            }
            return;
        }
        if to_pool {
            if !payload.has_media() {
                return;
            }
            let mut add = vv_core::AddEntities::new(vv_core::CommandLabel::PasteMedia);
            let pasted = payload.merge_into(&mut self.session.project, &mut add);
            self.add_pasted_entities(add);
            self.media_pool_state.select_only(pasted.media);
            return;
        }
        if self.timeline_id.is_none() {
            return;
        }
        let mark = self.session.history.begin_group();
        let mut add = vv_core::AddEntities::new(vv_core::CommandLabel::PasteClips);
        let pasted = payload.merge_into(&mut self.session.project, &mut add);
        self.add_pasted_entities(add);
        self.paste_entries_at_playhead(pasted.clips);
        self.session
            .history
            .end_group_as(mark, vv_core::CommandLabel::PasteClips);
    }

    /// Pasted media need their thumbnail, proxy and waveform like imported
    /// ones. Nothing new (every file already in the pool) leaves no step.
    pub(crate) fn add_pasted_entities(&mut self, add: vv_core::AddEntities) {
        if add.is_empty() {
            return;
        }
        let added: Vec<MediaId> = add.media_ids().collect();
        self.session.apply(Box::new(add));
        for media_id in added {
            self.enqueue_media_background_jobs(media_id);
        }
    }

    /// Copies the selected clips (the linked groups are all already inside the
    /// selection).
    pub(crate) fn copy_selected_clips(&mut self) -> bool {
        let Some(timeline_id) = self.timeline_id else {
            return false;
        };
        if self.timeline_state.selected.is_empty() {
            return false;
        }

        let tl = &self.session.project.timelines[timeline_id];
        // The groups are remapped onto local tags: the paste needs new groups.
        let mut collected: Vec<(Option<vv_core::LinkGroupId>, timeline_ui::ClipboardEntry)> = self
            .timeline_state
            .selected
            .iter()
            .filter_map(|&(track_index, clip_id)| {
                let clip = tl.clip(track_index, clip_id)?;
                Some((
                    clip.linked_group,
                    timeline_ui::ClipboardEntry {
                        track_kind: tl.tracks[track_index].kind,
                        track_number: tl.track_number(track_index),
                        relative_start: clip.timeline_start,
                        clip: clip.clone(),
                        timeline_fps: tl.fps,
                        link_tag: None,
                    },
                ))
            })
            .collect();

        if collected.is_empty() {
            return false;
        }

        let anchor = collected
            .iter()
            .map(|(_, e)| e.relative_start)
            .min()
            .unwrap_or(0);
        for (_, e) in &mut collected {
            e.relative_start -= anchor;
        }

        let mut tag_of: std::collections::HashMap<vv_core::LinkGroupId, u64> =
            std::collections::HashMap::new();
        for (group, e) in &mut collected {
            if let Some(g) = group {
                let next_tag = tag_of.len() as u64;
                e.link_tag = Some(*tag_of.entry(*g).or_insert(next_tag));
            }
        }

        self.timeline_state.clipboard = collected.into_iter().map(|(_, e)| e).collect();
        self.copied_media.clear();
        true
    }

    /// Pastes the clipboard at the playhead preserving the distances and the linked
    /// groups; the pasted clips overwrite what was underneath.
    pub(crate) fn paste_clipboard_at_playhead(&mut self) {
        self.paste_entries_at_playhead(self.timeline_state.clipboard.clone());
    }

    pub(crate) fn paste_entries_at_playhead(&mut self, entries: Vec<timeline_ui::ClipboardEntry>) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if entries.is_empty() {
            return;
        }
        let playhead = self.timeline_state.playhead;
        let entries: Vec<timeline_ui::ClipboardEntry> = entries
            .into_iter()
            // Pasting a copied compound clip inside its own
            // nested timeline (directly or through another
            // compound clip) would close a cycle — see
            // `Project::would_create_a_cycle`, the same check as the drop
            // from the media pool.
            .filter(|e| match &e.clip.source {
                vv_core::ClipSource::Media(media_id) => !self
                    .session
                    .project
                    .would_create_a_cycle(*media_id, timeline_id),
                vv_core::ClipSource::SolidColor
                | vv_core::ClipSource::Text
                | vv_core::ClipSource::Adjustment => true,
            })
            .collect();
        if entries.is_empty() {
            return;
        }

        let mark = self.session.history.begin_group();
        self.add_tracks_for_clipboard(timeline_id, &entries);

        let tl = &self.session.project.timelines[timeline_id];
        let entries: Vec<(usize, timeline_ui::ClipboardEntry)> = entries
            .into_iter()
            .filter_map(|e| {
                let index = tl.track_of_kind_numbered(e.track_kind, e.track_number)?;
                (!tl.is_locked(index)).then_some((index, e))
            })
            .collect();
        if entries.is_empty() {
            self.session.history.end_group(mark);
            return;
        }

        let timeline_fps = self.session.project.timelines[timeline_id].fps;
        let clips: Vec<(usize, vv_core::Clip, Option<u64>)> = entries
            .iter()
            .map(|(track_index, entry)| {
                let mut clip = entry.clip.clone();
                clip.id = self.session.project.alloc_clip_id();
                clip.timeline_start = entry.relative_start;
                clip.linked_group = None;
                if entry.timeline_fps != timeline_fps {
                    // A media gone from the pool keeps the conform rate it had.
                    let conform_rate = match &clip.source {
                        vv_core::ClipSource::Media(media_id) => self
                            .session
                            .project
                            .media_pool
                            .get(*media_id)
                            .map_or(clip.conform_rate(), |item| {
                                vv_core::Rational::conform_rate(timeline_fps, item.meta.fps)
                            }),
                        vv_core::ClipSource::SolidColor
                        | vv_core::ClipSource::Text
                        | vv_core::ClipSource::Adjustment => clip.conform_rate(),
                    };
                    clip.retime(entry.timeline_fps, timeline_fps, conform_rate);
                }
                clip.timeline_start += playhead;
                (*track_index, clip, entry.link_tag)
            })
            .collect();
        let new_selection: BTreeSet<(usize, ClipId)> = clips
            .iter()
            .map(|(track, clip, _)| (*track, clip.id))
            .collect();
        let end = clips.iter().map(|(_, clip, _)| clip.timeline_end()).max();
        self.insert_clips_overwriting(timeline_id, clips, vv_core::CommandLabel::PasteClips);
        self.session
            .history
            .end_group_as(mark, vv_core::CommandLabel::PasteClips);
        let anchor = new_selection.iter().next().copied();
        self.timeline_state.set_selection(new_selection, anchor);
        if let Some(end) = end {
            self.timeline_state.playhead = end;
            self.ensure_active_clip_matches_playhead(true);
        }
    }

    /// Tracks missing to paste `entries`: a compound clip with video
    /// tracks only, pasted into a timeline having a single one, must
    /// create its own V2, not end up on the audio.
    pub(crate) fn add_tracks_for_clipboard(
        &mut self,
        timeline_id: TimelineId,
        entries: &[timeline_ui::ClipboardEntry],
    ) {
        for kind in [TrackKind::Video, TrackKind::Audio] {
            let wanted = entries
                .iter()
                .filter(|e| e.track_kind == kind)
                .map(|e| e.track_number)
                .max()
                .unwrap_or(0);
            let existing = self.session.project.timelines[timeline_id]
                .tracks_of_kind(kind)
                .count();
            for _ in existing..wanted {
                self.session.history.do_command(
                    &mut self.session.project,
                    Box::new(vv_core::AddTrack::new(timeline_id, kind)),
                );
            }
        }
    }
}
