# Moonberry Behavior Reference

## Product Intent

Moonberry was a GM/ST-facing QQ TRPG helper for infinite-flow campaigns. It managed player onboarding, private QQ chat, group/table state, character stats, skills, buffs, random/unit/skill pools, and visual world/chat-area concepts.

Willowblossom should preserve useful workflows, not the old frontend architecture.

## Communication Model

Old Moonberry used `mirai-api-http` through a WebSocket at `ws://localhost:8080/message?verifyKey=1234567890`.

Important behaviors:

- Private player messages were the main player command surface.
- Unknown private message senders could become pending join requests.
- The GM/ST could approve or reject a new sender.
- Approved players were added to the chat list and received onboarding text.
- Outbound private messages were also inserted into the local chat timeline as self/ST messages.
- Group/batch send helpers sent one or more text segments to selected QQ ids.

Old commands in private chat:

- `.兑换` / `。兑换`: start character creation.
- `.观察` / `。观察`: request scene/observation integration.
- `.抽取天赋`: draw normal talents after character initialization.
- `.抽取辅助天赋`: draw support talents after character initialization.
- `.状态`: send character status.
- `.已兑换`: send exchanged/creation information.
- `.冷却`: send cooldown information.
- `.频道人员`: list channel members.
- `.` / `。`: advance a character-creation step.
- `..`, `.。`, `。.` or `。。`: go back during creation.
- `.<属性> <数字>` / `。<属性> <数字>`: add points to a finished character status.

## Character Creation

Old creation is named `exchange`.

State sequence:

1. `normal`
2. `str`
3. `agi`
4. `dex`
5. `vit`
6. `int`
7. `wis`
8. `k`
9. `cha`
10. `confirmStatus`
11. `skill`
12. `confirmSkill`
13. `img`
14. `nickname`

The old default character started with:

- `hp = maxHP = 5`
- `mp = maxMP = 0`
- `lv = 1`
- `speed = 3`
- `statusPoint = group.initStatusPoint`
- `exchangePoint = group.initExchangePoint`
- damage/heal modifiers all `1`
- eight status keys: `str`, `agi`, `dex`, `vit`, `int`, `wis`, `k`, `cha`

Old group defaults:

- `initStatusPoint = 5` in `basicConfig`, but `Group` also had an older class field set to `6`.
- `initExchangePoint = 6`.

When preserving behavior, check current Willowblossom defaults and tests before changing defaults. Avoid silently changing existing persisted data.

## Stats And Config

Old stat labels:

- `str`: 力量
- `agi`: 敏捷
- `dex`: 灵巧
- `vit`: 体质
- `int`: 智力
- `wis`: 智慧
- `k`: 知识
- `cha`: 魅力

Old basic config fields:

- `wisMPReg = 1`
- `wisMaxMP = 2.5`
- `intMaxMP = 5`
- `vitHPReg = 1`
- `vitMaxHP = 3`
- `lvMaxHP = 5`
- `strMaxHP = 1`
- `initStatusPoint = 5`
- `initExchangePoint = 6`
- `expGainPerLv = 3`
- `expGainPerLvPvP = 0.15`
- `basicSpeed = 3`
- `strDMGBenifit = 0.025`
- `intDMGBenifit = 0.02`
- `dexDMGBenifit = 0.01`
- `dexRangeDMGBenifit = 0.03`
- `wisHealBenifit = 0.02`
- `intHealBenifit = 0.01`
- `agiDMGBenifit = 0.02`
- `strSpeed = 0.5`
- `agiSpeed = 1`
- `dexSpeed = 0.5`

## Groups, Teams, Worlds, Chat Areas

Moonberry concepts:

- `Group`: a campaign/table.
- `Team`: a channel-like subset with `pcs`, buffs, visibility, window geometry, and local chat.
- `IWorld`: a world containing PC numbers, NPC numbers, map, `chatAreas`, and `Areas`.
- `IArea`: rectangular world/chat area with members and `combat` flag.
- `currentSendPanes`: multiple send panes, each with selected targets.
- `sendTo.targets`: mixed ids for all, players, teams, and chat areas.

Migration principle:

- Keep these as explicit data models if reintroduced.
- Do not conflate QQ group chats with TRPG parties.
- Do not let a selection/broadcast UI bypass privacy checks.

## Skills, Buffs, And Rules

Old skill fields:

- name, type, target count, target class, caster id, cost, cooldown, range, description.
- `stInited`: GM/ST approved.
- `pcInited`: player confirmed.
- `poolId`: link to skill pool.
- `args`: typed skill parameters.
- `buffMachine`: graph-derived effects.

Old skill target classes:

- `无目标`
- `单目标`
- `多目标`
- `范围`

Old skill types:

- `法术`
- `道具`
- `异能`
- `动作`
- `血统`
- `职业`
- `召唤物`
- `远程`

Old buff/effect concepts:

- Damage types: Magical, Physical, Cursed, Diseased, bleed, Range, poisoning, None.
- Heal types: Instant, continue.
- Targets: self and skill target were encoded as sentinel values.
- Effects could modify HP, MP, max HP/MP, regen, stats, damage/heal modifiers, deal damage, heal, or grant another buff.

Willowblossom should express these as typed rules/effects. Avoid copying the old graph object shape unless a real graph editor is being implemented.

## Pools And Import/Export

Old root data included:

- `skillsPool`
- `unitPool`
- `randomPool`
- `dataObj`
- groups, current group, chat messages, chat list, player characters

Old import/export behavior wrote JSON blobs for root, PCs, chat messages, and chat lists.

When adding this to Willowblossom, prefer versioned, typed persisted data plus explicit import/export commands. Preserve user data through migrations.
