use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

use audio::SoundQueue;
use bevy::{
    app::AppExit,
    input_focus::tab_navigation::{TabGroup, TabIndex},
    picking::hover::Hovered,
    prelude::*,
    text::{EditableText, TextCursorStyle},
    ui::{InteractionDisabled, Pressed},
    ui_widgets::{Activate, Button, TextInput},
};
use common::{BindingSlot, InputAction, PromptDeviceMode, config::CRITICAL_MASS_VERSION};
use gameplay::session::{ServerAddr, SinglePlayerConfig};
use http_common::{LobbyInfo, RegisterRequest};

use crate::{
    GameState, SimState, UiState,
    hosting::{
        available_gametypes, available_maps, fetch_lan_lobbies, fetch_remote_lobbies,
        gametype_path, start_hosted_server,
    },
    settings::{
        CaptureDevice, ControlsCapture, DisplayMode, PhysicsInterp, Settings, SettingsSection,
        ShadowQuality, SsaoQuality, VsyncMode,
        controls::{button_label, gamepad_button_label, poll_binding_capture},
        persistence,
    },
    sound::{AudioOutputDevices, UI_BACK_EVENT, UI_CLICK_EVENT, queue_ui_sound},
    ui::UI_FONT,
};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .add_observer(handle_button)
            .add_systems(
                Update,
                (poll_menu_input, poll_browser, rebuild_menu, style_buttons).chain(),
            );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Screen {
    #[default]
    Root,
    Credits,
    Settings,
    SinglePlayer,
    Multiplayer,
    CustomGames,
    JoinLan,
    JoinByIp,
    Matchmaking,
    Host,
}

impl Screen {
    fn back(self) -> Self {
        match self {
            Screen::Root => Screen::Root,
            Screen::Credits | Screen::Settings | Screen::SinglePlayer | Screen::Multiplayer => {
                Screen::Root
            }
            Screen::CustomGames
            | Screen::JoinLan
            | Screen::JoinByIp
            | Screen::Matchmaking
            | Screen::Host => Screen::Multiplayer,
        }
    }
}

#[derive(Default)]
struct HostState {
    maps: Vec<String>,
    gametypes: Vec<String>,
    map_idx: usize,
    gametype_idx: usize,
    port: String,
    join_host: String,
    join_port: String,
    join_error: String,
    name: String,
    max_players: String,
    advertise: bool,
}

#[derive(Default)]
struct LobbyBrowser {
    rx: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Result<Vec<LobbyInfo>, String>>>>,
    lobbies: Vec<LobbyInfo>,
    error: String,
    fetching: bool,
    done: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuView {
    Main,
    Pause,
    Settings,
}

#[derive(Resource)]
struct MenuState {
    screen: Screen,
    settings_section: SettingsSection,
    host: HostState,
    browser: LobbyBrowser,
    view: Option<MenuView>,
    dirty: bool,
}

#[derive(bevy::ecs::system::SystemParam)]
struct ButtonParams<'w, 's> {
    commands: Commands<'w, 's>,
    exit: MessageWriter<'w, AppExit>,
    next_game: ResMut<'w, NextState<GameState>>,
    next_ui: ResMut<'w, NextState<UiState>>,
    next_sim: ResMut<'w, NextState<SimState>>,
    server_addr: ResMut<'w, ServerAddr>,
    sp_config: ResMut<'w, SinglePlayerConfig>,
    settings: ResMut<'w, Settings>,
    audio_outputs: Res<'w, AudioOutputDevices>,
    capture: ResMut<'w, ControlsCapture>,
    sound_queue: ResMut<'w, SoundQueue>,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            screen: Screen::Root,
            settings_section: SettingsSection::Graphics,
            host: HostState {
                port: "42070".to_string(),
                join_port: default_server_port(),
                name: "My Lobby".to_string(),
                max_players: "8".to_string(),
                ..default()
            },
            browser: default(),
            view: None,
            dirty: true,
        }
    }
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuButton;

#[derive(Component)]
struct MenuField(Field);

#[derive(Component, Clone)]
enum MenuAction {
    Screen(Screen),
    Back,
    Exit,
    StartSinglePlayer,
    JoinByIp,
    Host,
    Refresh,
    Connect(String, Option<String>),
    NextMap,
    NextMode,
    ToggleAdvertise,
    Resume,
    PauseSettings,
    QuitToMenu,
    SettingsBack,
    Setting(SettingAction),
}

#[derive(Clone)]
enum SettingAction {
    Section(SettingsSection),
    Cycle(CycleSetting),
    Toggle(ToggleSetting),
    Step(StepSetting, f32),
    AudioOutput,
    ResetAll,
    RevealFile,
    ResetBindings,
    Capture(InputAction, BindingSlot, CaptureDevice),
    ClearBinding(InputAction),
}

#[derive(Clone, Copy)]
enum Field {
    JoinHost,
    JoinPort,
    HostPort,
    LobbyName,
    MaxPlayers,
}

#[derive(Clone, Copy)]
enum CycleSetting {
    Display,
    Vsync,
    FpsCap,
    Shadow,
    Ssao,
    Physics,
    Prompt,
    FmodBuffer,
}

#[derive(Clone, Copy)]
enum ToggleSetting {
    AntiAliasing,
    AutoExposure,
    Bloom,
    MotionBlur,
    Vignette,
    LensDistortion,
    GamepadInvertY,
    DebugPanel,
    CinematicMode,
    DebugRender,
}

#[derive(Clone, Copy)]
enum StepSetting {
    UiScale,
    ReticleScale,
    BloomIntensity,
    BloomThreshold,
    MotionBlurShutter,
    VignetteIntensity,
    LensDistortionIntensity,
    Gamma,
    Contrast,
    Saturation,
    OutlineRed,
    OutlineGreen,
    OutlineBlue,
    OutlineOpacity,
    Fov,
    MouseSensitivity,
    VehicleSensitivity,
    ZoomSensitivity,
    GamepadLookSensitivity,
    GamepadMoveDeadzone,
    GamepadLookDeadzone,
}

