# Moonberry Gap Audit

Date: 2026-06-10

## Sources Checked

- Old Moonberry source cloned from `https://github.com/moe-moe-pupil/moonberry` at commit `da85a9e`.
- Moonberry reference skill docs:
  - `.codex/skills/moonberry-rewrite-reference/references/moonberry-behavior.md`
  - `.codex/skills/moonberry-rewrite-reference/references/willowblossom-migration-map.md`
- Willowblossom current worktree, especially:
  - `src/napcat/mod.rs`
  - `src/ui/mod.rs`
  - `src/rule_engine.rs`
  - `src/battle_round.rs`
  - `src/scene.rs`
  - `src/deepseek/mod.rs`
  - `design.md`

Verification during audit: `cargo test` passed: 76 passed, 1 ignored live DeepSeek API test, 0 failed. After the access-model, private-command, chat-approval, summary-only DeepSeek, root NapCat/TRPG import/export, targeted PC/chat-list exports, targeted PC/chat-list/summary/scene import-merge, DeepSeek summary export, voxel scene export, party-scoped private broadcast, per-group guide/config, active-group admission, join-request gate, party-scoped group-summary, scene voxel visibility metadata, visibility-aware private auto-forward, automatic private-approval guide onboarding, scene-capture voxel visibility filtering, live player-view voxel visibility filtering, skill approval/source metadata, unit-pool export/UI, unit-template battle insertion, unit-template scene standee placement, unit-template generic scene token persistence/UI/live gizmos, partial Moonberry legacy JSON import, random-pool text/min-max result, random-pool group/tag metadata preservation and filtering, random-pool checked per-PC result staging and sends, legacy skill-pool metadata/raw graph-buff payload import, character-skill shape metadata/raw buff-machine preservation, per-group `basicConfig` stat formula implementation, `basicConfig` damage/heal coefficient application, low-HP damage penalty, Moonberry experience threshold/manual GM award, basic rule grant-buff action, common typed grant-buff effect, imported `cooldownLeft` execution, preserved `target_count` execution, preserved range fallback execution, preserved no/single target-class execution, preserved `范围` target-class area expansion, preserved numeric skill-arg amount execution, preserved string/BUFF skill-arg text substitution, active legacy `技能释放` buff-machine damage/heal/basic modifier conversion, passive legacy `被动` buff-machine basic stat/modifier conversion, legacy graph active/passive simple damage/heal/basic modifier chain conversion, pool-backed legacy `给予BUFF`/graph `BUFF变量` basic-buff expansion for rule sync, pool-backed legacy `给予BUFF` damage/heal tick actions for rule sync and quick-cast turn advancement, preserved full active Moonberry normal/support talent tables with all-table trigger/effect category metadata, always-on numeric Moonberry talent passives for max HP/max MP/MP regen/healing output, `大魔法师` magical damage, `人类基因工程` disease/poison damage reduction, `抗魔体质` magical damage reduction, `溃伤` on-damage healing-taken debuff execution, `禅宗古训` physical-damage lifesteal execution, `过度免疫` large-hit damage reduction execution, and `生死时速` dying-target healing bonus execution, preserved skill-type default damage execution, preserved single-target range filtering, old target-sentinel parser coverage, old skill type/target-class metadata selectors, per-turn damage/heal counter coverage, player-submitted skill approval coverage, old table run/sort/negative defaults coverage, old per-PC negative timer import/reply/timeout coverage, old team/world/chat-area/send-pane metadata migration with private broadcast target expansion, GM-controlled legacy team/chat-area promotion into live party visibility scopes, old team local chat excerpt/window-geometry preservation with appendable private-send composer, editable parsed local messages, and independent old-channel chat floating windows with old geometry defaults, legacy world/chat-area scene marker persistence/UI, visibility-filtered legacy area marker live gizmos, legacy area voxel-outline/fill stamping into editable scene maps, legacy unit-template aliasing plus old world/chat-area unit-token sync/remove controls, random-pool batch text sends through current-group/private legacy send-pane scopes, standalone imported legacy send-pane composers, independent imported legacy send-pane floating windows, old send-pane duplicate direct-PC pruning/all-target collapse, old send-pane multi-select target editor/add-remove-clear controls, GM party merge/delete lifecycle controls, GM player-visible chat preview filtering, explicit scene editor visibility-mode controls, persisted character/unit standee visibility with live/capture player filtering, generic unit scene token visibility filtering, generic unit scene token position/visibility editing, and GM chat-list player-visible filtering with hidden unread activity suppressed pass, `cargo test -j 1` passes: 280 passed, 1 ignored live DeepSeek API test, 0 failed. `cargo fmt --check` passes.

Additional 2026-06-25 to 2026-06-26 update: `菜鸡猛啄` approved-talent minimum damage floor now executes in the rule engine, quick-cast, and parsed battle skill paths; `火源之力` approved-talent healing output now scales by the healer's HP band in rule-engine, quick-cast, buff-tick, and parsed battle healing; `互帮互助` approved-talent healing feedback now returns 50% healing to the healer from source/target talent hooks in rule-engine, quick-cast, buff-tick, and parsed battle healing; `数魔转换器` approved-talent range damage now receives positive magical damage bonuses in rule sync, quick-cast, and parsed battle skill paths; `瞄准镜Tex-30` approved-talent range damage now treats skill range as at least `等级*15` meters in quick-cast and parsed battle target filtering; `魔网延伸` approved-talent spell skills now receive +5% metadata range in quick-cast and parsed battle target filtering while summon-distance behavior remains GM-handled; `狂风恶浪` approved-talent passive movement speed now grants +20% through the same effective-buff path as other numeric passives, and parsed battle order now raises it to 35% while live player-character participants are <=3; `越战越勇` approved-talent parsed battle damage now gains +2% per completed participant turn up to +20%; `斗志昂扬` approved-talent parsed battle incoming skill damage now reduces by 50%/10%/2% on the target's first/second/third own turn; `狂妄` approved-talent parsed battle damage now gains +10% for each unique damage source that has hurt the actor, capped at +30%; `无尽痛楚` approved-talent parsed battle damage now records successful damage-taken stacks and consumes up to 2 stacks on the next positive skill hit for `等级*1.5` untyped damage per stack; `无限专注` approved-talent parsed battle damage now stacks +10%/+20% on repeated successful single-target attacks against the same target and resets when switching target; `总冠军` approved-talent parsed battle now stacks +2% damage dealt and -1% incoming damage whenever a player-character target is eliminated; `忏悔` approved-talent base healing-output bonus now grants +25% through the passive effective-buff path, and parsed battle now decays that bonus by 10% per kill/assist contribution down to 0%; `混沌无序` approved-talent output variance now rolls -15%~+15% for each damage/healing effect in rule-engine, quick-cast, and parsed battle skill paths; and `苏萨斯之爪` approved-talent physical damage now schedules a one-turn-later magical follow-up for 35% of the actual physical damage in rule-engine, quick-cast, and parsed battle paths. Speed is now a typed buff field for rule-engine and legacy passive buff-machine conversion. Focused verification passes with `CARGO_INCREMENTAL=0 cargo test -j 1 minimum_damage`, `CARGO_INCREMENTAL=0 cargo test -j 1 wounded_healing`, `CARGO_INCREMENTAL=0 cargo test -j 1 mutual_aid`, `CARGO_INCREMENTAL=0 cargo test -j 1 converter`, `cargo test --lib -j 1 tex30 -- --nocapture`, `cargo test --lib -j 1 magic_web -- --nocapture`, `cargo test --lib -j 1 speed -- --nocapture`, `cargo test --lib -j 1 gale_force -- --nocapture`, `cargo test --lib -j 1 valorous -- --nocapture`, `cargo test --lib -j 1 fighting_spirit -- --nocapture`, `cargo test --lib -j 1 arrogance -- --nocapture`, `cargo test --lib -j 1 endless_pain -- --nocapture`, `cargo test --lib -j 1 infinite_focus -- --nocapture`, `cargo test --lib -j 1 champion -- --nocapture`, `cargo test --lib -j 1 penance -- --nocapture`, `cargo test --lib -j 1 legacy_moonberry_buff_machine_converts_passive_basic_buffs -- --nocapture`, `cargo test --lib -j 1 moonberry_passive_talents_apply_to_effective_character_stats -- --nocapture`, `cargo test --lib -j 1 chaos -- --nocapture`, `cargo test --lib -j 1 sousas -- --nocapture`, and `cargo test --lib -j 1 basic_config_applies_moonberry_damage_and_heal_attribute_multipliers -- --nocapture`: 3, 3, 4, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 3, and 1 passed respectively, 0 failed. Full-suite verification passes with `cargo test -j 1`: 311 passed, 1 ignored live DeepSeek API test, 0 failed.

Additional 2026-06-26 update: `罪上加罪` approved-talent parsed battle now grants kill/assist contributors one stack, recovers 10% of missing HP/MP, and tracks the capped 2.5% per-stack experience-bonus metadata up to 10%. Focused verification passes with `cargo test --lib -j 1 sin_on_sin -- --nocapture`: 1 passed, 0 failed. Full-suite verification passes with `cargo test -j 1`: 312 passed, 1 ignored live DeepSeek API test, 0 failed.

Additional 2026-06-26 update: `役于我手` approved-talent parsed battle now grants alive participants in an active encounter 5% of a defeated target's maximum HP as max-HP bonus, capped by a fixed 20% cap from the holder's battle-entry max HP. Defeats while the encounter is resting do not trigger the talent. Focused verification passes with `cargo test --lib -j 1 dominion -- --nocapture`: 1 passed, 0 failed. Full-suite verification passes with `cargo test -j 1`: 313 passed, 1 ignored live DeepSeek API test, 0 failed.

Additional 2026-06-26 update: `一心` approved-talent parsed battle now tracks single-target healing on the same target, applies +5% healing per existing stack up to +25%, and resets to one stack when switching targets. Focused verification passes with `cargo test --lib -j 1 one_heart -- --nocapture`: 1 passed, 0 failed. Full-suite verification passes with `cargo test -j 1`: 314 passed, 1 ignored live DeepSeek API test, 0 failed.

Additional 2026-06-26 update: `千万回忆` approved-talent parsed battle now schedules delayed healing echoes from successful single-target immediate heals: 15% on the next round and 5% on the following round. Focused verification passes with `cargo test --lib -j 1 echoing_memory -- --nocapture`: 1 passed, 0 failed. Full-suite verification passes with `cargo test -j 1`: 315 passed, 1 ignored live DeepSeek API test, 0 failed.

Additional 2026-06-26 update: `液态躯体` approved-talent parsed battle now splits resolved direct incoming skill damage into 50% immediate damage plus 50% delayed damage on the next round, and heals the holder for 5% of the previous turn's damage taken when the battle round advances. Focused verification passes with `cargo test --lib -j 1 liquid_body -- --nocapture`: 1 passed, 0 failed. Full-suite verification passes with `cargo test -j 1`: 316 passed, 1 ignored live DeepSeek API test, 0 failed.

Additional 2026-06-26 update: `敏锐` approved-talent parsed battle now preserves a once-per-battle dodge charge and consumes it to fully evade the first positive range/non-targeted incoming skill damage, without spending the charge on normal single-target damage. Focused verification passes with `cargo test --lib -j 1 keen_evasion -- --nocapture`: 1 passed, 0 failed. Full-suite verification passes with `cargo test -j 1`: 317 passed, 1 ignored live DeepSeek API test, 0 failed.

