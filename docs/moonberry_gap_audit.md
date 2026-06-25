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

Additional 2026-06-25 update: `菜鸡猛啄` approved-talent minimum damage floor now executes in the rule engine, quick-cast, and parsed battle skill paths; `火源之力` approved-talent healing output now scales by the healer's HP band in rule-engine, quick-cast, buff-tick, and parsed battle healing; `互帮互助` approved-talent healing feedback now returns 50% healing to the healer from source/target talent hooks in rule-engine, quick-cast, buff-tick, and parsed battle healing; and `数魔转换器` approved-talent range damage now receives positive magical damage bonuses in rule sync, quick-cast, and parsed battle skill paths. Focused verification passes with `CARGO_INCREMENTAL=0 cargo test -j 1 minimum_damage`, `CARGO_INCREMENTAL=0 cargo test -j 1 wounded_healing`, `CARGO_INCREMENTAL=0 cargo test -j 1 mutual_aid`, and `CARGO_INCREMENTAL=0 cargo test -j 1 converter`: 3, 3, 4, and 2 passed respectively, 0 failed. Full-suite verification passes with `CARGO_INCREMENTAL=0 cargo test -j 1`: 292 passed, 1 ignored live DeepSeek API test, 0 failed.

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

   Remaining differences: talent draw now uses Moonberry's full active normal/support talent tables, preserves the old one-draw guard, records the chosen talent as a zero-cost Willowblossom skill with structured trigger/effect category metadata for every preserved talent entry, executes the clear immediate knowledge-stat effects for `那美克星之慧` and `物理专长`, applies the deterministic always-on numeric clauses for `大魔法师`, `人类基因工程`, `矢量压缩能量池`, and `狡黠之思` as derived passive buffs or typed damage modifiers, executes `溃伤` as an on-damage one-turn healing-received debuff, executes `禅宗古训` as 15% lifesteal from final physical damage, executes `过度免疫` as 20% reduction to hits greater than 20% of target max HP, and executes `生死时速` as +50% healing when the target is at or below 20% max HP. It does not yet reproduce most executable conditional combat/timing talent triggers, summon/item side effects, other conditional/type-specific talent damage clauses, or any richer player choice/approval UX that old campaign operations may have handled outside chat commands.

   Impact: the old player chat workflow now works through QQ for the common commands, campaigns can draw from the old talent text pool, and the unambiguous immediate/passive numeric talents now affect character stats, but campaigns that rely on conditional combat talent triggers still need migration work.

   Additional update: `菜鸡猛啄` now applies an approved-talent minimum damage floor equal to character level in rule sync, quick-cast, and battle skill use. The floor is applied after damage reductions/boosts as untyped damage, while zero-damage effects remain zero.

   Additional update: `数魔转换器` now lets approved range damage enjoy positive magical damage bonuses, including INT-configured magical damage and `大魔法师`'s magical bonus, without inheriting negative magical penalties.

   Additional update: `火源之力` now applies approved-talent healing output scaling from the healer's current HP band: 20% while above 60% HP, 10% while above 20% HP, and no bonus while at or below 20% HP.

   Additional update: `互帮互助` now applies approved-talent healing feedback: healing another target sends 50% of the resolved heal back to the healer when the healer has the talent, and receiving healing sends 50% back to the healer when the target has the talent. Self-heals do not recursively trigger feedback.

4. Join approval now has explicit rejection, active-group admission, and automatic group guide onboarding.

   Current unknown targets become pending chat requests when the current TRPG group allows join requests. The UI can approve them into open chat windows or reject them into a persisted refusal set so they do not reappear as pending requests. Approving a private-message target now adds that player to the current TRPG group and syncs turn/party bookkeeping; approving a QQ group-chat target does not accidentally add it as a player. TRPG groups now persist GM-authored player guide text, assigned players can request it with `.指南` / `.引导` without exposing it to non-members, and private-message approval automatically sends the current group's guide text through the normal NapCat send/ack/local-history path when a guide is configured.

   Remaining differences: Willowblossom uses the current TRPG group's GM-authored guide as the onboarding source instead of migrating any separate Moonberry hardcoded onboarding template. Empty guides are not auto-sent.