fn poll_menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<common::ActiveBindings>,
    gamepads: Query<&Gamepad>,
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    mut next_ui: ResMut<NextState<UiState>>,
    mut next_sim: ResMut<NextState<SimState>>,
    mut settings: ResMut<Settings>,
    mut capture: ResMut<ControlsCapture>,
    mut state: ResMut<MenuState>,
    mut sound_queue: ResMut<SoundQueue>,
) {
    if poll_binding_capture(&mut settings, &keys, &mouse, &gamepads, &mut capture) {
        state.dirty = true;
        return;
    }
    if capture.is_active() {
        return;
    }
    if !bindings.just_pressed(
        InputAction::Pause,
        &keys,
        &mouse,
        common::active_gamepad(gamepads.iter()),
    ) {
        return;
    }
    if *game_state.get() == GameState::NotPlaying {
        back_screen(&mut *state, &mut sound_queue);
    } else if *ui_state.get() == UiState::Playing {
        next_ui.set(UiState::PauseMenu);
        next_sim.set(SimState::Paused);
        state.dirty = true;
    } else if *ui_state.get() == UiState::Settings {
        queue_ui_sound(&mut sound_queue, UI_BACK_EVENT);
        next_ui.set(UiState::PauseMenu);
        state.dirty = true;
    } else {
        queue_ui_sound(&mut sound_queue, UI_BACK_EVENT);
        next_ui.set(UiState::Playing);
        next_sim.set(SimState::Playing);
        state.dirty = true;
    }
}

fn poll_browser(mut state: ResMut<MenuState>) {
    let fetch = match state.screen {
        Screen::CustomGames => Some(fetch_remote_lobbies as fn() -> Result<Vec<LobbyInfo>, String>),
        Screen::JoinLan => Some(fetch_lan_lobbies as fn() -> Result<Vec<LobbyInfo>, String>),
        _ => None,
    };
    let Some(fetch) = fetch else { return };
    if !state.browser.fetching && state.browser.rx.is_none() && !state.browser.done {
        let (tx, rx) = std::sync::mpsc::channel();
        state.browser.rx = Some(std::sync::Mutex::new(rx));
        state.browser.fetching = true;
        state.browser.error.clear();
        std::thread::spawn(move || {
            let _ = tx.send(fetch());
        });
        state.dirty = true;
    }
    let result = {
        let Some(rx) = &state.browser.rx else {
            return;
        };
        rx.lock().ok().map(|rx| rx.try_recv())
    };
    match result {
        Some(Ok(Ok(lobbies))) => {
            state.browser.lobbies = lobbies;
            state.browser.rx = None;
            state.browser.fetching = false;
            state.browser.done = true;
            state.dirty = true;
        }
        Some(Ok(Err(e))) => {
            state.browser.error = e;
            state.browser.lobbies.clear();
            state.browser.rx = None;
            state.browser.fetching = false;
            state.dirty = true;
        }
        Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {}
        Some(Err(_)) | None => {
            state.browser.rx = None;
            state.browser.fetching = false;
            state.dirty = true;
        }
    }
}

fn rebuild_menu(
    mut commands: Commands,
    roots: Query<Entity, With<MenuRoot>>,
    assets: Res<AssetServer>,
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    settings: Res<Settings>,
    audio_outputs: Res<AudioOutputDevices>,
    capture: Res<ControlsCapture>,
    mut state: ResMut<MenuState>,
) {
    let view = if *game_state.get() == GameState::NotPlaying {
        Some(MenuView::Main)
    } else {
        match ui_state.get() {
            UiState::PauseMenu => Some(MenuView::Pause),
            UiState::Settings => Some(MenuView::Settings),
            UiState::Playing => None,
        }
    };
    if state.view != view {
        state.view = view;
        state.dirty = true;
    }
    if !state.dirty {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(view) = view else {
        state.dirty = false;
        return;
    };
    let font = assets.load(UI_FONT);
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                padding: UiRect::top(percent(12)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.0, 0.03, 0.9)),
            ZIndex(100),
            TabGroup::default(),
        ))
        .with_children(|root| {
            root.spawn(Node {
                width: px(560),
                max_height: percent(78),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: px(14).all(),
                overflow: Overflow::scroll_y(),
                ..default()
            })
            .with_children(|panel| match view {
                MenuView::Main => {
                    build_main(panel, &font, &settings, &audio_outputs, &capture, &*state)
                }
                MenuView::Pause => build_pause(panel, &font),
                MenuView::Settings => {
                    heading(panel, &font, "Settings");
                    build_settings(panel, &font, &settings, &audio_outputs, &capture, &*state);
                    button(panel, &font, "Back", MenuAction::SettingsBack, false);
                }
            });
            root.spawn((
                Text::new(format!("v{CRITICAL_MASS_VERSION}")),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
                Node {
                    position_type: PositionType::Absolute,
                    right: px(16),
                    bottom: px(12),
                    ..default()
                },
            ));
        });
    state.dirty = false;
}