Additional 2026-07-16 update: `奥术护盾` approved-talent battle participants now enter each encounter with a persisted shield equal to 10% of maximum MP. The central battle damage path consumes the shield before HP for manual damage, parsed skills, and scheduled damage; fully absorbed hits do not count as HP damage or damage-source contributions. Focused verification passes with `cargo test --lib -j 1 arcane_shield -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 386 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: `过度治疗` approved-talent battle healing now converts overheal into an encounter-local shield capped at 30% of the target's maximum HP. The shield is persisted, consumed before other HP damage, and remains through the following full battle round; parsed skills, delayed healing, buff ticks, lifesteal, mutual-aid feedback, liquid-body recovery, and kill recovery share the same healing path while passive regeneration remains excluded. Focused verification passes with `cargo test --lib -j 1 overhealing -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 387 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: `不死者之怒` approved-talent battle participants now negate their first lethal post-shield hit per encounter when the remaining damage does not exceed maximum HP. The resulting 100% damage reduction and +10% outgoing damage remain active until the next battle-round boundary; oversized hits bypass the effect, and negated hits do not create damage-taken stacks or contributor credit. The old Moonberry commit stored this talent as description-only data, so these runtime edge semantics follow its preserved wording explicitly. Focused verification passes with `cargo test --lib -j 1 undying_rage -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 388 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: battle damage now returns a shared typed resolution containing applied damage, absorbed damage, lethal outcome, and `不死者之怒` activation. Manual actions, parsed skills, delayed damage, and buff ticks log post-absorption damage. Parsed-skill `溃伤`, `禅宗古训`, `苏萨斯之爪`, and `无限专注` now require positive applied damage; `无尽痛楚` stacks remain when a shield absorbs the entire effect, but are consumed when `液态躯体` commits part of that bonus to delayed damage. Lifesteal and delayed physical follow-up scale from the applied physical share instead of the pre-shield amount. Focused verification passes with `cargo test --lib -j 1 shield_absorption_gates -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 389 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: `希望化身` approved-talent battle participants now transform on their first lethal post-shield hit, remain actionable at 0 HP, become immune to subsequent damage, and may use healing effects but not normal attacks or damaging skills. The encounter-local state persists across saves and expires at the second battle-round boundary after activation, forcing death and resolving the original damage contributors normally. The old Moonberry source stored this as description-only data; Willowblossom does not yet model channeled casts, so the described channel interruption has no executable state to cancel. Focused verification passes with `cargo test --lib -j 1 hope_avatar -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 390 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `溃伤`'s one-round healing-received penalty now expires in the global `next_round` path used by the battle UI. Its regression now proves reduced healing during the hit round and normal healing after the next global round boundary instead of exercising only the unused per-participant advance helper. Focused verification passes with `cargo test --lib -j 1 wound_healing_taken_debuff -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 390 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: `振奋` approved-talent parsed-battle single-target healing now grants the healed target +10% effective battle-order speed and outgoing damage until the next global round boundary. Each healer can maintain the effect on only one target, changing targets transfers that healer's contribution, multiple healers do not stack the numeric bonus beyond 10%, area/multi-target healing does not trigger it, and the source/target ownership state persists safely. Focused verification passes with `cargo test --lib -j 1 inspiration -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 391 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: `息心` approved-talent battle participants now persist the post-mitigation damage they take while an encounter is active and recover 50% of that amount when the GM changes the encounter from active to resting. Prevented damage does not enter the tally, resting damage is excluded, re-entering battle starts a fresh tally, repeated resting-state writes cannot retrigger recovery, and defeated participants are not revived. The old Moonberry source stored this talent as description-only data, so the encounter's existing GM-controlled active/resting transition is the executable battle-exit boundary. Focused verification passes with `cargo test calm_heart_heals_active_combat_damage_once_on_battle_exit -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 392 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `奥术护盾` now persists its 10% maximum-MP grant rate separately from remaining shield strength. Changing an encounter from active to resting removes any leftover arcane shield as the talent describes, and changing it back to active grants a fresh shield from the participant's current maximum MP even if the previous shield was depleted. Focused verification passes with `cargo test --lib -j 1 arcane_shield -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 392 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `敏锐`'s persisted dodge charge now follows the encounter lifecycle promised by its “进入战斗轮” trigger. Leaving combat clears the charge, resting encounters cannot consume stale or migrated charges, and entering combat rearms one dodge for approved holders even when their previous charge was spent. Focused verification passes with `cargo test --lib -j 1 keen_evasion -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 392 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `不死者之怒` now obeys its documented encounter-local lifecycle. Resting encounters cannot trigger or preserve the lethal-hit immunity, leaving combat clears the same-round damage bonus, and entering a later combat resets the consumed flag so the holder receives one fresh charge. Repeated lethal hits within the same combat remain limited to the original single activation. Focused verification passes with `cargo test --lib -j 1 undying_rage -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 392 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `希望化身` now obeys the active encounter boundary. Lethal damage during rest cannot activate the avatar, its immunity and healing-only action restriction apply only during active combat, entering a later combat resets prior consumption, and ending combat while the avatar is active immediately performs its promised death and normal defeat-contributor handling rather than leaving an immortal 0-HP resting participant. Focused verification passes with `cargo test --lib -j 1 hope_avatar -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 392 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: battle damage contributors are now scoped to one active combat. Surviving targets clear contributor attribution when combat ends, battle entry defensively removes stale persisted attribution, and exit-forced defeats resolve their legitimate contributors before cleanup. This prevents attackers from an earlier combat receiving `忏悔` or `罪上加罪` kill/assist credit when another attacker defeats the target later. Focused verification passes with `cargo test --lib -j 1 battle_exit_prevents_cross_combat_kill_assist_credit -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `振奋` is now fully scoped to active combat. Resting healing cannot grant it, stale persisted links cannot modify resting order speed or damage, combat exit clears healer/target ownership, and combat entry defensively clears stale links before new healing can establish them. The existing active-combat transfer, non-stacking, round expiry, speed, and damage behavior remains unchanged. Focused verification passes with `cargo test --lib -j 1 inspiration -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `一心` is now fully scoped to active combat. Resting healing neither gains stacks nor receives a stale persisted focus bonus, combat exit clears the healer's target/stacks, and combat entry defensively clears migrated stale focus. Active-combat same-target stacking, the +25% cap, and target-switch reset remain unchanged. Focused verification passes with `cargo test --lib -j 1 one_heart -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `无限专注` is now fully scoped to active combat. Resting attacks neither gain stacks nor receive a stale persisted focus bonus, combat exit clears the attacker's target/stacks, combat entry defensively clears migrated stale focus, and the roster hides inactive focus state. Active-combat same-target +10%/+20% escalation and target-switch reset remain unchanged. Focused verification passes with `cargo test --lib -j 1 infinite_focus -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `狂妄` and `无尽痛楚` are now fully scoped to active combat, matching their preserved “进入战斗轮” trigger. Resting damage cannot add unique-source or pain stacks, stale persisted state cannot boost resting attacks, combat exit and entry clear both states, and the roster hides them during rest. Their active-combat +10% per unique source (up to +30%) and next-hit `等级*1.5` untyped damage (up to two stacks) remain unchanged. Focused verification passes with `cargo test --lib -j 1 arrogance -- --nocapture` and `cargo test --lib -j 1 endless_pain -- --nocapture`: 1 passed in each command, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `液态躯体` now obeys its preserved active battle-round trigger. Resting skill damage applies fully and schedules no liquid-body delayed tick, while resting round or participant advancement cannot trigger the previous-round self-heal. Active-combat 50/50 damage splitting and 5% prior-round-damage healing remain unchanged; delayed damage already committed during combat is not erased merely by changing the encounter to resting. Focused verification passes with `cargo test --lib -j 1 liquid_body -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `越战越勇` and `斗志昂扬` now use dedicated persisted combat-turn counters instead of the participants' inherited world/cooldown clocks. A newly created or re-entered combat therefore starts at +0% valorous damage and the defender's 50% first-turn reduction even when the campaign clock is already advanced; completed active actions then advance the shared/per-participant counters, combat boundaries reset them, and neither modifier applies during rest. Existing world turns and skill cooldown timing remain unchanged. Focused verification passes with `cargo test --lib -j 1 valorous -- --nocapture`, `cargo test --lib -j 1 fighting_spirit -- --nocapture`, `cargo test --lib -j 1 active_battle_turn_suppresses -- --nocapture`, and `cargo test --lib -j 1 skill_cooldown_starts -- --nocapture`: 1 passed in each command, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `不死者之怒`'s +10% outgoing damage now checks the active encounter at every application point, not only through normal exit cleanup. A stale or migrated `undying_rage_active` flag in a resting encounter therefore cannot boost parsed skill damage or continuing buff-tick damage, while the same-round active-combat bonus remains unchanged. Focused verification passes with `cargo test --lib -j 1 undying_rage -- --nocapture` and `cargo test --lib -j 1 unit_instance_buff_ticks_damage_without_mutating_template -- --nocapture`: 1 passed in each command, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: imported `cooldownLeft` is now materialized once as a persisted per-skill ready turn. The remaining value decreases with the character or battle turn clock in `.冷却`, quick-cast, player encounters, and independent unit instances; reaching zero stays usable instead of reapplying the original imported value. A successful cast clears the migration-only ready turn and continues through the normal local cast-clock path. Focused verification passes for the private cooldown command, quick-cast, and battle execution regressions.

Additional 2026-07-16 correction: automatic battle effects now respect the encounter's explicit defeated state. Delayed healing plus healing, damage, and fixed-damage buff ticks expire without changing a participant whose `alive` flag is false, consuming residual shields/counters, or emitting false combat logs; direct healing skills retain the established ability to restore a zero-HP target, and `希望化身` remains healable while active. Revival therefore requires a directed healing action or an explicit GM/state transition rather than an accidental background effect. Focused verification passes with `cargo test --lib background_effects_do_not_modify_defeated_participants`.

Additional 2026-07-16 correction: `奥术护盾` now checks the active encounter at damage application as well as at normal combat exit. Resting damage ignores and clears stale or migrated arcane shield values, the roster hides inactive arcane shield state, and entering combat still replaces any stale value with a fresh 10% maximum-MP shield. Ordinary `过度治疗` shielding remains usable outside this combat-only rule. Focused verification passes with `cargo test --lib -j 1 arcane_shield -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 393 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: `以逸待劳` approved-talent participants now persist one recovery charge whenever their natural turn advances during a resting encounter, capped at ten charges. Entering active combat consumes the charges once and restores 5% maximum HP per charge up to 50%; active turns do not accumulate charges, full-health entry still consumes them, and defeated participants are not revived. The recovery is direct HP restoration rather than overheal, so it cannot create an unintended `过度治疗` shield. Focused verification passes with `cargo test --lib -j 1 rest_then_fight -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 394 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: parsed-battle delayed damage now removes each scheduled tick immediately after its one intended execution. `苏萨斯之爪` and `液态躯体` still fire at the next round boundary, but no longer leave an already-applied amount displayed and persisted for another round. Compatibility is preserved for saved encounters: a pre-fire legacy tick at countdown `2` executes once and is removed, while an already-fired stale tick at countdown `1` expires without repeating its damage. Focused verification passes with `cargo test --lib -j 1 sousas -- --nocapture` and `cargo test --lib -j 1 liquid_body -- --nocapture`: 3 and 1 passed respectively, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 394 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: manual battle-roster `存活` edits now preserve the combat HP/alive invariant across character synchronization. Marking a participant defeated sets HP to zero and ends any active `希望化身`; marking a valid zero-HP participant alive restores one HP. A later manager refresh therefore no longer silently revives a manually defeated positive-HP snapshot or immediately defeats a manually revived zero-HP snapshot. Focused verification passes with `cargo test --lib -j 1 manual_alive_edits_keep_hp_and_refresh_state_consistent -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 396 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `役于我手` max-HP growth is now strictly encounter-local and remains authoritative during manager-backed buff processing. Combat exit removes the earned bonus and clamps HP to the restored base cap, combat entry clears stale migrated bonuses, and an exit-forced `希望化身` death cannot grant a new bonus after combat has ended. While combat remains active, round buff healing and grant-buff recalculation use a scoped temporary manager cap so valid HP above the durable character maximum is neither truncated nor prevented from healing to the battle cap; the durable character mirror returns to its base maximum afterward. Character buff-base HP/MP now follows battle synchronization deltas as well, preventing later buff recalculation from restoring stale vitals. Focused verification passes with `cargo test --lib -j 1 dominion -- --nocapture`: 4 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 399 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: battle actor eligibility is now enforced inside the persisted state methods instead of relying only on the current-actor UI. Missing actors, defeated participants, and participants that already completed the round cannot apply manual damage, resolve parsed skills, finish a second action, or gain a negative-skip layer. Rejected known actors receive Chinese feedback while target HP, actor MP/cooldowns, turn clocks, combat counters, and negative layers remain unchanged; an active `希望化身` remains eligible because its zero-HP form intentionally stays alive. Focused verification passes with `cargo test --lib -j 1 ineligible_battle_actors_cannot_mutate_targets_resources_or_clocks -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 400 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: battle action resolution and completion are now one UI/state transaction. A failed manual attack or rejected skill no longer falls through to `finish_actor_action`, so it cannot consume the actor's turn or advance the round. Successful resolution uses a dedicated completion path that may close an action even if that action defeated its own actor; direct completion by a participant that was already defeated remains rejected. This prevents both free turn consumption after failed input and a final self-defeating action deadlocking round advancement. Focused verification passes with `cargo test --lib -j 1 battle_resolution_and_action_completion_are_one_transaction -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 401 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: directed battle revival is now reachable from the GUI and protected by an explicit defeated-target policy. The target selector includes defeated participants with a `（倒下）` label while preferring a living non-self default. Manual attacks, direct damage skills, and status grants reject defeated targets before spending MP, starting cooldowns, or consuming an action; only single-target healing may resolve against and revive them. Self/area/no-target effects remain independent of the selected target, and area effects continue to exclude defeated participants so background-style mass healing cannot revive them accidentally. Focused verification passes with `cargo test --lib -j 1 direct_healing_can_revive_defeated_targets_but_attacks_and_buffs_cannot -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 402 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 update: `疲惫行者` is now executable anywhere Willowblossom applies Moonberry's low-HP outgoing-damage penalty. The approved talent reduces each wound-band penalty by 20%, and the old source's severe/dying boundary is preserved by clamping dying HP to the 5% severe-injury floor before calculating the penalty. Rule-engine attacks, quick casts, continuing buff damage, and parsed battle skills share the same calculation. Focused verification passes with `cargo test --lib low_hp_damage`: 6 passed, 0 failed. Full library verification passes with `cargo test --lib`: 405 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: `生死时速` now belongs to the healer, matching its preserved “your healing” wording, rather than incorrectly granting the bonus when the dying target owns the talent. Rule-engine healing, quick casts, parsed battle skills, and continuing buff healing apply the healer's approved +50% modifier when the target is at or below 20% maximum HP; battle buff ticks use the encounter participant's current HP instead of a stale durable character snapshot. Focused verification passes with `cargo test --lib dying_target` and `cargo test --lib battle_buff_healing_uses_source_talent_and_encounter_target_vitals`: 3 and 1 passed respectively, 0 failed. Full library verification passes with `cargo test --lib --quiet`: 406 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: continuing battle buff damage and healing now use the shared encounter-participant source multiplier pipeline instead of a partial duplicate based mostly on the durable character. Damage ticks therefore honor active encounter state such as `振奋`, `狂妄`, `总冠军`, `越战越勇`, and `不死者之怒`; healing ticks honor encounter-local `忏悔` decay; and both use the encounter's campaign stat configuration and current source vitals while retaining talent metadata. Sources outside the encounter retain the prior character-based fallback. Focused verification passes with `cargo test --lib buff_tick`: 3 passed, 0 failed. Full library verification passes with `cargo test --lib --quiet`: 407 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: continuing typed battle damage now uses the same target mitigation pipeline as parsed skills. Buff damage ticks honor encounter-local `总冠军` reduction, active-turn `斗志昂扬` reduction, typed talent defenses, and the post-multiplier large-hit threshold for `过度免疫`; fixed damage remains intentionally unmodified. The direct-skill path now calls the same shared target helper, preventing the two delivery paths from drifting again. Focused verification passes for the combined buff-damage regression plus the existing parsed `斗志昂扬` and `过度免疫` regressions. Full library verification passes with `cargo test --lib --quiet`: 408 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: rule-engine healing, quick casts, and non-encounter world-turn healing ticks now record HP actually restored rather than the pre-cap healing request. `互帮互助` feedback is calculated from that effective amount, so partial or fully wasted healing can no longer create phantom feedback or inflate per-turn healing totals; capped feedback likewise records only restored HP and no longer emits a zero-healing trigger log. Parsed battle healing already used effective resolution and remains unchanged. Focused verification passes with `cargo test --lib mutual_aid`: 4 passed, 0 failed. Full library verification passes with `cargo test --lib --quiet`: 408 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: rule-engine attacks/fixed damage, quick casts, and non-encounter world-turn damage ticks now record actual HP loss rather than pre-cap overkill. Damage-derived buffs, `禅宗古训` lifesteal, `苏萨斯之爪` follow-up, and rule-engine damage events use effective damage; lifesteal counters also use HP actually restored. Zero/post-defeat damage no longer emits rule events, so recursive damage rules terminate naturally when the target reaches zero HP instead of continuing through phantom hits until the recursion guard. Focused verification passes with `cargo test --lib overkill`: 3 passed, 0 failed. Full library verification passes with `cargo test --lib --quiet`: 409 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: parsed battle damage now also caps applied damage at the target's actual remaining HP instead of counting overkill. Battle logs, turn and combat damage counters, `禅宗古训` lifesteal, `苏萨斯之爪` delayed follow-up, hit-trigger gates, and combat-exit recovery all consume the effective HP loss while shield absorption remains reported separately from harmless excess damage. Focused verification passes with `cargo test --lib -j 1 overkill -- --nocapture`: 4 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 411 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-16 correction: non-encounter world-turn buff ticks now apply the same relevant talent modifiers as quick casts and battle ticks. Outgoing damage/healing rolls `混沌无序`, healer-owned `生死时速` boosts healing to targets at or below 20% maximum HP, and typed damage applies the post-multiplier large-hit threshold for target-owned `过度免疫`; fixed damage remains unmodified. Focused verification passes with `cargo test --lib world_buff_ticks_apply_source_and_target_talent_modifiers`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib --quiet`: 410 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 privacy correction: group-chat retrieval now honors the campaign, character, party, and visibility scope persisted when each message was ingested instead of recomputing historical scope from the sender's current party. Moving a player between split parties can no longer expose their earlier party-only messages to the destination party, and messages recorded as public remain public after later party assignment. Unscoped legacy group records retain the existing current-membership fallback, while private records remain restricted to their peer. Focused verification passes with `cargo test --lib -j 1 persisted_group_message_visibility_survives_party_moves -- --nocapture` and `cargo test --lib -j 1 visible_messages_for_player_filters_split_party_group_chat -- --nocapture`: 2 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 412 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 privacy correction: player-visible chat retrieval and every DeepSeek summary-input scope now require the message's persisted campaign id to match the active campaign before applying player/party visibility. New private, public-group, and party-group summaries use collision-safe campaign-namespaced storage/count keys, so reusing the same QQ friend or group target in another campaign cannot merge raw inputs, suppress new summaries with another campaign's count, or display the prior campaign's generated summary as current. Existing unscoped summary records remain readable as explicitly legacy history. Focused verification passes with `cargo test --lib -j 1 campaign -- --nocapture`: 3 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 413 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 ingest correction: live NapCat events now discard any app-only campaign, character, party, or visibility fields before annotation because the local OneBot event schema provides only transport identity such as message type, user id, and group id. Willowblossom assigns scope from the active matching TRPG group or a unique configured QQ group/player mapping, allowing non-current campaign chats to retain their correct boundary while ambiguous or unknown targets fall back to the unscoped default instead of borrowing an unrelated open table. Focused verification passes with `cargo test --lib -j 1 incoming_message_scope_uses_trusted_noncurrent_target_mapping -- --nocapture`: 1 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 414 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 outbound-history correction: locally persisted copies of automatic private replies and GUI-sent private/group messages now derive campaign, character, party, and visibility metadata from the actual QQ recipient or group target. Sending to a uniquely mapped non-current campaign can no longer save the bot reply under whichever table the GM currently has open. Private replies remain player-only, while group sends remain public inside the correctly mapped campaign. Focused verification passes for all three non-current-target regressions: 3 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 417 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 private-command routing correction: player-originated commands now use the active matching or uniquely mapped TRPG group instead of requiring that group to be open in the GM UI. Non-current campaigns therefore use their own character-creation points, stat coefficients, guide, party-visible channel roster, and cooldown turn; most importantly, quoted private-message auto-forwarding continues to enforce that campaign's split-party boundary instead of falling back to an unrestricted chat-group broadcast. Ambiguous players shared by multiple non-current groups receive no inferred group context, and their quoted messages are not auto-forwarded until the GM's active table disambiguates the scope. Focused verification passes for non-current configuration, group commands, party-isolated forwarding, and ambiguous-scope denial: 4 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 421 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 chat-admission correction: first-contact private chats from players already assigned to a uniquely mapped non-current campaign now use that campaign's join-request policy and remain in that campaign when the GM opens the chat. Approval can no longer silently add the player to whichever unrelated table is open, and onboarding sends the mapped campaign's guide. Players mapped ambiguously across multiple non-current campaigns remain pending without automatic assignment until the active table disambiguates them; genuinely unknown senders retain active-table onboarding. Focused verification passes for mapped allow/deny, membership preservation, ambiguity-safe pending approval, and mapped guide selection: 4 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 425 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 scene-observation correction: `#观察`/`.观察` capture requests are now created only for players or configured GM users in the active TRPG campaign, and each request carries that campaign id for revalidation immediately before the Bevy scene begins rendering. A player messaging from a background campaign, an unknown sender, or a request left queued while the GM switches campaigns can no longer receive the foreground campaign's public scene image. Existing voxel, standee, token, and area-marker party/player visibility filters still apply after the campaign gate. Focused scene-capture verification passes: 4 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 426 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 battle-turn correction: the roster action checkbox now completes an action through the same canonical lifecycle used by attacks, skills, skipping, and the dedicated completion button instead of directly mutating `action_done`. Manual roster completion therefore advances participant and combat clocks, progresses cooldown state and persisted group turns, and starts the next round when all living actors finish. Completed actions are shown as checked but cannot be unchecked to grant a second action after damage, MP costs, or cooldowns have already been committed. Focused roster/cooldown verification passes: 2 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 427 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 forced-round correction: confirming a round advance while living actors still have pending actions now consumes those skipped turns before resetting the roster. Each unfinished living actor advances its participant clock and, during active combat, its per-actor and encounter combat counters, so cooldowns and later persisted group-turn synchronization cannot remain one turn behind or grant a free replacement action. Actors that already finished are not advanced twice, defeated actors remain unchanged, and normal automatic rollover keeps its existing single-advance behavior. Focused forced/automatic rollover verification passes: 2 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 428 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 battle-clock synchronization correction: the parsed battle clock now maps a participant's just-completed action to the TRPG group's separate completed-turn plus `acted` representation without counting the same action twice. A stale open battle no longer clears a newer GM-set group skip or rolls back a newer group round, while an actual battle round advance still clears prior completion flags and advances the group clock. This keeps cooldown, negative-timer, and buff timing inputs consistent across both GM control surfaces. Focused synchronization verification passes: 4 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 441 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 reverse battle-clock synchronization correction: opening an encounter whose TRPG group clock is newer now imports the manager's current player vitals before advancing each missing encounter round, ages encounter-local and unit-buff timing once, and adopts current-round group `acted`/`skipped` completion before actions are offered. The catch-up updates combat counters and regen without repeating player buff advancement already performed by the group control, while stale older group completion flags cannot finish a newer battle action. Focused reverse-synchronization verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 444 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 experience-threshold safety correction: Moonberry's next-level formula now evaluates in a wide integer before clamping to the persisted `i32` range. Valid but extreme imported character levels can no longer overflow and panic when the status/editor UI asks for the next threshold; non-positive levels still normalize to level 1 and oversized thresholds display as `i32::MAX`. Focused minimum/maximum-level verification passes: 1 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 444 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 turn-counter safety correction: canonical TRPG-group and parsed-battle round transitions now reject advancement at `u32::MAX` before clearing action flags, applying regen/delayed effects, ticking buffs, or writing a new round log. Per-participant turn and negative counters saturate when the final representable action is consumed, so extreme but valid imported clocks cannot panic or wrap into a fresh campaign round. Focused group/battle boundary verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 447 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 bounded battle-clock catch-up correction: reverse TRPG-group-to-battle synchronization now replays at most 64 missing rounds per rendered frame instead of iterating an arbitrary imported `u32` gap synchronously. Every replayed round still applies its existing encounter-local, regen, delayed, and unit-buff semantics, but a very large valid imported clock can no longer freeze one UI frame. While any gap remains, the battle panel shows Chinese synchronization progress and exposes no roster or action controls, preventing stale-round actions until the encounter reaches the canonical group clock. Focused extreme-gap and ordinary catch-up verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 448 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 battle-identity persistence correction: new encounter creation no longer trusts the persisted next-id counter as an unused, incrementable value. Allocation now skips every occupied `battle-N` id and wraps safely from `u64::MAX` to 1, so stale or extreme imported counters cannot overwrite an existing encounter or panic when the GM creates a battle. Focused boundary and ordinary-creation verification passes: 2 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 449 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 duplicate-encounter ownership correction: each TRPG group now has one deterministic canonical writable encounter, preferring the greatest persisted round and using the selected encounter to break equal-round ties. Creating another battle for that group reopens the canonical encounter instead of creating a competing copy. Existing imported duplicates remain visible with a Chinese explanation and delete control, but UI and state-layer guards prevent them from advancing rounds, synchronizing clocks or buffs, resolving actions or skills, resuming actors, or applying negative skips. This prevents duplicate regen/buff aging and stale HP/MP writes against the same campaign characters. Focused canonical-selection, duplicate-mutation, and boundary-allocation verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 452 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 deleted-group battle correction: an encounter that explicitly references a removed TRPG group is now shown as locked with a Chinese explanation and delete control. Manager synchronization validates that binding before writing any HP, MP, turn totals, cooldowns, or buffs, so an orphaned campaign battle cannot mutate characters that may now belong to another table. Deliberately unbound standalone encounters retain their existing local/player synchronization behavior. Focused deleted-group isolation and standalone/dominion/skill compatibility verification passes: 5 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 453 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 campaign-identity creation correction: GUI-created TRPG groups and fallback Moonberry bundle groups now receive a campaign ID derived from their group name instead of every group silently sharing `default`. Allocation checks both live groups and persisted message history, adding a numeric suffix when necessary; deleting and recreating a group name therefore cannot make the new table inherit the old table's private chat history. Existing groups and explicitly edited campaign IDs remain unchanged. Focused distinct-creation, delete/recreate history isolation, and non-current routing verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 455 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 battle campaign-binding correction: newly created group battles now persist both the TRPG group key and its durable campaign ID. Canonical encounter selection, group-clock catch-up, buff advancement, skill execution, and manager writes require that binding to match, so deleting and recreating a group name cannot reactivate the old campaign's battle or overwrite replacement-table characters. Legacy battle saves without the new field bind once to the first valid current campaign when opened; a mismatch is then shown as a locked Chinese delete-only panel. Focused creation binding, delete/recreate separation, legacy one-time migration, and cross-campaign write rejection verification passes: 4 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 458 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 ambiguous legacy campaign correction: persisted saves containing two or more TRPG groups with the same effective campaign ID now re-key every group in that ambiguous set to a distinct ID unused by live groups or message history. Ambiguous old messages remain GM-visible operational history but fail closed for player views and summary input instead of being assigned to an unverifiable table. A single legacy `default` campaign retains its ID and history unchanged. Campaign IDs are now read-only metadata in the GUI, preventing accidental edits from collapsing established privacy boundaries again. Focused ambiguous-history denial, unique-legacy preservation, and new-group compatibility verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 460 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 no-active-campaign correction: player-visible history, unread badges, summary scope discovery, summary input filtering, automatic summary queuing, and scene-capture campaign resolution now require an explicitly selected valid TRPG group. Clearing or deleting the current group no longer silently treats legacy campaign `default` as active and cannot expose or summarize its history. Private local replies retain their existing recipient-only visibility once their campaign is actually selected. Focused no-selection surfaces, private-reply compatibility, group-summary filtering, and split-party player filtering verification passes: 4 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 462 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 public-history scope correction: NapCat messages now persist whether their campaign/character/party visibility scope was explicitly resolved. A public group message sent by an unbound QQ user therefore remains public if that user later joins a split party instead of being retroactively reclassified as party-only. Inbound payloads cannot spoof the marker because ingest clears and recomputes all access metadata; imported Moonberry records and locally stored sends are also marked resolved. Unmarked legacy records retain current-membership fallback for compatibility. Focused immutable-public-history, party-move, and trusted-target-ingest verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 463 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 duplicate battle-identity correction: TRPG group turn synchronization now removes duplicate player IDs while preserving the surviving player's clock/action state, and new group battles defensively create only one participant per target ID. Loaded battle stores repair and persist duplicate participant snapshots across every encounter before runtime entities are synchronized; manual roster refresh applies the same repair. This prevents action lookup from repeatedly selecting the first duplicate while an unreachable second copy permanently blocks automatic round completion. Focused group normalization, new-round completion, refreshed-roster repair, and multi-encounter loaded-store repair verification passes: 4 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 467 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 linked-battle roster correction: the player picker for a battle bound to a TRPG group now lists only that group's players instead of every character and chat target known across all campaigns; standalone encounters retain the broader GM-managed roster. Persisted outsider player snapshots are pruned before a linked encounter exposes combat controls while unit-template instances remain valid, and manager synchronization independently refuses to write any outsider snapshot into global character HP, MP, counters, or cooldown state. Missing group links fail closed with no player candidates. Focused linked/standalone candidate filtering, outsider-roster pruning, and cross-group write-denial verification passes: 3 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 470 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 unread-privacy correction: the player-visible chat-list filter now applies the active campaign boundary to unread badges as well as target inclusion, message counts, and timestamps. A public or otherwise readable message persisted for another campaign can no longer reveal background activity through an unread count while its content remains hidden. Party/player visibility filtering and the GM's unfiltered operational view remain unchanged. Focused player-filter verification passes: 2 passed, 0 failed. Full library verification passes with `cargo test --lib -j 1 --quiet`: 429 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 batch-delivery correction: NapCat multi-recipient sends now track each request together with its actual target until every queued response arrives. Partial or out-of-order failures no longer let a later success erase the error or clear the GM's draft, and successfully acknowledged recipients are still written to their correctly scoped local chat histories instead of being lost with the failed targets. Fully successful private/group sends retain the existing draft-clear behavior, while an enqueue failure after earlier recipients were queued is treated as a partial delivery. Focused acknowledgement and legacy-composer verification passes: 5 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 431 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 batch-retry correction: when a multi-recipient send is only partially delivered, retrying the unchanged retained draft now excludes recipients whose matching NapCat requests already succeeded. Failed, rejected, or not-yet-queued targets remain eligible, preventing duplicate private clues or party instructions after transient failures. Editing the draft intentionally starts a fresh delivery to every currently selected target. Focused send-state verification passes: 5 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 438 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 NapCat acknowledgement correction: outbound WebSocket actions now carry a unique `echo` and remain pending until NapCat returns the matching action response. Socket writes are no longer treated as successful delivery before the documented `status: "ok"` and `retcode: 0` response arrives; API rejections keep GM-composed drafts/errors active and do not create false local sent history for those sends. Unanswered requests fail after 15 seconds, and disconnects fail every outstanding request so the UI cannot remain stuck in `发送中...`. Existing semantic group-info echoes are preserved and still update group names. Focused echo, response, rejection, and timeout verification passes: 3 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 434 passed, 1 ignored live API test, 0 failed.

