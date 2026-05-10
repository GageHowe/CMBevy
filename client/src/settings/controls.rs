use bevy::prelude::*;
use bevy_egui::egui;
use common::{
    ActionBinding, BindingButton, BindingSlot, GamepadActionBinding, GamepadBindingButton,
    InputAction, KeyBindings, active_gamepad,
};

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

pub fn show_controls_settings(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    keyboard: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    gamepads: &Query<&Gamepad>,
    capture: &mut ControlsCapture,
) {
    poll_binding_capture(settings, keyboard, mouse, gamepads, capture);

    ui.horizontal(|ui| {
        if ui.button("Reset to defaults").clicked() {
            settings.keybindings = KeyBindings::default();
            settings.gamepad_bindings = common::GamepadBindings::default();
            capture.cancel();
        }
        if capture.is_active() {
            ui.label(match capture.device {
                CaptureDevice::KeyboardMouse => "Press a key or mouse button. Esc cancels.",
                CaptureDevice::Gamepad => "Press a gamepad button. Esc cancels.",
            });
        } else {
            ui.label("Two binds per action per device.");
        }
    });

    egui::CollapsingHeader::new("Movement")
        .default_open(true)
        .show(ui, |ui| {
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::MoveForward,
                "Move forward",
            );
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::MoveBackward,
                "Move backward",
            );
            show_binding_row(ui, settings, capture, InputAction::MoveRight, "Move right");
            show_binding_row(ui, settings, capture, InputAction::MoveLeft, "Move left");
            show_binding_row(ui, settings, capture, InputAction::Jump, "Jump / ascend");
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::Crouch,
                "Crouch / descend",
            );
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::Ability1,
                "Ability 1 / boost",
            );
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::RollLeft,
                "Ship roll left",
            );
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::RollRight,
                "Ship roll right",
            );
        });

    egui::CollapsingHeader::new("Actions")
        .default_open(true)
        .show(ui, |ui| {
            show_binding_row(ui, settings, capture, InputAction::Melee, "Melee");
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::Interact,
                "Interact / use",
            );
            show_binding_row(ui, settings, capture, InputAction::Ability2, "Ability 2");
            show_binding_row(ui, settings, capture, InputAction::Reload, "Reload");
            show_binding_row(ui, settings, capture, InputAction::Fire, "Fire");
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::AltFire,
                "Alt fire / zoom",
            );
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::DropWeapon,
                "Drop weapon",
            );
        });

    egui::CollapsingHeader::new("Interface")
        .default_open(true)
        .show(ui, |ui| {
            show_binding_row(ui, settings, capture, InputAction::Pause, "Pause / back");
            show_binding_row(ui, settings, capture, InputAction::Chat, "Chat");
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::CaptureCursor,
                "Resume cursor lock",
            );
        });
}

fn show_binding_row(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    capture: &mut ControlsCapture,
    action: InputAction,
    label: &str,
) {
    let keybinding = settings.keybindings.binding(action);
    let gamepad_binding = settings.gamepad_bindings.binding(action);

    ui.horizontal(|ui| {
        ui.set_min_width(180.0);
        ui.label(label);
        binding_slot_button(
            ui,
            capture,
            action,
            BindingSlot::Primary,
            CaptureDevice::KeyboardMouse,
            button_label(keybinding.primary),
        );
        binding_slot_button(
            ui,
            capture,
            action,
            BindingSlot::Secondary,
            CaptureDevice::KeyboardMouse,
            button_label(keybinding.secondary),
        );
        binding_slot_button(
            ui,
            capture,
            action,
            BindingSlot::Primary,
            CaptureDevice::Gamepad,
            gamepad_button_label(gamepad_binding.primary),
        );
        binding_slot_button(
            ui,
            capture,
            action,
            BindingSlot::Secondary,
            CaptureDevice::Gamepad,
            gamepad_button_label(gamepad_binding.secondary),
        );

        if ui.small_button("Clear").clicked() {
            *settings.keybindings.binding_mut(action) = ActionBinding::default();
            *settings.gamepad_bindings.binding_mut(action) = GamepadActionBinding::default();
            if capture.action == Some(action) {
                capture.cancel();
            }
        }
    });
}

fn binding_slot_button(
    ui: &mut egui::Ui,
    capture: &mut ControlsCapture,
    action: InputAction,
    slot: BindingSlot,
    device: CaptureDevice,
    label: String,
) {
    let waiting =
        capture.action == Some(action) && capture.slot == slot && capture.device == device;
    let label = if waiting {
        "Press input...".to_string()
    } else {
        label
    };
    if ui.button(label).clicked() {
        capture.begin(action, slot, device);
    }
}

fn poll_binding_capture(
    settings: &mut Settings,
    keyboard: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
    gamepads: &Query<&Gamepad>,
    capture: &mut ControlsCapture,
) {
    let Some(action) = capture.action else {
        return;
    };
    if capture.skip_frame {
        capture.skip_frame = false;
        return;
    }

    if keyboard.just_pressed(KeyCode::Escape) {
        capture.cancel();
        return;
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
                        MouseButton::Left | MouseButton::Right | MouseButton::Middle => {
                            Some(BindingButton::Mouse(*button))
                        }
                        MouseButton::Back | MouseButton::Forward => {
                            Some(BindingButton::Mouse(*button))
                        }
                        MouseButton::Other(_) => None,
                    })
                });
            let Some(button) = button else {
                return;
            };
            let binding = settings.keybindings.binding_mut(action);
            if binding.button(other_slot(capture.slot)) == Some(button) {
                binding.set_button(other_slot(capture.slot), None);
            }
            binding.set_button(capture.slot, Some(button));
        }
        CaptureDevice::Gamepad => {
            let Some(gamepad) = active_gamepad(gamepads.iter()) else {
                return;
            };
            let button = gamepad
                .get_just_pressed()
                .find_map(|button| from_bevy_gamepad_button(*button));
            let Some(button) = button else {
                return;
            };
            let binding = settings.gamepad_bindings.binding_mut(action);
            if binding.button(other_slot(capture.slot)) == Some(button) {
                binding.set_button(other_slot(capture.slot), None);
            }
            binding.set_button(capture.slot, Some(button));
        }
    }
    capture.cancel();
}

fn other_slot(slot: BindingSlot) -> BindingSlot {
    match slot {
        BindingSlot::Primary => BindingSlot::Secondary,
        BindingSlot::Secondary => BindingSlot::Primary,
    }
}

fn button_label(button: Option<BindingButton>) -> String {
    match button {
        Some(button) => binding_button_name(button),
        None => "Unbound".to_string(),
    }
}

fn gamepad_button_label(button: Option<GamepadBindingButton>) -> String {
    button
        .map(|button| button.label().to_string())
        .unwrap_or_else(|| "Unbound".to_string())
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
