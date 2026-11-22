use super::*;

pub(super) fn request() -> Request {
    Request {
        prompt: "Animated bars".into(),
        context: String::new(),
        style: String::new(),
        placement: Placement::Fullscreen,
        media: Vec::new(),
        material: Vec::new(),
        ask_material: false,
        render_only: false,
        provider: Provider::Gemini,
        api_access: Default::default(),
        model: "gemini-4-hydrogen".into(),
        limits: Default::default(),
        composition: Composition {
            width: 1920,
            height: 1080,
            fps: 30.0,
            frames: 150,
            transparent: true,
        },
        parent: None,
        reuse_session: true,
        insert: true,
        auto_duration: false,
        timeline: Some(1),
        anchor_clip: None,
        anchor_details: None,
        anchor_offset: 0,
        start: 12,
    }
}

pub(super) fn directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "sequence-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    path
}

#[test]
fn restart_preserves_completed_sources_and_marks_interrupted_jobs_failed() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let interrupted = store.create(request()).unwrap();
    let mut completed = store.create(request()).unwrap();
    completed.stage = Stage::Completed;
    completed.source = Some("export default () => null".into());
    store.save(&completed).unwrap();
    let recovered = store.recover().unwrap();
    assert_eq!(
        recovered
            .iter()
            .find(|j| j.id == interrupted.id)
            .unwrap()
            .stage,
        Stage::Failed
    );
    assert_eq!(store.load(&completed.id).unwrap().source, completed.source);
    assert_eq!(store.load(&completed.id).unwrap().stage, Stage::Completed);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn revisions_require_a_completed_parent_and_preserve_its_source() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let mut parent = store.create(request()).unwrap();
    let mut revision = request();
    revision.parent = Some(parent.id.clone());
    assert!(store.create(revision.clone()).is_err());
    parent.stage = Stage::Completed;
    parent.source = Some("export default () => null".into());
    store.save(&parent).unwrap();
    let child = store.create(revision).unwrap();
    assert_eq!(child.request.parent.as_deref(), Some(parent.id.as_str()));
    assert_eq!(store.load(&parent.id).unwrap().source, parent.source);
    for id in ["../outside", "", "foo/bar", "foo.bar"] {
        assert!(store.directory(id).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_render_parameters_are_rejected_before_persistence() {
    let mut config = request().composition;
    config.fps = f64::NAN;
    assert!(config.validate().is_err());
    config.fps = 30.0;
    config.frames = 0;
    assert!(config.validate().is_err());
    config.frames = 1;
    assert!(config.validate().is_ok());
}

#[test]
fn source_parser_rejects_descriptions_and_missing_exports() {
    assert!(provider::parse_source("Here is your composition").is_err());
    assert!(provider::parse_source(r#"{"source":"const X = 1"}"#).is_err());
    assert_eq!(
        provider::parse_source(r#"{"source":"export default () => null"}"#).unwrap(),
        "export default () => null"
    );
}

#[cfg(unix)]
#[test]
fn process_timeout_kills_descendants_holding_output_pipes() {
    let started = std::time::Instant::now();
    let result = process::run(
        std::process::Command::new("sh").args(["-c", "sleep 30 & wait"]),
        &std::sync::atomic::AtomicBool::new(false),
        std::time::Duration::from_millis(100),
    );
    assert!(result.unwrap_err().contains("timed out"));
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
}

#[cfg(unix)]
#[test]
fn process_cancellation_terminates_running_work() {
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let signal = cancel.clone();
    let worker = std::thread::spawn(move || {
        process::run(
            std::process::Command::new("sh").args(["-c", "sleep 30"]),
            &signal,
            std::time::Duration::from_secs(60),
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(worker.join().unwrap().unwrap_err(), "Cancelled");
}

#[test]
#[ignore = "Requires the installed local Remotion runtime and Linux namespace permissions"]
fn remotion_render_smoke() {
    let root = directory();
    let runtime = local_runtime();
    let store = Store::new(&root).unwrap();
    let mut req = request();
    req.composition.width = 320;
    req.composition.height = 180;
    req.composition.frames = 5;
    req.composition.transparent = false;
    let mut job = store.create(req).unwrap();
    job.source = Some("import React from 'react'; import {AbsoluteFill, useCurrentFrame} from 'remotion'; export default () => <AbsoluteFill style={{backgroundColor:'transparent',color:'white'}}>{useCurrentFrame()}</AbsoluteFill>;".into());
    let directory = store.directory(&job.id).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    render::preflight(&runtime, &cancel).unwrap();
    render::prepare(&job, &directory).unwrap();
    render::run(&runtime, &job, &directory, true, &cancel).unwrap();
    let output = render::run(&runtime, &job, &directory, false, &cancel).unwrap();
    assert!(std::fs::metadata(output).unwrap().len() > 100);
    job.request.composition.transparent = true;
    render::prepare(&job, &directory).unwrap();
    let alpha_output = render::run(&runtime, &job, &directory, false, &cancel).unwrap();
    assert_eq!(alpha_output.extension().unwrap(), "mov");
    assert!(std::fs::metadata(&alpha_output).unwrap().len() > 100);
    assert!(vv_media::has_alpha(&alpha_output));
    let mut decoder = vv_media::Decoder::open(&alpha_output).unwrap();
    let (_, frame) = decoder.next_frame().unwrap().unwrap();
    let alpha = frame
        .alpha
        .as_ref()
        .expect("Transparent Remotion output must keep its alpha plane in the editor decoder");
    assert!(
        alpha.contains(&0),
        "Background must remain transparent after import decoding"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn applying_revision_preserves_clip_edits_and_supports_undo_and_conflicts() {
    use vv_core::*;
    let mut project = Project::default();
    let timeline = project.timelines.insert(Timeline {
        name: "Sequence".into(),
        fps: Rational::new(30, 1),
        resolution: (1920, 1080),
        tracks: vec![Track::new(TrackKind::Video)],
        markers: Vec::new(),
        master: Default::default(),
    });
    let original = project.media_pool.insert(MediaItem {
        path: "original.mp4".into(),
        meta: MediaMeta {
            duration_frames: 150,
            fps: Rational::new(30, 1),
            width: 1920,
            height: 1080,
            has_video: true,
            has_audio: false,
            sample_rate: 0,
            channels: 0,
            audio_streams: 0,
            file: Default::default(),
        },
        content_hash: 1,
        compound: None,
        folder: None,
    });
    let mut revised = project.media_pool[original].clone();
    revised.path = "revision.mp4".into();
    revised.content_hash = 2;
    let replacement = project.media_pool.insert(revised);
    let clip_id = project.alloc_clip_id();
    let mut clip = Clip::from_source_range(
        clip_id,
        ClipSource::Media(original),
        12,
        100,
        400,
        Rational::one(),
    );
    clip.fade_in = 5;
    clip.disabled = true;
    project.timelines[timeline].tracks[0]
        .clips
        .push(clip.clone());
    let command =
        revision::replacement(&project, timeline, clip_id, original, replacement).unwrap();
    let mut history = History::default();
    history.do_command(&mut project, command);
    let changed = project.timelines[timeline].clip(0, clip_id).unwrap();
    assert!(matches!(changed.source,ClipSource::Media(id) if id == replacement));
    assert_eq!(changed.timeline_start, 400);
    assert_eq!(changed.timeline_len, clip.timeline_len);
    assert_eq!(changed.source_offset, clip.source_offset);
    assert_eq!(changed.fade_in, 5);
    assert!(changed.disabled);
    assert!(revision::replacement(&project, timeline, clip_id, original, replacement).is_err());
    history.undo(&mut project);
    assert!(
        matches!(project.timelines[timeline].clip(0,clip_id).unwrap().source,ClipSource::Media(id) if id == original)
    );
    history.redo(&mut project);
    assert!(
        matches!(project.timelines[timeline].clip(0,clip_id).unwrap().source,ClipSource::Media(id) if id == replacement)
    );
    project.media_pool[original].meta.duration_frames = 10;
    assert!(revision::replacement(&project, timeline, clip_id, replacement, original).is_err());
    project.media_pool[original].meta.duration_frames = 150;
    project.timelines[timeline].tracks[0].locked = true;
    assert!(revision::replacement(&project, timeline, clip_id, replacement, original).is_err());
}

#[test]
fn visual_analysis_requires_valid_ranges_and_does_not_auto_select_results() {
    let valid = r#"{"suggestions":[{"start_ms":1000,"end_ms":4000,"description":"Explain the CPU pipeline","reason":"The narration describes sequential stages"}]}"#;
    let result = suggestions::parse(valid, 5000).unwrap();
    assert_eq!(result.len(), 1);
    assert!(!result[0].selected);
    assert!(suggestions::parse(valid, 3000).is_err());
    assert!(
        suggestions::parse(
            r#"{"suggestions":[{"start_ms":4000,"end_ms":1000,"description":"x","reason":"y"}]}"#,
            5000
        )
        .is_err()
    );
    assert!(
        suggestions::parse(r#"{"suggestions":[]}"#, 5000)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn accepted_suggestion_rebases_captions_and_keeps_a_stable_timeline_anchor() {
    let mut base = request();
    base.context = r#"[{"start_ms":0,"end_ms":800,"text":"Before"},{"start_ms":1200,"end_ms":1800,"text":"CPU"},{"start_ms":4200,"end_ms":4800,"text":"After"}]"#.into();
    base.anchor_clip = Some(42);
    let suggestion = suggestions::Suggestion {
        start_ms: 1000,
        end_ms: 4000,
        description: "Show a CPU diagram".into(),
        reason: "Explain stages".into(),
        material: Vec::new(),
        selected: true,
    };
    let generated =
        suggestions::generation_request(&base, &suggestion, Provider::Codex, String::new())
            .unwrap();
    assert_eq!(generated.provider, Provider::Codex);
    assert_eq!(generated.start, 42);
    assert_eq!(generated.anchor_offset, 30);
    assert_eq!(generated.anchor_clip, Some(42));
    assert_eq!(generated.composition.frames, 90);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&generated.context).unwrap(),
        serde_json::json!([{"start_ms":200,"end_ms":800,"text":"CPU"}])
    );
    assert!(generated.parent.is_none());
}

#[test]
fn corrupt_manifests_do_not_hide_valid_jobs_and_interrupted_writes_are_recoverable() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let valid = store.create(request()).unwrap();
    std::fs::create_dir(root.join("corrupt")).unwrap();
    std::fs::write(root.join("corrupt/job.json"), "{broken").unwrap();
    std::fs::write(root.join("corrupt/job.pending"), "unfinished write").unwrap();
    let (jobs, warnings) = store.recover_report().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, valid.id);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("corrupt"));
    assert!(root.join("corrupt/job.json").is_file());
    assert!(!root.join("corrupt/job.pending").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn storage_rejects_symlinks_and_a_second_queue_owner() {
    let root = directory();
    let external = directory();
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&external, &alias).unwrap();
    assert!(Store::new(&alias).is_err());
    let store = Store::new(&root).unwrap();
    let job = store.create(request()).unwrap();
    let file = store.directory(&job.id).unwrap().join("job.json");
    let backup = store.directory(&job.id).unwrap().join("backup.json");
    std::fs::rename(&file, &backup).unwrap();
    std::os::unix::fs::symlink(&backup, &file).unwrap();
    assert!(store.load(&job.id).is_err());
    let owner = store.exclusive().unwrap();
    assert!(store.exclusive().is_err());
    drop(owner);
    assert!(store.exclusive().is_ok());
    std::fs::remove_dir_all(root).unwrap();
    std::fs::remove_dir_all(external).unwrap();
}

#[test]
fn separate_project_files_have_separate_source_archives_and_copy_never_overwrites() {
    let root = directory();
    let first = root.join("first.tandem");
    let second = root.join("second.tandem");
    let source = portable::directory(&first);
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("source.tsx"), "original").unwrap();
    assert_ne!(source, portable::directory(&second));
    assert!(portable::copy(&first, &second).unwrap());
    assert_eq!(
        std::fs::read_to_string(portable::directory(&second).join("source.tsx")).unwrap(),
        "original"
    );
    std::fs::write(portable::directory(&second).join("source.tsx"), "user edit").unwrap();
    assert!(portable::copy(&first, &second).is_err());
    assert_eq!(
        std::fs::read_to_string(portable::directory(&second).join("source.tsx")).unwrap(),
        "user edit"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn insertion_follows_a_moved_reference_and_rejects_trimmed_or_replaced_sources() {
    use vv_core::*;
    let media = MediaId::from_raw(7);
    let mut timeline = Timeline {
        name: "T".into(),
        fps: Rational::new(30, 1),
        resolution: (1920, 1080),
        tracks: vec![Track::new(TrackKind::Video)],
        markers: Vec::new(),
        master: Default::default(),
    };
    timeline.tracks[0].clips.push(Clip::from_source_range(
        ClipId(42),
        ClipSource::Media(media),
        12,
        112,
        400,
        Rational::one(),
    ));
    let mut req = request();
    req.anchor_clip = Some(42);
    req.anchor_offset = 30;
    req.anchor_details = Some(Anchor {
        source_frame: 12,
        rate: Rational::one(),
        media: Some(7),
    });
    assert_eq!(insertion::position(&timeline, &req).unwrap(), 430);
    timeline.tracks[0].clips[0].timeline_start = 500;
    assert_eq!(insertion::position(&timeline, &req).unwrap(), 530);
    timeline.tracks[0].clips[0].source_offset = 20;
    assert!(insertion::position(&timeline, &req).is_err());
    timeline.tracks[0].clips[0].source_offset = 12;
    timeline.tracks[0].clips[0].source = ClipSource::Media(MediaId::from_raw(8));
    assert!(insertion::position(&timeline, &req).is_err());
    timeline.tracks[0].clips.clear();
    assert!(insertion::position(&timeline, &req).is_err());
}

pub(super) fn local_runtime() -> Runtime {
    let runtime_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/remotion-runtime")
        .canonicalize()
        .unwrap();
    let node = std::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(node.status.success());
    let browser = std::process::Command::new("node").current_dir(&runtime_directory).args(["-e", "require('@remotion/renderer').ensureBrowser({chromeMode:'headless-shell'}).then(b=>console.log(b.path))"]).output().unwrap();
    assert!(
        browser.status.success(),
        "{}",
        String::from_utf8_lossy(&browser.stderr)
    );
    Runtime {
        directory: runtime_directory,
        node: PathBuf::from(String::from_utf8(node.stdout).unwrap().trim()),
        browser: PathBuf::from(String::from_utf8(browser.stdout).unwrap().trim()),
    }
}

fn cli_render_smoke(provider: Provider) {
    let root = directory();
    let runtime = local_runtime();
    let store = Store::new(&root).unwrap();
    let mut req = request();
    req.provider = provider;
    req.model.clear();
    req.composition.width = 320;
    req.composition.height = 180;
    req.composition.frames = 5;
    let mut job = store.create(req).unwrap();
    let directory = store.directory(&job.id).unwrap();
    let cancel = std::sync::atomic::AtomicBool::new(false);
    render::preflight(&runtime, &cancel).unwrap();
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let pixels = [255, 0, 0, 255].repeat(16 * 16);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    std::fs::write(directory.join("context-frame-0.png"), png).unwrap();
    job.attachments.push(context::Attachment {
        file: "context-frame-0.png".into(),
        mime: "image/png".into(),
        at_ms: 0,
        description: "Synthetic color reference".into(),
    });
    let output = provider::adapter(provider).generate(&job,"Return ONLY JSON with one field source containing a complete TSX default React component. It must use React and Remotion AbsoluteFill and useCurrentFrame to animate a red circle moving horizontally on a transparent background. Only import react and remotion. No tools, filesystem access, external assets, network or dependencies. Do not register a Remotion root or Composition.",&directory,&cancel).unwrap();
    job.session = output.session;
    assert!(
        job.session.is_some(),
        "CLI did not report a resumable session"
    );
    let first_session = job.session.clone();
    job.source = Some(output.source);
    let revision = provider::adapter(provider).generate(&job, "Return the same JSON source schema. Revise the previous component by making the circle blue. Keep its React default export and frame animation. Do not use tools or external dependencies.", &directory, &cancel).unwrap();
    assert!(revision.session.is_some());
    assert_ne!(
        revision.session, first_session,
        "A revision should fork the original session"
    );
    job.source = Some(revision.source);
    render::prepare(&job, &directory).unwrap();
    let rendered = render::run(&runtime, &job, &directory, false, &cancel).unwrap();
    assert!(std::fs::metadata(rendered).unwrap().len() > 100);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "Uses the authenticated Codex CLI and a real local Remotion runtime"]
fn cli_codex_render_smoke() {
    cli_render_smoke(Provider::Codex);
}

#[test]
#[ignore = "Uses the authenticated Claude Code CLI and a real local Remotion runtime"]
fn cli_claude_render_smoke() {
    cli_render_smoke(Provider::Claude);
}

#[test]
#[ignore = "Uses the authenticated OpenCode CLI and its configured model"]
fn cli_opencode_render_smoke() {
    cli_render_smoke(Provider::OpenCode);
}

#[test]
fn cli_protocols_recover_structured_output_sessions_events_and_usage() {
    let source = serde_json::json!({"source":"export default () => null"}).to_string();
    let codex = [
        serde_json::json!({"type":"thread.started","thread_id":"abc-123"}),
        serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":source}}),
        serde_json::json!({"type":"turn.completed","usage":{"input_tokens":12,"output_tokens":8}}),
    ]
    .into_iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join("\n");
    let reply = provider::parse_events(&codex).unwrap();
    assert_eq!(reply.session.as_deref(), Some("abc-123"));
    assert_eq!(reply.usage.unwrap()["input_tokens"], 12);
    assert_eq!(
        reply.events,
        vec!["thread.started", "item.completed", "turn.completed"]
    );
    assert!(provider::parse_source(&reply.text).is_ok());
    let claude = serde_json::json!({"type":"result","session_id":"claude-123","result":"Human description","structured_output":{"source":"export default () => null"},"usage":{"input_tokens":6}}).to_string();
    let reply = provider::parse_events(&claude).unwrap();
    assert!(provider::parse_source(&reply.text).is_ok());
    assert_eq!(reply.session.as_deref(), Some("claude-123"));
    let opencode =
        serde_json::json!({"type":"text","sessionID":"ses_123","part":{"text":source}}).to_string();
    assert_eq!(
        provider::parse_events(&opencode)
            .unwrap()
            .session
            .as_deref(),
        Some("ses_123")
    );
    assert!(
        provider::parse_events(r#"{"type":"turn.failed","error":{"message":"Quota exhausted"}}"#)
            .unwrap_err()
            .contains("Quota exhausted")
    );
    for id in ["", "--unsafe", "../session", "id with spaces"] {
        assert!(!provider::valid_session(id));
    }
}

#[test]
fn cli_json_schema_uses_standard_types_without_changing_property_names() {
    let schema = serde_json::json!({"type":"OBJECT","properties":{"type":{"type":"STRING"},"values":{"type":"ARRAY","items":{"type":"OBJECT","properties":{"title":{"type":"STRING"}},"required":["title"]}}},"required":["type","values"]});
    let converted = provider::json_schema(&schema);
    assert_eq!(converted["type"], "object");
    assert_eq!(converted["properties"]["type"]["type"], "string");
    assert_eq!(converted["additionalProperties"], false);
    assert_eq!(
        converted["properties"]["values"]["items"]["additionalProperties"],
        false
    );
    assert_eq!(schema["type"], "OBJECT");
}

#[test]
fn source_audio_context_seeks_and_crops_without_sending_the_whole_media() {
    let root = directory();
    let file = root.join("source.wav");
    let mut samples = vec![0.0f32; 16_000];
    samples.extend(vec![0.5; 16_000]);
    samples.extend(vec![-0.5; 16_000]);
    std::fs::write(&file, context::pcm_wav(&samples, 16_000)).unwrap();
    let excerpt = context::audio_wav(
        &file,
        1.0,
        0.25,
        0,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(&excerpt[..4], b"RIFF");
    assert_eq!(excerpt.len(), 44 + 4_000 * 2);
    let values: Vec<_> = excerpt[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    assert!(
        values
            .iter()
            .all(|sample| *sample > 16_000 && *sample < 17_000)
    );
    assert!(
        context::audio_wav(
            &file,
            0.0,
            31.0,
            0,
            &std::sync::atomic::AtomicBool::new(false)
        )
        .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn multimodal_context_is_bounded_local_and_not_resent_for_repairs_or_revisions() {
    let root = directory();
    let store = Store::new(&root).unwrap();
    let mut job = store.create(request()).unwrap();
    let directory = store.directory(&job.id).unwrap();
    std::fs::write(directory.join("context-frame-0.png"), b"test image").unwrap();
    job.attachments.push(context::Attachment {
        file: "context-frame-0.png".into(),
        mime: "image/png".into(),
        at_ms: 50,
        description: "Source frame".into(),
    });
    let parts = context::inline_parts(&job, &directory).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1]["inlineData"]["mimeType"], "image/png");
    assert!(parts[0]["text"].as_str().unwrap().contains("50 ms"));
    job.attempt = 2;
    assert!(context::inline_parts(&job, &directory).unwrap().is_empty());
    job.attempt = 1;
    job.request.parent = Some("previous".into());
    assert!(context::inline_parts(&job, &directory).unwrap().is_empty());
    job.attachments[0].file = "../secret.key".into();
    assert!(context::attachment_path(&directory, &job.attachments[0]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn suggestion_audio_and_frames_are_cropped_and_rebased_to_the_accepted_range() {
    let sources = vec![
        context::Source::Frame {
            path: "video.mp4".into(),
            source_seconds: 10.0,
            at_ms: 1500,
        },
        context::Source::Frame {
            path: "video.mp4".into(),
            source_seconds: 12.0,
            at_ms: 4000,
        },
        context::Source::Audio {
            path: "audio.wav".into(),
            source_seconds: 10.0,
            source_duration: 6.0,
            at_ms: 0,
            duration_ms: 3000,
            stream: 1,
        },
    ];
    let cropped = context::subrange(&sources, 1000, 2000);
    assert_eq!(cropped.len(), 2);
    assert!(matches!(
        &cropped[0],
        context::Source::Frame { at_ms: 500, .. }
    ));
    assert!(
        matches!(&cropped[1],context::Source::Audio {source_seconds,source_duration,at_ms:0,duration_ms:1000,stream:1,..} if *source_seconds==12.0 && *source_duration==2.0)
    );
}

#[cfg(unix)]
#[test]
fn provider_input_is_written_without_shell_interpolation() {
    let input = b"JSON input with $HOME and `commands`\n".to_vec();
    let output = process::run_with_input(
        &mut std::process::Command::new("cat"),
        &std::sync::atomic::AtomicBool::new(false),
        std::time::Duration::from_secs(2),
        Some(input.clone()),
    )
    .unwrap();
    assert_eq!(output.as_bytes(), &input);
}

#[test]
fn saved_visual_analysis_is_reused_without_starting_a_provider_request() {
    let root = directory();
    let mut req = request();
    req.context = r#"[{"start_ms":0,"end_ms":1000,"text":"Explain CPU stages"}]"#.into();
    let workspace = suggestions::workspace(&root, &req).unwrap();
    std::fs::create_dir(&workspace).unwrap();
    let report = suggestions::Report {
        request: req.clone(),
        suggestions: vec![suggestions::Suggestion {
            start_ms: 100,
            end_ms: 1200,
            description: "CPU pipeline diagram".into(),
            reason: "Clarifies stages".into(),
            material: Vec::new(),
            selected: true,
        }],
        usage: Some(serde_json::json!({"totalTokenCount":100})),
        events: vec!["generateContent: STOP".into()],
        cached: false,
    };
    suggestions::save_report(&workspace, &report).unwrap();
    let analysis = suggestions::Analysis::start(req.clone(), root.clone(), false).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let result = loop {
        if let Some(result) = analysis.poll() {
            break result.unwrap();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Saved analysis was not returned"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(result.cached);
    assert_eq!(result.suggestions.len(), 1);
    assert!(!result.suggestions[0].selected);
    assert_eq!(result.usage.unwrap()["totalTokenCount"], 100);
    req.model = "different-model".into();
    assert_ne!(suggestions::workspace(&root, &req).unwrap(), workspace);
    drop(analysis);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn code_repairs_do_not_retry_environment_failures_or_unbounded_limits() {
    assert!(render::repairable(
        "TANDEM_COMPOSITION_ERROR: ReferenceError: missingVariable is not defined"
    ));
    assert!(!render::repairable(
        "TANDEM_RENDER_ENVIRONMENT_ERROR: ENOSPC"
    ));
    assert!(!render::repairable("Local process timed out"));
    let mut limits = Limits::default();
    assert!(limits.validate().is_ok());
    limits.max_attempts = 0;
    assert!(limits.validate().is_err());
    limits.max_attempts = 6;
    assert!(limits.validate().is_err());
    limits.max_attempts = 1;
    limits.provider_timeout_secs = 0;
    assert!(limits.validate().is_err());
}

#[test]
#[ignore = "Uses the configured shared Gemini key, Gemini 4 Hydrogen and real local rendering"]
fn gemini_sequence_workflow_smoke() {
    let root = directory();
    let runtime = local_runtime();
    let project = root.join("gemini.tandem");
    let mut queue = Queue::open(&project).unwrap();
    let mut req = request();
    req.api_access = ApiAccess::FreeProject {
        key_identity: crate::gemini::key_identity(&crate::gemini::configured_key().unwrap()),
    };
    req.composition.transparent = false;
    req.composition.width = 320;
    req.composition.height = 180;
    req.composition.frames = 5;
    req.limits.max_attempts = 2;
    req.prompt="Animate a red circle moving horizontally on a transparent background, with a short CPU title. Use only shapes and React/Remotion, no external assets.".into();
    let image = root.join("source.png");
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 16, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 0, 0, 255].repeat(16 * 16))
            .unwrap();
    }
    std::fs::write(&image, bytes).unwrap();
    let audio = root.join("source.wav");
    std::fs::write(&audio, context::pcm_wav(&vec![0.1; 2400], 16_000)).unwrap();
    req.media = vec![
        context::Source::Frame {
            path: image,
            source_seconds: 0.0,
            at_ms: 0,
        },
        context::Source::Audio {
            path: audio,
            source_seconds: 0.0,
            source_duration: 0.15,
            at_ms: 0,
            duration_ms: 150,
            stream: 0,
        },
    ];
    let id = queue.submit(req, runtime.clone()).unwrap();
    let wait = |queue: &mut Queue, id: &str| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(900);
        loop {
            queue.poll();
            let job = queue.jobs.iter().find(|job| job.id == id).unwrap();
            if job.stage.terminal() {
                assert_eq!(job.stage, Stage::Completed, "{:?}", job.error);
                return job.clone();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Gemini sequence did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let original = wait(&mut queue, &id);
    assert!(original.source.as_ref().unwrap().contains("export default"));
    assert!(original.usage.is_some());
    assert!(
        original.request.composition.transparent,
        "The natural-language transparency request must update the render settings"
    );
    let mut revision = original.request.clone();
    revision.parent = Some(original.id.clone());
    revision.prompt =
        "Change only the circle's color from red to blue; preserve the motion and title.".into();
    let revised_id = queue.submit(revision, runtime).unwrap();
    let revised = wait(&mut queue, &revised_id);
    assert!(queue.directory(&id).unwrap().join("result.mov").is_file());
    assert!(
        queue
            .directory(&revised.id)
            .unwrap()
            .join("result.mov")
            .is_file()
    );
    assert_ne!(original.source, revised.source);
    drop(queue);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn gemini_free_only_access_is_key_bound_and_cannot_be_loaded_from_a_project() {
    let key = "synthetic-key-for-policy-test";
    let access = ApiAccess::FreeProject {
        key_identity: crate::gemini::key_identity(key),
    };
    assert!(access.check(key, "gemini-4-hydrogen").is_ok());
    assert!(access.check(key, "gemini-4-carbon").is_err());
    assert!(access.check(key, "gemini-unknown").is_err());
    assert!(access.check("changed-key", "gemini-4-hydrogen").is_err());
    let mut req = request();
    req.api_access = access;
    let serialized = serde_json::to_string(&req).unwrap();
    assert!(!serialized.contains("api_access"));
    assert!(!serialized.contains("key_identity"));
    let restored: Request = serde_json::from_str(&serialized).unwrap();
    assert!(restored.api_access.check(key, "gemini-4-hydrogen").is_err());
    let paid = ApiAccess::ConfiguredBilling {
        key_identity: crate::gemini::key_identity(key),
    };
    assert!(paid.check(key, "gemini-4-carbon").is_ok());
}

#[test]
fn transparency_revisions_control_the_render_format_and_reject_invalid_flags() {
    assert_eq!(
        provider::parse_transparency(
            r#"{"source":"export default () => null","transparent":true}"#
        )
        .unwrap(),
        Some(true)
    );
    assert_eq!(
        provider::parse_transparency(
            r#"{"source":"export default () => null","transparent":false}"#
        )
        .unwrap(),
        Some(false)
    );
    assert!(
        provider::parse_transparency(
            r#"{"source":"export default () => null","transparent":"yes"}"#
        )
        .is_err()
    );
}

#[test]
fn provider_duration_is_parsed_only_when_positive() {
    use super::provider::parse_duration;
    assert_eq!(parse_duration(r#"{"duration_seconds":4.5}"#), Some(4.5));
    assert_eq!(parse_duration(r#"{"duration_seconds":0}"#), None);
    assert_eq!(parse_duration(r#"{"duration_seconds":"x"}"#), None);
    assert_eq!(parse_duration("{}"), None);
}

#[test]
fn ai_chosen_length_is_clamped_between_one_second_and_the_maximum() {
    let mut request = request();
    request.auto_duration = true;
    for (seconds, frames) in [(4.0, 120), (0.2, 30), (99.0, 150)] {
        request.composition.frames = 150;
        queue::apply_auto_duration(&mut request, Some(seconds));
        assert_eq!(request.composition.frames, frames, "{seconds} s");
    }
    request.composition.frames = 150;
    queue::apply_auto_duration(&mut request, None);
    assert_eq!(request.composition.frames, 150);
    request.auto_duration = false;
    queue::apply_auto_duration(&mut request, Some(2.0));
    assert_eq!(request.composition.frames, 150);
}

#[test]
fn jobs_saved_before_placement_existed_load_as_fullscreen() {
    let mut value = serde_json::to_value(request()).unwrap();
    value.as_object_mut().unwrap().remove("placement");
    let loaded: Request = serde_json::from_value(value).unwrap();
    assert_eq!(loaded.placement, Placement::Fullscreen);
    assert!(!Placement::Fullscreen.is_overlay());
    assert!(Placement::Pill.is_overlay() && Placement::AboveHead.is_overlay());
    assert!(Placement::Side.is_overlay());
    assert!(Placement::Side.instructions().contains("Side-by-side"));
}

#[test]
fn improvements_are_parsed_leniently_deduplicated_and_capped_at_three() {
    let text = r#"{"source":"x","suggestions":[
        {"id":"a b!","title":"Bigger code","description":"Make the code larger.","instruction":"Enlarge the code block"},
        {"id":"","title":"bigger code","description":"Same title","instruction":"Other"},
        {"id":"x","title":"","description":"No title","instruction":"Nothing"},
        {"id":"two","title":"Fade in","description":"Items appear one at a time.","instruction":"Fade the items in one by one"},
        {"id":"two","title":"Contrast","description":"Easier to read.","instruction":"Raise the contrast"},
        {"id":"four","title":"Fourth","description":"Extra","instruction":"Extra"}]}"#;
    let ideas = provider::parse_improvements(text);
    assert_eq!(ideas.len(), 3);
    assert_eq!(ideas[0].id, "ab");
    assert_eq!(ideas[1].title, "Fade in");
    assert_eq!(ideas[2].id, "idea-3");
    assert_eq!(ideas[2].instruction, "Raise the contrast");
}

#[test]
fn missing_or_malformed_improvements_mean_no_suggestions() {
    for text in [
        "not json",
        r#"{"source":"x"}"#,
        r#"{"suggestions":"x"}"#,
        r#"{"suggestions":[{"title":"Only a title"}]}"#,
    ] {
        assert!(provider::parse_improvements(text).is_empty(), "{text}");
    }
}

#[test]
fn jobs_saved_before_improvements_existed_load_without_them() {
    let store = Store::new(&directory()).unwrap();
    let job = store.create(request()).unwrap();
    let mut value = serde_json::to_value(&job).unwrap();
    value.as_object_mut().unwrap().remove("improvements");
    let loaded: Job = serde_json::from_value(value).unwrap();
    assert!(loaded.improvements.is_empty());
}

#[test]
fn revisions_list_the_earlier_requests_oldest_first() {
    let store = Store::new(&directory()).unwrap();
    let mut first = store.create(request()).unwrap();
    first.stage = Stage::Completed;
    store.save(&first).unwrap();
    let mut second_request = request();
    second_request.parent = Some(first.id.clone());
    second_request.prompt = "Make it blue".into();
    let mut second = store.create(second_request).unwrap();
    second.stage = Stage::Completed;
    store.save(&second).unwrap();
    let mut third_request = request();
    third_request.parent = Some(second.id.clone());
    let third = store.create(third_request).unwrap();
    let text = queue::earlier_requests(&store, &third);
    assert!(text.find("Animated bars") < text.find("Make it blue"));
    assert!(text.contains("Make it blue"));
    assert!(queue::earlier_requests(&store, &first).is_empty());
}

#[test]
fn speech_context_asks_for_sync_only_when_words_exist() {
    assert!(queue::speech_context("[]").contains("none available"));
    let words = r#"[{"start_ms":0,"end_ms":400,"text":"ciao"}]"#;
    let context = queue::speech_context(words);
    assert!(context.contains("MUST be synchronized") && context.ends_with(words));
}