Additional 2026-07-18 automatic-reply correction: private command/character-creation replies and privacy-filtered quoted-message forwards now retain typed pending recipient/text metadata until their correlated NapCat result arrives. Successful acknowledgements append the reply to the actual recipient's locally persisted, campaign/party-annotated private history; rejected, timed-out, disconnected, or mismatched results append nothing. This removes both the previous false optimistic command-reply history and the missing history for successful automatic forwards. Focused acknowledged/rejected automatic-reply verification passes: 2 passed, 0 failed. `cargo check -j 1 --quiet` passes. Full library verification passes with `cargo test --lib -j 1 --quiet`: 436 passed, 1 ignored live API test, 0 failed.

## What Moonberry Had

Moonberry was a React/Umi/MobX GM/ST tool backed by `mirai-api-http`. Its useful behavior surface was much larger than just chat:

- QQ private-chat onboarding:
  - unknown private senders became pending join requests,
  - ST could approve/reject,
  - accepted players were added to the chat list and received onboarding text,
  - outbound ST messages were appended locally as self messages.
- Private command handling:
  - `.兑换` / `。兑换`
  - `.观察` / `。观察`
  - `.抽取天赋`
  - `.抽取辅助天赋`
  - `.状态`
  - `.已兑换`
  - `.冷却`
  - `.频道人员`
  - `.` / `。` for creation advance
  - `..`, `.。`, `。.`, `。。` for creation back
  - `.<属性> <数字>` / `。<属性> <数字>` for post-creation stat spending.
