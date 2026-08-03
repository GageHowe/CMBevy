use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use audio::SoundQueue;
use bevy::{
    app::AppExit,
    input_focus::tab_navigation::{TabGroup, TabIndex},
    picking::hover::Hovered,
    prelude::*,
    ui::{InteractionDisabled, Pressed},
    ui_widgets::{Activate, Button},
};
use common::{InputAction, config::CRITICAL_MASS_VERSION};
use gameplay::session::{ServerAddr, SinglePlayerConfig};
use http_common::LobbyInfo;

use crate::{
    GameState, SimState, UiState,
    hosting::{
        available_gametypes, available_maps, fetch_lan_lobbies, fetch_remote_lobbies,
        gametype_path, start_hosted_server,
    },
    settings::{
        DisplayMode, PhysicsInterp, Settings, ShadowQuality, SsaoQuality, VsyncMode, persistence,
    },
    sound::{UI_BACK_EVENT, UI_CLICK_EVENT, queue_ui_sound},
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
    Host,
}

impl Screen {
    fn back(self) -> Self {
        match self {
            Screen::Root => Screen::Root,
            Screen::Credits | Screen::Settings | Screen::SinglePlayer | Screen::Multiplayer => {
                Screen::Root
            }
            Screen::CustomGames | Screen::JoinLan | Screen::Host => Screen::Multiplayer,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuView {
    Main,
    Pause,
    Settings,
}

#[derive(Resource, Default)]
struct MenuState {
    screen: Screen,
    maps: Vec<String>,
    modes: Vec<String>,
    map: usize,
    mode: usize,
    browser: LobbyBrowser,
    view: Option<MenuView>,
    dirty: bool,
}

#[derive(Default)]
struct LobbyBrowser {
    rx: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Result<Vec<LobbyInfo>, String>>>>,
    lobbies: Vec<LobbyInfo>,
    error: String,
    fetching: bool,
    done: bool,
}

#[derive(Component)]
struct MenuRoot;

#[derive(Component)]
struct MenuButton;

#[derive(Component, Clone)]
enum MenuAction {
    Screen(Screen),
    Back,
    Exit,
    StartSinglePlayer,
    Host,
    Refresh,
    Connect(String, Option<String>),
    NextMap,
    NextMode,
    Resume,
    PauseSettings,
    QuitToMenu,
    SettingsBack,
    Setting(SettingAction),
}

#[derive(Component, Clone)]
enum SettingAction {
    Display,
    Vsync,
    Shadow,
    Ssao,
    Physics,
    UiScale(f32),
    ReticleScale(f32),
    DebugPanel,
    DebugRender,
    RevealFile,
    Reset,
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
    sound_queue: ResMut<'w, SoundQueue>,
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
    mut state: ResMut<MenuState>,
    mut sound_queue: ResMut<SoundQueue>,
) {
    if !bindings.just_pressed(
        InputAction::Pause,
        &keys,
        &mouse,
        common::active_gamepad(gamepads.iter()),
    ) {
        return;
    }
    if *game_state.get() == GameState::NotPlaying {
        back_screen(&mut state, &mut sound_queue);
    } else if *ui_state.get() == UiState::Settings {
        next_ui.set(UiState::PauseMenu);
        queue_ui_sound(&mut sound_queue, UI_BACK_EVENT);
    } else if *ui_state.get() != UiState::Playing {
        next_ui.set(UiState::Playing);
        next_sim.set(SimState::Playing);
        queue_ui_sound(&mut sound_queue, UI_BACK_EVENT);
    }
    state.dirty = true;
}

fn poll_browser(mut state: ResMut<MenuState>) {
    let fetch = match state.screen {
        Screen::CustomGames => fetch_remote_lobbies,
        Screen::JoinLan => fetch_lan_lobbies,
        _ => return,
    };
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
            state.browser.done = true;
        }
        Some(Ok(Err(e))) => state.browser.error = e,
        Some(Err(std::sync::mpsc::TryRecvError::Empty)) => return,
        Some(Err(_)) | None => {}
    }
    state.browser.rx = None;
    state.browser.fetching = false;
    state.dirty = true;
}

fn rebuild_menu(
    mut commands: Commands,
    roots: Query<Entity, With<MenuRoot>>,
    assets: Res<AssetServer>,
    game_state: Res<State<GameState>>,
    ui_state: Res<State<UiState>>,
    settings: Res<Settings>,
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
                width: px(520),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                padding: px(14).all(),
                ..default()
            })
            .with_children(|root| match view {
                MenuView::Main => build_main(root, &font, &state, &settings),
                MenuView::Pause => {
                    heading(root, &font, "Paused");
                    button(root, &font, "Resume", MenuAction::Resume, false);
                    button(root, &font, "Settings", MenuAction::PauseSettings, false);
                    button(root, &font, "Quit to Menu", MenuAction::QuitToMenu, false);
                }
                MenuView::Settings => {
                    heading(root, &font, "Settings");
                    build_settings(root, &font, &settings);
                    button(root, &font, "Back", MenuAction::SettingsBack, false);
                }
            });
            root.spawn((
                Text::new(format!("v{CRITICAL_MASS_VERSION}")),
                TextFont {
                    font: font.into(),
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
    state: &MenuState,
    settings: &Settings,
) {
    heading(
        root,
        font,
        match state.screen {
            Screen::Root => "Critical Mass",
            Screen::Credits => "Credits",
            Screen::Settings => "Settings",
            Screen::SinglePlayer => "Singleplayer",
            Screen::Multiplayer => "Multiplayer",
            Screen::CustomGames => "Custom Games",
            Screen::JoinLan => "LAN Games",
            Screen::Host => "Host",
        },
    );
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
            build_settings(root, font, settings);
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::SinglePlayer => {
            map_mode_buttons(root, font, state);
            button(
                root,
                font,
                "Start",
                MenuAction::StartSinglePlayer,
                state.maps.is_empty() || state.modes.is_empty(),
            );
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
                "Custom Games",
                MenuAction::Screen(Screen::CustomGames),
                false,
            );
            button(root, font, "Host", MenuAction::Screen(Screen::Host), false);
            button(root, font, "Back", MenuAction::Back, false);
        }
        Screen::CustomGames | Screen::JoinLan => {
            if state.browser.fetching {
                label(root, font, "Loading...", 15.0);
            } else if !state.browser.error.is_empty() {
                label(root, font, &state.browser.error, 15.0);
            } else if state.browser.lobbies.is_empty() {
                label(root, font, "No lobbies found.", 15.0);
            } else {
                for lobby in &state.browser.lobbies {
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
        Screen::Host => {
            map_mode_buttons(root, font, state);
            button(
                root,
                font,
                "Start & Join",
                MenuAction::Host,
                state.maps.is_empty() || state.modes.is_empty(),
            );
            button(root, font, "Back", MenuAction::Back, false);
        }
    }
}

fn build_settings(root: &mut ChildSpawnerCommands, font: &Handle<Font>, settings: &Settings) {
    button(
        root,
        font,
        &format!("Display: {:?}", settings.display_mode),
        MenuAction::Setting(SettingAction::Display),
        false,
    );
    button(
        root,
        font,
        &format!("VSync: {:?}", settings.vsync),
        MenuAction::Setting(SettingAction::Vsync),
        false,
    );
    button(
        root,
        font,
        &format!("Shadows: {:?}", settings.shadow_quality),
        MenuAction::Setting(SettingAction::Shadow),
        false,
    );
    button(
        root,
        font,
        &format!("SSAO: {:?}", settings.ssao_quality),
        MenuAction::Setting(SettingAction::Ssao),
        false,
    );
    button(
        root,
        font,
        &format!("Physics interp: {:?}", settings.physics_interp),
        MenuAction::Setting(SettingAction::Physics),
        false,
    );
    row(root, |row| {
        label(
            row,
            font,
            &format!("UI size: {:.0}%", settings.ui_scale * 100.0),
            14.0,
        );
        button(
            row,
            font,
            "-",
            MenuAction::Setting(SettingAction::UiScale(-0.05)),
            false,
        );
        button(
            row,
            font,
            "+",
            MenuAction::Setting(SettingAction::UiScale(0.05)),
            false,
        );
    });
    row(root, |row| {
        label(
            row,
            font,
            &format!("Reticle: {:.2}", settings.reticle_scale),
            14.0,
        );
        button(
            row,
            font,
            "-",
            MenuAction::Setting(SettingAction::ReticleScale(-0.05)),
            false,
        );
        button(
            row,
            font,
            "+",
            MenuAction::Setting(SettingAction::ReticleScale(0.05)),
            false,
        );
    });
    button(
        root,
        font,
        &format!(
            "Debug panel: {}",
            if settings.debug_panel { "On" } else { "Off" }
        ),
        MenuAction::Setting(SettingAction::DebugPanel),
        false,
    );
    button(
        root,
        font,
        &format!(
            "Debug rendering: {}",
            if settings.debug_render { "On" } else { "Off" }
        ),
        MenuAction::Setting(SettingAction::DebugRender),
        false,
    );
    row(root, |row| {
        button(
            row,
            font,
            "Settings file",
            MenuAction::Setting(SettingAction::RevealFile),
            false,
        );
        button(
            row,
            font,
            "Reset",
            MenuAction::Setting(SettingAction::Reset),
            false,
        );
    });
}

fn handle_button(
    activate: On<Activate>,
    actions: Query<&MenuAction>,
    mut state: ResMut<MenuState>,
    mut p: ButtonParams,
) {
    let Ok(action) = actions.get(activate.entity).cloned() else {
        return;
    };
    match action {
        MenuAction::Screen(screen) => {
            if matches!(screen, Screen::SinglePlayer | Screen::Host) {
                state.maps = available_maps();
                state.modes = available_gametypes();
                state.map = 0;
                state.mode = 0;
            }
            if matches!(screen, Screen::CustomGames | Screen::JoinLan) {
                state.browser = default();
            }
            state.screen = screen;
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::Back => back_screen(&mut state, &mut p.sound_queue),
        MenuAction::Exit => {
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
            p.exit.write(AppExit::Success);
        }
        MenuAction::StartSinglePlayer => {
            p.sp_config.map = format!("maps/{}.ron", state.maps[state.map]);
            p.sp_config.gametype = gametype_path(&state.modes[state.mode]);
            state.screen = Screen::Root;
            p.next_game.set(GameState::SinglePlayer);
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::Host => host_game(
            &mut p.commands,
            &mut state,
            &mut p.server_addr,
            &mut p.next_game,
        ),
        MenuAction::Refresh => {
            state.browser = default();
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::Connect(addr, lobby_id) => {
            if let Ok(addr) = addr.parse() {
                p.server_addr.addr = addr;
                p.server_addr.lobby_id = lobby_id;
                state.screen = Screen::Root;
                state.browser = default();
                p.next_game.set(GameState::Multiplayer);
            }
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::NextMap => {
            if !state.maps.is_empty() {
                state.map = (state.map + 1) % state.maps.len();
            }
            queue_ui_sound(&mut p.sound_queue, UI_CLICK_EVENT);
        }
        MenuAction::NextMode => {
            if !state.modes.is_empty() {
                state.mode = (state.mode + 1) % state.modes.len();
            }
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
        MenuAction::Setting(action) => apply_setting(action, &mut p.settings),
    }
    state.dirty = true;
}

fn apply_setting(action: SettingAction, settings: &mut Settings) {
    match action {
        SettingAction::Display => {
            settings.display_mode = match settings.display_mode {
                DisplayMode::Windowed => DisplayMode::BorderlessFullscreen,
                DisplayMode::BorderlessFullscreen => DisplayMode::Windowed,
            };
        }
        SettingAction::Vsync => {
            settings.vsync = match settings.vsync {
                VsyncMode::AutoVsync => VsyncMode::AutoNoVsync,
                VsyncMode::AutoNoVsync => VsyncMode::Fifo,
                VsyncMode::Fifo => VsyncMode::FifoRelaxed,
                VsyncMode::FifoRelaxed => VsyncMode::Immediate,
                VsyncMode::Immediate => VsyncMode::Mailbox,
                VsyncMode::Mailbox => VsyncMode::AutoVsync,
            };
        }
        SettingAction::Shadow => {
            settings.shadow_quality = match settings.shadow_quality {
                ShadowQuality::Off => ShadowQuality::Low,
                ShadowQuality::Low => ShadowQuality::Medium,
                ShadowQuality::Medium => ShadowQuality::High,
                ShadowQuality::High => ShadowQuality::Off,
            };
        }
        SettingAction::Ssao => {
            settings.ssao_quality = match settings.ssao_quality {
                SsaoQuality::Off => SsaoQuality::Medium,
                SsaoQuality::Medium => SsaoQuality::High,
                SsaoQuality::High => SsaoQuality::Ultra,
                SsaoQuality::Ultra => SsaoQuality::Off,
            };
        }
        SettingAction::Physics => {
            settings.physics_interp = match settings.physics_interp {
                PhysicsInterp::Off => PhysicsInterp::Interpolate,
                PhysicsInterp::Interpolate => PhysicsInterp::Extrapolate,
                PhysicsInterp::Extrapolate => PhysicsInterp::Balanced,
                PhysicsInterp::Balanced => PhysicsInterp::Off,
            };
        }
        SettingAction::UiScale(delta) => {
            settings.ui_scale = (settings.ui_scale + delta).clamp(0.5, 2.5)
        }
        SettingAction::ReticleScale(delta) => {
            settings.reticle_scale = (settings.reticle_scale + delta).clamp(0.5, 2.0);
        }
        SettingAction::DebugPanel => settings.debug_panel = !settings.debug_panel,
        SettingAction::DebugRender => settings.debug_render = !settings.debug_render,
        SettingAction::RevealFile => {
            if let Err(err) = persistence::reveal_settings_file() {
                warn!("failed to open settings file location: {err}");
            }
        }
        SettingAction::Reset => persistence::reset_settings(settings),
    }
}

fn host_game(
    commands: &mut Commands,
    state: &mut MenuState,
    server_addr: &mut ServerAddr,
    next_state: &mut NextState<GameState>,
) {
    let port = default_server_port();
    let map = format!("maps/{}.ron", state.maps[state.map]);
    let mode = gametype_path(&state.modes[state.mode]);
    match start_hosted_server(port, &map, &mode, None) {
        Ok(()) => {
            server_addr.addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            server_addr.lobby_id = None;
            state.screen = Screen::Root;
            next_state.set(GameState::Multiplayer);
        }
        Err(e) => gameplay::messages::push(commands, format!("Failed to start gameserver: {e}")),
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

fn default_server_port() -> u16 {
    common::config::SERVER_BIND_ADDRESS
        .parse::<SocketAddr>()
        .map(|addr| addr.port())
        .unwrap_or(42070)
}

fn map_mode_buttons(root: &mut ChildSpawnerCommands, font: &Handle<Font>, state: &MenuState) {
    let map = state.maps.get(state.map).map_or("-", String::as_str);
    let mode = state.modes.get(state.mode).map_or("-", String::as_str);
    button(
        root,
        font,
        &format!("Map: {map}"),
        MenuAction::NextMap,
        state.maps.is_empty(),
    );
    button(
        root,
        font,
        &format!("Mode: {mode}"),
        MenuAction::NextMode,
        state.modes.is_empty(),
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
            max_width: px(500),
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
