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

Verification during audit: `cargo test` passed: 76 passed, 1 ignored live DeepSeek API test, 0 failed. After the access-model, private-command, chat-approval, summary-only DeepSeek, root NapCat/TRPG import/export, targeted PC/chat-list exports, targeted PC/chat-list/summary/scene import-merge, DeepSeek summary export, voxel scene export, party-scoped private broadcast, per-group guide/config, active-group admission, join-request gate, party-scoped group-summary, scene voxel visibility metadata, visibility-aware private auto-forward, automatic private-approval guide onboarding, scene-capture voxel visibility filtering, live player-view voxel visibility filtering, skill approval/source metadata, unit-pool export/UI, unit-template battle insertion, partial Moonberry legacy JSON import, random-pool text/min-max result, legacy skill-pool metadata import, character-skill shape metadata preservation, and per-group `basicConfig` stat formula implementation pass, `cargo test` passes: 149 passed, 1 ignored live DeepSeek API test, 0 failed. Targeted formatting for changed Rust files passes.

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
  - reusable unit/NPC template pool editing,
  - skill pool editing with visible legacy type/tags/args metadata,
  - all-member or current-party private broadcasts from chat group windows,
  - group world-turn/player-turn controls,
  - quick character windows and quick skill casting.
- TRPG groups exist as explicit data:
  - campaign id,
  - group public description, GM/ST description, player guide text,
  - per-group initial status and exchange point values for character creation,
  - per-group basic stat formula coefficients for HP, MP, regen, and speed,
  - stored legacy damage/heal/experience coefficients for later rule-engine use,
  - per-group join-request gate for unknown private senders,
  - GM QQ users,
  - players,
  - group chats,
  - parties and per-player party assignment,
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
  - supports damage types and target selectors,
  - applies modifiers,
  - stores active buffs in ECS,
  - supports buff expiry and recomputation from base stats,
  - has focused unit tests.
- Scene tooling exists:
  - Bevy voxel scene and planet/space starter content,
  - runtime voxel edit add/erase/brush,
  - persistent voxel maps,
  - persisted voxel edits with scene visibility metadata and legacy public default migration,
  - versioned voxel scene JSON export for maps, status snapshots, capture cameras, standees, and legacy scene edits,
  - map copy/delete/rename/clear,
  - map status snapshots and restore,
  - minimap,
  - persistent player capture cameras,
  - character standees from character images,
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

   Implemented now: persisted message `campaign_id`, `party_id`, `character_id`, `Visibility`, `PlayerAccess`, `can_read`, TRPG `parties`, `player_parties`, `gm_users`, incoming NapCat access annotation, local GM message annotation, access-filtered summary input, public/per-party DeepSeek summary requests for group chats, persisted scene voxel edit `SceneVisibility` metadata that is preserved through scene edit indexing and hashed in map signatures, requester-specific scene-capture and live player-view voxel visibility filtering, and visibility-aware private auto-forward recipients for current TRPG group members.

   Still missing: player-facing chat views are not all routed through the access gate; broader scene/editor workflows still need a first-class GM/player visibility mode beyond capture camera previews; and old arbitrary send-pane target pruning is not fully ported.

2. Moonberry teams/worlds/chat areas are not ported as privacy surfaces.

   Current `TrpgGroup` has players, group chats, parties, GM users, and turn state. It does not cover old `Team` local chat/buffs/window visibility, `IWorld`, rectangular `IArea`, `chatAreas`, combat flags, NPC membership, or chat-area membership lookup.

   Impact: split-party play inside one QQ group is not represented strongly enough for the requested private knowledge model.

3. Old private commands are now mostly present, but not a full Moonberry clone.

   Implemented: `.兑换`, `。兑换`, creation `.`/`。`, creation back aliases, `.观察`/`#观察` aliases, `.状态`, `.已兑换`, `.冷却`, `.频道人员`, `.抽取天赋`, `.抽取辅助天赋`, and post-creation `.<属性> <数字>` / `。<属性> <数字>`.

   Remaining differences: talent draw uses a built-in starter pool and records the talent as a zero-cost skill; it does not yet reproduce Moonberry's full talent database, approval flags, or pool metadata.

   Impact: the old player chat workflow now works through QQ for the common commands, but campaigns that rely on exact old talent tables still need migration work.