- Character creation:
  - state sequence `normal -> str -> agi -> dex -> vit -> int -> wis -> k -> cha -> confirmStatus -> skill -> confirmSkill -> img -> nickname`,
  - eight base stats: `str`, `agi`, `dex`, `vit`, `int`, `wis`, `k`, `cha`,
  - defaults around `hp=5`, `mp=0`, `lv=1`, `speed=3`, `statusPoint`, `exchangePoint`,
  - group-level initial status/exchange points and stat formula config.
- Campaign/table model:
  - `Group` with description, ST description, guide, basic config, PCs, chat list, chat history, send panes, teams, worlds, current modals, negative/timeout state.
  - `Team` as a channel-like subset with PCs, buffs, visibility/window geometry, local chat, and nickname-repeat/nemo flags.
  - `IWorld` with PC/NPC ids, map, chat areas, and areas.
  - `IArea` / chat area with rectangle, members, and combat flag.
  - multiple send panes with mixed targets: all, players, teams, and chat areas, plus duplicate-target pruning.
- Skill/talent/pool model:
  - skill fields for name, type, target count, target class, caster, cost, cooldown, cooldown-left, range, description, `stInited`, `pcInited`, `poolId`, args, and `buffMachine`,
  - skill target classes: `无目标`, `单目标`, `多目标`, `范围`,
  - skill types: `法术`, `道具`, `异能`, `动作`, `血统`, `职业`, `召唤物`, `远程`,
  - large built-in talent pool with normal/support talents,
  - unit pool and random pool.
