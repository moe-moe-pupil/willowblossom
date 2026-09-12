use bevy_egui::egui::{
    TextEdit,
    Ui,
};

use super::{
    invalidate_replay_timeline,
    replay_dialogue_is_playable,
    replay_has_live_take,
    ReplayDialogue,
    ReplayMode,
    ReplayStudio,
};

fn selected_dialogue(studio: &ReplayStudio) -> Option<&ReplayDialogue> {
    let line_id = studio.selected_dialogue_id?;
    studio
        .replay
        .as_ref()?
        .dialogue
        .iter()
        .find(|line| line.line_id == line_id && replay_dialogue_is_playable(line))
}

fn can_edit_selected_dialogue(studio: &ReplayStudio) -> bool {
    studio.mode != ReplayMode::Recording
        && !replay_has_live_take(studio)
        && studio.video_render.is_none()
        && studio.video_encoding.is_none()
}

/// Uses the authored cue id, so reordering the timeline cannot redirect an edit.
/// Subtitle edits preserve the timing and camera choices, like the live editor.
fn set_selected_dialogue_text(studio: &mut ReplayStudio, text: String) -> bool {
    if !can_edit_selected_dialogue(studio) {
        return false;
    }
    let Some(selected) = selected_dialogue(studio) else { return false };
    if selected.text == text {
        return false;
    }
    let line_id = selected.line_id;
    let replay = studio.replay.as_mut().expect("selected cue has a replay");
    let line = replay
        .dialogue
        .iter_mut()
        .find(|line| line.line_id == line_id)
        .expect("selected cue is present");
    line.text = text;
    // A custom pronunciation must never keep speaking the pre-edit subtitle.
    line.speech_text = None;
    if studio.mode == ReplayMode::Playing {
        studio.mode = ReplayMode::Paused;
    }
    invalidate_replay_timeline(studio);
    studio.speech_wait_cue = None;
    studio.speech_wait_elapsed_seconds = 0.0;
    studio.speech_bypass_cues.clear();
    studio.director_response_hash = None;
    studio.director_request_pending = false;
    studio.auto_export_after_director = false;
    studio.status = "已更新所选台词，语音将按新文字生成".into();
    true
}

fn precise_time(time_ms: u64) -> String {
    format!(
        "{:02}:{:02}.{:03}",
        time_ms / 60_000,
        (time_ms / 1_000) % 60,
        time_ms % 1_000
    )
}

/// A compact inspector remains visible above the broader scene editing tools.
pub(super) fn selected_dialogue_inspector(ui: &mut Ui, studio: &mut ReplayStudio) -> bool {
    let Some(line) = selected_dialogue(studio) else {
        studio.selected_dialogue_id = None;
        ui.weak("在时间轴选择台词，即可在这里修改字幕。");
        return false;
    };
    let line_id = line.line_id;
    let mut text = line.text.clone();
    ui.strong("所选台词");
    ui.horizontal_wrapped(|ui| {
        ui.label(&line.name);
        if !line.role.trim().is_empty() {
            ui.weak(&line.role);
        }
        ui.weak(format!(
            "角色 {} · 台词 {}",
            line.sender_id, line_id
        ));
    });
    ui.label(format!(
        "{} — {} · {:.3} 秒",
        precise_time(line.time_ms),
        precise_time(line.time_ms.saturating_add(line.duration_ms)),
        line.duration_ms as f64 / 1_000.0,
    ));
    ui.small("拖动时间轴句尾或应用精确时长可修剪台词。");
    let changed = ui
        .add_enabled(
            can_edit_selected_dialogue(studio),
            TextEdit::multiline(&mut text)
                .id_salt(("replay-selected-dialogue-text", line_id))
                .desired_width(ui.available_width())
                .desired_rows(4),
        )
        .changed();
    if changed {
        set_selected_dialogue_text(studio, text);
    }
    ui.separator();
    true
}

#[cfg(test)]
mod tests {
    use serde_json::{
        from_value,
        json,
        to_value,
    };

