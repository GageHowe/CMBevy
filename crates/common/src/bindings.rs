use bevy::{
    input::{
        gamepad::{Gamepad, GamepadButton},
        mouse::MouseButton,
    },
    prelude::*,
};
use serde::{Deserialize, Serialize};

pub const INPUT_ACTION_COUNT: usize = 20;

#[repr(u8)]
#[derive(Clone, Copy, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug)]
pub enum InputAction {
    MoveForward,
    MoveBackward,
    MoveRight,
    MoveLeft,
    Jump,
    Crouch,
    Ability1,
    Melee,
    Interact,
    Ability2,
    Reload,
    Fire,
    AltFire,
    DropWeapon,
    DropAbility,
    RollLeft,
    RollRight,
    Pause,
    Chat,
    CaptureCursor,
}

pub const INPUT_ACTIONS: [InputAction; INPUT_ACTION_COUNT] = [
    InputAction::MoveForward,
    InputAction::MoveBackward,
    InputAction::MoveRight,
    InputAction::MoveLeft,
    InputAction::Jump,
    InputAction::Crouch,
    InputAction::Ability1,
    InputAction::Melee,
    InputAction::Interact,
    InputAction::Ability2,
    InputAction::Reload,
    InputAction::Fire,
    InputAction::AltFire,
    InputAction::DropWeapon,
    InputAction::DropAbility,
    InputAction::RollLeft,
    InputAction::RollRight,
    InputAction::Pause,
    InputAction::Chat,
    InputAction::CaptureCursor,
];

#[derive(Clone, Copy, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug)]
pub enum BindingButton {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl BindingButton {
    pub fn pressed(
        self,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        match self {
            BindingButton::Key(key) => keyboard.pressed(key),
            BindingButton::Mouse(button) => mouse.pressed(button),
        }
    }

    pub fn just_pressed(
        self,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        match self {
            BindingButton::Key(key) => keyboard.just_pressed(key),
            BindingButton::Mouse(button) => mouse.just_pressed(button),
        }
    }

    pub fn label(self) -> String {
        match self {
            BindingButton::Key(key) => key_label(key),
            BindingButton::Mouse(button) => match button {
                MouseButton::Left => "Mouse Left".to_string(),
                MouseButton::Right => "Mouse Right".to_string(),
                MouseButton::Middle => "Mouse Middle".to_string(),
                MouseButton::Back => "Mouse Back".to_string(),
                MouseButton::Forward => "Mouse Forward".to_string(),
                MouseButton::Other(idx) => format!("Mouse {idx}"),
            },
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug)]
pub enum GamepadBindingButton {
    South,
    East,
    North,
    West,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    Select,
    Start,
    Mode,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

impl GamepadBindingButton {
    pub fn pressed(self, gamepad: Option<&Gamepad>) -> bool {
        gamepad.is_some_and(|gamepad| gamepad.pressed(self.to_bevy()))
    }

    pub fn just_pressed(self, gamepad: Option<&Gamepad>) -> bool {
        gamepad.is_some_and(|gamepad| gamepad.just_pressed(self.to_bevy()))
    }

    /// TODO is there a better way? maybe reflected or something instead of hardcoded?
    pub fn label(self) -> &'static str {
        match self {
            Self::South => "Gamepad South",
            Self::East => "Gamepad East",
            Self::North => "Gamepad North",
            Self::West => "Gamepad West",
            Self::LeftTrigger => "Gamepad L1",
            Self::LeftTrigger2 => "Gamepad L2",
            Self::RightTrigger => "Gamepad R1",
            Self::RightTrigger2 => "Gamepad R2",
            Self::Select => "Gamepad Select",
            Self::Start => "Gamepad Start",
            Self::Mode => "Gamepad Mode",
            Self::LeftThumb => "Gamepad L3",
            Self::RightThumb => "Gamepad R3",
            Self::DPadUp => "Gamepad D-Pad Up",
            Self::DPadDown => "Gamepad D-Pad Down",
            Self::DPadLeft => "Gamepad D-Pad Left",
            Self::DPadRight => "Gamepad D-Pad Right",
        }
    }