5. Skill approval/talent workflow is partially ported.

   Implemented now: character skills persist PC/GM approval flags, source kind, source pool id/label, and copied skill-pool source links. Player-submitted skills from `.兑换` now enter as PC-confirmed but GM-pending, show `GM待确认` in `.已兑换`, are counted in the GM character list, and stay out of quick-cast/rule sync/skill-pool sync until the GM approves them. Legacy skills with old `poolId` are marked as skill-pool sourced. Talent draw commands use Moonberry's full active normal/support talent tables, block a second talent draw, record talent source/trigger/effect category metadata for every preserved talent entry, execute the clear immediate knowledge-stat effects for `那美克星之慧` and `物理专长`, apply deterministic always-on numeric passive effects for `大魔法师`, `人类基因工程`, `矢量压缩能量池`, and `狡黠之思` through the same effective-buff path as legacy passives, apply `大魔法师`'s approved-talent +0.5% per INT magical damage bonus through the shared typed damage multiplier, apply `人类基因工程` disease/poison -15% incoming damage plus `抗魔体质` magical -10% incoming damage through the shared typed target-damage multiplier, apply `溃伤` as an approved-talent on-damage one-turn -25% healing-received debuff in rule sync, quick-cast, and battle skill use, apply `禅宗古训` as approved-talent 15% lifesteal from final physical damage in those same paths, apply `过度免疫` as approved-talent 20% reduction to final incoming hits above 20% max HP, and apply `生死时速` as approved-talent +50% healing received while at or below 20% max HP. The GUI exposes PC/GM approval toggles, pending labels, source labels, talent trigger/effect hints, and a compact optional skill-structure editor for type, target class/count, range, exchange point, cooldown-left, old caster id, old args, and old buff-machine presence/raw-size hints; the type and target fields now offer Moonberry's known old values while preserving editable custom/imported text. Auto skill-pool sync, rule sync, and quick-cast omit unapproved skills. Imported `cooldownLeft` now reports in `.冷却` and blocks quick-cast/battle skill use until a local cast record supersedes it, preserved `target_count` caps quick-cast/battle resolved targets, `无目标`/`单目标` target classes enforce zero/one target caps, `范围` target class expands otherwise single-target effects into area target resolution, preserved positive `range` fills missing area radii and filters single selected targets for quick-cast target discovery and battle skill resolution, preserved numeric skill args execute as named amount placeholders and string/BUFF args execute as exact text substitutions in rule sync, quick-cast, and battle skill parsing, preserved active old `技能释放` buff-machine entries now convert common damage/heal/basic modifier effects into typed rule actions for rule sync, quick-cast, and battle skill use, approved legacy `被动` buff-machine entries now derive permanent effective buffs from skill args for common stats, HP/MP/regen, and damage/heal modifiers without persisting them as manual active buffs, graph-only or empty-eventBuff legacy blueprints now follow the old exec chain and convert simple active/passive damage/heal/basic stat/resource/modifier nodes, pool-backed `给予BUFF` plus graph `BUFF变量` references can now resolve imported skill-pool raw payloads into simple granted basic buffs during rule sync, pool-backed `给予BUFF` damage/heal payloads now become typed per-turn buff tick actions in the rule engine and quick-cast group turn path, and preserved skill type supplies the default damage type for untyped damage notes while explicit damage text still wins. `SkillPoolEntry` now keeps legacy pool id, type/category, tags, custom args, group/created-at hints, old buff/event-buff/graph presence flags, and compact raw JSON for old buff, eventBuffs, graph, and character-derived buffMachine payloads; old `skillsPool` root data imports into those fields.

   Remaining differences: Willowblossom still lacks non-damage skill type behavior, richer graph-backed BUFF arg semantics beyond pool-backed basic buff grants/tick actions and exact text substitution, graph branching/conditions beyond the old single exec chain, most executable conditional/battle talent triggers/effects beyond the already implemented immediate knowledge-stat effects, always-on numeric passive talents, `溃伤` on-damage debuff, `禅宗古训` physical-damage lifesteal, `过度免疫` large-hit reduction, and `生死时速` dying-target healing, and any richer target-class runtime semantics that old campaign data may require beyond count/range resolution.

   Impact: current skill handling now has durable approval/source state for GM workflows, player-submitted skills wait for GM approval, and old talent text data is preserved, but campaigns that rely on executable talent effects still need migration work.

   Additional implemented talent execution: `菜鸡猛啄` now floors single damage effects to at least the source level in the same rule sync, quick-cast, and battle paths as the other executable talent hooks.

   Additional implemented talent execution: `数魔转换器` now applies positive magical damage bonuses to approved range damage in helper/rule-sync, quick-cast, and parsed battle skill paths.

   Additional implemented talent execution: `火源之力` now applies a dynamic healer injury-state multiplier to direct rule-engine healing, quick-cast healing, continuing buff-tick healing, and parsed battle skill healing.

   Additional implemented talent execution: `互帮互助` now applies non-recursive source/target healing feedback in direct rule-engine healing, quick-cast healing, continuing buff-tick healing, and parsed battle skill healing.

