---
name: willowblossom-trpg-access
description: Repo-local implementation guidance for Willowblossom, a Rust/Bevy 0.18.1 GUI QQ TRPG bot using NapCat, focused on private character/party knowledge, group and friend messaging, GM-controlled split-party isolation, and summary-only AI/MCP access. Use when adding, reviewing, or refactoring Bevy app/UI/ECS code, NapCat message ingest/send behavior, persisted chat history, GUI controls for parties/players, or LLM/MCP summarization that must not generate story content or leak hidden context.
---

# Willowblossom TRPG Access

## Overview

Use this skill to keep TRPG knowledge boundaries explicit in Willowblossom. The GM writes and controls the story; AI/MCP features may summarize allowed chat history, but must not create plot, describe new scenes, decide outcomes, or roleplay as narrator.

## Communication

Reply to the user in English by default. The Chinese-first rule below applies to Willowblossom user-facing app UI text, not to Codex chat responses.

## Network downloads

Before downloading dependencies, models, repositories, or other remote assets for this workspace in PowerShell, set:

```powershell
$env:http_proxy="http://127.0.0.1:10809"
$env:https_proxy="http://127.0.0.1:10809"
```

Keep both variables in the same shell process that performs the download.

## Repo Map

- `src/napcat/mod.rs`: active QQ/NapCat websocket integration and persisted message manager.
- `src/mirai/mod.rs`: obsolete Mirai integration. Do not build new behavior on Mirai; it can be removed when cleanup is requested.
- `src/ui/`: Bevy/egui GUI code. New player, party, visibility, and summary controls should be GUI-first.
- `.data/willowblossom/messages.toml`: current persisted message store path.
- `README.md`: project TODO includes QQ communication, persisted chat history, and private chat groups.
- `references/napcat-docs/`: local checkout of NapCatDocs. Search this with `rg`; do not load the whole docs tree.
- `references/bevy-0.18.1/`: local sparse checkout of Bevy `release-0.18.1`. Search this with `rg`; do not rely on older remembered Bevy APIs.

Implement QQ behavior through NapCat. Support both private friend messages and group messages by sending the correct NapCat action (`send_private_msg`/friend message behavior and `send_group_msg`) instead of designing CLI commands.

## Product Direction

- This is a GUI program, not a CLI command bot.
- The GM is the only actor who should split/merge parties or change visibility.
- Players may be in one QQ group while their characters are separated in the story.
- The bot should preserve and summarize chat history by allowed scope.
- AI must be a summarizer only. It should never continue the story, invent facts, control NPCs, resolve actions, or reveal hidden information.
- NapCat is the active QQ framework. Mirai should be treated as legacy code.
- Bevy target version is `0.18.1`. Before adding or migrating Bevy app, ECS, UI, input, window, asset, schedule, or plugin code, verify current API patterns from local Bevy 0.18.1 source/examples/docs.

## Access Rules

Treat visibility as data, not prompt text. Persist and check these concepts before any message history, game memory, summary, or QQ outbound message is returned:

- `player_id`: QQ user id.
- `character_id`: in-game character identity controlled by a player.
- `party_id`: temporary split-party/group channel inside a campaign.
- `campaign_id`: table/session boundary.
- `visibility`: one of `public`, `party:<party_id>`, `player:<player_id>`, `gm`, or `system`.

Never infer visibility only from a QQ group id. In TRPG play, a single QQ group can contain players whose characters are separated.

## Workflow

1. Inspect the current message flow before editing: start with `src/napcat/mod.rs`, then check shared state in `src/lib.rs`.
2. If the task touches NapCat API names, websocket envelopes, event schemas, message segment formats, or send/receive payloads, verify the exact details from `references/napcat-docs/` first. Do not rely on memory/training knowledge for NapCat protocol details.
3. If the task touches Bevy APIs, verify the exact 0.18.1 API from `references/bevy-0.18.1/` first. Do not use remembered Bevy 0.14/0.15-era patterns without checking 0.18.1.
4. Identify whether the request affects ingest, persistence, retrieval, outbound friend/group messages, summary generation, or GUI controls.
5. Add or preserve explicit visibility metadata at the boundary where messages or memories enter the system.
6. Filter by requesting actor before constructing any summary context or outbound message.
7. For denied access, avoid hinting at hidden state. The UI or bot response should simply show no unavailable content.
8. Persist only the minimum needed fields and keep old data migration behavior intentional.
9. Add tests or small deterministic checks for cross-party denial, own-party allow, GM allow, and public allow.

## AI/MCP Policy

When implementing LLM/MCP-style access, use it for summarization only and pass only filtered context to the model/tool. Do not rely on a prompt like "do not reveal secrets" while still sending forbidden messages. The tool input should already exclude:

- other parties' messages,
- private player whispers,
- GM notes,
- hidden rolls or results,
- future scene plans,
- NPC secrets not discovered by the requesting party.

Summaries must be labeled by scope in the app state, for example public summary, party summary, player-private summary, or GM summary. A summary inherits the strictest visibility of the source messages unless the GM explicitly publishes it to a wider scope.

## GUI Expectations

Display Chinese text first in all user-facing UI. Existing or new labels, buttons, window titles, tooltips, empty states, and help text should be written in Chinese by default; keep English only when it is a protocol/API name, compact game stat abbreviation, player-authored content, or a secondary clarification after the Chinese text.

Do not place character, inventory, item-effect, or other editable form controls inside `menu_button`, popup, or context-menu surfaces. Use an inline collapsing section, scrollable panel, or normal window so pointer input and keyboard focus remain reliable.

Prefer GUI controls over chat commands for:

- selecting the active campaign,
- binding QQ users to players/characters,
- marking GM users,
- creating and merging split parties,
- moving characters between parties,
- choosing whether an outbound QQ message goes to a friend or group,
- generating summaries for the GM or a visible party scope.

Chat commands are out of scope unless the user explicitly asks for them later.

## Reference

Read `references/access-model.md` when designing data structures, GUI state, NapCat message routing, summarization, or tests for private parties and character-specific knowledge.

Read `references/napcat-docs-index.md` before touching NapCat API/event handling. Then use targeted searches in `references/napcat-docs/`, especially for `send_private_msg`, `send_group_msg`, `message_type`, `post_type`, `group_id`, `user_id`, and websocket connection behavior.

When local NapCat docs conflict with remembered API details, follow the local docs and mention the discrepancy briefly in the work summary.

Read `references/bevy-0.18.1-index.md` before touching Bevy code. Then use targeted searches in `references/bevy-0.18.1/`, especially under `examples/`, `crates/`, `docs/`, `release-content/`, and `migration-guides/`.

When local Bevy 0.18.1 source/examples conflict with remembered API details, follow Bevy 0.18.1 and mention the discrepancy briefly in the work summary.