- Rule/buff resolution:
  - damage types: Magical, Physical, Cursed, Diseased, bleed, Range, poisoning, None,
  - heal types: Instant and continue,
  - buff effects over HP/MP/max/regen/stats/damage/heal modifiers,
  - graph editor nodes converted to buffs,
  - damage/heal pipelines with pre/post buff hooks, low-HP punishment, overflow heal, talent triggers, and turn/buff ticking.
- Import/export:
  - root export,
  - PC export,
  - chat history/chat list export,
  - combined PC/chat import/export.
- UE4 observation bridge:
  - `.观察` could send an observation/capture request to the external UE4 sidecar.

## Implemented In Willowblossom

These are present now, often as a Rust/Bevy redesign rather than a direct port:

- NapCat has replaced Mirai for the active QQ path:
  - parses private and group messages,
  - persists messages through `bevy_persistent`,
  - supports versioned JSON export/import for the persisted NapCat/TRPG store,
  - stores campaign/character/party/visibility metadata on messages,
  - tracks editable chat target metadata,
  - handles pending/open/rejected chat windows,
  - filters quoted private auto-forward recipients through the current TRPG group's party visibility,
  - sends private and group text through `send_private_msg` / `send_group_msg`,
  - appends successful local sent messages as GM/self messages,
  - requests group info for group chat display names,
  - caches incoming images.
- Character creation is mostly ported:
  - `.兑换` / `。兑换`,
  - `.` / `。` advance,
  - `..`, `.。`, `。.`, `。。` back,
  - `.状态`,
  - `.已兑换`,
  - `.冷却`,
  - `.频道人员`,
  - `.抽取天赋`,
  - `.抽取辅助天赋`,
  - post-creation `.<属性> <数字>` / `。<属性> <数字>` stat spending,
  - the full old creation step sequence,
  - eight old stats and old default status/exchange point values,
  - derived HP/MP/regen/speed recalculation from the current TRPG group's formula config,
  - image and nickname steps,
  - duplicate nickname rejection,
  - local private text replies.
- Player character data is expanded:
  - HP/MP/max/regen/level/exp/speed/modifiers,
  - old Moonberry next-level experience threshold display and GM manual XP award with carryover leveling,
  - old Moonberry per-turn damage/heal counters imported from `tdpt`/`thpt`,
  - creation status and GM extra status,
  - skills with name, note, MP cost, cooldown,
  - active buffs,
  - inventory/equipment/gold.
- GM UI exists for:
  - chat list and chat windows,
  - pending chat request approval/rejection,
  - approving private pending chats into the current TRPG group,
  - versioned NapCat/TRPG data import/export,
  - TRPG group creation/deletion/current group selection,
  - partial Moonberry legacy JSON import for old root/config exports,
  - assigning private players and QQ group chats to TRPG groups,
  - per-group build/stat formula controls,
  - character editing,
  - buff editing,
  - inventory editing,
  - random pool item/text min-max editing and draw previews,
  - random pool old group/tag/description metadata editing and filtering,
  - random pool checked per-PC result staging with editable/disableable rows and private sends,
  - random pool batch text draws sent to current TRPG group private scopes, including imported old send-pane scopes,
  - reusable unit/NPC template pool editing,
  - matching old world NPC/member ids to imported unit templates and placing them as scoped scene tokens,
  - skill pool editing with visible legacy type/tags/args metadata,
  - all-member or current-party private broadcasts from chat group windows,
  - imported Moonberry old send-pane scopes for private broadcasts through legacy teams and chat areas,
  - standalone imported old send-pane composers in TRPG settings, using the normal NapCat send/ack/local-history path,
  - independent floating windows for imported old send panes, including fixed old panes that auto-open,
  - old send-pane duplicate direct-PC pruning when a selected old channel/chat area/all target already covers that PC,
  - old send-pane multi-select target editing for all-player, direct PC, old channel, and virtual chat-area targets, with local add/remove/clear controls,
  - GM-controlled promotion of imported Moonberry old teams/chat areas into live Willowblossom parties,
  - live party creation, per-player movement, party merge, and party delete controls,
  - GM chat-window previews filtered through the selected player's access scope,
  - GM chat-list filtering by selected player's access scope, including filtered message/unread counts,
  - group world-turn/player-turn controls,
  - quick character windows and quick skill casting.
- TRPG groups exist as explicit data:
  - campaign id,
  - group public description, GM/ST description, player guide text,
  - per-group initial status and exchange point values for character creation,
  - per-group basic stat formula coefficients for HP, MP, regen, and speed,
  - legacy damage/heal coefficients applied to parsed rule/skill resolution,
  - Moonberry low-HP source damage penalty applied to parsed rule/skill resolution,
  - stored legacy experience coefficients for later progression use,
  - old Moonberry next-level experience threshold display and GM manual XP award with carryover leveling,
  - per-group join-request gate for unknown private senders,
  - GM QQ users,
  - players,
  - group chats,
  - parties, per-player party assignment, party merge, and party removal,
  - imported Moonberry legacy teams, worlds, rectangular areas/chat areas, NPC ids, and send panes as typed metadata,
  - GM-controlled promotion from imported legacy teams/chat areas into the current party visibility assignment model,
  - world turn,
  - per-player turn state,
  - tested public/party/player/GM/system access gates.
- Battle round support exists:
  - persistent encounters,
  - encounter creation from TRPG groups,
  - participants synced from characters,
  - reusable unit/NPC templates inserted as battle participants,
  - round advance/back,
  - action done/pending handling,
  - negative/lagging participant marking,
  - MP cost and cooldown checks,
  - static damage/heal skill application,
  - area target resolution from scene positions.
- Rule engine exists as a typed replacement direction:
  - parses simple Chinese rules for damage taken/dealt and skill cast,
  - supports damage/heal actions,
  - supports named grant-buff actions with turn duration and common typed field/value effects,
  - supports damage types and target selectors,
  - applies source/target modifiers and Moonberry low-HP source damage penalty,
  - stores active buffs in ECS,
  - supports buff expiry and recomputation from base stats,
  - has focused unit tests.
- Scene tooling exists:
  - Bevy voxel scene and planet/space starter content,
  - runtime voxel edit add/erase/brush,
  - persistent voxel maps,
  - persisted voxel edits with scene visibility metadata and legacy public default migration,
  - versioned voxel scene JSON export for maps, status snapshots, capture cameras, standees, unit scene tokens, legacy area markers, and legacy scene edits,
  - map copy/delete/rename/clear,
  - map status snapshots and restore,
  - minimap,
  - explicit voxel edit visibility selection for public, GM-only, current parties, or individual players,
  - persistent player capture cameras,
  - GM current-camera player visibility filtering without moving to a capture camera,
  - character and explicitly placed unit-template standees from character images with persisted visibility and player-filtered live/capture rendering,
  - image-free unit-template scene tokens with persisted visibility, GM unit-pool controls, scene export/merge, player-filtered live gizmo rendering, and scene-side position/visibility editing,
  - legacy world/chat-area markers can stamp visibility-preserving voxel borders or filled floor areas into the active editable scene map,
  - imported old world NPC ids and old area member ids can resolve matching unit templates and place scoped unit scene tokens,
  - `.观察` / `#观察` / `.gc` / `#gc` scene capture to private QQ image.
- DeepSeek summaries exist:
  - summaries are requested from access-filtered eligible player text,
  - group-chat summaries are split into public and per-party scoped keys,
  - per-party group-chat summary payloads include public plus that party's messages and exclude other parties,
  - scene-capture control commands are excluded from summary text,
  - summary prompt is constrained to summarize existing chat facts,
  - scoped summary blocks can be exported as versioned JSON without raw source text,
  - non-summary/legacy FIM-style DeepSeek messages are ignored rather than routed to story generation.

## Still Missing Or Partial

### High Priority

1. Explicit campaign/party/privacy access model is partial.

   Implemented now: persisted message `campaign_id`, `party_id`, `character_id`, `Visibility`, `PlayerAccess`, `can_read`, TRPG `parties`, `player_parties`, `gm_users`, incoming NapCat access annotation, local GM message annotation, access-filtered summary input, public/per-party DeepSeek summary requests for group chats, GM-side chat-window previews and chat-list filtering by a selected player's `can_read` scope, filtered player-visible unread/message counts for the chat list, persisted scene voxel edit `SceneVisibility` metadata that is preserved through scene edit indexing, undo/redo state, and hashed map signatures, explicit voxel-editor new-edit visibility selection for public/GM/current-party/player scopes, requester-specific scene-capture and live player-view voxel visibility filtering, GM current-camera player visibility filtering without jumping to a capture camera, persisted standee `SceneVisibility` with requester-specific live/capture visibility filtering, and visibility-aware private auto-forward recipients for current TRPG group members.

   Still missing: any future player-owned chat/export surfaces still need to be routed through the same access gate; and future non-voxel scene objects beyond character/unit standees and generic unit tokens still need first-class access-scoped visibility.

2. Moonberry teams/worlds/chat areas are partially ported as typed metadata, target surfaces, and optional live party scopes.

   Current `TrpgGroup` has players, group chats, parties, GM users, turn state, imported old `Team` records, imported `IWorld` records, rectangular `IArea`/`chatAreas`, combat flags, NPC id lists, chat-area membership lookup, and imported old `currentSendPanes`. The GM settings UI displays these legacy surfaces, private broadcast scope selection can expand old send panes through mixed direct player, team, and chat-area targets, the GM can explicitly promote an imported old team or chat area into the current Willowblossom `Visibility::Party` assignment model, and live parties can be merged or deleted with player access assignments updated with the party lifecycle.

   Remaining differences: old team local chat contents are preserved as imported GM-visible excerpts, have a GM private-send composer that appends new GM messages to the preserved local timeline, expose parsed local messages for GM edit/delete in settings, and can now be opened as independent egui floating chat windows using imported old window geometry as defaults. They still do not recreate Moonberry's exact browser chat component/window lifecycle; old team visibility remains metadata on the channel rather than a full browser window state model. Old worlds/chat areas can now be placed as persisted scene markers, rendered as visibility-filtered live scene gizmo overlays, stamped as editable voxel borders or filled floor areas, and linked to matching imported unit templates as scoped scene tokens with GM sync/remove lifecycle controls, but not yet as full semantic area entities with automatic gameplay membership behavior; and old overlapping team membership still requires GM choice because the current live party model assigns one party per player.

   Impact: old campaign surfaces are no longer discarded during import, can drive private sends, and can now be mapped into live party privacy when the GM chooses the matching old surface.