    pub fn to_bevy(self) -> GamepadButton {
        match self {
            Self::South => GamepadButton::South,
            Self::East => GamepadButton::East,
            Self::North => GamepadButton::North,
            Self::West => GamepadButton::West,
            Self::LeftTrigger => GamepadButton::LeftTrigger,
            Self::LeftTrigger2 => GamepadButton::LeftTrigger2,
            Self::RightTrigger => GamepadButton::RightTrigger,
            Self::RightTrigger2 => GamepadButton::RightTrigger2,
            Self::Select => GamepadButton::Select,
            Self::Start => GamepadButton::Start,
            Self::Mode => GamepadButton::Mode,
            Self::LeftThumb => GamepadButton::LeftThumb,
            Self::RightThumb => GamepadButton::RightThumb,
            Self::DPadUp => GamepadButton::DPadUp,
            Self::DPadDown => GamepadButton::DPadDown,
            Self::DPadLeft => GamepadButton::DPadLeft,
            Self::DPadRight => GamepadButton::DPadRight,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug, Default)]
pub struct ActionBinding {
    pub primary: Option<BindingButton>,
    pub secondary: Option<BindingButton>,
}

#[derive(Clone, Copy, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug, Default)]
pub struct GamepadActionBinding {
    pub primary: Option<GamepadBindingButton>,
    pub secondary: Option<GamepadBindingButton>,
}

impl GamepadActionBinding {
    pub const fn new(
        primary: GamepadBindingButton,
        secondary: Option<GamepadBindingButton>,
    ) -> Self {
        Self {
            primary: Some(primary),
            secondary,
        }
    }

    pub fn pressed(self, gamepad: Option<&Gamepad>) -> bool {
        self.primary.is_some_and(|button| button.pressed(gamepad))
            || self.secondary.is_some_and(|button| button.pressed(gamepad))
    }

    pub fn just_pressed(self, gamepad: Option<&Gamepad>) -> bool {
        self.primary
            .is_some_and(|button| button.just_pressed(gamepad))
            || self
                .secondary
                .is_some_and(|button| button.just_pressed(gamepad))
    }

    pub fn button(self, slot: BindingSlot) -> Option<GamepadBindingButton> {
        match slot {
            BindingSlot::Primary => self.primary,
            BindingSlot::Secondary => self.secondary,
        }
    }

    pub fn set_button(&mut self, slot: BindingSlot, button: Option<GamepadBindingButton>) {
        match slot {
            BindingSlot::Primary => self.primary = button,
            BindingSlot::Secondary => self.secondary = button,
        }
        if self.primary == self.secondary {
            self.secondary = None;
        }
    }

    pub fn prompt_label(self) -> String {
        self.primary
            .or(self.secondary)
            .map(|button| button.label().to_string())
            .unwrap_or_else(|| "Unbound".to_string())
    }
}

impl ActionBinding {
    pub const fn new(primary: BindingButton, secondary: Option<BindingButton>) -> Self {
        Self {
            primary: Some(primary),
            secondary,
        }
    }

    pub fn pressed(
        self,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.primary
            .is_some_and(|button| button.pressed(keyboard, mouse))
            || self
                .secondary
                .is_some_and(|button| button.pressed(keyboard, mouse))
    }

    pub fn just_pressed(
        self,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.primary
            .is_some_and(|button| button.just_pressed(keyboard, mouse))
            || self
                .secondary
                .is_some_and(|button| button.just_pressed(keyboard, mouse))
    }

    pub fn button(self, slot: BindingSlot) -> Option<BindingButton> {
        match slot {
            BindingSlot::Primary => self.primary,
            BindingSlot::Secondary => self.secondary,
        }
    }

    pub fn set_button(&mut self, slot: BindingSlot, button: Option<BindingButton>) {
        match slot {
            BindingSlot::Primary => self.primary = button,
            BindingSlot::Secondary => self.secondary = button,
        }
        if self.primary == self.secondary {
            self.secondary = None;
        }
    }

