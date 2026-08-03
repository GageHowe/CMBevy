use bevy::prelude::*;
use gameplay::{
    health::Health,
    messages::{GameMessages, MESSAGE_TTL_SECS},
    pawn::{Controller, InteractionHint, WeaponSlots, biped::BipedPawnComponent},
    weapon::{WeaponConfig, WeaponState},
};

use super::UI_FONT;

#[derive(Component)]
pub enum HudBar {
    Health,
    Ability,
}

#[derive(Component)]
pub enum HudText {
    Ammo,
    Notifications,
    Interaction,
}

pub fn spawn_hud(mut commands: Commands, assets: Res<AssetServer>) {
    let font = assets.load(UI_FONT);
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
            ZIndex(20),
        ))
        .with_children(|root| {
            bar(
                root,
                HudBar::Health,
                px(10),
                px(10),
                Color::srgb(0.3, 0.8, 0.3),
            );
            bar(root, HudBar::Ability, px(10), px(26), Color::WHITE);
            text(
                root,
                HudText::Ammo,
                &font,
                22.0,
                UiRect {
                    right: px(10),
                    bottom: px(10),
                    ..default()
                },
                "0/0",
            );
            text(
                root,
                HudText::Notifications,
                &font,
                14.0,
                UiRect {
                    right: px(10),
                    top: px(70),
                    ..default()
                },
                "",
            );
            text(
                root,
                HudText::Interaction,
                &font,
                16.0,
                UiRect {
                    left: percent(50),
                    bottom: px(80),
                    ..default()
                },
                "",
            );
        });
}

pub fn sync_health(
    health_q: Query<&Health, With<Controller>>,
    mut bars: Query<(&HudBar, &mut Node, &mut BackgroundColor, &mut Visibility)>,
) {
    let Ok(health) = health_q.single() else {
        set_bar(&mut bars, HudBar::Health, 0.0, Color::WHITE, false);
        return;
    };
    let fraction = (health.current as f32 / health.max() as f32).clamp(0.0, 1.0);
    let color = if fraction > 0.5 {
        Color::srgb(0.3, 0.8, 0.3)
    } else if fraction > 0.25 {
        Color::srgb(0.85, 0.7, 0.0)
    } else {
        Color::srgb(0.85, 0.2, 0.2)
    };
    set_bar(&mut bars, HudBar::Health, fraction, color, true);
}

pub fn sync_ability(
    biped_q: Query<&BipedPawnComponent, With<Controller>>,
    mut bars: Query<(&HudBar, &mut Node, &mut BackgroundColor, &mut Visibility)>,
) {
    let fraction = biped_q
        .single()
        .ok()
        .and_then(|biped| biped.ability.as_ref())
        .map(|ability| ability.status_fraction());
    set_bar(
        &mut bars,
        HudBar::Ability,
        fraction.unwrap_or(0.0),
        Color::WHITE,
        fraction.is_some(),
    );
}

pub fn sync_ammo(
    slots_q: Query<&WeaponSlots, With<Controller>>,
    weapon_q: Query<(&WeaponState, &WeaponConfig)>,
    mut texts: Query<(&HudText, &mut Text, &mut Visibility)>,
) {
    let text = slots_q
        .single()
        .ok()
        .and_then(|slots| slots.active().1)
        .and_then(|weapon| weapon_q.get(weapon).ok())
        .map(|(state, config)| {
            if state.reload_ticks > 0 && config.reload_ticks > 0 {
                let progress = 1.0 - state.reload_ticks as f32 / config.reload_ticks.max(1) as f32;
                format!(
                    "{}/{}  Reloading {:.0}%",
                    state.ammo_in_mag,
                    state.reserve_ammo,
                    progress.clamp(0.0, 1.0) * 100.0
                )
            } else {
                format!("{}/{}", state.ammo_in_mag, state.reserve_ammo)
            }
        });
    set_text(&mut texts, HudText::Ammo, text.as_deref());
}

pub fn sync_notifications(
    time: Res<Time>,
    mut messages: ResMut<GameMessages>,
    mut texts: Query<(&HudText, &mut Text, &mut Visibility)>,
) {
    let now = time.elapsed_secs_f64();
    messages
        .0
        .retain(|entry| now - entry.created_at < MESSAGE_TTL_SECS);
    let text = messages
        .0
        .iter()
        .map(|entry| entry.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    set_text(
        &mut texts,
        HudText::Notifications,
        (!text.is_empty()).then_some(&text),
    );
}

pub fn sync_interaction_hint(
    hint: Res<InteractionHint>,
    mut texts: Query<(&HudText, &mut Text, &mut Visibility)>,
) {
    set_text(&mut texts, HudText::Interaction, hint.0.as_deref());
}

fn bar(root: &mut ChildSpawnerCommands, tag: HudBar, right: Val, top: Val, color: Color) {
    root.spawn((
        Node {
            position_type: PositionType::Absolute,
            right,
            top,
            width: px(100),
            height: px(10),
            border: px(1).all(),
            ..default()
        },
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.6)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
        Visibility::Hidden,
        children![(
            tag,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            BackgroundColor(color),
            Visibility::Visible,
        )],
    ));
}

fn text(
    root: &mut ChildSpawnerCommands,
    tag: HudText,
    font: &Handle<Font>,
    size: f32,
    edges: UiRect,
    value: &str,
) {
    root.spawn((
        tag,
        Text::new(value),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout {
            justify: Justify::Right,
            linebreak: LineBreak::WordOrCharacter,
        },
        Node {
            position_type: PositionType::Absolute,
            left: edges.left,
            right: edges.right,
            top: edges.top,
            bottom: edges.bottom,
            max_width: px(420),
            ..default()
        },
        Visibility::Hidden,
    ));
}

fn set_bar(
    bars: &mut Query<(&HudBar, &mut Node, &mut BackgroundColor, &mut Visibility)>,
    target: HudBar,
    fraction: f32,
    color: Color,
    visible: bool,
) {
    for (bar, mut node, mut bg, mut visibility) in bars {
        if std::mem::discriminant(bar) != std::mem::discriminant(&target) {
            continue;
        }
        node.width = percent(fraction.clamp(0.0, 1.0) * 100.0);
        bg.0 = color;
        *visibility = if visible {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}

fn set_text(
    texts: &mut Query<(&HudText, &mut Text, &mut Visibility)>,
    target: HudText,
    value: Option<&str>,
) {
    for (tag, mut text, mut visibility) in texts {
        if std::mem::discriminant(tag) != std::mem::discriminant(&target) {
            continue;
        }
        **text = value.unwrap_or("").to_string();
        *visibility = if value.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
}
