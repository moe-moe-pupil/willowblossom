use std::{
    env::var_os,
    path::PathBuf,
};

use bevy_egui::egui::{
    vec2,
    Context,
    Event,
    FontData,
    FontDefinitions,
    FontFamily,
    FullOutput,
    Modifiers,
    PointerButton,
    Pos2,
    RawInput,
    Rect,
    Shape,
};
use serde_json::{
    from_value,
    json,
};

use super::{
    encounter_action_ui,
    over_time::{
        gm_over_time_ui,
        GmOverTimeDraft,
        GmOverTimeKind,
    },
    BattleEncounter,
    BattleRoundStore,
    BattleRoundUiState,
    BattleTargetingLine,
};
use crate::{
    napcat::NapcatMessageManager,
    replay::ui_render::EguiFrameRenderer,
};

struct DotUiHarness {
    context: Context,
    draft: GmOverTimeDraft,
    store: BattleRoundStore,
    manager: NapcatMessageManager,
    ui_state: BattleRoundUiState,
    targeting_line: BattleTargetingLine,
    show_actions: bool,
    renderer: EguiFrameRenderer,
    output: FullOutput,
    frame: u64,
}

impl DotUiHarness {
    fn new() -> Self {
        let context = Context::default();
        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "cjk".into(),
            FontData::from_static(include_bytes!(
                "../../assets/fonts/AlibabaHealthFont.ttf"
            ))
            .into(),
        );
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk".into());
        context.set_fonts(fonts);
        let theme = context.theme();
        let mut style = (*context.style_of(theme)).clone();
        style.animation_time = 0.0;
        context.set_style_of(theme, style);

        let mut store = BattleRoundStore::default();
        store
            .encounters
            .insert("battle".to_owned(), BattleEncounter {
                participants: vec![from_value(json!({
                    "target_id": "player",
                    "display_name": "测试角色",
                    "player_character": true,
                    "hp": 20.0,
                    "max_hp": 20.0,
                    "mp": 6.0,
                    "max_mp": 6.0,
                    "turn": 5,
                    "combat_turns_completed": 4,
                }))
                .expect("synthetic combat participant")],
                ..Default::default()
            });
        let mut harness = Self {
            context,
            draft: GmOverTimeDraft::default(),
            store,
            manager: from_value(json!({"messages": {}})).unwrap(),
            ui_state: BattleRoundUiState::default(),
            targeting_line: BattleTargetingLine::default(),
            show_actions: false,
            renderer: EguiFrameRenderer::default(),
            output: FullOutput::default(),
            frame: 0,
        };
        harness.advance(Vec::new());
        harness.advance(Vec::new());
        harness
    }

    fn advance(&mut self, events: Vec<Event>) -> bool {
        self.frame += 1;
        let mut changed = false;
        self.output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    vec2(900.0, 600.0),
                )),
                time: Some(self.frame as f64 / 60.0),
                events,
                ..RawInput::default()
            },
            |ui| {
                changed |= gm_over_time_ui(
                    ui,
                    &mut self.draft,
                    self.store.encounters.get_mut("battle").unwrap(),
                );
                if self.show_actions {
                    changed |= encounter_action_ui(
                        ui,
                        &mut self.ui_state,
                        "battle",
                        &mut self.store,
                        &mut self.manager,
                        None,
                        &mut self.targeting_line,
                    );
                }
            },
        );
        self.renderer.apply(&self.output.textures_delta).unwrap();
        changed
    }

    fn click(&mut self, label: &str) -> bool {
        let pos = text_rect(&self.output, label).center();
        let mut changed = false;
        for pressed in [true, false] {
            changed |= self.advance(vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
        changed | self.advance(Vec::new())
    }

    fn encounter(&self) -> &BattleEncounter { &self.store.encounters["battle"] }

    fn save_artifact(&self, name: &str) {
        if let Some(directory) = var_os("WILLOWBLOSSOM_UI_ARTIFACT_DIR") {
            self.renderer
                .render(
                    &self.context,
                    &self.output,
                    PathBuf::from(directory).join(name),
                    [900, 600],
                )
                .unwrap();
        }
    }
}

fn text_rect(output: &FullOutput, expected: &str) -> Rect {
    fn find(shape: &Shape, expected: &str) -> Option<Rect> {
        match shape {
            Shape::Text(text) if text.galley.job.text == expected => {
                Some(text.visual_bounding_rect())
            },
            Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, expected)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, expected))
        .unwrap_or_else(|| panic!("missing rendered text: {expected}"))
}