3. Old private commands are now mostly present, but not a full Moonberry clone.

   Implemented: `.兑换`, `。兑换`, creation `.`/`。`, creation back aliases, `.观察`/`#观察` aliases, `.状态`, `.已兑换`, `.冷却`, `.频道人员`, `.抽取天赋`, `.抽取辅助天赋`, and post-creation `.<属性> <数字>` / `。<属性> <数字>`.

   Remaining differences: talent draw now uses Moonberry's full active normal/support talent tables, preserves the old one-draw guard, records the chosen talent as a zero-cost Willowblossom skill with structured trigger/effect category metadata for every preserved talent entry, executes the clear immediate knowledge-stat effects for `那美克星之慧` and `物理专长`, applies the deterministic always-on numeric clauses for `大魔法师`, `人类基因工程`, `矢量压缩能量池`, and `狡黠之思` as derived passive buffs or typed damage modifiers, executes `混沌无序` as per-effect outgoing damage/heal variance, executes `苏萨斯之爪` as delayed magical follow-up from physical damage, executes `狂风恶浪` low-player-count speed escalation in parsed battle order, executes `越战越勇` per-completed-turn damage escalation in parsed battle, executes `斗志昂扬` first/second/third-turn parsed battle damage reduction, executes `狂妄` unique-damage-source battle damage escalation, executes `无尽痛楚` damage-taken next-hit battle damage escalation, executes `液态躯体` target-side delayed damage split and previous-turn self-healing in parsed battle, executes `敏锐` first range/non-targeted damage dodge in parsed battle, executes `无限专注` repeated single-target battle damage escalation, executes `总冠军` player-elimination damage/reduction stacking, executes `忏悔` kill/assist healing-bonus decay in parsed battle, executes `溃伤` as an on-damage one-turn healing-received debuff, executes `禅宗古训` as 15% lifesteal from final physical damage, executes `过度免疫` as 20% reduction to hits greater than 20% of target max HP, and executes `生死时速` as +50% healing when the target is at or below 20% max HP. It does not yet reproduce most executable conditional combat/timing talent triggers, summon/item side effects, other conditional/type-specific talent damage clauses, or any richer player choice/approval UX that old campaign operations may have handled outside chat commands.

   Additional update: `罪上加罪` now executes in parsed battle when a damage contributor participates in a kill, including assist credit. It increments the talent stack, restores 10% of missing HP and MP, and reports the capped experience-bonus metadata.

   Additional update: `役于我手` now executes when any target dies during an active encounter. Alive holders gain 5% of the defeated target's maximum HP as battle max HP, capped at 20% of the holder's battle-entry max HP; defeats while resting do not trigger it.

   Additional update: `一心` now executes in parsed battle for single-target healing, tracking the currently focused target and increasing same-target healing by +5% per stack up to +25%; switching targets resets the stack.

   Additional update: `千万回忆` now executes in parsed battle for single-target healing, scheduling 15% and 5% delayed healing echoes over the next two rounds from the resolved immediate heal amount.

   Additional update: `液态躯体` now executes during active parsed battle when direct skill damage is resolved against the holder, applying half immediately, delaying half to the next battle round, and healing from previous-turn damage on later active round advances. Resting damage and advances do not activate either effect.

   Additional update: `敏锐` now executes in parsed battle as a once-per-battle charge that dodges the first positive range/non-targeted incoming skill damage without being consumed by ordinary single-target hits.

   Impact: the old player chat workflow now works through QQ for the common commands, campaigns can draw from the old talent text pool, and the unambiguous immediate/passive numeric talents now affect character stats, but campaigns that rely on conditional combat talent triggers still need migration work.

   Additional update: `菜鸡猛啄` now applies an approved-talent minimum damage floor equal to character level in rule sync, quick-cast, and battle skill use. The floor is applied after damage reductions/boosts as untyped damage, while zero-damage effects remain zero.

   Additional update: `数魔转换器` now lets approved range damage enjoy positive magical damage bonuses, including INT-configured magical damage and `大魔法师`'s magical bonus, without inheriting negative magical penalties.

   Additional update: `火源之力` now applies approved-talent healing output scaling from the healer's current HP band: 20% while above 60% HP, 10% while above 20% HP, and no bonus while at or below 20% HP.

   Additional update: `互帮互助` now applies approved-talent healing feedback: healing another target sends 50% of the resolved heal back to the healer when the healer has the talent, and receiving healing sends 50% back to the healer when the target has the talent. Self-heals do not recursively trigger feedback.

   Additional update: `混沌无序` now applies approved-talent per-effect outgoing damage/healing variance, rolling each damage or healing effect between 85% and 115% in direct rule-engine resolution, quick-cast, and parsed battle skill use.

   Additional update: `苏萨斯之爪` now applies approved-talent delayed magical follow-up damage: physical damage schedules a one-turn-later magic hit equal to 35% of the actual physical damage in direct rule-engine resolution, quick-cast, and parsed battle skill use.

4. Join approval now has explicit rejection, active-group admission, and automatic group guide onboarding.

   Current unknown targets become pending chat requests when the current TRPG group allows join requests. The UI can approve them into open chat windows or reject them into a persisted refusal set so they do not reappear as pending requests. Approving a private-message target now adds that player to the current TRPG group and syncs turn/party bookkeeping; approving a QQ group-chat target does not accidentally add it as a player. TRPG groups now persist GM-authored player guide text, assigned players can request it with `.指南` / `.引导` without exposing it to non-members, and private-message approval automatically sends the current group's guide text through the normal NapCat send/ack/local-history path when a guide is configured.

   Remaining differences: Willowblossom uses the current TRPG group's GM-authored guide as the onboarding source instead of migrating any separate Moonberry hardcoded onboarding template. Empty guides are not auto-sent.

5. Skill approval/talent workflow is partially ported.

   Implemented now: character skills persist PC/GM approval flags, source kind, source pool id/label, and copied skill-pool source links. Player-submitted skills from `.兑换` now enter as PC-confirmed but GM-pending, show `GM待确认` in `.已兑换`, are counted in the GM character list, and stay out of quick-cast/rule sync/skill-pool sync until the GM approves them. Legacy skills with old `poolId` are marked as skill-pool sourced. Talent draw commands use Moonberry's full active normal/support talent tables, block a second talent draw, record talent source/trigger/effect category metadata for every preserved talent entry, execute the clear immediate knowledge-stat effects for `那美克星之慧` and `物理专长`, apply deterministic always-on numeric passive effects for `大魔法师`, `人类基因工程`, `矢量压缩能量池`, and `狡黠之思` through the same effective-buff path as legacy passives, apply `大魔法师`'s approved-talent +0.5% per INT magical damage bonus through the shared typed damage multiplier, apply `人类基因工程` disease/poison -15% incoming damage plus `抗魔体质` magical -10% incoming damage through the shared typed target-damage multiplier, apply `混沌无序` as approved-talent per-effect -15%~+15% outgoing damage/healing variance, apply `苏萨斯之爪` as approved-talent one-turn-later magical follow-up for 35% of actual physical damage, apply `狂风恶浪` as approved-talent battle order speed escalation to 35% while live player-character participants are <=3, apply `越战越勇` as approved-talent parsed-battle +2% damage per completed participant turn up to +20%, apply `斗志昂扬` as approved-talent parsed-battle incoming skill damage reduction of 50%/10%/2% on the target's first/second/third own turn, apply `狂妄` as approved-talent parsed-battle +10% damage per unique damage source that has hurt the actor up to +30%, apply `无尽痛楚` as approved-talent parsed-battle `等级*1.5` untyped next-hit damage per successful damage-taken stack up to 2 stacks, apply `液态躯体` as approved-talent parsed-battle 50% incoming direct damage delay plus 5% previous-turn damage self-healing, apply `敏锐` as approved-talent parsed-battle once-per-battle first range/non-targeted incoming skill damage dodge, apply `无限专注` as approved-talent parsed-battle +10%/+20% damage for repeated successful single-target attacks against the same target, apply `总冠军` as approved-talent parsed-battle +2% damage dealt and -1% incoming damage for each eliminated player-character target, apply `忏悔` as approved-talent parsed-battle healing-bonus decay by 10% per kill/assist contribution, apply `溃伤` as an approved-talent on-damage one-turn -25% healing-received debuff in rule sync, quick-cast, and battle skill use, apply `禅宗古训` as approved-talent 15% lifesteal from final physical damage in those same paths, apply `过度免疫` as approved-talent 20% reduction to final incoming hits above 20% max HP, and apply `生死时速` as the approved healer's +50% healing output when the target is at or below 20% max HP. The GUI exposes PC/GM approval toggles, pending labels, source labels, talent trigger/effect hints, and a compact optional skill-structure editor for type, target class/count, range, exchange point, cooldown-left, old caster id, old args, and old buff-machine presence/raw-size hints; the type and target fields now offer Moonberry's known old values while preserving editable custom/imported text. Auto skill-pool sync, rule sync, and quick-cast omit unapproved skills. Imported `cooldownLeft` now reports in `.冷却`, counts down against a persisted ready turn, and blocks quick-cast/battle skill use only until that turn is reached; preserved `target_count` caps quick-cast/battle resolved targets, `无目标`/`单目标` target classes enforce zero/one target caps, `范围` target class expands otherwise single-target effects into area target resolution, preserved positive `range` fills missing area radii and filters single selected targets for quick-cast target discovery and battle skill resolution, preserved numeric skill args execute as named amount placeholders and string/BUFF args execute as exact text substitutions in rule sync, quick-cast, and battle skill parsing, preserved active old `技能释放` buff-machine entries now convert common damage/heal/basic modifier effects into typed rule actions for rule sync, quick-cast, and battle skill use, approved legacy `被动` buff-machine entries now derive permanent effective buffs from skill args for common stats, HP/MP/regen, and damage/heal modifiers without persisting them as manual active buffs, graph-only or empty-eventBuff legacy blueprints now follow the old exec chain and convert simple active/passive damage/heal/basic stat/resource/modifier nodes, pool-backed `给予BUFF` plus graph `BUFF变量` references can now resolve imported skill-pool raw payloads into simple granted basic buffs during rule sync, pool-backed `给予BUFF` damage/heal payloads now become typed per-turn buff tick actions in the rule engine and quick-cast group turn path, and preserved skill type supplies the default damage type for untyped damage notes while explicit damage text still wins. `SkillPoolEntry` now keeps legacy pool id, type/category, tags, custom args, group/created-at hints, old buff/event-buff/graph presence flags, and compact raw JSON for old buff, eventBuffs, graph, and character-derived buffMachine payloads; old `skillsPool` root data imports into those fields.

   Remaining differences: Willowblossom still lacks non-damage skill type behavior, richer graph-backed BUFF arg semantics beyond pool-backed basic buff grants/tick actions and exact text substitution, graph branching/conditions beyond the old single exec chain, most executable conditional/battle talent triggers/effects beyond the implemented talent hooks listed above, and any richer target-class runtime semantics that old campaign data may require beyond count/range resolution.

   Impact: current skill handling now has durable approval/source state for GM workflows, player-submitted skills wait for GM approval, and old talent text data is preserved, but campaigns that rely on executable talent effects still need migration work.

   Additional implemented talent execution: `菜鸡猛啄` now floors single damage effects to at least the source level in the same rule sync, quick-cast, and battle paths as the other executable talent hooks.

   Additional implemented talent execution: `数魔转换器` now applies positive magical damage bonuses to approved range damage in helper/rule-sync, quick-cast, and parsed battle skill paths.

   Additional implemented talent execution: `火源之力` now applies a dynamic healer injury-state multiplier to direct rule-engine healing, quick-cast healing, continuing buff-tick healing, and parsed battle skill healing.

   Additional implemented talent execution: `互帮互助` now applies non-recursive source/target healing feedback in direct rule-engine healing, quick-cast healing, continuing buff-tick healing, and parsed battle skill healing.

   Additional implemented talent execution: `混沌无序` now applies a per-effect random 85%~115% outgoing damage/healing multiplier in direct rule-engine resolution, quick-cast, and parsed battle skill use.

   Additional implemented talent execution: `苏萨斯之爪` now applies a delayed magical damage tick equal to 35% of actual physical damage one turn later in direct rule-engine resolution, quick-cast, and parsed battle skill use.

   Additional implemented talent execution: 狂风恶浪 now raises parsed battle order speed from the normal +20% talent speed to +35% while live player-character participants are <=3.

   Additional implemented talent execution: `越战越勇` now raises active parsed-battle skill damage by 2% for each action completed in the current combat, capped at 20%; inherited world turns, resting actions, and prior combats do not preload the bonus.

   Additional implemented talent execution: `斗志昂扬` now reduces active parsed-battle incoming skill damage by 50%, 10%, and 2% before the target completes its first, second, and third actions in the current combat; inherited world turns and rest do not consume or apply the sequence.

