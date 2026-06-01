---
name: moonberry-rewrite-reference
description: Use when modifying Willowblossom as a rewrite of the old moe-moe-pupil/moonberry TypeScript TRPG bot/app, especially for QQ/NapCat chat behavior, character creation, player/group pools, skill/status/buff/rule systems, import/export, scene visibility, or preserving old Moonberry domain behavior while implementing it in Rust/Bevy/egui.
---

# Moonberry Rewrite Reference

## Purpose

Use Moonberry as a behavioral reference, not as an implementation template. The old repo is a React/Umi/MobX app using mirai-api-http and Ant/MUI UI; Willowblossom is a Rust/Bevy/egui rewrite using NapCat, `bevy_persistent`, explicit resources, and local GM-controlled state.

The user is refactoring/replacing Moonberry with Willowblossom. Preserve the useful TRPG workflows while improving architecture, persistence, privacy boundaries, and maintainability.

## Workflow

1. Inspect the current Willowblossom code first.
2. Load [references/moonberry-behavior.md](references/moonberry-behavior.md) when the task touches old Moonberry feature behavior or terminology.
3. Load [references/willowblossom-migration-map.md](references/willowblossom-migration-map.md) when mapping old concepts to current Rust modules.
4. Prefer current repo patterns over old TypeScript patterns.
5. Add focused tests when changing command parsing, character creation, privacy, summaries, persistence, or rule resolution.

## Non-Negotiables

- Do not port MobX, Umi, Ant Design, MUI, or browser-localStorage architecture into Willowblossom.
- Do not let AI systems generate story content or expose hidden GM/private/party context. Summaries must be summaries of allowed chat content only.
- Treat private player chats, TRPG groups, chat groups, scene visibility, and summaries as separate privacy surfaces.
- Persist durable data through repo-established `Persistent<T>` stores under `.data/willowblossom`, not ad hoc globals.
- Use explicit typed Rust enums/structs for old stringly typed Moonberry flows.
- Keep GM authority central: joining, player setup, group membership, scene visibility, and skill/rule approval should remain controllable by the GM UI.

## Old Source

The old repo is `https://github.com/moe-moe-pupil/moonberry`.

If deeper inspection is needed and it is not already cloned, clone it outside this repo, for example:

```powershell
git clone https://github.com/moe-moe-pupil/moonberry "$env:TEMP\moonberry-skill-source"
```

Useful old files:

- `src/stores/RootStore.tsx`: root orchestration, persistence, mirai WebSocket, pools, send helpers, import/export.
- `src/stores/GroupStore.ts`: group/table data, teams, worlds, chat areas, base config.
- `src/stores/PcStore.ts`: player character data.
- `src/stores/SkillStore.ts`: skill metadata and approval flags.
- `src/stores/BuffStore.ts`: buff/effect/damage/heal concepts.
- `src/api/handle/msgHandle.ts`: private-message command parsing and character creation routing.
- `src/api/handle/exchangeHandle.ts`: old `.兑换` creation state machine.
- `src/utils/buffMachine.ts`: graph-to-buff conversion.
- `src/component/chart/*`: old visual graph/editor ideas.
