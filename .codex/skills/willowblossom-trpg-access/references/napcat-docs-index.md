# NapCat Local Docs Index

The NapCatDocs repository is checked out under `references/napcat-docs/` for targeted local search. Do not paste or load the whole docs tree into context; use `rg` and read only the relevant files/lines.

The checkout is intentionally ignored by git to avoid vendoring a large external docs repo. If it is missing, recreate it from the repo root:

```powershell
git clone --depth 1 https://github.com/NapNeko/NapCatDocs.git .\.codex\skills\willowblossom-trpg-access\references\napcat-docs
```

## High-Value Files

- `references/napcat-docs/src/onebot/api.md`: compact OneBot API list. Use this first to confirm action names and required parameters.
- `references/napcat-docs/src/onebot/basic_event.md`: private and group message event shapes.
- `references/napcat-docs/src/onebot/event.md`: fuller event definitions.
- `references/napcat-docs/src/onebot/network.md`: HTTP/WebSocket transport examples and request envelope shape.
- `references/napcat-docs/src/api/4.18.4/openapi.json`: latest OpenAPI snapshot present in the local checkout at the time this index was written.

## Search Patterns

Use these before changing `src/napcat/mod.rs`:

```powershell
rg -n "send_private_msg|send_group_msg|send_msg" .\.codex\skills\willowblossom-trpg-access\references\napcat-docs\src\onebot
rg -n "message_type|post_type|group_id|user_id|raw_message" .\.codex\skills\willowblossom-trpg-access\references\napcat-docs\src\onebot
rg -n "\"send_private_msg\"|\"send_group_msg\"" .\.codex\skills\willowblossom-trpg-access\references\napcat-docs\src\api\4.18.4\openapi.json
```

Do this even when the API name seems obvious. NapCat behavior and docs can change; local docs are the source of truth for this repo. Memory/training knowledge is only a fallback after local docs have been checked.

## Details Relevant To Willowblossom

- WebSocket requests use an envelope with `action`, `params`, and optional `echo`.
- `send_private_msg` takes `user_id` and `message`.
- `send_group_msg` takes `group_id` and `message`.
- Message events include `post_type: "message"`.
- Private messages have `message_type: "private"` and `user_id`.
- Group messages have `message_type: "group"`, `group_id`, and `user_id`.
- Message content can be a string or OneBot segment array, depending on the API.

Treat these notes as an index, not a replacement for the docs. Verify exact fields against the local docs before implementation.
