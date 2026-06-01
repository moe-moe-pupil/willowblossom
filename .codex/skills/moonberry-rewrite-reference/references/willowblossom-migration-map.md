# Willowblossom Migration Map

## Current Rewrite Direction

Willowblossom is a Rust/Bevy/egui rewrite of Moonberry. It should become the maintainable desktop GM tool: NapCat QQ communication, private player knowledge, TRPG group management, summaries, character/player editing, rule handling, and 3D scene tooling.

Current repo signposts:

- `src/napcat/mod.rs`: NapCat WebSocket IO, message persistence, player characters, TRPG groups, command handling.
- `src/ui/mod.rs`: egui chat windows, group pools, player/group settings, character editor, summary panel.
- `src/rule_engine.rs`: typed rule parser and event resolver.
- `src/scene.rs`: Bevy voxel scene, player scene cameras, scene capture, visibility-aware scene storage.
- `src/deepseek/mod.rs`: summary requests/responses; must remain summary-only.
- `design.md`: voxel scene and visibility plan.

## Concept Mapping

| Moonberry | Willowblossom |
| --- | --- |
| mirai-api-http WebSocket | NapCat WebSocket in `src/napcat/mod.rs` |
| `RootStore` | `NapcatMessageManager` plus focused Bevy resources |
| MobX localStorage persistence | `bevy_persistent::Persistent<T>` |
| `Group` campaign/table | `TrpgGroup` plus future campaign/table resource |
| `Team` | TRPG party/subgroup, not necessarily QQ group |
| `currentChatList` | `chat_targets`, `open_chat_targets`, `pending_chat_targets` |
| `chatMsg` | `messages: HashMap<String, Vec<NapcatMessage>>` |
| unknown sender notification | pending chat request window |
| `.兑换` state machine | `CharacterCreationStep` and `handle_private_text_command` |
| `Pc` | `PlayerCharacter` and possibly `rule_engine::Character` projection |
| `Status` | `CharacterStatus` |
| `Skill` | current `skill_names`/`skill_notes`, future typed skill/effect model |
| `Buff`/`buffMachine` | `RuleAst`, `Action`, future typed effect system |
| `IWorld`/`IArea` | `scene.rs` scene store, visibility metadata, player capture cameras |
| UE4 observation bridge | Bevy scene capture plus NapCat private image send |

## Implementation Guidance

When modifying `src/napcat/mod.rs`:

- Keep inbound parsing separate from command effects.
- Preserve tests around pending chat windows, target sync, creation workflow, and TRPG group pruning.
- Treat private `user_id` targets and group `group_id` targets differently.
- Keep character creation commands compatible with old `.兑换`, `.`, and `..` flows unless intentionally changing UX.

When modifying `src/ui/mod.rs`:

- Keep chat list, chat windows, group windows, character editor, and summary panel as separate UI responsibilities.
- Avoid letting group broadcast or chat-group membership leak private player messages.
- Persist user-visible window and chat target state only through established persistent resources.

When modifying `src/rule_engine.rs`:

- Prefer a typed, testable rule/effect model over the old `buffMachine` graph object.
- Preserve old domain vocabulary in parser/user-facing labels where useful.
- Add parser and resolution tests for every new rule grammar.

When modifying `src/scene.rs`:

- Follow `design.md` visibility rules: `public`, `party:<party_id>`, `player:<player_id>`, `gm`.
- Scene observation sent to a player must be based on that player's allowed camera/visibility context.
- Persist scene edits as explicit app data, then replay into Bevy/voxel state.

When modifying `src/deepseek/mod.rs`:

- Use AI only for summaries of eligible text.
- Do not ask AI to invent plot, NPC actions, hidden facts, or GM-only context.
- Ensure summary eligibility filters exclude commands and hidden/private scopes.

## Behavioral Compatibility Checklist

Before finishing a Moonberry-related change, check:

- Does the old command still work, or is the UX change intentional?
- Does the change preserve private-player isolation?
- Does the GM retain control over join/group/party/character/skill decisions?
- Is persisted data migrated or defaulted safely?
- Are summaries based only on allowed visible content?
- Are tests updated for command parsing, persistence migration, or visibility behavior?
