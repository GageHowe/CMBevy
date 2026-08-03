use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui_widgets::TextInput,
};
use common::{ActiveBindings, InputAction, active_gamepad};
use gameplay::{
    net::{
        message::{ChatMessage, MsgType},
        quic::{Channel, QuicManager},
    },
    session::GuiState,
};

use crate::{GameState, steam::SteamClient, ui::UI_FONT};

#[derive(Component)]
pub struct ChatRoot;
#[derive(Component)]
pub struct ChatLog;
#[derive(Component)]
pub struct ChatInput;

pub fn spawn_chat(mut commands: Commands, assets: Res<AssetServer>) {
    let font = assets.load(UI_FONT);
    commands
        .spawn((
            ChatRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(10),
                bottom: px(10),
                width: px(420),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: px(8).all(),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.04, 0.0, 0.05, 0.55)),
            ZIndex(30),
        ))
        .with_children(|root| {
            root.spawn((
                ChatLog,
                Text::new(""),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                TextLayout {
                    linebreak: LineBreak::WordOrCharacter,
                    ..default()
                },
                Node {
                    width: percent(100),
                    height: px(150),
                    ..default()
                },
            ));
            root.spawn((
                ChatInput,
                TextInput,
                EditableText::new(""),
                TextCursorStyle::default(),
                TextFont {
                    font: font.into(),
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                TextLayout::no_wrap(),
                Node {
                    width: percent(100),
                    height: px(28),
                    border: px(1).all(),
                    padding: px(5).all(),
                    ..default()
                },
                BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.4)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ));
        });
}

pub fn sync_chat(
    game_state: Res<State<GameState>>,
    state: Option<Res<GuiState>>,
    focus: Res<InputFocus>,
    input_q: Query<Entity, With<ChatInput>>,
    mut roots: Query<&mut Node, With<ChatRoot>>,
    mut logs: Query<&mut Text, With<ChatLog>>,
) {
    let Ok(input) = input_q.single() else { return };
    let visible = *game_state.get() == GameState::Multiplayer
        && (focus.get() == Some(input)
            || state.as_ref().is_some_and(|state| !state.chat.is_empty()));
    for mut node in &mut roots {
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }
    let Some(state) = state else { return };
    let text = state
        .chat
        .iter()
        .rev()
        .take(8)
        .map(|line| line.1.as_str())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    for mut log in &mut logs {
        **log = text.clone();
    }
}

pub fn submit_chat(
    mut quic: ResMut<QuicManager>,
    steam: Option<Res<SteamClient>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<ActiveBindings>,
    gamepads: Query<&Gamepad>,
    mut focus: ResMut<InputFocus>,
    input_q: Query<Entity, With<ChatInput>>,
    mut inputs: Query<&mut EditableText, With<ChatInput>>,
) {
    let Ok(input) = input_q.single() else { return };
    if bindings.just_pressed(
        InputAction::Chat,
        &keys,
        &mouse,
        active_gamepad(gamepads.iter()),
    ) && focus.get().is_none()
    {
        focus.set(input, FocusCause::Pressed);
        return;
    }
    if focus.get() != Some(input) {
        return;
    }
    let Ok(mut text) = inputs.single_mut() else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        text.clear();
        focus.clear();
        return;
    }
    if !keys.just_pressed(KeyCode::Enter) || text.is_composing() {
        return;
    }
    let txt = text.value().to_string();
    let txt = txt.trim();
    if !txt.is_empty() {
        let name = steam
            .as_ref()
            .map(|s| s.friends().name())
            .unwrap_or_else(|| "Player".to_string());
        quic.send_to_server(
            Channel::Ordered,
            &MsgType::ChatMessage(ChatMessage(Color::WHITE, format!("{name}: {txt}"))),
        );
    }
    text.clear();
    focus.clear();
}