    use super::{
        selected_dialogue,
        set_selected_dialogue_text,
        ReplayMode,
        ReplayStudio,
    };

    fn studio() -> ReplayStudio {
        let line = |id, time_ms, text| {
            json!({
                "line_id": id, "time_ms": time_ms, "duration_ms": 1500,
                "duration_locked": true, "sender_id": 42, "name": "角色", "role": "玩家",
                "text": text, "speech_text": "原有读音", "avatar": "",
                "visibility": {"scope": "public"},
            })
        };
        ReplayStudio {
            replay: Some(
                from_value(json!({
                    "format_version": 3, "title": "所选台词", "campaign_id": "campaign",
                    "created_at_unix_ms": 1, "duration_ms": 5000,
                    "audience": {"scope": "public"}, "scene": {"voxels": []},
                    "dialogue": [line(10, 0, "第一句"), line(20, 2500, "第二句")],
                    "camera": [{"time_ms": 3500, "translation": [1.0, 2.0, 3.0],
                        "rotation": [0.0, 0.0, 0.0, 1.0]}],
                }))
                .unwrap(),
            ),
            selected_dialogue_id: Some(20),
            mode: ReplayMode::Playing,
            playback_ms: 2700,
            ..ReplayStudio::default()
        }
    }

    #[test]
    fn selection_and_text_edit_follow_authored_id_after_reorder() {
        let mut studio = studio();
        studio.replay.as_mut().unwrap().dialogue.swap(0, 1);
        assert_eq!(
            selected_dialogue(&studio).unwrap().text,
            "第二句"
        );
        let mut expected = to_value(studio.replay.as_ref().unwrap()).unwrap();
        expected["dialogue"][0]["text"] = json!("新的字幕");
        expected["dialogue"][0]
            .as_object_mut()
            .unwrap()
            .remove("speech_text");
        studio.speech_wait_cue = Some((20, 0));
        studio.speech_bypass_cues.insert((20, 0));
        studio.director_response_hash = Some(99);
        studio.director_request_pending = true;
        studio.auto_export_after_director = true;

        assert!(set_selected_dialogue_text(
            &mut studio,
            "新的字幕".into()
        ));
        assert_eq!(
            to_value(studio.replay.as_ref().unwrap()).unwrap(),
            expected
        );
        assert_eq!(studio.selected_dialogue_id, Some(20));
        assert_eq!(studio.playback_ms, 2700);
        assert_eq!(studio.mode, ReplayMode::Paused);
        assert_eq!(studio.timeline_revision, 1);
        assert!(studio.speech_wait_cue.is_none());
        assert!(studio.speech_bypass_cues.is_empty());
        assert!(studio.director_response_hash.is_none());
        assert!(!studio.director_request_pending);
        assert!(!studio.auto_export_after_director);
    }

    #[test]
    fn missing_excluded_recording_and_unchanged_cues_do_not_mutate_the_project() {
        let mut studio = studio();
        let original = to_value(studio.replay.as_ref().unwrap()).unwrap();
        assert!(!set_selected_dialogue_text(
            &mut studio,
            "第二句".into()
        ));
        studio.selected_dialogue_id = Some(999);
        assert!(selected_dialogue(&studio).is_none());
        assert!(!set_selected_dialogue_text(
            &mut studio,
            "错误目标".into()
        ));
        studio.selected_dialogue_id = Some(20);
        studio.mode = ReplayMode::Recording;
        assert!(!set_selected_dialogue_text(
            &mut studio,
            "录制中编辑".into()
        ));
        assert_eq!(
            to_value(studio.replay.as_ref().unwrap()).unwrap(),
            original
        );
        studio.mode = ReplayMode::Paused;
        studio.replay.as_mut().unwrap().dialogue[1].included = false;
        assert!(selected_dialogue(&studio).is_none());
        assert!(!set_selected_dialogue_text(
            &mut studio,
            "已排除".into()
        ));
        assert_eq!(studio.timeline_revision, 0);
    }
}
