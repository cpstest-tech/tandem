use super::*;
use crate::ai_sequence::tests::{directory, local_runtime, request};
use crate::ai_sequence::{Job, Store, render};

fn png(path: &Path, rgba: [u8; 4], width: u32, height: u32) {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba.repeat((width * height) as usize))
            .unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

fn job_with(store: &Store, material: Vec<Slot>, parent: Option<String>) -> Job {
    let mut req = request();
    req.material = material;
    req.parent = parent;
    store.create(req).unwrap()
}

#[test]
fn requested_spaces_are_capped_deduplicated_and_sanitized() {
    let value = serde_json::json!({"material": [
        {"id": "Bank Login!", "label": "Screenshot: login", "kind": "image", "reason": "Real step"},
        {"id": "banklogin", "label": "Same id", "kind": "video", "reason": ""},
        {"id": "x", "label": "  ", "kind": "image", "reason": ""},
        {"id": "c", "label": "Third", "kind": "image", "reason": ""},
        {"id": "d", "label": "Fourth", "kind": "image", "reason": ""}
    ]});
    let slots = parse_requested(&value);
    assert_eq!(slots.len(), 3);
    assert_eq!(slots[0].id, "banklogin");
    assert_eq!(slots[1].id, "space-2");
    assert_eq!(slots[1].kind, Kind::Video);
    assert_eq!(slots[2].label, "Third");
    assert!(
        slots
            .iter()
            .all(|slot| !slot.filled() && slot.path.is_none())
    );
    assert!(parse_requested(&serde_json::json!({"material": "nope"})).is_empty());
}