Additional implemented talent execution: `狂妄` now records unique active-combat damage sources and raises the actor's skill damage by 10% per source, capped at 30%; resting damage and combat boundaries cannot retain or apply those sources.

Additional implemented talent execution: `无尽痛楚` now records active-combat successful damage-taken stacks and consumes up to two stacks on the actor's next positive active-combat skill hit, adding `等级*1.5` untyped damage per stack; resting damage and combat boundaries cannot retain or apply those stacks.

Additional implemented talent execution: `无限专注` now tracks active-combat repeated single-target attacks against the same target and raises damage by 10% then 20%, resetting when the actor successfully hits a different single target or crosses a combat boundary. Resting attacks neither gain stacks nor receive a stale focus bonus.

Additional implemented talent execution: `总冠军` now tracks parsed-battle player-character eliminations and grants the talent holder +2% damage dealt and -1% incoming damage per stack.

Additional implemented talent execution: `役于我手` now tracks parsed-battle target deaths and grants alive talent holders capped battle max-HP growth equal to 5% of the defeated target's max HP.

Additional implemented talent execution: `罪上加罪` now tracks parsed-battle kill/assist participation, grants one stack, restores 10% of missing HP/MP, and caps the exposed experience-bonus metadata at 10%.

Additional implemented talent execution: `忏悔` now tracks parsed-battle damage contributors and decays the talent's healing bonus by 10% for each kill/assist credit, bottoming at 0%.

Additional implemented talent execution: `一心` now tracks active-combat repeated single-target healing against the same target and raises healing by +5% per existing stack up to +25%, resetting when the healer switches targets or crosses a combat boundary.

Additional implemented talent execution: `千万回忆` now records parsed-battle delayed healing echoes from successful single-target heals, resolving 15% then 5% of the original heal on later round advances.

Additional implemented talent execution: `液态躯体` now records active-combat delayed damage and previous-turn damage healing, halving direct incoming skill damage into immediate and next-round portions without modifying resting hits or healing during resting advances.

Additional implemented talent execution: `敏锐` now records a parsed-battle once-per-battle dodge charge, spends it on the first positive range/non-targeted incoming skill damage, clears it during rest, and rearms it on battle re-entry while leaving ordinary single-target damage untouched.

Additional implemented talent execution: `奥术护盾` now grants battle entrants 10% of maximum MP as encounter-local shielding, consumes it before HP damage across the shared active-battle damage path, clears it on battle exit, ignores/normalizes stale resting values, and replenishes it on battle re-entry.

Additional implemented talent execution: `过度治疗` now converts battle overheal into one-round encounter-local shielding capped at 30% of the healed target's maximum HP across immediate and delayed healing paths.

Additional implemented talent execution: `不死者之怒` now provides one active-encounter lethal-hit negation, same-round damage immunity, and +10% outgoing parsed-skill/buff-tick damage while hits above maximum HP bypass it; rest clears the active effect, stale resting flags cannot apply it, and battle re-entry rearms one charge.

Battle damage resolution now distinguishes attempted, absorbed, and applied damage so shields/evasion do not falsely trigger successful-hit talent effects or inflate combat logs, and contributor attribution is cleared at combat boundaries so kill/assist rewards cannot leak across encounters.

Parsed-battle delayed damage now has one-shot persisted execution semantics: next-round `苏萨斯之爪` and `液态躯体` ticks are removed as soon as they apply, and stale post-fire countdowns from older saves expire without duplicate damage.

`希望化身` now executes as a persisted active-combat lethal transformation with two-round damage immunity, healing-only actions, forced expiry or battle-exit death, and fresh eligibility on later combat entry; channel interruption remains pending until battle channeling itself is represented.

`振奋` now executes only during active combat for positive single-target healing, with one-target transfer, non-stacking +10% speed/damage, global-round expiry, and boundary cleanup of persisted ownership.

`息心` now executes at the active-to-resting encounter transition, restoring 50% of persisted post-mitigation active-combat damage once without reviving defeated participants.

`以逸待劳` now accumulates up to ten persisted charges as the participant's natural turn advances during rest and consumes them on the next active-combat entry for 5% maximum-HP recovery per charge, capped at 50%, without reviving defeated participants or producing overheal shielding.

6. Import/export is partial.

   Willowblossom now has a versioned JSON export/import wrapper for the persisted `NapcatMessageManager`, exposed in the TRPG settings UI. That covers messages, chat metadata, character cards, TRPG groups, skill pools, random pools, unit pools, and chat window state that live in that store. It also has targeted JSON export/import-merge for PC/character cards, reusable unit/NPC templates, chat-list metadata without message bodies, scoped DeepSeek summary blocks without raw source text, and voxel scene data. It can also merge old Moonberry root/config JSON exports for groups, basic group descriptions/guide/initial points, old `basicConfig` stat formula coefficients, PCs, chat-list metadata, chat messages, skill-pool metadata, per-character skill shape metadata, unit-pool templates, random-pool text/min-max items plus old id/group/tag/description/created-at metadata, old per-PC negative timers, old teams, old worlds/chat areas, and old send panes.

   Remaining differences: the Moonberry importer is intentionally partial. It preserves old worlds, teams, chat areas, send panes, old team local chat excerpts, old team window geometry, and optional scene-store markers for legacy areas as typed metadata, private broadcast/GM preview surfaces, appendable GM local team-chat sends, editable parsed local team-chat messages, independent old-channel chat floating windows, visibility-filtered scene gizmo overlays, editable voxel border/fill stamping, and old NPC/member ids for unit-template token placement with scoped sync/remove controls, but does not recreate Moonberry's exact browser modal/window layout behavior, full executable graph/buff machines, semantic area entities, automatic gameplay membership from those entities, or UE4 bridge state. Skill-pool migration preserves metadata, old graph/buff presence flags, and compact raw JSON payloads; common active `技能释放` damage/heal/basic modifier payloads now convert into Willowblossom rule actions, common passive `被动` basic stat/modifier payloads now apply as derived effective buffs, simple graph-only active/passive exec chains now convert into the same typed effects, and pool-backed `给予BUFF`/graph `BUFF变量` references now expand imported skill-pool basic buffs and damage/heal tick buffs for rule sync and quick-cast turn advancement, but branching and full graph-editor behavior remain partial. Random-pool migration preserves item text, min/max counts, and old group/tag metadata, and the GM UI can filter/edit that metadata, stage checked per-PC results, and batch-send drawn text results through current-group/private imported send-pane scopes.

### Medium Priority

7. Rule/buff behavior is only a narrow typed subset.

   Willowblossom covers simple damage/heal rules, named grant-buff actions with common typed field/value effects, modifiers, Moonberry low-HP source damage penalty, buff fields, expiry, imported `cooldownLeft` blocking, `target_count` caps, no/single target-class caps, `范围` target-class area expansion, positive range fallback for area skills, range filtering for single selected targets, numeric skill args from preserved metadata as named amount placeholders, string/BUFF skill args as exact text substitutions, raw old buff/graph/buffMachine JSON preservation, active legacy `技能释放` buff-machine damage/heal/basic modifier conversion, passive legacy `被动` buff-machine basic stat/modifier conversion with derived status formulas, graph-only active/passive single-exec-chain conversion for simple damage/heal/basic nodes, pool-backed `给予BUFF`/graph `BUFF变量` conversion for simple granted basic buffs in rule sync, pool-backed `给予BUFF` damage/heal tick actions on turn advancement, immediate knowledge-stat talent effects plus always-on numeric passive talent buffs, `混沌无序` per-effect outgoing damage/heal variance, `苏萨斯之爪` delayed magical follow-up from physical damage, `狂风恶浪` low-player-count speed escalation in parsed battle order, `越战越勇` completed-turn damage escalation in parsed battle, `斗志昂扬` first/second/third-turn incoming damage reduction in parsed battle, `狂妄` unique-source damage escalation in parsed battle, `无尽痛楚` damage-taken next-hit escalation in parsed battle, `液态躯体` target-side delayed damage split and previous-turn self-healing in parsed battle, `敏锐` first range/non-targeted incoming damage dodge in parsed battle, `无限专注` repeated single-target escalation in parsed battle, `总冠军` player-elimination damage/reduction stacking in parsed battle, `忏悔` kill/assist healing-bonus decay in parsed battle, `溃伤` on-damage healing-received debuff execution, `禅宗古训` physical-damage lifesteal execution, `过度免疫` large-hit damage reduction execution, `生死时速` dying-target healing bonus execution, and all-table talent trigger/effect category metadata, preserved skill type as the default damage type for untyped damage notes, old `自己`/`技能目标` target wording, Moonberry's overflow-heal cap-at-max behavior, and per-turn damage/heal counters for character cards, quick-cast, battle snapshots, and the rule engine. Missing or partial relative to Moonberry: graph editor UI, graph branching/conditions beyond the old single exec chain, most executable conditional combat/timing talent triggers, and the fuller damage/heal hook pipeline.

   Additional update: rule/buff damage resolution now includes the `菜鸡猛啄` level-based minimum untyped damage floor after reductions/boosts, with focused rule, quick-cast, and battle coverage.

   Additional update: rule/buff damage resolution now includes `数魔转换器` range damage sharing positive magical damage bonuses, with focused helper, quick-cast, and battle coverage.

   Additional update: rule/buff healing resolution now includes `火源之力` as a source-side wounded healing multiplier, with focused rule, quick-cast, and battle coverage.

   Additional update: rule/buff healing resolution now includes `互帮互助` as source-side and target-side healing feedback to the healer, with focused rule, quick-cast, buff-tick, and battle coverage.

   Additional update: rule/buff damage and healing resolution now include `混沌无序` as a per-effect outgoing 85%~115% random multiplier, with focused rule, quick-cast, battle, and helper coverage.

   Additional update: rule/buff damage resolution now includes `苏萨斯之爪` as a one-turn delayed fixed magical damage tick equal to 35% of actual physical damage, with focused rule, quick-cast, battle, and helper coverage.

   Additional update: parsed battle order now carries participant speed plus a 狂风恶浪 low-survivor speed override, so the talent's +20% speed becomes +35% while live player-character participants are <=3.

   Additional update: active parsed battle skill damage now applies `越战越勇` as +2% damage per action completed in the current combat, capped at +20%, using a dedicated encounter-local counter.

   Additional update: active parsed battle skill damage now applies `斗志昂扬` from a dedicated per-combat action counter: 50% before the target's first completed action, then 10%, then 2%.

   Additional update: parsed battle now records unique damage sources for `狂妄`; each unique source raises the damaged actor's later skill damage by 10%, capped at 30%.

   Additional update: parsed battle damage now records contributors on each target; when a target is defeated, those contributors gain `忏悔` kill/assist credit and the talent's healing bonus decays from +25% to +15%, +5%, then +0%.

   Additional update: parsed battle defeat handling now also grants `罪上加罪` kill/assist credit to alive contributors, restoring 10% of missing HP/MP and exposing the capped per-stack experience bonus for battle UI/state.

   Additional update: parsed battle defeat handling now also grants `役于我手` battle max-HP growth to alive holders in the same encounter, using the defeated target's max HP and preserving the earned bonus across participant refreshes.

   Additional update: parsed battle healing now records `一心` same-target healing focus on the caster, applies the capped healing multiplier on subsequent single-target heals, and exposes the current stack in the battle roster.

   Additional update: parsed battle healing now schedules `千万回忆` delayed healing ticks from successful single-target heals and advances them with the battle round clock as separate healing events.

   Additional update: parsed battle incoming damage now applies `液态躯体` as a target-side delayed-damage split and previous-turn self-heal, with pending delayed damage visible in the battle roster.

   Additional update: parsed battle incoming damage now applies `敏锐` as a target-side once-per-battle dodge of the first positive range/non-targeted skill hit, with the ready charge visible in the battle roster.

   Additional update: parsed battle encounter exit now applies `息心` as a one-shot heal for 50% of post-mitigation damage recorded while the encounter was active, then clears the persisted tally before resting.