#[test]
fn pointer_applies_and_removes_gm_dot_without_spending_an_action() {
    let mut harness = DotUiHarness::new();
    assert!(!harness.click("GM持续效果（DOT / HOT）"));
    harness.draft.target_id = "player".to_owned();
    harness.draft.amount = 3.0;
    harness.draft.rounds = 2;
    harness.advance(Vec::new());

    assert!(harness.click("施加持续伤害"));
    let participant = &harness.encounter().participants[0];
    assert_eq!(
        participant.delayed_damage_ticks.len(),
        1
    );
    let tick = &participant.delayed_damage_ticks[0];
    assert!(tick.repeat_each_round);
    assert_eq!(tick.amount, 3.0);
    assert_eq!(tick.turns_remaining, 2);
    assert_eq!(participant.hp, 20.0);
    assert_eq!(participant.mp, 6.0);
    assert_eq!(participant.turn, 5);
    assert_eq!(participant.combat_turns_completed, 4);
    assert!(!participant.action_done);
    assert_eq!(harness.encounter().round, 0);
    harness.save_artifact("gm-dot.png");

    assert!(harness.store.next_round("battle"));
    let participant = &harness.encounter().participants[0];
    assert_eq!(participant.hp, 17.0);
    assert_eq!(
        participant.delayed_damage_ticks.len(),
        1
    );
    assert_eq!(
        participant.delayed_damage_ticks[0].turns_remaining,
        1
    );
    harness.advance(Vec::new());
    assert_eq!(
        harness.encounter().participants[0].hp,
        17.0
    );

    assert!(harness.click("移除"));
    assert!(harness.encounter().participants[0]
        .delayed_damage_ticks
        .is_empty());
    assert_eq!(
        harness.encounter().participants[0].hp,
        17.0
    );
    assert!(harness.store.next_round("battle"));
    assert_eq!(
        harness.encounter().participants[0].hp,
        17.0
    );
}

#[test]
fn pointer_selects_applies_and_removes_gm_hot_without_spending_an_action() {
    let mut harness = DotUiHarness::new();
    harness
        .store
        .encounters
        .get_mut("battle")
        .unwrap()
        .participants[0]
        .hp = 10.0;
    assert!(!harness.click("GM持续效果（DOT / HOT）"));
    assert!(!harness.click("持续治疗（HOT）"));
    assert_eq!(
        harness.draft.kind,
        GmOverTimeKind::Healing
    );
    harness.draft.amount = 3.0;
    harness.draft.rounds = 2;
    harness.advance(Vec::new());

    assert!(harness.click("施加持续治疗"));
    let participant = &harness.encounter().participants[0];
    assert_eq!(
        participant.delayed_healing_ticks.len(),
        1
    );
    assert_eq!(
        participant.delayed_healing_ticks[0].turns_remaining,
        2
    );
    assert_eq!(participant.hp, 10.0);
    assert_eq!(participant.mp, 6.0);
    assert_eq!(participant.turn, 5);
    assert!(!participant.action_done);
    harness.save_artifact("gm-hot.png");

    assert!(harness.store.next_round("battle"));
    assert_eq!(
        harness.encounter().participants[0].hp,
        13.0
    );
    harness.advance(Vec::new());
    assert!(harness.click("移除"));
    assert!(harness.encounter().participants[0]
        .delayed_healing_ticks
        .is_empty());
    assert!(harness.store.next_round("battle"));
    assert_eq!(
        harness.encounter().participants[0].hp,
        13.0
    );
}

#[test]
fn pointer_ordinary_heal_uses_amount_and_finishes_action_without_a_skill() {
    let mut harness = DotUiHarness::new();
    let encounter = harness.store.encounters.get_mut("battle").unwrap();
    encounter.participants[0].agi = 20;
    encounter.participants.push(
        from_value(json!({
            "target_id": "target",
            "display_name": "受伤角色",
            "hp": 10.0,
            "max_hp": 20.0,
            "turn": 5,
        }))
        .unwrap(),
    );
    harness
        .ui_state
        .selected_action_actor
        .insert("battle".to_owned(), "player".to_owned());
    harness
        .ui_state
        .selected_action_target
        .insert("battle".to_owned(), "target".to_owned());
    harness
        .ui_state
        .action_amount
        .insert("battle".to_owned(), 3.0);
    harness.show_actions = true;
    harness.advance(Vec::new());
    harness.advance(Vec::new());
    // Both shortcuts are exposed together on the normal action row.
    text_rect(&harness.output, "普通攻击");
    text_rect(&harness.output, "普通治疗");
    harness.save_artifact("ordinary-healing.png");
    assert!(harness.click("普通治疗"));
    let encounter = harness.encounter();
    assert_eq!(encounter.participants[1].hp, 13.0);
    assert_eq!(encounter.participants[0].mp, 6.0);
    assert!(encounter.participants[0].action_done);
    assert!(!encounter.participants[1].action_done);
    assert_eq!(encounter.round, 0);
}