6. Import/export is partial.

   Willowblossom now has a versioned JSON export/import wrapper for the persisted `NapcatMessageManager`, exposed in the TRPG settings UI. That covers messages, chat metadata, character cards, TRPG groups, skill pools, random pools, unit pools, and chat window state that live in that store. It also has targeted JSON export/import-merge for PC/character cards, reusable unit/NPC templates, chat-list metadata without message bodies, scoped DeepSeek summary blocks without raw source text, and voxel scene data. It can also merge old Moonberry root/config JSON exports for groups, basic group descriptions/guide/initial points, old `basicConfig` stat formula coefficients, PCs, chat-list metadata, chat messages, skill-pool metadata, per-character skill shape metadata, unit-pool templates, random-pool text/min-max items plus old id/group/tag/description/created-at metadata, old per-PC negative timers, old teams, old worlds/chat areas, and old send panes.

   Remaining differences: the Moonberry importer is intentionally partial. It preserves old worlds, teams, chat areas, send panes, old team local chat excerpts, old team window geometry, and optional scene-store markers for legacy areas as typed metadata, private broadcast/GM preview surfaces, appendable GM local team-chat sends, editable parsed local team-chat messages, independent old-channel chat floating windows, visibility-filtered scene gizmo overlays, editable voxel border/fill stamping, and old NPC/member ids for unit-template token placement with scoped sync/remove controls, but does not recreate Moonberry's exact browser modal/window layout behavior, full executable graph/buff machines, semantic area entities, automatic gameplay membership from those entities, or UE4 bridge state. Skill-pool migration preserves metadata, old graph/buff presence flags, and compact raw JSON payloads; common active `技能释放` damage/heal/basic modifier payloads now convert into Willowblossom rule actions, common passive `被动` basic stat/modifier payloads now apply as derived effective buffs, simple graph-only active/passive exec chains now convert into the same typed effects, and pool-backed `给予BUFF`/graph `BUFF变量` references now expand imported skill-pool basic buffs and damage/heal tick buffs for rule sync and quick-cast turn advancement, but branching and full graph-editor behavior remain partial. Random-pool migration preserves item text, min/max counts, and old group/tag metadata, and the GM UI can filter/edit that metadata, stage checked per-PC results, and batch-send drawn text results through current-group/private imported send-pane scopes.

### Medium Priority

7. Rule/buff behavior is only a narrow typed subset.

   Willowblossom covers simple damage/heal rules, named grant-buff actions with common typed field/value effects, modifiers, Moonberry low-HP source damage penalty, buff fields, expiry, imported `cooldownLeft` blocking, `target_count` caps, no/single target-class caps, `范围` target-class area expansion, positive range fallback for area skills, range filtering for single selected targets, numeric skill args from preserved metadata as named amount placeholders, string/BUFF skill args as exact text substitutions, raw old buff/graph/buffMachine JSON preservation, active legacy `技能释放` buff-machine damage/heal/basic modifier conversion, passive legacy `被动` buff-machine basic stat/modifier conversion with derived status formulas, graph-only active/passive single-exec-chain conversion for simple damage/heal/basic nodes, pool-backed `给予BUFF`/graph `BUFF变量` conversion for simple granted basic buffs in rule sync, pool-backed `给予BUFF` damage/heal tick actions on turn advancement, immediate knowledge-stat talent effects plus always-on numeric passive talent buffs, `溃伤` on-damage healing-received debuff execution, `禅宗古训` physical-damage lifesteal execution, `过度免疫` large-hit damage reduction execution, `生死时速` dying-target healing bonus execution, and all-table talent trigger/effect category metadata, preserved skill type as the default damage type for untyped damage notes, old `自己`/`技能目标` target wording, Moonberry's overflow-heal cap-at-max behavior, and per-turn damage/heal counters for character cards, quick-cast, battle snapshots, and the rule engine. Missing or partial relative to Moonberry: graph editor UI, graph branching/conditions beyond the old single exec chain, most executable conditional combat/timing talent triggers, and the fuller damage/heal hook pipeline.

   Additional update: rule/buff damage resolution now includes the `菜鸡猛啄` level-based minimum untyped damage floor after reductions/boosts, with focused rule, quick-cast, and battle coverage.

   Additional update: rule/buff damage resolution now includes `数魔转换器` range damage sharing positive magical damage bonuses, with focused helper, quick-cast, and battle coverage.

   Additional update: rule/buff healing resolution now includes `火源之力` as a source-side wounded healing multiplier, with focused rule, quick-cast, and battle coverage.

   Additional update: rule/buff healing resolution now includes `互帮互助` as source-side and target-side healing feedback to the healer, with focused rule, quick-cast, buff-tick, and battle coverage.

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
   - keep extending from the newly executable `菜鸡猛啄` level-based damage floor, `数魔转换器` range/magic bonus sharing, `火源之力` wounded healing multiplier, and `互帮互助` healing feedback into other concrete trigger/effect clauses,
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
