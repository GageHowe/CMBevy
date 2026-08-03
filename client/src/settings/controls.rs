use bevy::prelude::*;
use common::{BindingButton, BindingSlot, GamepadBindingButton, InputAction, active_gamepad};

use super::data::Settings;

#[derive(Resource, Default)]
pub struct ControlsCapture {
    pub action: Option<InputAction>,
    pub slot: BindingSlot,
    pub device: CaptureDevice,
    skip_frame: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum CaptureDevice {
    #[default]
    KeyboardMouse,
    Gamepad,
}

impl ControlsCapture {
    pub fn is_active(&self) -> bool {
        self.action.is_some()
    }

    pub fn begin(&mut self, action: InputAction, slot: BindingSlot, device: CaptureDevice) {
        self.action = Some(action);
        self.slot = slot;
        self.device = device;
        self.skip_frame = true;
    }

    pub fn cancel(&mut self) {
        self.action = None;
        self.skip_frame = false;
    }
}

pub fn poll_binding_capture(
    settings: &mut Settings,
    keyboard: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    gamepads: &Query<&Gamepad>,
    capture: &mut ControlsCapture,
) -> bool {
    let Some(action) = capture.action else {
        return false;
    };
    if capture.skip_frame {
        capture.skip_frame = false;
        return false;
    }

    if keyboard.just_pressed(KeyCode::Escape) {
        capture.cancel();
        return true;
    }

    match capture.device {
        CaptureDevice::KeyboardMouse => {
            let button = keyboard
                .get_just_pressed()
                .next()
                .copied()
                .map(BindingButton::Key)
                .or_else(|| {
                    mouse.get_just_pressed().find_map(|button| match button {
                        MouseButton::Left
                        | MouseButton::Right
                        | MouseButton::Middle
                        | MouseButton::Back
                        | MouseButton::Forward => Some(BindingButton::Mouse(*button)),
                        MouseButton::Other(_) => None,
                    })
                });
            let Some(button) = button else {
                return false;
            };
            let binding = settings.keybindings.binding_mut(action);
            if binding.button(other_slot(capture.slot)) == Some(button) {
                binding.set_button(other_slot(capture.slot), None);
            }
            binding.set_button(capture.slot, Some(button));
        }
        CaptureDevice::Gamepad => {
            let Some(gamepad) = active_gamepad(gamepads.iter()) else {
                return false;
            };
            let Some(button) = gamepad
                .get_just_pressed()
                .find_map(|button| from_bevy_gamepad_button(*button))
            else {
                return false;
            };
            let binding = settings.gamepad_bindings.binding_mut(action);
            if binding.button(other_slot(capture.slot)) == Some(button) {
                binding.set_button(other_slot(capture.slot), None);
            }
            binding.set_button(capture.slot, Some(button));
        }
    }
    capture.cancel();
    true
}

pub fn button_label(button: Option<BindingButton>) -> String {
    match button {
        Some(button) => binding_button_name(button),
        None => "Unbound".to_string(),
    }
}

pub fn gamepad_button_label(button: Option<GamepadBindingButton>) -> String {
    button
        .map(|button| button.label().to_string())
        .unwrap_or_else(|| "Unbound".to_string())
}

fn other_slot(slot: BindingSlot) -> BindingSlot {
    match slot {
        BindingSlot::Primary => BindingSlot::Secondary,
        BindingSlot::Secondary => BindingSlot::Primary,
    }
}

fn binding_button_name(button: BindingButton) -> String {
    match button {
        BindingButton::Key(key) => format!("{key:?}"),
        BindingButton::Mouse(button) => match button {
            MouseButton::Left => "Mouse Left".to_string(),
            MouseButton::Right => "Mouse Right".to_string(),
            MouseButton::Middle => "Mouse Middle".to_string(),
            MouseButton::Back => "Mouse Back".to_string(),
            MouseButton::Forward => "Mouse Forward".to_string(),
            MouseButton::Other(id) => format!("Mouse {id}"),
        },
    }
}

fn from_bevy_gamepad_button(button: GamepadButton) -> Option<GamepadBindingButton> {
    match button {
        GamepadButton::South => Some(GamepadBindingButton::South),
        GamepadButton::East => Some(GamepadBindingButton::East),
        GamepadButton::North => Some(GamepadBindingButton::North),
        GamepadButton::West => Some(GamepadBindingButton::West),
        GamepadButton::LeftTrigger => Some(GamepadBindingButton::LeftTrigger),
        GamepadButton::LeftTrigger2 => Some(GamepadBindingButton::LeftTrigger2),
        GamepadButton::RightTrigger => Some(GamepadBindingButton::RightTrigger),
        GamepadButton::RightTrigger2 => Some(GamepadBindingButton::RightTrigger2),
        GamepadButton::Select => Some(GamepadBindingButton::Select),
        GamepadButton::Start => Some(GamepadBindingButton::Start),
        GamepadButton::Mode => Some(GamepadBindingButton::Mode),
        GamepadButton::LeftThumb => Some(GamepadBindingButton::LeftThumb),
        GamepadButton::RightThumb => Some(GamepadBindingButton::RightThumb),
        GamepadButton::DPadUp => Some(GamepadBindingButton::DPadUp),
        GamepadButton::DPadDown => Some(GamepadBindingButton::DPadDown),
        GamepadButton::DPadLeft => Some(GamepadBindingButton::DPadLeft),
        GamepadButton::DPadRight => Some(GamepadBindingButton::DPadRight),
        GamepadButton::C | GamepadButton::Z | GamepadButton::Other(_) => None,
    }
}