    pub fn prompt_label(self) -> String {
        self.primary
            .or(self.secondary)
            .map(BindingButton::label)
            .unwrap_or_else(|| "Unbound".to_string())
    }
}

fn key_label(key: KeyCode) -> String {
    let name = format!("{key:?}");
    if let Some(rest) = name.strip_prefix("Key") {
        return rest.to_string();
    }
    if let Some(rest) = name.strip_prefix("Digit") {
        return rest.to_string();
    }
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() && !name[..i].ends_with(' ') {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum BindingSlot {
    #[default]
    Primary,
    Secondary,
}

#[derive(Resource, Clone)]
pub struct ActiveBindings {
    keybindings: [ActionBinding; INPUT_ACTION_COUNT],
    gamepad_bindings: [GamepadActionBinding; INPUT_ACTION_COUNT],
}

impl Default for ActiveBindings {
    fn default() -> Self {
        Self::from_settings(&KeyBindings::default(), &GamepadBindings::default())
    }
}

impl ActiveBindings {
    pub fn from_settings(keybindings: &KeyBindings, gamepad_bindings: &GamepadBindings) -> Self {
        let mut keys = [ActionBinding::default(); INPUT_ACTION_COUNT];
        let mut gamepads = [GamepadActionBinding::default(); INPUT_ACTION_COUNT];
        for action in INPUT_ACTIONS {
            keys[action as usize] = keybindings.binding(action);
            gamepads[action as usize] = gamepad_bindings.binding(action);
        }
        Self {
            keybindings: keys,
            gamepad_bindings: gamepads,
        }
    }

    pub fn sync_from(&mut self, keybindings: &KeyBindings, gamepad_bindings: &GamepadBindings) {
        for action in INPUT_ACTIONS {
            self.keybindings[action as usize] = keybindings.binding(action);
            self.gamepad_bindings[action as usize] = gamepad_bindings.binding(action);
        }
    }

    pub fn keybinding(&self, action: InputAction) -> ActionBinding {
        self.keybindings[action as usize]
    }

    pub fn gamepad_binding(&self, action: InputAction) -> GamepadActionBinding {
        self.gamepad_bindings[action as usize]
    }

    pub fn prompt_label(&self, action: InputAction) -> String {
        let key = self.keybinding(action).prompt_label();
        let gamepad = self.gamepad_binding(action).prompt_label();
        match (key.as_str(), gamepad.as_str()) {
            ("Unbound", "Unbound") => "Unbound".to_string(),
            (_, "Unbound") => key,
            ("Unbound", _) => gamepad,
            _ => format!("{key} / {gamepad}"),
        }
    }

    pub fn pressed(
        &self,
        action: InputAction,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
        gamepad: Option<&Gamepad>,
    ) -> bool {
        self.keybinding(action).pressed(keyboard, mouse)
            || self.gamepad_binding(action).pressed(gamepad)
    }

    pub fn just_pressed(
        &self,
        action: InputAction,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
        gamepad: Option<&Gamepad>,
    ) -> bool {
        self.keybinding(action).just_pressed(keyboard, mouse)
            || self.gamepad_binding(action).just_pressed(gamepad)
    }
}

fn default_melee_keybinding() -> ActionBinding {
    ActionBinding::new(BindingButton::Key(KeyCode::KeyV), None)
}

fn default_melee_gamepad_binding() -> GamepadActionBinding {
    GamepadActionBinding::new(GamepadBindingButton::West, None)
}

#[derive(Clone, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug)]
pub struct KeyBindings {
    pub move_forward: ActionBinding,
    pub move_backward: ActionBinding,
    pub move_right: ActionBinding,
    pub move_left: ActionBinding,
    pub jump: ActionBinding,
    pub crouch: ActionBinding,
    #[serde(alias = "sprint")]
    pub ability1: ActionBinding,
    #[serde(default = "default_melee_keybinding")]
    pub melee: ActionBinding,
    pub interact: ActionBinding,
    #[serde(alias = "ability")]
    pub ability2: ActionBinding,
    pub reload: ActionBinding,
    pub fire: ActionBinding,
    pub alt_fire: ActionBinding,
    pub drop_weapon: ActionBinding,
    pub drop_ability: ActionBinding,
    pub roll_left: ActionBinding,
    pub roll_right: ActionBinding,
    pub pause: ActionBinding,
    pub chat: ActionBinding,
    pub capture_cursor: ActionBinding,
}

#[derive(Clone, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug)]
pub struct GamepadBindings {
    pub move_forward: GamepadActionBinding,
    pub move_backward: GamepadActionBinding,
    pub move_right: GamepadActionBinding,
    pub move_left: GamepadActionBinding,
    pub jump: GamepadActionBinding,
    pub crouch: GamepadActionBinding,
    pub ability1: GamepadActionBinding,
    #[serde(default = "default_melee_gamepad_binding")]
    pub melee: GamepadActionBinding,
    pub interact: GamepadActionBinding,
    pub ability2: GamepadActionBinding,
    pub reload: GamepadActionBinding,
    pub fire: GamepadActionBinding,
    pub alt_fire: GamepadActionBinding,
    pub drop_weapon: GamepadActionBinding,
    pub drop_ability: GamepadActionBinding,
    pub roll_left: GamepadActionBinding,
    pub roll_right: GamepadActionBinding,
    pub pause: GamepadActionBinding,
    pub chat: GamepadActionBinding,
    pub capture_cursor: GamepadActionBinding,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            move_forward: ActionBinding::new(BindingButton::Key(KeyCode::KeyW), None),
            move_backward: ActionBinding::new(BindingButton::Key(KeyCode::KeyS), None),
            move_right: ActionBinding::new(BindingButton::Key(KeyCode::KeyD), None),
            move_left: ActionBinding::new(BindingButton::Key(KeyCode::KeyA), None),
            jump: ActionBinding::new(BindingButton::Key(KeyCode::Space), None),
            crouch: ActionBinding::new(BindingButton::Key(KeyCode::ControlLeft), None),
            ability1: ActionBinding::new(BindingButton::Key(KeyCode::ShiftLeft), None),
            melee: default_melee_keybinding(),
            interact: ActionBinding::new(BindingButton::Key(KeyCode::KeyF), None),
            ability2: ActionBinding::new(BindingButton::Key(KeyCode::KeyE), None),
            reload: ActionBinding::new(BindingButton::Key(KeyCode::KeyR), None),
            fire: ActionBinding::new(BindingButton::Mouse(MouseButton::Left), None),
            alt_fire: ActionBinding::new(BindingButton::Mouse(MouseButton::Right), None),
            drop_weapon: ActionBinding::new(BindingButton::Key(KeyCode::KeyP), None),
            drop_ability: ActionBinding::new(BindingButton::Key(KeyCode::KeyG), None),
            roll_left: ActionBinding::new(BindingButton::Key(KeyCode::KeyQ), None),
            roll_right: ActionBinding::new(BindingButton::Key(KeyCode::KeyE), None),
            pause: ActionBinding::new(BindingButton::Key(KeyCode::Escape), None),
            chat: ActionBinding::new(BindingButton::Key(KeyCode::KeyT), None),
            capture_cursor: ActionBinding::new(BindingButton::Mouse(MouseButton::Left), None),
        }
    }
}

impl Default for GamepadBindings {
    fn default() -> Self {
        Self {
            move_forward: GamepadActionBinding::new(GamepadBindingButton::DPadUp, None),
            move_backward: GamepadActionBinding::new(GamepadBindingButton::DPadDown, None),
            move_right: GamepadActionBinding::new(GamepadBindingButton::DPadRight, None),
            move_left: GamepadActionBinding::new(GamepadBindingButton::DPadLeft, None),
            jump: GamepadActionBinding::new(GamepadBindingButton::South, None),
            crouch: GamepadActionBinding::new(GamepadBindingButton::East, None),
            ability1: GamepadActionBinding::new(GamepadBindingButton::LeftTrigger2, None),
            melee: default_melee_gamepad_binding(),
            interact: GamepadActionBinding::new(GamepadBindingButton::West, None),
            ability2: GamepadActionBinding::new(GamepadBindingButton::North, None),
            reload: GamepadActionBinding::new(GamepadBindingButton::North, None),
            fire: GamepadActionBinding::new(GamepadBindingButton::RightTrigger2, None),
            alt_fire: GamepadActionBinding::new(GamepadBindingButton::LeftTrigger2, None),
            drop_weapon: GamepadActionBinding::new(GamepadBindingButton::DPadDown, None),
            drop_ability: GamepadActionBinding::new(GamepadBindingButton::DPadUp, None),
            roll_left: GamepadActionBinding::new(GamepadBindingButton::LeftTrigger, None),
            roll_right: GamepadActionBinding::new(GamepadBindingButton::RightTrigger, None),
            pause: GamepadActionBinding::new(GamepadBindingButton::Start, None),
            chat: GamepadActionBinding::new(GamepadBindingButton::Select, None),
            capture_cursor: GamepadActionBinding::new(GamepadBindingButton::South, None),
        }
    }
}

impl KeyBindings {
    pub fn binding(&self, action: InputAction) -> ActionBinding {
        match action {
            InputAction::MoveForward => self.move_forward,
            InputAction::MoveBackward => self.move_backward,
            InputAction::MoveRight => self.move_right,
            InputAction::MoveLeft => self.move_left,
            InputAction::Jump => self.jump,
            InputAction::Crouch => self.crouch,
            InputAction::Ability1 => self.ability1,
            InputAction::Melee => self.melee,
            InputAction::Interact => self.interact,
            InputAction::Ability2 => self.ability2,
            InputAction::Reload => self.reload,
            InputAction::Fire => self.fire,
            InputAction::AltFire => self.alt_fire,
            InputAction::DropWeapon => self.drop_weapon,
            InputAction::DropAbility => self.drop_ability,
            InputAction::RollLeft => self.roll_left,
            InputAction::RollRight => self.roll_right,
            InputAction::Pause => self.pause,
            InputAction::Chat => self.chat,
            InputAction::CaptureCursor => self.capture_cursor,
        }
    }

