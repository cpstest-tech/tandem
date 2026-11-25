//! «Voice changer» section of the Audio tab.

use vv_core::{ClipId, ClipSource, SetClipsVoice, VoiceEffect};

use crate::properties_panel::{BoxedCommand, section_header};
use crate::{PanelTarget, VenturiApp};

pub(crate) fn voice_label(voice: VoiceEffect) -> std::borrow::Cow<'static, str> {
    match voice {
        VoiceEffect::Chipmunk => t!("props.voice_chipmunk"),
        VoiceEffect::Deep => t!("props.voice_deep"),
        VoiceEffect::Robot => t!("props.voice_robot"),
        VoiceEffect::Echo => t!("props.voice_echo"),
        VoiceEffect::Telephone => t!("props.voice_telephone"),
        VoiceEffect::Megaphone => t!("props.voice_megaphone"),
    }
}

impl VenturiApp {
    pub(crate) fn show_voice_section(
        &mut self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        pending: &mut Vec<BoxedCommand>,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let clips: Vec<(usize, ClipId)> = targets
            .iter()
            .filter(|t| {
                tl.clip(t.track_index, t.clip_id)
                    .is_some_and(|c| matches!(c.source, ClipSource::Media(_)))
            })
            .map(|t| (t.track_index, t.clip_id))
            .collect();
        let Some(&(track, id)) = clips.first() else {
            return;
        };
        let current = tl.clip(track, id).and_then(|c| c.voice);
        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.voice_changer"), false);
        if !open {
            return;
        }
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(current.is_none(), t!("props.curve_none"))
                .clicked()
            {
                chosen = Some(None);
            }
            for voice in VoiceEffect::ALL {
                if ui
                    .selectable_label(current == Some(voice), voice_label(voice))
                    .clicked()
                {
                    chosen = Some(Some(voice));
                }
            }
        });
        if let Some(voice) = chosen
            && voice != current
        {
            pending.push(Box::new(SetClipsVoice::new(timeline_id, clips, voice)));
        }
    }
}