4. Join approval now has explicit rejection, active-group admission, and automatic group guide onboarding.

   Current unknown targets become pending chat requests when the current TRPG group allows join requests. The UI can approve them into open chat windows or reject them into a persisted refusal set so they do not reappear as pending requests. Approving a private-message target now adds that player to the current TRPG group and syncs turn/party bookkeeping; approving a QQ group-chat target does not accidentally add it as a player. TRPG groups now persist GM-authored player guide text, assigned players can request it with `.指南` / `.引导` without exposing it to non-members, and private-message approval automatically sends the current group's guide text through the normal NapCat send/ack/local-history path when a guide is configured.

   Remaining differences: Willowblossom uses the current TRPG group's GM-authored guide as the onboarding source instead of migrating any separate Moonberry hardcoded onboarding template. Empty guides are not auto-sent.

5. Skill approval/talent workflow is partially ported.

   Implemented now: character skills persist PC/GM approval flags, source kind, source pool id/label, and copied skill-pool source links. Legacy skills with old `poolId` are marked as skill-pool sourced. Talent draw commands record normal/support talent source metadata. The GUI exposes PC/GM approval toggles, source labels, and a compact optional skill-structure editor for type, target class/count, range, exchange point, cooldown-left, old caster id, old args, and old buff-machine presence. Auto skill-pool sync, rule sync, and quick-cast omit unapproved skills. `SkillPoolEntry` now keeps legacy pool id, type/category, tags, custom args, group/created-at hints, and whether old buff/event-buff/graph data existed; old `skillsPool` root data imports into those fields.

   Remaining differences: Willowblossom still lacks executable use of preserved target/type/range/args metadata in skill resolution, graph-to-rule conversion, full normal/support talent database, and player-submitted skill approval queue.

   Impact: current skill handling now has durable approval/source state for GM workflows, but campaigns that rely on exact old talent tables or player-submitted skill approval still need migration work.

6. Import/export is partial.

   Willowblossom now has a versioned JSON export/import wrapper for the persisted `NapcatMessageManager`, exposed in the TRPG settings UI. That covers messages, chat metadata, character cards, TRPG groups, skill pools, random pools, unit pools, and chat window state that live in that store. It also has targeted JSON export/import-merge for PC/character cards, reusable unit/NPC templates, chat-list metadata without message bodies, scoped DeepSeek summary blocks without raw source text, and voxel scene data. It can also merge old Moonberry root/config JSON exports for groups, basic group descriptions/guide/initial points, old `basicConfig` stat formula coefficients, PCs, chat-list metadata, chat messages, skill-pool metadata, per-character skill shape metadata, unit-pool templates, and random-pool text/min-max items.

   Remaining differences: the Moonberry importer is intentionally partial. It does not recreate old worlds, teams, chat areas, executable graph/buff machines, send panes, modal/window geometry, or UE4 bridge state. Skill-pool migration preserves metadata and old graph/buff presence flags but does not convert graph effects into Willowblossom rules. Random-pool migration preserves item text and min/max counts, but does not recreate old multi-target send-pane assignments.

### Medium Priority

7. Rule/buff behavior is only a narrow typed subset.

   Willowblossom covers simple damage/heal rules, modifiers, buff fields, and expiry. Missing or partial relative to Moonberry: graph editor, grant-buff parser action, skill args, pool-arg propagation, cooldown-left as a persisted skill field, total per-turn damage/heal fields, overflow heal behavior, talent triggers, target sentinels equivalent to old `自己`/`技能目标`, and the fuller damage/heal hook pipeline.

8. Per-group rules/config are partial.

   Implemented now: editable `TrpgGroup` campaign id, public description, GM/ST description, player guide text, initial status points, initial exchange points, per-group `basicConfig` formula coefficients, and an `allow_join_requests` gate. `.兑换` uses the current assigned TRPG group's initial point config, and backtracking refunds the configured total rather than the global default. Character creation, post-creation stat spending, and GM character editing derive HP/MP/regen/speed from the target player's current group formula. Old Moonberry `basicConfig` fields for HP/MP/regen/speed, damage/heal coefficients, and experience coefficients are imported and editable.

   Remaining differences: damage/heal/experience coefficients are currently preserved as configuration data but not applied by the rule engine. Old table flags such as run count, `orderByTurn`, and `negative` are still not modeled.

