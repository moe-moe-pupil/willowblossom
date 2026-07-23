# Willowblossom Access Model

## Core Principle

The bot must enforce knowledge boundaries before summary generation, GUI display, QQ outbound messages, or MCP/tool calls. A player can access their own character, their current party, and public campaign state. The GM can access every scope. Other players cannot access separated-party or private-player state.

AI is summary-only in this project. It must not write story continuations, create scene descriptions, control NPCs, resolve actions, or invent facts. The GM owns the story.

## Suggested Data Model

Keep the model small at first:

```rust
pub enum Visibility {
    Public,
    Party(String),
    Player(u64),
    Gm,
    System,
}

pub struct CampaignMessage {
    pub campaign_id: String,
    pub sender_id: u64,
    pub source: MessageSource,
    pub character_id: Option<String>,
    pub party_id: Option<String>,
    pub visibility: Visibility,
    pub text: String,
    pub time: u64,
}

pub enum MessageSource {
    Friend { user_id: u64 },
    Group { group_id: u64, user_id: u64 },
    Gui,
    Summary,
}

pub struct PlayerAccess {
    pub player_id: u64,
    pub character_id: Option<String>,
    pub party_id: Option<String>,
    pub is_gm: bool,
}
```

Use owned strings or stronger id types based on local style. Avoid encoding all access rules in string keys like `target_id.to_string()`.

## Visibility Check

The basic rule should be easy to unit test:

```rust
fn can_read(access: &PlayerAccess, visibility: &Visibility) -> bool {
    if access.is_gm {
        return true;
    }

    match visibility {
        Visibility::Public => true,
        Visibility::Party(id) => access.party_id.as_ref() == Some(id),
        Visibility::Player(id) => access.player_id == *id,
        Visibility::Gm | Visibility::System => false,
    }
}
```

If the current implementation does not yet have campaigns, add the field in a way that can default to a single campaign for old data.

## NapCat Direction

Use NapCat as the QQ framework:

- Ingest both friend/private messages and group messages.
- Preserve whether a message came from a friend chat or a group chat.
- Send private/friend messages with `send_private_msg`.
- Send group messages with `send_group_msg`.
- Do not infer story party membership from QQ group membership.
- Treat Mirai as legacy and avoid adding new Mirai behavior.

Use QQ numeric ids for GM/player checks, never nicknames.

For exact NapCat/OneBot payloads, search `napcat-docs` rather than guessing. The key event fields are usually `post_type`, `message_type`, `user_id`, optional `group_id`, `message`, `raw_message`, `sender`, and `self_id`.

## GUI Shape

Prefer GUI state and controls instead of chat commands:

- GM users list.
- Player and character binding table.
- Campaign selector.
- Party list with assigned characters.
- Visibility selector for stored notes/messages/summaries.
- NapCat send target selector: friend or group.
- Summary button scoped to public, party, player, or GM visibility.

Only add chat commands if the user explicitly asks later.

## Retrieval Rules

When building context for a summary or GUI view:

1. Resolve `PlayerAccess` from QQ user id.
2. Select only messages from the same campaign.
3. Apply `can_read` to every message or memory item.
4. Only then summarize, display, or pass context to an LLM/MCP tool.

Do not summarize hidden content and then expose the summary. The summary has the same visibility as its source content unless explicitly downgraded by the GM.

## Summary Rules

- Summaries are derived records with their own `visibility`.
- Summary input must already be filtered before calling an AI model.
- Summary output must not add new story facts.
- Summary output should mention uncertainty if the source messages are unclear.
- GM summaries may include every scope.
- Player/party summaries may include only what that player/party can read.

## Tests To Add

Cover these cases whenever access logic changes:

- same party can read party messages,
- different party cannot read party messages,
- player can read their own private messages,
- player cannot read another player's private messages,
- GM can read all scopes,
- public messages are visible to everyone,
- system/GM notes are not visible to normal players.
