use crate::settings::Settings;
use bevy::prelude::*;
use bevy_egui::egui;
use common::{ActionBinding, BindingButton, BindingSlot, InputAction, KeyBindings};

#[derive(Resource, Default)]
pub struct ControlsCapture {
    pub action: Option<InputAction>,
    pub slot: BindingSlot,
    skip_frame: bool,
}

impl ControlsCapture {
    pub fn is_active(&self) -> bool {
        self.action.is_some()
    }

    pub fn begin(&mut self, action: InputAction, slot: BindingSlot) {
        self.action = Some(action);
        self.slot = slot;
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
    capture: &mut ControlsCapture,
) {
    poll_binding_capture(settings, keyboard, mouse, capture);

    ui.horizontal(|ui| {
        if ui.button("Reset to defaults").clicked() {
            settings.keybindings = KeyBindings::default();
            capture.cancel();
        }
        if capture.is_active() {
            ui.label("Press a key or mouse button. Esc cancels.");
        } else {
            ui.label("Two binds per action.");
        }
    });
    ui.add_space(8.0);

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
            show_binding_row(ui, settings, capture, InputAction::Sprint, "Sprint / boost");
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
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::Interact,
                "Interact / use",
            );
            show_binding_row(
                ui,
                settings,
                capture,
                InputAction::Ability2,
                "Secondary ability",
            );
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
                InputAction::ToggleFlashlight,
                "Toggle flashlight",
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
    let binding = settings.keybindings.binding(action);

    ui.horizontal(|ui| {
        ui.set_min_width(180.0);
        ui.label(label);
        binding_slot_button(ui, capture, action, BindingSlot::Primary, binding.primary);
        binding_slot_button(
            ui,
            capture,
            action,
            BindingSlot::Secondary,
            binding.secondary,
        );

        if ui.small_button("Clear").clicked() {
            *settings.keybindings.binding_mut(action) = ActionBinding::default();
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
    button: Option<BindingButton>,
) {
    let waiting = capture.action == Some(action) && capture.slot == slot;
    let label = if waiting {
        "Press input...".to_string()
    } else {
        button_label(button)
    };
    if ui.button(label).clicked() {
        capture.begin(action, slot);
    }
}

fn poll_binding_capture(
    settings: &mut Settings,
    keyboard: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
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
                MouseButton::Back | MouseButton::Forward => Some(BindingButton::Mouse(*button)),
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

fn binding_button_name(button: BindingButton) -> String {
    match button {
        BindingButton::Mouse(MouseButton::Left) => "Mouse Left".to_string(),
        BindingButton::Mouse(MouseButton::Right) => "Mouse Right".to_string(),
        BindingButton::Mouse(MouseButton::Middle) => "Mouse Middle".to_string(),
        BindingButton::Mouse(MouseButton::Back) => "Mouse Back".to_string(),
        BindingButton::Mouse(MouseButton::Forward) => "Mouse Forward".to_string(),
        BindingButton::Mouse(MouseButton::Other(value)) => format!("Mouse {value}"),
        BindingButton::Key(key) => key_name(key),
    }
}

fn key_name(key: KeyCode) -> String {
    match key {
        KeyCode::Space => "Space".to_string(),
        KeyCode::Escape => "Esc".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::ShiftLeft => "Left Shift".to_string(),
        KeyCode::ShiftRight => "Right Shift".to_string(),
        KeyCode::ControlLeft => "Left Ctrl".to_string(),
        KeyCode::ControlRight => "Right Ctrl".to_string(),
        KeyCode::AltLeft => "Left Alt".to_string(),
        KeyCode::AltRight => "Right Alt".to_string(),
        KeyCode::SuperLeft => "Left Super".to_string(),
        KeyCode::SuperRight => "Right Super".to_string(),
        KeyCode::ArrowUp => "Up".to_string(),
        KeyCode::ArrowDown => "Down".to_string(),
        KeyCode::ArrowLeft => "Left".to_string(),
        KeyCode::ArrowRight => "Right".to_string(),
        _ => format!("{key:?}").replace("Key", ""),
    }
}
