//! Spaces in a composition that show the user's own photos or videos.
use super::Job;
use super::context::Attachment;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_SLOTS: usize = 6;
const MAX_REQUESTED: usize = 3;
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const PUBLIC: &str = "public";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    #[default]
    Image,
    Video,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default)]
    pub reason: String,
    /// The user's file; `None` while the space waits for it.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Copy inside the job's `public/` directory, the one the scene renders.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub seconds: f64,
}

impl Slot {
    pub fn filled(&self) -> bool {
        self.file.is_some()
    }

    pub fn for_file(id: String, path: PathBuf, label: &str) -> Self {
        let label = if label.trim().is_empty() {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            label.trim().to_owned()
        };
        Self {
            kind: if vv_media::is_image_path(&path) {
                Kind::Image
            } else {
                Kind::Video
            },
            id,
            label,
            path: Some(path),
            ..Self::default()
        }
    }
}

pub fn slug(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(32)
        .collect::<String>()
        .to_ascii_lowercase()
}

pub fn empty_count(job: &Job) -> usize {
    job.slots.iter().filter(|slot| !slot.filled()).count()
}

/// Inherits the parent's spaces and files, then applies the request's material.
/// In `request.material` a `None` path on a known space empties it.
pub fn prepare(
    job: &mut Job,
    directory: &Path,
    parent: Option<(&Job, &Path)>,
) -> Result<(), String> {
    let public = directory.join(PUBLIC);
    std::fs::create_dir_all(&public).map_err(|e| e.to_string())?;
    let mut slots = Vec::new();
    if let Some((parent, previous)) = parent {
        for slot in &parent.slots {
            if let Some(file) = &slot.file {
                let to = public.join(file);
                if !to.exists() {
                    link_or_copy(&previous.join(PUBLIC).join(checked_name(file)?), &to)?;
                }
            }
            slots.push(slot.clone());
        }
    }
    for wanted in &job.request.material {
        let id = slug(&wanted.id);
        if id.is_empty() {
            continue;
        }
        match slots.iter().position(|slot| slot.id == id) {
            Some(index) => {
                let slot = &mut slots[index];
                if slot.path != wanted.path {
                    slot.path = wanted.path.clone();
                    slot.file = None;
                }
                if !wanted.label.trim().is_empty() {
                    slot.label = wanted.label.clone();
                }
            }
            None if slots.len() < MAX_SLOTS => slots.push(Slot {
                id,
                file: None,
                ..wanted.clone()
            }),
            None => {
                return Err(format!(
                    "At most {MAX_SLOTS} material spaces per infographic"
                ));
            }
        }
    }
    for slot in slots.iter_mut().filter(|slot| slot.file.is_none()) {
        let Some(path) = slot.path.clone() else {
            continue;
        };
        let meta = vv_media::probe_media(&path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        if !meta.has_video {
            return Err(format!("{} is not a photo or a video", path.display()));
        }
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > MAX_FILE_BYTES {
            return Err(format!("{} is larger than 1 GB", path.display()));
        }
        let image = meta.is_image();
        slot.kind = if image { Kind::Image } else { Kind::Video };
        slot.width = meta.width;
        slot.height = meta.height;
        slot.seconds = if image {
            0.0
        } else {
            meta.duration_frames as f64 / meta.fps.as_f64()
        };
        let extension = slug(
            &path
                .extension()
                .map(|ext| ext.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        // Never overwrite: earlier versions may share the file through a hard link.
        let mut name = format!("slot-{}.{extension}", slot.id);
        let mut index = 1;
        while public.join(&name).exists() {
            index += 1;
            name = format!("slot-{}-{index}.{extension}", slot.id);
        }
        link_or_copy(&path, &public.join(&name))?;
        slot.file = Some(name);
    }
    job.slots = slots;
    Ok(())
}

fn checked_name(file: &str) -> Result<&str, String> {
    if file.is_empty()
        || file.starts_with('.')
        || !file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        return Err("Invalid material file name".into());
    }
    Ok(file)
}

fn link_or_copy(from: &Path, to: &Path) -> Result<(), String> {
    if std::fs::hard_link(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|e| format!("Cannot copy {}: {e}", from.display()))
}

/// Previews the model sees, so it can point at details of a real screenshot.
pub fn attachments(job: &mut Job, directory: &Path) {
    let mut added = Vec::new();
    for slot in job.slots.iter().filter(|slot| slot.filled()) {
        let Some(path) = &slot.path else {
            continue;
        };
        let times: &[f64] = match slot.kind {
            Kind::Image => &[0.0],
            Kind::Video => &[0.2, 0.6],
        };
        for (index, fraction) in times.iter().enumerate() {
            let at = slot.seconds * fraction;
            match crate::ai_context::frame_png_sized(path, at, 1024) {
                Ok(bytes) => {
                    let file = format!("material-{}-{index}.png", slot.id);
                    if std::fs::write(directory.join(&file), bytes).is_err() {
                        continue;
                    }
                    added.push(Attachment {
                        file,
                        mime: "image/png".into(),
                        at_ms: 0,
                        description: match slot.kind {
                            Kind::Image => format!(
                                "The user's real photo for material slot \"{}\" ({}), {}x{} pixels",
                                slot.id, slot.label, slot.width, slot.height
                            ),
                            Kind::Video => format!(
                                "Frame at {at:.1} s of the user's real {:.1} s video for material slot \"{}\" ({})",
                                slot.seconds, slot.id, slot.label
                            ),
                        },
                    });
                }
                Err(error) => job
                    .events
                    .push(format!("Material preview unavailable: {error}")),
            }
        }
    }
    // First, so the per-request image limits keep them over timeline frames.
    added.append(&mut job.attachments);
    job.attachments = added;
}

pub fn prompt(job: &Job) -> String {
    let fps = job.request.composition.fps;
    let mut text = String::new();
    if !job.slots.is_empty() {
        let list: Vec<_> = job
            .slots
            .iter()
            .map(|slot| {
                serde_json::json!({
                    "id": slot.id,
                    "label": slot.label,
                    "kind": slot.kind,
                    "filled": slot.filled(),
                    "width": slot.width,
                    "height": slot.height,
                    "durationInFrames": duration_frames(slot, fps),
                })
            })
            .collect();
        text.push_str(&format!(
            "User material slots (the composition MUST show each of them): {}\n",
            serde_json::Value::from(list)
        ));
        let previews: Vec<_> = super::context::active_attachments(job)
            .filter(|attachment| attachment.file.starts_with("material-"))
            .map(|attachment| attachment.description.as_str())
            .collect();
        if !previews.is_empty() {
            text.push_str(&format!(
                "The first attached images, in order, preview the user's material: {}.\n",
                previews.join("; ")
            ));
        }
    }
    if job.request.ask_material {
        text.push_str("You may add up to 3 new material slots, but ONLY when the composition would otherwise have to invent something real and specific that only the user has: a screenshot of a real app or website step, a real document, a real product, place or person. Never for logos, icons, illustrations, charts or decorations: draw those yourself in the composition style. Most compositions need none. List new slots in material.\n");
    }
    if text.is_empty() {
        return text;
    }
    text.push_str("How slots work: the component receives props {slots, SlotMedia}. slots is keyed by slot id: {label, kind: 'image'|'video', filled, width, height, durationInFrames}. Render a slot with <SlotMedia id=\"...\" fit=\"cover\" or \"contain\" startFrom={0}>{placeholder}</SlotMedia> inside a box with explicit size and position: it fills the box with the user's media and shows the children only while the slot is empty. Design that placeholder in the composition style: a frame with the slot aspect ratio, a simple icon and the slot label, so the user sees where the material goes. Never draw a fake version of the material. A video slot plays muted from startFrom (frames), loops when it is shorter than the time it stays on screen and is cut when longer: when you can see its frames, pick startFrom to show its most relevant part. To annotate a filled photo (circles, arrows, highlights), give the box the photo aspect ratio and place annotations in fractions of the box, using the attached preview to locate the details.\n");
    text
}

fn duration_frames(slot: &Slot, fps: f64) -> Option<u32> {
    (slot.kind == Kind::Video && slot.seconds > 0.0).then(|| (slot.seconds * fps).round() as u32)
}

pub fn schema() -> serde_json::Value {
    serde_json::json!({
        "type": "ARRAY",
        "items": {
            "type": "OBJECT",
            "properties": {
                "id": {"type": "STRING"},
                "label": {"type": "STRING"},
                "kind": {"type": "STRING", "enum": ["image", "video"]},
                "reason": {"type": "STRING"}
            },
            "required": ["id", "label", "kind", "reason"]
        }
    })
}

pub const SCHEMA_HINT: &str = " Also return material: the new material slots you added (an empty list when none). Each has an id (short slug used in the source), a label telling the user what to upload in a few words of the user request language (for example 'Screenshot: bank login page'), kind image or video, and reason: one short plain sentence.";

/// Lenient: a malformed list only means no new spaces.
pub fn parse_requested(value: &serde_json::Value) -> Vec<Slot> {
    let Some(items) = value.get("material").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let field = |item: &serde_json::Value, name: &str| {
        item.get(name)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("")
            .to_owned()
    };
    let mut slots: Vec<Slot> = Vec::new();
    for item in items {
        let label: String = field(item, "label").chars().take(80).collect();
        if label.is_empty() {
            continue;
        }
        let mut id = slug(&field(item, "id"));
        if id.is_empty() || slots.iter().any(|slot| slot.id == id) {
            id = format!("space-{}", slots.len() + 1);
        }
        slots.push(Slot {
            id,
            label,
            kind: if field(item, "kind") == "video" {
                Kind::Video
            } else {
                Kind::Image
            },
            reason: field(item, "reason").chars().take(240).collect(),
            ..Slot::default()
        });
        if slots.len() == MAX_REQUESTED {
            break;
        }
    }
    slots
}

/// Keeps only the spaces the source actually renders.
pub fn merge_requested(job: &mut Job, requested: Vec<Slot>, source: &str) {
    for slot in requested {
        if job.slots.len() >= MAX_SLOTS || job.slots.iter().any(|known| known.id == slot.id) {
            continue;
        }
        if source.contains(&format!("\"{}\"", slot.id))
            || source.contains(&format!("'{}'", slot.id))
        {
            job.slots.push(slot);
        }
    }
}

/// Entry module: the scene gets the slot data and a ready media component.
pub fn entry(job: &Job) -> Result<String, String> {
    let config = &job.request.composition;
    let mut data = serde_json::Map::new();
    for slot in &job.slots {
        if let Some(file) = &slot.file {
            checked_name(file)?;
        }
        data.insert(
            slot.id.clone(),
            serde_json::json!({
                "label": slot.label,
                "kind": slot.kind,
                "file": slot.file,
                "width": slot.width,
                "height": slot.height,
                "durationInFrames": duration_frames(slot, config.fps),
            }),
        );
    }
    let data = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    Ok(format!(
        r#"import React from 'react';
import {{registerRoot, Composition, Img, OffthreadVideo, Loop, staticFile}} from 'remotion';
import Scene from './Scene';
const data = {data};
const slots = Object.fromEntries(Object.entries(data).map(([id, s]) => [id, {{label: s.label, kind: s.kind, filled: !!s.file, width: s.width, height: s.height, durationInFrames: s.durationInFrames}}]));
const Empty = ({{label}}) => <div style={{{{width: '100%', height: '100%', boxSizing: 'border-box', border: '4px dashed rgba(128,128,128,0.8)', borderRadius: 16, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center', padding: 16, color: 'rgba(128,128,128,1)', fontFamily: 'sans-serif', fontSize: 32}}}}>{{label}}</div>;
const SlotMedia = ({{id, fit = 'cover', startFrom = 0, style, children}}) => {{
  const slot = data[id];
  if (!slot || !slot.file) return children ?? <Empty label={{slot ? slot.label : id}}/>;
  const look = {{width: '100%', height: '100%', display: 'block', objectFit: fit, ...style}};
  if (slot.kind !== 'video') return <Img src={{staticFile(slot.file)}} style={{look}}/>;
  const from = Math.max(0, Math.round(startFrom || 0));
  const video = <OffthreadVideo src={{staticFile(slot.file)}} muted startFrom={{from}} style={{look}}/>;
  const length = (slot.durationInFrames || 0) - from;
  return length > 0 ? <Loop durationInFrames={{length}} layout="none">{{video}}</Loop> : video;
}};
const Root = () => <Scene slots={{slots}} SlotMedia={{SlotMedia}}/>;
registerRoot(() => <Composition id="Sequence" component={{Root}} width={{{}}} height={{{}}} fps={{{}}} durationInFrames={{{}}}/>);
"#,
        config.width, config.height, config.fps, config.frames
    ))
}

#[cfg(test)]
#[path = "../tests/ai_sequence/material.rs"]
mod tests;