fn build_main(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    settings: &Settings,
    audio_outputs: &AudioOutputDevices,
    capture: &ControlsCapture,
    state: &MenuState,
) {
    let title = match state.screen {
        Screen::Root => "Critical Mass",
        Screen::Credits => "Credits",
        Screen::Settings => "Settings",
        Screen::SinglePlayer => "Singleplayer",
        Screen::Multiplayer => "Multiplayer",
        Screen::CustomGames => "Custom Games",
        Screen::JoinLan => "LAN Games",
        Screen::JoinByIp => "Join by IP",
        Screen::Matchmaking => "Matchmaking",
        Screen::Host => "Host",
    };
    heading(root, font, title);
    match state.screen {
        Screen::Root => {
            button(
                root,
                font,
                "Singleplayer",
                MenuAction::Screen(Screen::SinglePlayer),
                false,
            );
            button(
                root,
                font,
                "Multiplayer",
                MenuAction::Screen(Screen::Multiplayer),
                false,
            );
            button(
                root,
                font,
                "Settings",
                MenuAction::Screen(Screen::Settings),
                false,
            );
            button(
                root,
                font,
                "Credits",
                MenuAction::Screen(Screen::Credits),
                false,
            );
            button(root, font, "Exit", MenuAction::Exit, false);
        }
        Screen::Credits => {
            for line in credits_lines() {
                label(root, font, line, 15.0);
            }
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::Settings => {
            build_settings(root, font, settings, audio_outputs, capture, state);
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::SinglePlayer => {
            map_rows(root, font, &state.host);
            let disabled = state.host.maps.is_empty() || state.host.gametypes.is_empty();
            button(root, font, "Start", MenuAction::StartSinglePlayer, disabled);
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::Multiplayer => {
            button(
                root,
                font,
                "Join LAN",
                MenuAction::Screen(Screen::JoinLan),
                false,
            );
            button(
                root,
                font,
                "Join by IP",
                MenuAction::Screen(Screen::JoinByIp),
                false,
            );
            button(
                root,
                font,
                "Custom Games",
                MenuAction::Screen(Screen::CustomGames),
                false,
            );
            button(
                root,
                font,
                "Matchmaking",
                MenuAction::Screen(Screen::Matchmaking),
                false,
            );
            button(root, font, "Host", MenuAction::Screen(Screen::Host), false);
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::CustomGames | Screen::JoinLan => browser(root, font, &state.browser),
        Screen::JoinByIp => {
            field(
                root,
                font,
                "Host/IP",
                Field::JoinHost,
                &state.host.join_host,
            );
            field(root, font, "Port", Field::JoinPort, &state.host.join_port);
            if !state.host.join_error.is_empty() {
                label(root, font, &state.host.join_error, 14.0);
            }
            let disabled = state.host.join_host.trim().is_empty()
                || state.host.join_port.parse::<u16>().is_err();
            button(root, font, "Join", MenuAction::JoinByIp, disabled);
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::Matchmaking => {
            label(root, font, "Matchmaking coming soon.", 15.0);
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::Host => {
            map_rows(root, font, &state.host);
            field(root, font, "Port", Field::HostPort, &state.host.port);
            button(
                root,
                font,
                if state.host.advertise {
                    "Advertise: On"
                } else {
                    "Advertise: Off"
                },
                MenuAction::ToggleAdvertise,
                false,
            );
            if state.host.advertise {
                field(root, font, "Lobby name", Field::LobbyName, &state.host.name);
                field(
                    root,
                    font,
                    "Max players",
                    Field::MaxPlayers,
                    &state.host.max_players,
                );
            }
            let disabled = state.host.maps.is_empty()
                || state.host.gametypes.is_empty()
                || state.host.port.parse::<u16>().is_err()
                || (state.host.advertise && state.host.max_players.parse::<u8>().is_err());
            button(root, font, "Start & Join", MenuAction::Host, disabled);
            button(root, font, "Back", MenuAction::Back, false);
        }
    }
}

fn build_pause(root: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    heading(root, font, "Paused");
    button(root, font, "Resume", MenuAction::Resume, false);
    button(root, font, "Settings", MenuAction::PauseSettings, false);
    button(root, font, "Quit to Menu", MenuAction::QuitToMenu, false);
}

fn build_settings(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    settings: &Settings,
    audio_outputs: &AudioOutputDevices,
    capture: &ControlsCapture,
    state: &MenuState,
) {
    row(root, |row| {
        for section in [
            SettingsSection::Graphics,
            SettingsSection::Audio,
            SettingsSection::Input,
            SettingsSection::Controls,
            SettingsSection::Misc,
        ] {
            button(
                row,
                font,
                section_label(section),
                MenuAction::Setting(SettingAction::Section(section)),
                state.settings_section == section,
            );
        }
    });
    match state.settings_section {
        SettingsSection::Graphics => graphics_settings(root, font, settings),
        SettingsSection::Audio => audio_settings(root, font, settings, audio_outputs),
        SettingsSection::Input => input_settings(root, font, settings),
        SettingsSection::Controls => controls_settings(root, font, settings, capture),
        SettingsSection::Misc => misc_settings(root, font, settings),
    }
    row(root, |row| {
        button(
            row,
            font,
            "View settings file",
            MenuAction::Setting(SettingAction::RevealFile),
            false,
        );
        button(
            row,
            font,
            "Reset all",
            MenuAction::Setting(SettingAction::ResetAll),
            false,
        );
    });
}

fn graphics_settings(root: &mut ChildSpawnerCommands, font: &Handle<Font>, settings: &Settings) {
    cycle(
        root,
        font,
        "Display",
        display_label(settings.display_mode),
        CycleSetting::Display,
    );
    cycle(
        root,
        font,
        "VSync",
        vsync_label(settings.vsync),
        CycleSetting::Vsync,
    );
    step(
        root,
        font,
        "UI size",
        format!("{:.0}%", settings.ui_scale * 100.0),
        StepSetting::UiScale,
        0.05,
    );
    step(
        root,
        font,
        "Reticle size",
        format!("{:.2}", settings.reticle_scale),
        StepSetting::ReticleScale,
        0.05,
    );
    cycle(
        root,
        font,
        "FPS cap",
        fps_cap_label(settings.fps_cap),
        CycleSetting::FpsCap,
    );
    cycle(
        root,
        font,
        "Shadow quality",
        shadow_label(settings.shadow_quality),
        CycleSetting::Shadow,
    );
    toggle(
        root,
        font,
        "Anti-aliasing",
        settings.anti_aliasing,
        ToggleSetting::AntiAliasing,
    );
    toggle(
        root,
        font,
        "Auto exposure",
        settings.auto_exposure,
        ToggleSetting::AutoExposure,
    );
    toggle(root, font, "Bloom", settings.bloom, ToggleSetting::Bloom);
    step(
        root,
        font,
        "Bloom intensity",
        format!("{:.2}", settings.bloom_intensity),
        StepSetting::BloomIntensity,
        0.1,
    );
    step(
        root,
        font,
        "Bloom threshold",
        format!("{:.2}", settings.bloom_threshold),
        StepSetting::BloomThreshold,
        0.1,
    );
    toggle(
        root,
        font,
        "Motion blur",
        settings.motion_blur,
        ToggleSetting::MotionBlur,
    );
    step(
        root,
        font,
        "Shutter angle",
        format!("{:.2}", settings.motion_blur_shutter_angle),
        StepSetting::MotionBlurShutter,
        0.1,
    );
    toggle(
        root,
        font,
        "Vignette",
        settings.vignette,
        ToggleSetting::Vignette,
    );
    step(
        root,
        font,
        "Vignette intensity",
        format!("{:.2}", settings.vignette_intensity),
        StepSetting::VignetteIntensity,
        0.05,
    );
    toggle(
        root,
        font,
        "Lens distortion",
        settings.lens_distortion,
        ToggleSetting::LensDistortion,
    );
    step(
        root,
        font,
        "Distortion",
        format!("{:.2}", settings.lens_distortion_intensity),
        StepSetting::LensDistortionIntensity,
        0.02,
    );
    cycle(
        root,
        font,
        "SSAO",
        ssao_label(settings.ssao_quality),
        CycleSetting::Ssao,
    );
    step(
        root,
        font,
        "Gamma",
        format!("{:.2}", settings.gamma),
        StepSetting::Gamma,
        0.05,
    );
    step(
        root,
        font,
        "Contrast",
        format!("{:.2}", settings.contrast),
        StepSetting::Contrast,
        0.05,
    );
    step(
        root,
        font,
        "Saturation",
        format!("{:.2}", settings.saturation),
        StepSetting::Saturation,
        0.05,
    );
    step(
        root,
        font,
        "Outline R",
        format!("{:.2}", settings.outline_red),
        StepSetting::OutlineRed,
        0.05,
    );
    step(
        root,
        font,
        "Outline G",
        format!("{:.2}", settings.outline_green),
        StepSetting::OutlineGreen,
        0.05,
    );
    step(
        root,
        font,
        "Outline B",
        format!("{:.2}", settings.outline_blue),
        StepSetting::OutlineBlue,
        0.05,
    );
    step(
        root,
        font,
        "Outline opacity",
        format!("{:.2}", settings.outline_opacity),
        StepSetting::OutlineOpacity,
        0.01,
    );
    step(
        root,
        font,
        "FOV",
        format!("{:.0}", settings.fov),
        StepSetting::Fov,
        5.0,
    );
    cycle(
        root,
        font,
        "Physics interp",
        physics_label(settings.physics_interp),
        CycleSetting::Physics,
    );
}

fn audio_settings(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    settings: &Settings,
    audio_outputs: &AudioOutputDevices,
) {
    let output = if settings.audio_output_device.is_empty() {
        "System Default"
    } else {
        settings.audio_output_device.as_str()
    };
    cycle_action(root, font, "Output", output, SettingAction::AudioOutput);
    cycle(
        root,
        font,
        "FMOD buffer",
        &settings.fmod_buffer_size.to_string(),
        CycleSetting::FmodBuffer,
    );
    if audio_outputs.names.is_empty() {
        label(root, font, "No alternate output devices detected.", 13.0);
    }
}

fn input_settings(root: &mut ChildSpawnerCommands, font: &Handle<Font>, settings: &Settings) {
    step(
        root,
        font,
        "Mouse sensitivity",
        format!("{:.4}", settings.mouse_sensitivity),
        StepSetting::MouseSensitivity,
        0.0002,
    );
    step(
        root,
        font,
        "Vehicle pitch/yaw",
        format!("{:.4}", settings.vehicle_pitch_yaw_sensitivity),
        StepSetting::VehicleSensitivity,
        0.0002,
    );
    step(
        root,
        font,
        "Zoom sensitivity",
        format!("{:.2}", settings.zoom_sensitivity_blend),
        StepSetting::ZoomSensitivity,
        0.05,
    );
    step(
        root,
        font,
        "Gamepad look",
        format!("{:.2}", settings.gamepad_look_sensitivity),
        StepSetting::GamepadLookSensitivity,
        0.25,
    );
    step(
        root,
        font,
        "Move deadzone",
        format!("{:.2}", settings.gamepad_move_deadzone),
        StepSetting::GamepadMoveDeadzone,
        0.02,
    );
    step(
        root,
        font,
        "Look deadzone",
        format!("{:.2}", settings.gamepad_look_deadzone),
        StepSetting::GamepadLookDeadzone,
        0.02,
    );
    toggle(
        root,
        font,
        "Invert gamepad Y",
        settings.gamepad_invert_y,
        ToggleSetting::GamepadInvertY,
    );
    cycle(
        root,
        font,
        "Prompt labels",
        prompt_label(settings.prompt_device_mode),
        CycleSetting::Prompt,
    );
}

fn misc_settings(root: &mut ChildSpawnerCommands, font: &Handle<Font>, settings: &Settings) {
    toggle(
        root,
        font,
        "Debug panel",
        settings.debug_panel,
        ToggleSetting::DebugPanel,
    );
    toggle(
        root,
        font,
        "Cinematic mode",
        settings.cinematic_mode,
        ToggleSetting::CinematicMode,
    );
    toggle(
        root,
        font,
        "Debug rendering",
        settings.debug_render,
        ToggleSetting::DebugRender,
    );
}

fn controls_settings(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    settings: &Settings,
    capture: &ControlsCapture,
) {
    row(root, |row| {
        button(
            row,
            font,
            "Reset bindings",
            MenuAction::Setting(SettingAction::ResetBindings),
            false,
        );
        label(
            row,
            font,
            if capture.is_active() {
                "Press input, Esc cancels"
            } else {
                "Two binds per action per device"
            },
            13.0,
        );
    });
    for action in InputAction::ALL {
        let key = settings.keybindings.binding(action);
        let pad = settings.gamepad_bindings.binding(action);
        row(root, |row| {
            label(row, font, action_label(action), 13.0);
            capture_button(
                row,
                font,
                capture,
                action,
                BindingSlot::Primary,
                CaptureDevice::KeyboardMouse,
                button_label(key.primary),
            );
            capture_button(
                row,
                font,
                capture,
                action,
                BindingSlot::Secondary,
                CaptureDevice::KeyboardMouse,
                button_label(key.secondary),
            );
            capture_button(
                row,
                font,
                capture,
                action,
                BindingSlot::Primary,
                CaptureDevice::Gamepad,
                gamepad_button_label(pad.primary),
            );
            capture_button(
                row,
                font,
                capture,
                action,
                BindingSlot::Secondary,
                CaptureDevice::Gamepad,
                gamepad_button_label(pad.secondary),
            );
            button(
                row,
                font,
                "Clear",
                MenuAction::Setting(SettingAction::ClearBinding(action)),
                false,
            );
        });
    }
}

fn handle_button(
    activate: On<Activate>,
    actions: Query<&MenuAction>,
    fields: Query<(&MenuField, &EditableText)>,
    mut state: ResMut<MenuState>,
    mut p: ButtonParams,
) {
    let Ok(action) = actions.get(activate.entity).cloned() else {
        return;
    };
    sync_fields(&fields, &mut state.host);
    match action {
        MenuAction::Screen(screen) => {
            if matches!(screen, Screen::SinglePlayer | Screen::Host) {
                reset_host_catalog(&mut state.host);
            }
            if matches!(screen, Screen::CustomGames | Screen::JoinLan) {
                state.browser = default();
            }
            if screen == Screen::JoinByIp {
                state.host.join_error.clear();
            }
            state.screen = screen;
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::Back => back_screen(&mut *state, &mut p.sound_queue),
        MenuAction::Exit => {
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
            p.exit.write(AppExit::Success);
        }
        MenuAction::StartSinglePlayer => {
            p.sp_config.map = format!("maps/{}.ron", state.host.maps[state.host.map_idx]);
            p.sp_config.gametype = gametype_path(&state.host.gametypes[state.host.gametype_idx]);
            state.screen = Screen::Root;
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
            p.next_game.set(GameState::SinglePlayer);
        }
        MenuAction::JoinByIp => join_by_ip(
            &mut *state,
            &mut p.server_addr,
            &mut p.next_game,
            &mut p.sound_queue,
        ),
        MenuAction::Host => host_game(
            &mut p.commands,
            &mut *state,
            &mut p.server_addr,
            &mut p.next_game,
            &mut p.sound_queue,
        ),
        MenuAction::Refresh => {
            state.browser = default();
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::Connect(addr, lobby_id) => {
            connect_to_lobby(
                &addr,
                lobby_id.as_deref(),
                &mut p.server_addr,
                &mut p.next_game,
                &mut *state,
            );
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::NextMap => {
            if !state.host.maps.is_empty() {
                state.host.map_idx = (state.host.map_idx + 1) % state.host.maps.len();
            }
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::NextMode => {
            if !state.host.gametypes.is_empty() {
                state.host.gametype_idx =
                    (state.host.gametype_idx + 1) % state.host.gametypes.len();
            }
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::ToggleAdvertise => {
            state.host.advertise = !state.host.advertise;
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::Resume => {
            p.next_ui.set(UiState::Playing);
            p.next_sim.set(SimState::Playing);
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::PauseSettings => {
            p.next_ui.set(UiState::Settings);
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::QuitToMenu => {
            p.next_game.set(GameState::NotPlaying);
            p.next_ui.set(UiState::Playing);
            p.next_sim.set(SimState::Playing);
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::SettingsBack => {
            p.next_ui.set(UiState::PauseMenu);
            queue_ui_sound(&mut p.sound_queue, UI_BACK_EVENT);
        }
        MenuAction::Setting(action) => apply_setting(
            action,
            &mut p.settings,
            &p.audio_outputs,
            &mut p.capture,
            &mut *state,
        ),
    }
    state.dirty = true;
}

fn apply_setting(
    action: SettingAction,
    settings: &mut Settings,
    audio_outputs: &AudioOutputDevices,
    capture: &mut ControlsCapture,
    state: &mut MenuState,
) {
    match action {
        SettingAction::Section(section) => state.settings_section = section,
        SettingAction::Cycle(setting) => cycle_setting(settings, setting),
        SettingAction::Toggle(setting) => toggle_setting(settings, setting),
        SettingAction::Step(setting, delta) => step_setting(settings, setting, delta),
        SettingAction::AudioOutput => {
            let mut outputs = vec![String::new()];
            outputs.extend(audio_outputs.names.iter().cloned());
            let i = outputs
                .iter()
                .position(|name| name == &settings.audio_output_device)
                .unwrap_or(0);
            settings.audio_output_device = outputs[(i + 1) % outputs.len()].clone();
        }
        SettingAction::ResetAll => {
            capture.cancel();
            persistence::reset_settings(settings);
        }
        SettingAction::RevealFile => {
            if let Err(err) = persistence::reveal_settings_file() {
                warn!("failed to open settings file location: {err}");
            }
        }
        SettingAction::ResetBindings => {
            settings.keybindings = common::KeyBindings::default();
            settings.gamepad_bindings = common::GamepadBindings::default();
            capture.cancel();
        }
        SettingAction::Capture(action, slot, device) => capture.begin(action, slot, device),
        SettingAction::ClearBinding(action) => {
            *settings.keybindings.binding_mut(action) = default();
            *settings.gamepad_bindings.binding_mut(action) = default();
            if capture.action == Some(action) {
                capture.cancel();
            }
        }
    }
}

fn cycle_setting(settings: &mut Settings, setting: CycleSetting) {
    match setting {
        CycleSetting::Display => {
            settings.display_mode = match settings.display_mode {
                DisplayMode::Windowed => DisplayMode::BorderlessFullscreen,
                DisplayMode::BorderlessFullscreen => DisplayMode::Windowed,
            }
        }
        CycleSetting::Vsync => {
            settings.vsync = match settings.vsync {
                VsyncMode::AutoVsync => VsyncMode::AutoNoVsync,
                VsyncMode::AutoNoVsync => VsyncMode::Fifo,
                VsyncMode::Fifo => VsyncMode::FifoRelaxed,
                VsyncMode::FifoRelaxed => VsyncMode::Immediate,
                VsyncMode::Immediate => VsyncMode::Mailbox,
                VsyncMode::Mailbox => VsyncMode::AutoVsync,
            }
        }
        CycleSetting::FpsCap => {
            let caps = [0, 30, 60, 90, 120, 144, 165, 240];
            let i = caps
                .iter()
                .position(|cap| *cap == settings.fps_cap)
                .unwrap_or(0);
            settings.fps_cap = caps[(i + 1) % caps.len()];
        }
        CycleSetting::Shadow => {
            settings.shadow_quality = match settings.shadow_quality {
                ShadowQuality::Off => ShadowQuality::Low,
                ShadowQuality::Low => ShadowQuality::Medium,
                ShadowQuality::Medium => ShadowQuality::High,
                ShadowQuality::High => ShadowQuality::Off,
            }
        }
        CycleSetting::Ssao => {
            settings.ssao_quality = match settings.ssao_quality {
                SsaoQuality::Off => SsaoQuality::Medium,
                SsaoQuality::Medium => SsaoQuality::High,
                SsaoQuality::High => SsaoQuality::Ultra,
                SsaoQuality::Ultra => SsaoQuality::Off,
            }
        }
        CycleSetting::Physics => {
            settings.physics_interp = match settings.physics_interp {
                PhysicsInterp::Off => PhysicsInterp::Interpolate,
                PhysicsInterp::Interpolate => PhysicsInterp::Extrapolate,
                PhysicsInterp::Extrapolate => PhysicsInterp::Balanced,
                PhysicsInterp::Balanced => PhysicsInterp::Off,
            }
        }
        CycleSetting::Prompt => {
            settings.prompt_device_mode = match settings.prompt_device_mode {
                PromptDeviceMode::KeyboardMouse => PromptDeviceMode::Gamepad,
                PromptDeviceMode::Gamepad => PromptDeviceMode::Both,
                PromptDeviceMode::Both => PromptDeviceMode::KeyboardMouse,
            }
        }
        CycleSetting::FmodBuffer => {
            let sizes = [128, 256, 512, 1024];
            let i = sizes
                .iter()
                .position(|size| *size == settings.fmod_buffer_size)
                .unwrap_or(1);
            settings.fmod_buffer_size = sizes[(i + 1) % sizes.len()];
        }
    }
}

fn toggle_setting(settings: &mut Settings, setting: ToggleSetting) {
    match setting {
        ToggleSetting::AntiAliasing => settings.anti_aliasing = !settings.anti_aliasing,
        ToggleSetting::AutoExposure => settings.auto_exposure = !settings.auto_exposure,
        ToggleSetting::Bloom => settings.bloom = !settings.bloom,
        ToggleSetting::MotionBlur => settings.motion_blur = !settings.motion_blur,
        ToggleSetting::Vignette => settings.vignette = !settings.vignette,
        ToggleSetting::LensDistortion => settings.lens_distortion = !settings.lens_distortion,
        ToggleSetting::GamepadInvertY => settings.gamepad_invert_y = !settings.gamepad_invert_y,
        ToggleSetting::DebugPanel => settings.debug_panel = !settings.debug_panel,
        ToggleSetting::CinematicMode => settings.cinematic_mode = !settings.cinematic_mode,
        ToggleSetting::DebugRender => settings.debug_render = !settings.debug_render,
    }
}

fn step_setting(settings: &mut Settings, setting: StepSetting, delta: f32) {
    let value = match setting {
        StepSetting::UiScale => &mut settings.ui_scale,
        StepSetting::ReticleScale => &mut settings.reticle_scale,
        StepSetting::BloomIntensity => &mut settings.bloom_intensity,
        StepSetting::BloomThreshold => &mut settings.bloom_threshold,
        StepSetting::MotionBlurShutter => &mut settings.motion_blur_shutter_angle,
        StepSetting::VignetteIntensity => &mut settings.vignette_intensity,
        StepSetting::LensDistortionIntensity => &mut settings.lens_distortion_intensity,
        StepSetting::Gamma => &mut settings.gamma,
        StepSetting::Contrast => &mut settings.contrast,
        StepSetting::Saturation => &mut settings.saturation,
        StepSetting::OutlineRed => &mut settings.outline_red,
        StepSetting::OutlineGreen => &mut settings.outline_green,
        StepSetting::OutlineBlue => &mut settings.outline_blue,
        StepSetting::OutlineOpacity => &mut settings.outline_opacity,
        StepSetting::Fov => &mut settings.fov,
        StepSetting::MouseSensitivity => &mut settings.mouse_sensitivity,
        StepSetting::VehicleSensitivity => &mut settings.vehicle_pitch_yaw_sensitivity,
        StepSetting::ZoomSensitivity => &mut settings.zoom_sensitivity_blend,
        StepSetting::GamepadLookSensitivity => &mut settings.gamepad_look_sensitivity,
        StepSetting::GamepadMoveDeadzone => &mut settings.gamepad_move_deadzone,
        StepSetting::GamepadLookDeadzone => &mut settings.gamepad_look_deadzone,
    };
    let (min, max) = match setting {
        StepSetting::UiScale => (0.5, 2.5),
        StepSetting::ReticleScale => (0.5, 2.0),
        StepSetting::BloomIntensity => (0.0, 2.0),
        StepSetting::BloomThreshold => (0.0, 5.0),
        StepSetting::MotionBlurShutter => (0.0, std::f32::consts::TAU),
        StepSetting::VignetteIntensity | StepSetting::OutlineOpacity => (0.0, 1.0),
        StepSetting::LensDistortionIntensity => (-0.25, 0.25),
        StepSetting::Gamma => (0.5, 2.0),
        StepSetting::Contrast => (0.5, 1.5),
        StepSetting::Saturation => (0.0, 2.0),
        StepSetting::OutlineRed | StepSetting::OutlineGreen | StepSetting::OutlineBlue => {
            (0.0, 1.0)
        }
        StepSetting::Fov => (60.0, 160.0),
        StepSetting::MouseSensitivity | StepSetting::VehicleSensitivity => (0.0001, 0.01),
        StepSetting::ZoomSensitivity => (0.0, 1.0),
        StepSetting::GamepadLookSensitivity => (0.5, 8.0),
        StepSetting::GamepadMoveDeadzone | StepSetting::GamepadLookDeadzone => (0.0, 0.5),
    };
    *value = (*value + delta).clamp(min, max);
}

fn sync_fields(fields: &Query<(&MenuField, &EditableText)>, host: &mut HostState) {
    for (field, text) in fields {
        let value = text.value().to_string();
        match field.0 {
            Field::JoinHost => host.join_host = value,
            Field::JoinPort => host.join_port = value,
            Field::HostPort => host.port = value,
            Field::LobbyName => host.name = value,
            Field::MaxPlayers => host.max_players = value,
        }
    }
}

fn join_by_ip(
    state: &mut MenuState,
    server_addr: &mut ServerAddr,
    next_state: &mut NextState<GameState>,
    sound_queue: &mut SoundQueue,
) {
    let addr = format!(
        "{}:{}",
        state.host.join_host.trim(),
        state.host.join_port.trim()
    );
    match addr
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
    {
        Some(addr) => {
            server_addr.addr = addr;
            server_addr.lobby_id = None;
            state.host.join_error.clear();
            state.screen = Screen::Root;
            next_state.set(GameState::Multiplayer);
            queue_ui_sound(sound_queue, UI_CLICK_EVENT);
        }
        None => state.host.join_error = format!("Invalid server address: {addr}"),
    }
}

fn host_game(
    commands: &mut Commands,
    state: &mut MenuState,
    server_addr: &mut ServerAddr,
    next_state: &mut NextState<GameState>,
    sound_queue: &mut SoundQueue,
) {
    let port = state.host.port.parse().unwrap_or(42070);
    let map = format!("maps/{}.ron", state.host.maps[state.host.map_idx]);
    let gametype = gametype_path(&state.host.gametypes[state.host.gametype_idx]);
    let advertise = state.host.advertise.then(|| RegisterRequest {
        quic_port: port,
        name: state.host.name.clone(),
        max_players: state.host.max_players.parse().unwrap_or(8),
    });
    match start_hosted_server(port, &map, &gametype, advertise) {
        Ok(()) => {
            server_addr.addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            server_addr.lobby_id = None;
            state.screen = Screen::Root;
            next_state.set(GameState::Multiplayer);
            queue_ui_sound(sound_queue, UI_CLICK_EVENT);
        }
        Err(e) => gameplay::messages::push(commands, format!("Failed to start gameserver: {e}")),
    }
}

fn connect_to_lobby(
    addr: &str,
    lobby_id: Option<&str>,
    server_addr: &mut ServerAddr,
    next_state: &mut NextState<GameState>,
    state: &mut MenuState,
) {
    if let Ok(sa) = addr.parse() {
        server_addr.addr = sa;
        server_addr.lobby_id = lobby_id.map(ToOwned::to_owned);
        state.screen = Screen::Root;
        state.browser = default();
        next_state.set(GameState::Multiplayer);
    }
}

fn back_screen(state: &mut MenuState, sound_queue: &mut SoundQueue) {
    let next = state.screen.back();
    if next == state.screen {
        return;
    }
    if matches!(state.screen, Screen::CustomGames | Screen::JoinLan) {
        state.browser = default();
    }
    state.screen = next;
    queue_ui_sound(sound_queue, UI_BACK_EVENT);
}

fn reset_host_catalog(host: &mut HostState) {
    host.maps = available_maps();
    host.gametypes = available_gametypes();
    host.map_idx = 0;
    host.gametype_idx = 0;
}

fn default_server_port() -> String {
    common::config::SERVER_BIND_ADDRESS
        .parse::<SocketAddr>()
        .map(|addr| addr.port())
        .unwrap_or(42070)
        .to_string()
}

fn browser(root: &mut ChildSpawnerCommands, font: &Handle<Font>, browser: &LobbyBrowser) {
    if browser.fetching {
        label(root, font, "Loading...", 15.0);
    } else if !browser.error.is_empty() {
        label(root, font, &browser.error, 15.0);
    } else if browser.lobbies.is_empty() {
        label(root, font, "No lobbies found.", 15.0);
    } else {
        for lobby in &browser.lobbies {
            row(root, |row| {
                label(
                    row,
                    font,
                    &format!(
                        "{}  {} ({}/{})",
                        lobby.name, lobby.host, lobby.player_count, lobby.max_players
                    ),
                    13.0,
                );
                button(
                    row,
                    font,
                    "Connect",
                    MenuAction::Connect(
                        lobby.host.clone(),
                        (!lobby.id.is_empty()).then(|| lobby.id.clone()),
                    ),
                    false,
                );
            });
        }
    }
    button(root, font, "Refresh", MenuAction::Refresh, false);
    button(root, font, "Back", MenuAction::Back, false);
}

fn map_rows(root: &mut ChildSpawnerCommands, font: &Handle<Font>, host: &HostState) {
    let map = host.maps.get(host.map_idx).map_or("-", String::as_str);
    let mode = host
        .gametypes
        .get(host.gametype_idx)
        .map_or("-", String::as_str);
    button(
        root,
        font,
        &format!("Map: {map}"),
        MenuAction::NextMap,
        host.maps.is_empty(),
    );
    button(
        root,
        font,
        &format!("Mode: {mode}"),
        MenuAction::NextMode,
        host.gametypes.is_empty(),
    );
}

fn heading(root: &mut ChildSpawnerCommands, font: &Handle<Font>, value: &str) {
    root.spawn((
        Text::new(value),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(32.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            margin: UiRect::bottom(px(8)),
            ..default()
        },
    ));
}

fn label(root: &mut ChildSpawnerCommands, font: &Handle<Font>, value: &str, size: f32) {
    root.spawn((
        Text::new(value),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(Color::WHITE),
        TextLayout {
            linebreak: LineBreak::WordOrCharacter,
            ..default()
        },
        Node {
            max_width: px(520),
            ..default()
        },
    ));
}

fn row(root: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    root.spawn(Node {
        width: percent(100),
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(6),
        ..default()
    })
    .with_children(build);
}

fn button(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    value: &str,
    action: MenuAction,
    disabled: bool,
) {
    let mut entity = root.spawn((
        MenuButton,
        Button,
        Hovered::default(),
        TabIndex(0),
        action,
        Node {
            min_width: px(96),
            height: px(30),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: px(1).all(),
            padding: UiRect::axes(px(8), px(0)),
            ..default()
        },
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.45)),
        BackgroundColor(Color::srgba(0.08, 0.02, 0.1, 0.8)),
    ));
    if disabled {
        entity.insert(InteractionDisabled);
    }
    entity.with_children(|button| label(button, font, value, 14.0));
}

fn field(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label_text: &str,
    field_tag: Field,
    value: &str,
) {
    row(root, |row| {
        label(row, font, label_text, 14.0);
        row.spawn((
            MenuField(field_tag),
            TextInput,
            EditableText::new(value),
            TextCursorStyle::default(),
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout::no_wrap(),
            Node {
                width: px(230),
                height: px(28),
                border: px(1).all(),
                padding: px(5).all(),
                ..default()
            },
            BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.45)),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            TabIndex(0),
        ));
    });
}

fn cycle(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    name: &str,
    value: &str,
    setting: CycleSetting,
) {
    cycle_action(root, font, name, value, SettingAction::Cycle(setting));
}

fn cycle_action(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    name: &str,
    value: &str,
    action: SettingAction,
) {
    button(
        root,
        font,
        &format!("{name}: {value}"),
        MenuAction::Setting(action),
        false,
    );
}

fn toggle(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    name: &str,
    enabled: bool,
    setting: ToggleSetting,
) {
    button(
        root,
        font,
        &format!("{name}: {}", if enabled { "On" } else { "Off" }),
        MenuAction::Setting(SettingAction::Toggle(setting)),
        false,
    );
}

fn step(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    name: &str,
    value: String,
    setting: StepSetting,
    amount: f32,
) {
    row(root, |row| {
        label(row, font, &format!("{name}: {value}"), 14.0);
        button(
            row,
            font,
            "-",
            MenuAction::Setting(SettingAction::Step(setting, -amount)),
            false,
        );
        button(
            row,
            font,
            "+",
            MenuAction::Setting(SettingAction::Step(setting, amount)),
            false,
        );
    });
}

fn capture_button(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    capture: &ControlsCapture,
    action: InputAction,
    slot: BindingSlot,
    device: CaptureDevice,
    value: String,
) {
    let waiting =
        capture.action == Some(action) && capture.slot == slot && capture.device == device;
    button(
        root,
        font,
        if waiting { "Press input" } else { &value },
        MenuAction::Setting(SettingAction::Capture(action, slot, device)),
        false,
    );
}

fn style_buttons(
    mut buttons: Query<
        (
            &Hovered,
            Has<Pressed>,
            Has<InteractionDisabled>,
            &mut BackgroundColor,
        ),
        With<MenuButton>,
    >,
) {
    for (hovered, pressed, disabled, mut bg) in &mut buttons {
        bg.0 = match (disabled, pressed, hovered.get()) {
            (true, _, _) => Color::srgba(0.05, 0.05, 0.05, 0.55),
            (false, true, _) => Color::srgba(0.4, 0.15, 0.45, 0.95),
            (false, false, true) => Color::srgba(0.18, 0.08, 0.22, 0.9),
            (false, false, false) => Color::srgba(0.08, 0.02, 0.1, 0.8),
        };
    }
}

fn section_label(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Graphics => "Graphics",
        SettingsSection::Audio => "Audio",
        SettingsSection::Input => "Input",
        SettingsSection::Controls => "Controls",
        SettingsSection::Misc => "Misc",
    }
}

fn action_label(action: InputAction) -> &'static str {
    match action {
        InputAction::MoveForward => "Move forward",
        InputAction::MoveBackward => "Move backward",
        InputAction::MoveRight => "Move right",
        InputAction::MoveLeft => "Move left",
        InputAction::Jump => "Jump / ascend",
        InputAction::Crouch => "Crouch / descend",
        InputAction::Ability1 => "Ability 1 / boost",
        InputAction::Melee => "Melee",
        InputAction::Interact => "Interact / use",
        InputAction::Ability2 => "Ability 2",
        InputAction::Reload => "Reload",
        InputAction::Fire => "Fire",
        InputAction::AltFire => "Alt fire / zoom",
        InputAction::DropWeapon => "Drop weapon",
        InputAction::DropAbility => "Drop ability",
        InputAction::RollLeft => "Ship roll left",
        InputAction::RollRight => "Ship roll right",
        InputAction::Pause => "Pause / back",
        InputAction::Chat => "Chat",
        InputAction::CaptureCursor => "Resume cursor lock",
    }
}

fn display_label(value: DisplayMode) -> &'static str {
    match value {
        DisplayMode::Windowed => "Windowed",
        DisplayMode::BorderlessFullscreen => "Borderless fullscreen",
    }
}

fn vsync_label(value: VsyncMode) -> &'static str {
    match value {
        VsyncMode::AutoVsync => "Auto VSync",
        VsyncMode::AutoNoVsync => "Auto No VSync",
        VsyncMode::Fifo => "Fifo",
        VsyncMode::FifoRelaxed => "Fifo Relaxed",
        VsyncMode::Immediate => "Immediate",
        VsyncMode::Mailbox => "Mailbox",
    }
}

fn shadow_label(value: ShadowQuality) -> &'static str {
    match value {
        ShadowQuality::Off => "Off",
        ShadowQuality::Low => "Low",
        ShadowQuality::Medium => "Medium",
        ShadowQuality::High => "High",
    }
}

fn ssao_label(value: SsaoQuality) -> &'static str {
    match value {
        SsaoQuality::Off => "Off",
        SsaoQuality::Medium => "Medium",
        SsaoQuality::High => "High",
        SsaoQuality::Ultra => "Ultra",
    }
}

fn physics_label(value: PhysicsInterp) -> &'static str {
    match value {
        PhysicsInterp::Off => "Off",
        PhysicsInterp::Interpolate => "Interpolate",
        PhysicsInterp::Extrapolate => "Extrapolate",
        PhysicsInterp::Balanced => "Balanced",
    }
}

fn prompt_label(value: PromptDeviceMode) -> &'static str {
    match value {
        PromptDeviceMode::KeyboardMouse => "Keyboard / mouse",
        PromptDeviceMode::Gamepad => "Gamepad",
        PromptDeviceMode::Both => "Both",
    }
}

fn fps_cap_label(fps_cap: u16) -> &'static str {
    match fps_cap {
        0 => "Uncapped",
        30 => "30",
        60 => "60",
        90 => "90",
        120 => "120",
        144 => "144",
        165 => "165",
        240 => "240",
        _ => "Custom",
    }
}

fn credits_lines() -> &'static [&'static str] {
    &[
        "Critical Mass",
        "",
        "Programming",
        "Gage Howe",
        "",
        "Sounds & Music",
        "Griffin Guge",
        "",
        "Art",
        "Various CC0 placeholder assets from kenney.nl, poly.pizza, and sketchfab",
        "",
        "Special Thanks",
        "Friends, testers, and contributors",
        "",
        "Thanks for playing!",
    ]
}