    pub fn binding_mut(&mut self, action: InputAction) -> &mut ActionBinding {
        match action {
            InputAction::MoveForward => &mut self.move_forward,
            InputAction::MoveBackward => &mut self.move_backward,
            InputAction::MoveRight => &mut self.move_right,
            InputAction::MoveLeft => &mut self.move_left,
            InputAction::Jump => &mut self.jump,
            InputAction::Crouch => &mut self.crouch,
            InputAction::Ability1 => &mut self.ability1,
            InputAction::Melee => &mut self.melee,
            InputAction::Interact => &mut self.interact,
            InputAction::Ability2 => &mut self.ability2,
            InputAction::Reload => &mut self.reload,
            InputAction::Fire => &mut self.fire,
            InputAction::AltFire => &mut self.alt_fire,
            InputAction::DropWeapon => &mut self.drop_weapon,
            InputAction::DropAbility => &mut self.drop_ability,
            InputAction::RollLeft => &mut self.roll_left,
            InputAction::RollRight => &mut self.roll_right,
            InputAction::Pause => &mut self.pause,
            InputAction::Chat => &mut self.chat,
            InputAction::CaptureCursor => &mut self.capture_cursor,
        }
    }
}

impl GamepadBindings {
    pub fn binding(&self, action: InputAction) -> GamepadActionBinding {
        match action {
            InputAction::MoveForward => self.move_forward,
            InputAction::MoveBackward => self.move_backward,
            InputAction::MoveRight => self.move_right,
            InputAction::MoveLeft => self.move_left,
            InputAction::Jump => self.jump,
            InputAction::Crouch => self.crouch,
            InputAction::Ability1 => self.ability1,
            InputAction::Melee => self.melee,
            InputAction::Interact => self.interact,
            InputAction::Ability2 => self.ability2,
            InputAction::Reload => self.reload,
            InputAction::Fire => self.fire,
            InputAction::AltFire => self.alt_fire,
            InputAction::DropWeapon => self.drop_weapon,
            InputAction::DropAbility => self.drop_ability,
            InputAction::RollLeft => self.roll_left,
            InputAction::RollRight => self.roll_right,
            InputAction::Pause => self.pause,
            InputAction::Chat => self.chat,
            InputAction::CaptureCursor => self.capture_cursor,
        }
    }