9. Unit/NPC pool is partial.

   Implemented now: `NapcatMessageManager` persists a reusable `unit_pool` keyed by unit id, each entry stores a label, note, and full `PlayerCharacter` template. The GM UI can create templates, copy existing player characters into the pool, edit core identity/resources/stats, delete templates, and import/export the pool as versioned JSON without pulling in chat history. Battle rounds can add multiple copies of unit templates as NPC participants, keep them through group-player refresh, sync their template stats, and use template skills. Old Moonberry `unitPool` entries can be imported into this pool.

   Remaining differences: unit templates are not yet first-class world/chat-area members and cannot yet be placed into scene standee workflows directly.

10. Random pool is redesigned and partial.

   Current random pools are weighted inventory-item pools with direct award to a character, and now also store optional textual results with min/max counts. The GM UI can edit text/count fields, a draw uses one selected entry for both the item award and text/count preview, and old Moonberry `randomPool` items import into those fields.

   Remaining differences: old random pools were grouped/tagged textual random items with batch result sending to selected PCs through send panes. Willowblossom preserves the useful data and draw preview, but does not yet send batch checked random results to QQ targets or model old group/tag scoping.

11. Old send-pane targeting is partial.

   Willowblossom has individual chat windows, chat groups, TRPG group workspaces, private/group sends, private group broadcasts, party-scoped private broadcasts based on the current TRPG group, and party-aware quoted private auto-forward. It does not yet have old `currentSendPanes` with arbitrary target sets mixing all players, individual players, teams, and chat areas.

12. Character stat formulas are configurable for derived stats, but not full old gameplay effects.

   Current derivation preserves Willowblossom defaults for new groups (`max_hp = 5 + level*5 + str + vit*3`, MP from int/wis, regen/speed from stats), imports old Moonberry group `basicConfig` coefficients, and lets the GM edit the formula per group. Missing pieces are the executable use of stored damage/heal coefficients and any broader gameplay comments or stat threshold effects that Moonberry treated outside simple stat derivation.

### Lower Priority Or Intentionally Different

13. Mirai code remains as legacy.

   `src/mirai/mod.rs` still exists, but the active path is NapCat. This is acceptable as legacy cleanup debt unless it causes confusion or accidental use.

14. UE4 bridge is not ported, and should probably remain obsolete.

   Moonberry used a UE4 sidecar for observation. Willowblossom now has Bevy scene capture, which is the correct replacement. Missing behavior should be measured against scene capture and visibility, not UE4 itself.

15. MobX/Umi/localStorage/Ant/MUI architecture is intentionally not ported.

   Current Rust/Bevy/egui plus typed persisted stores match the rewrite direction.

16. README TODO is stale.

   README still marks QQ communication, persisted chat history, private chat groups, and battle round as unchecked, even though parts of those exist now. It should be refreshed after deciding how to represent "partial" versus "done".

## Suggested Implementation Order

1. Extend the access model beyond messages:
   - scene records that carry visibility,
   - player-facing scene/chat views that call `can_read`,
   - per-party summary blocks for split-party group chat,
   - outbound send targeting that uses party/player visibility,
   - broader scene/editor tools that can operate in an explicit GM/player visibility mode.

2. Rework TRPG groups into campaign plus parties:
   - keep QQ group chats separate from story parties,
   - add GM-controlled split/merge/move UI,
   - add tests for same-party allow, cross-party deny, own-private allow, GM allow, public allow.

3. Replace starter talent support with the full old workflow:
   - full normal/support talent tables,
   - structured talent metadata,
   - optional GM approval hooks,
   - migration from old Moonberry talent records if needed.

4. Port skill approval workflows:
   - executable use of preserved character-skill target/type/range metadata,
   - executable cost args and graph-to-rule migration,
   - player-submitted skill approval queue,
   - GUI-first equivalents where chat commands are not enough.

5. Extend import/export beyond the root NapCat/TRPG store:
   - fuller old Moonberry JSON migration only when specific old campaign data needs it.

6. Expand rule/buff engine only after the privacy model is in place:
   - grant-buff actions,
   - typed skill args,
   - richer target classes,
   - unit/NPC pool integration,
   - broader tests for old Moonberry combat effects.
