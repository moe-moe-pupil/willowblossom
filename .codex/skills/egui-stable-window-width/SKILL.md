---
name: egui-stable-window-width
description: "Diagnose and fix Rust egui windows, panels, text edits, columns, and horizontal layouts that keep expanding toward full width after resize. Use when a Bevy/egui UI window grows every frame, ignores manual resizing, stretches because of available_width, desired_width, TextEdit, ui.columns, horizontal, or persisted egui memory, especially in Willowblossom character/player editor windows."
---

# Egui Stable Window Width

## Purpose

Use this skill when an egui window starts small but repeatedly grows to the full viewport or returns to a too-wide size after the user resizes it.

The usual cause is a feedback loop: a child widget asks for `ui.available_width()`, the parent window grows to satisfy the child, and the next frame reports a larger `available_width()`. Long single-line text, `TextEdit`, `ui.columns`, and `horizontal` rows make this worse.

## Diagnosis

Check these first:

- A resizable `egui::Window` with only `.default_width(...)` and no `.max_width(...)`.
- Child widgets using `.desired_width(ui.available_width())`, `f32::INFINITY`, or a value derived directly from the already-expanded window.
- `ui.columns(...)` containing single-line `TextEdit` fields. Columns divide the parent width, so the parent may expand to satisfy the editors.
- `Grid::num_columns(...)` inside an auto-sized window. In egui 0.35 it makes the last column fill the parent's remaining width, which can feed the parent width back into the grid and produce large gaps or full-screen growth.
- `ui.horizontal(...)` rows containing many controls or long text. Use `horizontal_wrapped` or break rows when fixed-width compact controls are acceptable.
- Persisted egui window memory. After a bad layout has saved a huge width, the fix may look ineffective until the window id changes or egui memory is cleared.

In Willowblossom, start around `src/ui/mod.rs` functions such as `quick_character_window`, `character_editor_ui`, and `character_skill_editor_ui`.

## Fix Pattern

Constrain the outer window first:

```rust
let screen_rect = ctx.screen_rect();
let max_width = screen_rect.width().min(720.0).max(360.0);

egui::Window::new(format!("Character: {display_name}"))
    .id(Id::new(("quick_character_window", target_id.as_str())))
    .default_width(360.0)
    .min_width(320.0)
    .max_width(max_width)
    .resizable(true)
    .show(ctx, |ui| {
        ui.set_max_width(max_width);
        // window contents
    });
```

Then make text fields use a local cap instead of an unconstrained available width:

```rust
let field_width = ui.available_width().min(420.0).max(160.0);
ui.add(
    egui::TextEdit::singleline(&mut character.image)
        .desired_width(field_width),
);
```

For two-column editors, avoid allowing each column to inherit a huge parent width:

```rust
ui.columns(2, |columns| {
    for column in &mut columns[..] {
        column.set_max_width(320.0);
    }

    columns[0].label("Character name");
    columns[0].add(
        egui::TextEdit::singleline(&mut character.name)
            .desired_width(columns[0].available_width().min(300.0)),
    );

    columns[1].label("Nickname");
    columns[1].add(
        egui::TextEdit::singleline(&mut character.nickname)
            .desired_width(columns[1].available_width().min(300.0)),
    );
});
```

For multiline editors inside a `horizontal` row, subtract button space and cap the result:

```rust
ui.horizontal(|ui| {
    let width = (ui.available_width() - 28.0)
        .clamp(160.0, 560.0);
    ui.add(
        egui::TextEdit::multiline(note)
            .desired_rows(2)
            .desired_width(width),
    );
    ui.button("-");
});
```

For fixed-size item catalogs, cap the window and omit `num_columns`; call `end_row()` at the intended boundary and cap cell widths. Version the window and grid ids once if their persisted layout already contains the oversized width:

```rust
egui::Window::new("Catalog")
    .id(Id::new("catalog_v2"))
    .default_width(440.0)
    .min_width(440.0)
    .max_width(440.0)
    .resizable(false)
    .show(ctx, |ui| {
        ui.set_max_width(440.0);
        Grid::new("catalog_grid_v2")
            .min_col_width(56.0)
            .max_col_width(72.0)
            .show(ui, |ui| {
                // Add cells and call ui.end_row() after each logical row.
            });
    });
```

## Verification

After patching:

1. Run `cargo check`.
2. Open the affected window and manually resize it narrower.
3. Interact with the text fields, especially long URLs and multiline skill descriptions.
4. Close and reopen the window. If it still reopens at the old 100% width, clear persisted egui memory or temporarily change the window `.id(...)` once to discard the stored oversized rect.

## Rules

- Keep `.default_width(...)` as the starting size only. It is not a maximum.
- Prefer `.max_width(...)` on resizable windows that contain text editors or columns.
- Avoid `Grid::num_columns(...)` in auto-sized catalog windows; use explicit `end_row()` boundaries and bounded column widths.
- Cap long editor windows to the viewport with `.max_height(...)` and enable vertical scrolling so controls remain reachable.
- Keep editable forms out of `menu_button`, popup, and context-menu surfaces; use inline collapsing sections or normal scrollable windows.
- Avoid `desired_width(ui.available_width())` unless the parent has already been capped.
- Clamp widths with realistic minimum and maximum values near the widget that requests size.
- Use `horizontal_wrapped` or separate rows for dense control groups that do not need to stay on one line.