    pub fn binding_mut(&mut self, action: InputAction) -> &mut GamepadActionBinding {
        match action {
            InputAction::MoveForward => &mut self.move_forward,
            InputAction::MoveBackward => &mut self.move_backward,
            InputAction::MoveRight => &mut self.move_right,
            InputAction::MoveLeft => &mut self.move_left,
            InputAction::Jump => &mut self.jump,
            InputAction::Crouch => &mut self.crouch,
            InputAction::Ability1 => &mut self.ability1,
            InputAction::Melee => &mut self.melee,
            InputAction::Interact => &mut self.interact,
            InputAction::Ability2 => &mut self.ability2,
            InputAction::Reload => &mut self.reload,
            InputAction::Fire => &mut self.fire,
            InputAction::AltFire => &mut self.alt_fire,
            InputAction::DropWeapon => &mut self.drop_weapon,
            InputAction::DropAbility => &mut self.drop_ability,
            InputAction::RollLeft => &mut self.roll_left,
            InputAction::RollRight => &mut self.roll_right,
            InputAction::Pause => &mut self.pause,
            InputAction::Chat => &mut self.chat,
            InputAction::CaptureCursor => &mut self.capture_cursor,
        }
    }
}

pub fn active_gamepad<'a>(gamepads: impl IntoIterator<Item = &'a Gamepad>) -> Option<&'a Gamepad> {
    gamepads.into_iter().next()
}

pub fn stick_with_deadzone(stick: Vec2, deadzone: f32) -> Vec2 {
    let len = stick.length();
    if len <= deadzone {
        return Vec2::ZERO;
    }
    let normalized = stick / len;
    let scaled = ((len - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0);
    normalized * scaled
}