#[test]
fn only_spaces_the_source_renders_are_kept() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let mut job = job_with(&store, Vec::new(), None);
    let requested = vec![
        Slot {
            id: "login".into(),
            label: "Login".into(),
            ..Slot::default()
        },
        Slot {
            id: "unused".into(),
            label: "Unused".into(),
            ..Slot::default()
        },
    ];
    merge_requested(&mut job, requested, "<SlotMedia id=\"login\"/>");
    assert_eq!(job.slots.len(), 1);
    assert_eq!(job.slots[0].id, "login");
    assert_eq!(empty_count(&job), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn files_are_copied_inherited_and_cleared() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let photo = root.join("photo.png");
    png(&photo, [255, 0, 0, 255], 40, 20);
    let mut first = job_with(
        &store,
        vec![Slot::for_file("material-1".into(), photo.clone(), "")],
        None,
    );
    let first_dir = store.directory(&first.id).unwrap();
    first.request.material.push(Slot {
        id: "later".into(),
        label: "Screenshot".into(),
        ..Slot::default()
    });
    prepare(&mut first, &first_dir, None).unwrap();
    assert_eq!(first.slots.len(), 2);
    let photo_slot = &first.slots[0];
    assert_eq!(photo_slot.label, "photo.png");
    assert_eq!(
        (photo_slot.kind, photo_slot.width, photo_slot.height),
        (Kind::Image, 40, 20)
    );
    let copied = first_dir
        .join("public")
        .join(photo_slot.file.as_ref().unwrap());
    assert_eq!(
        std::fs::read(&copied).unwrap(),
        std::fs::read(&photo).unwrap()
    );
    assert_eq!(empty_count(&first), 1);
    store.save(&first).unwrap();

    let screenshot = root.join("screen.png");
    png(&screenshot, [0, 0, 255, 255], 30, 60);
    let mut fill = job_with(
        &store,
        vec![Slot {
            id: "later".into(),
            path: Some(screenshot),
            ..Slot::default()
        }],
        None,
    );
    let fill_dir = store.directory(&fill.id).unwrap();
    prepare(&mut fill, &fill_dir, Some((&first, &first_dir))).unwrap();
    assert_eq!(empty_count(&fill), 0);
    assert_eq!(
        fill.slots[1].label, "Screenshot",
        "an empty label keeps the old one"
    );
    for slot in &fill.slots {
        assert!(
            fill_dir
                .join("public")
                .join(slot.file.as_ref().unwrap())
                .is_file()
        );
    }

    let mut clear = job_with(
        &store,
        vec![Slot {
            id: "material-1".into(),
            ..Slot::default()
        }],
        None,
    );
    let clear_dir = store.directory(&clear.id).unwrap();
    prepare(&mut clear, &clear_dir, Some((&fill, &fill_dir))).unwrap();
    assert!(!clear.slots[0].filled());
    assert!(clear.slots[1].filled());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn prompt_explains_spaces_only_when_there_are_any() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let mut job = job_with(&store, Vec::new(), None);
    assert!(prompt(&job).is_empty());
    job.request.ask_material = true;
    let asking = prompt(&job);
    assert!(asking.contains("Never for logos"));
    assert!(asking.contains("SlotMedia"));
    job.request.ask_material = false;
    job.slots.push(Slot {
        id: "login".into(),
        label: "Login".into(),
        kind: Kind::Video,
        seconds: 2.0,
        ..Slot::default()
    });
    let listed = prompt(&job);
    assert!(listed.contains("\"durationInFrames\":60"));
    assert!(!listed.contains("Never for logos"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn entry_rejects_unsafe_file_names() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let mut job = job_with(&store, Vec::new(), None);
    job.slots.push(Slot {
        id: "a".into(),
        file: Some("../escape.png".into()),
        ..Slot::default()
    });
    assert!(entry(&job).is_err());
    job.slots[0].file = Some("slot-a.png".into());
    let source = entry(&job).unwrap();
    assert!(source.contains("\"slot-a.png\""));
    assert!(source.contains("component={Root}"));
    std::fs::remove_dir_all(root).unwrap();
}

const SCENE: &str = r#"import React from 'react';
import {AbsoluteFill} from 'remotion';
export default function Scene({slots, SlotMedia}) {
  return <AbsoluteFill style={{backgroundColor: 'white'}}>
    <div style={{position: 'absolute', left: 0, top: 0, width: 160, height: 180}}>
      <SlotMedia id="photo" fit="cover"><div style={{width: '100%', height: '100%', background: 'black'}}/></SlotMedia>
    </div>
    <div style={{position: 'absolute', left: 160, top: 0, width: 80, height: 180}}>
      <SlotMedia id="clip" fit="cover" startFrom={2}/>
    </div>
    <div style={{position: 'absolute', left: 240, top: 0, width: 80, height: 180}}>
      <SlotMedia id="empty">{slots.empty.filled ? null : <div style={{width: '100%', height: '100%', background: 'rgb(0,0,255)'}}/>}</SlotMedia>
    </div>
  </AbsoluteFill>;
}
"#;

fn pixel(video: &Path, frame: u32, x: u32, y: u32) -> [u8; 3] {
    let output = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(video)
        .args([
            "-vf",
            &format!("select=eq(n\\,{frame})"),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let at = ((y * 320 + x) * 3) as usize;
    [
        output.stdout[at],
        output.stdout[at + 1],
        output.stdout[at + 2],
    ]
}

fn close(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(a, e)| (i16::from(*a) - i16::from(e)).abs() < 40)
}

#[test]
#[ignore = "Uses a real local Remotion runtime and ffmpeg"]
fn spaces_render_the_user_photo_and_a_looping_video() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let photo = root.join("photo.png");
    png(&photo, [255, 0, 0, 255], 64, 64);
    let clip = root.join("clip.mp4");
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x00ff00:s=64x64:r=30:d=0.3",
        ])
        .args(["-pix_fmt", "yuv420p"])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(made.success());
    let mut req = request();
    req.composition = crate::ai_sequence::Composition {
        width: 320,
        height: 180,
        fps: 30.0,
        frames: 20,
        transparent: false,
    };
    req.material = vec![
        Slot::for_file("photo".into(), photo, "Photo"),
        Slot::for_file("clip".into(), clip, "Clip"),
        Slot {
            id: "empty".into(),
            label: "Later".into(),
            ..Slot::default()
        },
    ];
    let mut job = store.create(req).unwrap();
    let work = store.directory(&job.id).unwrap();
    prepare(&mut job, &work, None).unwrap();
    assert_eq!(job.slots[1].kind, Kind::Video);
    job.source = Some(SCENE.into());
    let runtime = local_runtime();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    render::prepare(&job, &work).unwrap();
    let output = render::run(&runtime, &job, &work, false, &cancel).unwrap();
    // Frame 15 is past the 9-frame clip minus startFrom: it must have looped.
    for frame in [0, 15] {
        assert!(
            close(pixel(&output, frame, 80, 90), [255, 0, 0]),
            "photo at {frame}"
        );
        assert!(
            close(pixel(&output, frame, 200, 90), [0, 255, 0]),
            "video at {frame}"
        );
        assert!(
            close(pixel(&output, frame, 280, 90), [0, 0, 255]),
            "placeholder at {frame}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

fn wait(queue: &mut crate::ai_sequence::Queue, id: &str) -> Job {
    let start = std::time::Instant::now();
    loop {
        queue.poll();
        let job = queue.jobs.iter().find(|job| job.id == id).unwrap().clone();
        if job.stage.terminal() {
            return job;
        }
        assert!(start.elapsed().as_secs() < 900, "generation timed out");
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

#[test]
#[ignore = "Uses the authenticated Claude Code CLI and a real local Remotion runtime"]
fn claude_asks_for_a_real_screenshot_and_the_space_fills_without_the_model() {
    let root = directory();
    let project = root.join("project.vv");
    let mut queue = crate::ai_sequence::Queue::open(&project).unwrap();
    let runtime = local_runtime();
    let mut req = request();
    req.provider = crate::ai_sequence::Provider::Claude;
    req.model.clear();
    req.ask_material = true;
    req.timeline = None;
    req.insert = false;
    req.composition.width = 640;
    req.composition.height = 360;
    req.composition.frames = 90;
    req.composition.transparent = false;
    req.prompt = "Tutorial step 2 of opening an account in the MyBank app: on the home screen tap the green 'Open account' button at the bottom.".into();
    let id = queue.submit(req, runtime.clone()).unwrap();
    let job = wait(&mut queue, &id);
    assert_eq!(
        job.stage,
        crate::ai_sequence::Stage::Completed,
        "{:?}",
        job.error
    );
    assert!(
        !job.slots.is_empty(),
        "no space requested for the real app screen"
    );
    println!(
        "requested: {:?}",
        job.slots.iter().map(|s| &s.label).collect::<Vec<_>>()
    );

    let screenshot = root.join("screen.png");
    png(&screenshot, [0, 200, 0, 255], 360, 640);
    let mut fill = job.request.clone();
    fill.parent = Some(job.id.clone());
    fill.render_only = true;
    fill.prompt = "fill".into();
    fill.material = vec![Slot {
        id: job.slots[0].id.clone(),
        path: Some(screenshot),
        ..Slot::default()
    }];
    let fill_id = queue.submit(fill, runtime).unwrap();
    let filled = wait(&mut queue, &fill_id);
    assert_eq!(
        filled.stage,
        crate::ai_sequence::Stage::Completed,
        "{:?}",
        filled.error
    );
    assert!(filled.slots[0].filled());
    assert_eq!(
        filled.usage_history.len(),
        0,
        "filling must not call the model"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Uses the authenticated Claude Code CLI and a real local Remotion runtime"]
fn claude_draws_a_logo_itself_instead_of_asking_for_it() {
    let root = directory();
    let mut queue = crate::ai_sequence::Queue::open(&root.join("project.vv")).unwrap();
    let mut req = request();
    req.provider = crate::ai_sequence::Provider::Claude;
    req.model.clear();
    req.ask_material = true;
    req.timeline = None;
    req.insert = false;
    req.composition.width = 640;
    req.composition.height = 360;
    req.composition.frames = 60;
    req.prompt = "Intro card: the logo of our note-taking app Inkwell appears with the tagline 'Write it down'.".into();
    let id = queue.submit(req, local_runtime()).unwrap();
    let job = wait(&mut queue, &id);
    assert_eq!(
        job.stage,
        crate::ai_sequence::Stage::Completed,
        "{:?}",
        job.error
    );
    assert!(job.slots.is_empty(), "asked for {:?}", job.slots);
    std::fs::remove_dir_all(root).unwrap();
}
