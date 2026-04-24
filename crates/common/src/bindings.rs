use bevy::{input::mouse::MouseButton, prelude::*};
use serde::{Deserialize, Serialize};

pub const INPUT_ACTION_COUNT: usize = 19;

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

#[derive(Clone, Copy, Serialize, Deserialize, Reflect, PartialEq, Eq, Debug, Default)]
pub struct ActionBinding {
    pub primary: Option<BindingButton>,
    pub secondary: Option<BindingButton>,
}

impl ActionBinding {
    pub const fn new(primary: BindingButton, secondary: Option<BindingButton>) -> Self {
        Self { primary: Some(primary), secondary }
    }

    pub fn pressed(
        self,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.primary.is_some_and(|button| button.pressed(keyboard, mouse))
            || self.secondary.is_some_and(|button| button.pressed(keyboard, mouse))
    }

    pub fn just_pressed(
        self,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.primary.is_some_and(|button| button.just_pressed(keyboard, mouse))
            || self.secondary.is_some_and(|button| button.just_pressed(keyboard, mouse))
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
pub struct ActiveKeyBindings {
    bindings: [ActionBinding; INPUT_ACTION_COUNT],
}

impl Default for ActiveKeyBindings {
    fn default() -> Self {
        Self::from_settings(&KeyBindings::default())
    }
}

impl ActiveKeyBindings {
    pub fn from_settings(settings: &KeyBindings) -> Self {
        let mut bindings = [ActionBinding::default(); INPUT_ACTION_COUNT];
        for action in INPUT_ACTIONS {
            bindings[action as usize] = settings.binding(action);
        }
        Self { bindings }
    }

    pub fn sync_from(&mut self, settings: &KeyBindings) {
        for action in INPUT_ACTIONS {
            self.bindings[action as usize] = settings.binding(action);
        }
    }

    pub fn binding(&self, action: InputAction) -> ActionBinding {
        self.bindings[action as usize]
    }

    pub fn pressed(
        &self,
        action: InputAction,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.binding(action).pressed(keyboard, mouse)
    }

    pub fn just_pressed(
        &self,
        action: InputAction,
        keyboard: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.binding(action).just_pressed(keyboard, mouse)
    }
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