8. Per-group rules/config are partial.

   Implemented now: editable `TrpgGroup` campaign id, public description, GM/ST description, player guide text, initial status points, initial exchange points, per-group `basicConfig` formula coefficients, old `runTimes`, root/group battle sort default, root/group negative battle default, typed old per-PC negative timers, and an `allow_join_requests` gate. `.兑换` uses the current assigned TRPG group's initial point config, and backtracking refunds the configured total rather than the global default. Character creation, post-creation stat spending, and GM character editing derive HP/MP/regen/speed from the target player's current group formula. Old Moonberry `basicConfig` fields for HP/MP/regen/speed, damage/heal coefficients, and experience coefficients are imported and editable. New battle encounters inherit the group's sort/negative defaults. TRPG turn rows can now start/reset the old two-minute negative countdown, mark/send half-time and timeout notices, record negative layers, skip timed-out PCs through the existing turn path, start countdowns when half the table is ahead, and cancel active countdowns when a player replies. Damage/heal coefficients now apply to typed rule-engine damage/heal, quick character skill casts, and parsed battle-round skill damage/heal: magical damage uses INT, physical damage uses STR plus AGI modulo 50 plus DEX, ranged damage uses DEX, and healing uses INT plus WIS, multiplied with existing source/target combat modifiers. Damage paths also apply Moonberry's low-HP source damage penalty. Character status and the GM editor show Moonberry's old `geneMaxExp(level)` next-level threshold, and the GM editor can grant XP that auto-levels with carryover.

   Remaining differences: experience coefficients are still preserved as configuration data, but the old source did not reveal a concrete reward formula using them, so automatic level-difference XP reward calculation is not implemented.

9. Unit/NPC pool is partial.

   Implemented now: `NapcatMessageManager` persists a reusable `unit_pool` keyed by unit id, each entry stores a label, note, optional old member id, and full `PlayerCharacter` template. The GM UI can create templates, copy existing player characters into the pool, edit core identity/resources/stats and the old member id, delete templates, and import/export the pool as versioned JSON without pulling in chat history. Battle rounds can add multiple copies of unit templates as NPC participants, keep them through group-player refresh, sync their template stats, and use template skills. Unit templates with images can be explicitly placed, updated, and removed as scene standees; those standees use a unit namespace, persist through the voxel scene store, and participate in the existing live/capture visibility filtering. Unit templates can also be explicitly placed as image-free generic scene tokens; those tokens persist in the voxel scene store, export/merge with scene data, have GM unit-pool place/update/remove controls, render as visibility-filtered live scene gizmos, and can be repositioned or assigned public/GM/party/player visibility from a scene-side token editor. Old Moonberry `unitPool` entries can be imported into this pool with their old PC/NPC id preserved, and old world NPC ids or old area member ids can place, sync stale entries, and remove matching imported unit templates as scoped scene tokens.

   Remaining differences: legacy unit tokens cover matching, scoped lifecycle, position, visibility, and live-gizmo behavior rather than richer editable token stats or automatic world/chat-area gameplay membership behavior.

10. Random pool is redesigned and partial.

   Current random pools are weighted inventory-item pools with direct award to a character, and now also store optional textual results with min/max counts plus old Moonberry pool id/group/tag/description/created-at metadata. The GM UI can edit text/count/metadata fields, filter pools by old group and tags, a draw uses one selected entry for both the item award and text/count preview, old Moonberry `randomPool` items import into those fields, the TRPG settings UI can draw a batch of text results and send one numbered message to the current group's all-member, party, or imported old send-pane private scope, and it can generate a checked per-PC result staging list for the same scoped private targets, edit/disable rows, and send checked rows as individual private messages.

   Remaining differences: random pools remain integrated with Willowblossom inventory weighting and egui settings controls instead of recreating the old browser table/modal layout exactly.

11. Old send-pane targeting and editing are mostly ported for imported panes.

   Willowblossom has individual chat windows, chat groups, TRPG group workspaces, private/group sends, private group broadcasts, party-scoped private broadcasts based on the current TRPG group, party-aware quoted private auto-forward, imported old `currentSendPanes` that can expand mixed direct player, team/channel, all-player, and virtual chat-area targets into private broadcasts, TRPG settings composers for those imported send panes, independent floating windows for imported send panes, fixed old panes that auto-open when imported, old duplicate direct-PC pruning/all-target collapse, multi-select target editing for all-player/direct-PC/channel/chat-area targets, local add/remove/clear controls for those legacy panes, and random-pool batch text sends through those private scopes.

   Remaining differences: Willowblossom uses egui settings/floating-window composers instead of recreating the old browser tab component layout and modal/window geometry.

12. Character stat formulas are configurable for derived stats, but not full old gameplay effects.

   Current derivation preserves Willowblossom defaults for new groups (`max_hp = 5 + level*5 + str + vit*3`, MP from int/wis, regen/speed from stats), imports old Moonberry group `basicConfig` coefficients, and lets the GM edit the formula per group. Damage/heal coefficients and Moonberry's low-HP source damage penalty now participate in parsed skill/rule resolution, and Moonberry's old next-level XP threshold is used for status display and GM-awarded XP leveling. Missing pieces are executable use of stored experience reward coefficients and any broader gameplay comments or stat threshold effects that Moonberry treated outside simple stat derivation.

### Lower Priority Or Intentionally Different

13. Mirai code remains as legacy.

   `src/mirai/mod.rs` still exists, but the active path is NapCat. This is acceptable as legacy cleanup debt unless it causes confusion or accidental use.

14. UE4 bridge is not ported, and should probably remain obsolete.

   Moonberry used a UE4 sidecar for observation. Willowblossom now has Bevy scene capture, which is the correct replacement. Missing behavior should be measured against scene capture and visibility, not UE4 itself.

15. MobX/Umi/localStorage/Ant/MUI architecture is intentionally not ported.

   Current Rust/Bevy/egui plus typed persisted stores match the rewrite direction.

16. README TODO has been refreshed.

   README now separates implemented NapCat/TRPG/battle/scene work from remaining gaps and links back to this audit for detailed Moonberry migration differences.

## Suggested Implementation Order

1. Extend the access model beyond messages:
   - scene records that carry visibility,
   - player-facing scene/chat views that call `can_read`,
   - per-party summary blocks for split-party group chat,
   - outbound send targeting that uses party/player visibility,
   - broader non-voxel scene/editor tools that can operate in an explicit GM/player visibility mode.

2. Rework TRPG groups into campaign plus parties:
   - keep QQ group chats separate from story parties,
   - continue refining imported old team/chat-area promotion into live party/access scopes,
   - continue refining GM-controlled split/merge/move UI,
   - add tests for same-party allow, cross-party deny, own-private allow, GM allow, public allow.

3. Extend talent support beyond old text tables:
   - keep extending from the newly executable `菜鸡猛啄` level-based damage floor, `数魔转换器` range/magic bonus sharing, `火源之力` wounded healing multiplier, `互帮互助` healing feedback, `混沌无序` output variance, `苏萨斯之爪` delayed physical-damage follow-up, `狂风恶浪` low-survivor speed escalation, `越战越勇` completed-turn damage escalation, `斗志昂扬` opening-turn damage reduction, `狂妄` unique-source damage escalation, `无尽痛楚` damage-taken next-hit escalation, `液态躯体` delayed-damage/self-heal timing, `敏锐` first range/non-targeted damage dodge, `无限专注` repeated single-target escalation, `总冠军` player-elimination damage/reduction stacking, and `忏悔` kill/assist healing-bonus decay as stepping stones into other concrete trigger/effect clauses,
   - use the new `役于我手` encounter-death max-HP growth as a stepping stone for non-contributor encounter-participation talent clauses,
   - use the new `罪上加罪` kill/assist recovery stack metadata as another stepping stone for concrete participation-trigger talent clauses,
   - use the new `一心` repeated-healing focus stack as a stepping stone for support-side combat-round healing triggers,
   - use the new `千万回忆` delayed healing echo scheduler as a stepping stone for other delayed support-side talent triggers,
   - use the new `液态躯体` delayed-damage/self-heal hook as a stepping stone for other target-side timing talents,
   - use the new `敏锐` once-per-battle dodge hook as a stepping stone for other target-side avoidance or targeting-sensitive talents,
   - use the new `息心` active-to-resting transition hook as a stepping stone for other battle-entry and battle-exit talent clauses,
   - conditional combat/timing talent triggers/effects beyond immediate knowledge-stat and always-on numeric passive talents,
   - richer talent choice/approval UX if old campaign operations need it,
   - executable typed talent effects from the preserved trigger/category metadata,
   - optional GM approval hooks,
   - migration from old Moonberry talent records if needed.

4. Port skill approval workflows:
   - executable use of non-damage skill type behavior,
   - richer graph-backed BUFF arg semantics and complex graph migration beyond simple active/passive exec chains,
   - GUI-first equivalents where chat commands are not enough.

5. Extend import/export beyond the root NapCat/TRPG store:
   - fuller old Moonberry JSON migration only when specific old campaign data needs it.

6. Expand rule/buff engine only after the privacy model is in place:
   - broader old buff-machine hooks beyond common active/passive eventBuffs and simple graph chains,
   - richer BUFF arg semantics beyond exact text substitution,
   - richer target-class runtime semantics if old campaign data needs them,
   - richer unit/NPC world/chat-area membership and token stat behavior,
   - broader tests for old Moonberry combat effects.
