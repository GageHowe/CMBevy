use crate::session::{
    HostedServer, ServerAddr, SinglePlayerConfig, available_gametypes, available_maps,
    fetch_lan_lobbies, fetch_remote_lobbies, gametype_path, shutdown_session, start_hosted_server,
};
use crate::settings::{Settings, SettingsSection, show_settings_ui};
use crate::{GameState, UiState};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use http_common::{LobbyInfo, RegisterRequest};

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            EguiPrimaryContextPass,
            main_menu.run_if(in_state(GameState::MainMenu)),
        );
        app.add_systems(
            EguiPrimaryContextPass,
            pause_menu.run_if(in_state(UiState::Paused)),
        );
        app.add_systems(
            EguiPrimaryContextPass,
            settings_menu.run_if(in_state(UiState::Settings)),
        );
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Screen {
    #[default]
    Root,
    Credits,
    Settings,
    SinglePlayer,
    Multiplayer,
    CustomGames,
    JoinLan,
    Matchmaking,
    Host,
}

struct HostState {
    maps: Vec<String>,
    gametypes: Vec<String>,
    map_idx: usize,
    gametype_idx: usize,
    port: String,
    name: String,
    max_players: String,
    advertise: bool,
}

impl Default for HostState {
    fn default() -> Self {
        Self {
            maps: Vec::new(),
            gametypes: Vec::new(),
            map_idx: 0,
            gametype_idx: 0,
            port: "42070".to_string(),
            name: "My Lobby".to_string(),
            max_players: "8".to_string(),
            advertise: false,
        }
    }
}

#[derive(Default)]
struct CreditsState {
    offset: f32,
}

#[derive(Default)]
struct LobbyBrowser {
    rx: Option<std::sync::mpsc::Receiver<Result<Vec<LobbyInfo>, String>>>,
    lobbies: Vec<LobbyInfo>,
    error: String,
    fetching: bool,
    /// Set once the first result arrives; prevents the trigger from re-scanning every frame.
    done: bool,
}

fn show_fullscreen_menu(
    ctx: &egui::Context,
    id: &'static str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let rect = ctx.content_rect();
    egui::Area::new(id.into())
        .order(egui::Order::Foreground)
        .fixed_pos(rect.left_top())
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_premultiplied(5, 0, 8, 230))
                .show(ui, |ui| {
                    ui.set_min_size(rect.size());
                    ui.vertical_centered(|ui| {
                        ui.add_space((rect.height() * 0.16).max(40.0));
                        ui.set_max_width(520.0);
                        add_contents(ui);
                    });
                });
        });
}

fn main_menu(
    mut contexts: EguiContexts,
    mut exit: MessageWriter<AppExit>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut next_state: ResMut<NextState<GameState>>,
    mut server_addr: ResMut<ServerAddr>,
    mut hosted: ResMut<HostedServer>,
    mut sp_config: ResMut<SinglePlayerConfig>,
    mut settings: ResMut<Settings>,
    mut settings_section: Local<SettingsSection>,
    mut screen: Local<Screen>,
    mut host: Local<HostState>,
    mut credits: Local<CreditsState>,
    mut browser: Local<LobbyBrowser>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    // Keep credits as a menu-local screen so they stay decoupled from gameplay UI/state.
    if *screen == Screen::Credits && keys.just_pressed(KeyCode::Escape) {
        *screen = Screen::Root;
        credits.offset = 0.0;
    }
    let title = match *screen {
        Screen::Root => "Critical Mass",
        Screen::Credits => "Credits",
        Screen::Settings => "Settings",
        Screen::SinglePlayer => "Singleplayer",
        Screen::Multiplayer => "Multiplayer",
        Screen::CustomGames => "Custom Games",
        Screen::JoinLan => "LAN Games",
        Screen::Matchmaking => "Matchmaking",
        Screen::Host => "Host",
    };
    show_fullscreen_menu(ctx, "main_menu", |ui| {
        ui.set_min_width(280.0);
        ui.heading(title);
        ui.add_space(8.0);
        match *screen {
            Screen::Root => show_root_screen(ui, &mut host, &mut screen, &mut exit),
            Screen::Credits => show_credits_screen(ui, &time, &mut credits, &mut screen),
            Screen::Settings => {
                show_settings_screen(ui, &mut settings, &mut settings_section, &mut screen)
            }
            Screen::SinglePlayer => show_singleplayer_screen(
                ui,
                &mut host,
                &mut hosted,
                &mut sp_config,
                &mut next_state,
                &mut screen,
            ),
            Screen::Multiplayer => {
                show_multiplayer_screen(ui, &mut host, &mut browser, &mut screen)
            }
            Screen::CustomGames => show_browser_screen(
                ui,
                &mut browser,
                &mut hosted,
                &mut server_addr,
                &mut next_state,
                &mut screen,
                "Looking for lobbies…",
                "No lobbies found.",
                "Refresh",
                Screen::Multiplayer,
                fetch_remote_lobbies,
                |ui, lobby| {
                    ui.label(format!(
                        "{} · {} ({}/{})",
                        lobby.name, lobby.host, lobby.player_count, lobby.max_players
                    ));
                },
            ),
            Screen::JoinLan => show_browser_screen(
                ui,
                &mut browser,
                &mut hosted,
                &mut server_addr,
                &mut next_state,
                &mut screen,
                "Scanning LAN…",
                "No servers found on LAN.",
                "Scan again",
                Screen::Multiplayer,
                fetch_lan_lobbies,
                |ui, lobby| {
                    ui.label(format!("{} · {}", lobby.name, lobby.host));
                },
            ),
            Screen::Matchmaking => show_matchmaking_screen(ui, &mut screen),
            Screen::Host => show_host_screen(
                ui,
                &mut host,
                &mut hosted,
                &mut server_addr,
                &mut next_state,
                &mut screen,
            ),
        }
    });
}

fn reset_host_catalog(host: &mut HostState) {
    host.maps = available_maps();
    host.gametypes = available_gametypes();
    host.map_idx = 0;
    host.gametype_idx = 0;
}

fn show_root_screen(
    ui: &mut egui::Ui,
    host: &mut HostState,
    screen: &mut Screen,
    exit: &mut MessageWriter<AppExit>,
) {
    if ui.button("Singleplayer").clicked() {
        reset_host_catalog(host);
        *screen = Screen::SinglePlayer;
    }
    ui.add_space(4.0);
    if ui.button("Multiplayer").clicked() {
        *screen = Screen::Multiplayer;
    }
    ui.add_space(4.0);
    if ui.button("Settings").clicked() {
        *screen = Screen::Settings;
    }
    ui.add_space(4.0);
    if ui.button("Credits").clicked() {
        *screen = Screen::Credits;
    }
    ui.add_space(4.0);
    if ui.button("Exit").clicked() {
        exit.write(AppExit::Success);
    }
}

fn show_credits_screen(
    ui: &mut egui::Ui,
    time: &Time,
    credits: &mut CreditsState,
    screen: &mut Screen,
) {
    let lines = credits_lines();
    let line_height = 28.0;
    let viewport_height = 320.0;
    let total_height = lines.len() as f32 * line_height + 120.0;
    let max_offset = (total_height - viewport_height).max(0.0);

    credits.offset += 22.0 * time.delta_secs();
    if credits.offset > max_offset + viewport_height {
        credits.offset = 0.0;
    }

    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .id_salt("credits_scroll")
        .auto_shrink([false, false])
        .max_height(viewport_height)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .scroll_offset(egui::vec2(0.0, credits.offset.min(max_offset)))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                for line in lines {
                    match *line {
                        "" => ui.add_space(14.0),
                        text if text.starts_with('#') => {
                            ui.heading(text.trim_start_matches('#').trim());
                            ui.add_space(6.0);
                        }
                        text => {
                            ui.label(text);
                            ui.add_space(2.0);
                        }
                    }
                }
                ui.add_space(24.0);
            });
        });
    ui.add_space(8.0);
    if ui.button("Back").clicked() {
        credits.offset = 0.0;
        *screen = Screen::Root;
    }
}

fn show_settings_screen(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    settings_section: &mut SettingsSection,
    screen: &mut Screen,
) {
    show_settings_ui(ui, settings, settings_section);
    ui.add_space(8.0);
    if ui.button("Back").clicked() {
        *screen = Screen::Root;
    }
}

fn show_singleplayer_screen(
    ui: &mut egui::Ui,
    host: &mut HostState,
    hosted: &mut HostedServer,
    sp_config: &mut SinglePlayerConfig,
    next_state: &mut NextState<GameState>,
    screen: &mut Screen,
) {
    show_map_gametype_grid(ui, "sp", host);
    ui.add_space(8.0);

    let can_start = !host.maps.is_empty() && !host.gametypes.is_empty();
    if ui
        .add_enabled(can_start, egui::Button::new("Start"))
        .clicked()
    {
        shutdown_session(None, None, hosted);
        sp_config.map = format!("maps/{}.ron", host.maps[host.map_idx]);
        sp_config.gametype = gametype_path(&host.gametypes[host.gametype_idx]);
        *screen = Screen::Root;
        next_state.set(GameState::SinglePlayer);
    }
    ui.add_space(4.0);
    if ui.button("Back").clicked() {
        *screen = Screen::Root;
    }
}

fn show_multiplayer_screen(
    ui: &mut egui::Ui,
    host: &mut HostState,
    browser: &mut LobbyBrowser,
    screen: &mut Screen,
) {
    if ui.button("Join LAN").clicked() {
        *browser = LobbyBrowser::default();
        *screen = Screen::JoinLan;
    }
    ui.add_space(4.0);
    if ui.button("Custom Games").clicked() {
        *browser = LobbyBrowser::default();
        *screen = Screen::CustomGames;
    }
    ui.add_space(4.0);
    if ui.button("Matchmaking").clicked() {
        *screen = Screen::Matchmaking;
    }
    ui.add_space(4.0);
    if ui.button("Host").clicked() {
        reset_host_catalog(host);
        *screen = Screen::Host;
    }
    ui.add_space(4.0);
    if ui.button("Back").clicked() {
        *screen = Screen::Root;
    }
}

fn begin_browser_fetch(browser: &mut LobbyBrowser, fetch: fn() -> Result<Vec<LobbyInfo>, String>) {
    if browser.fetching || browser.rx.is_some() || browser.done {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    browser.rx = Some(rx);
    browser.fetching = true;
    browser.error.clear();
    std::thread::spawn(move || {
        let _ = tx.send(fetch());
    });
}

fn poll_browser_fetch(browser: &mut LobbyBrowser) {
    let Some(rx) = &browser.rx else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(lobbies)) => {
            browser.lobbies = lobbies;
            browser.rx = None;
            browser.fetching = false;
            browser.done = true;
        }
        Ok(Err(e)) => {
            browser.error = e;
            browser.lobbies.clear();
            browser.rx = None;
            browser.fetching = false;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(_) => {
            browser.rx = None;
            browser.fetching = false;
        }
    }
}

fn connect_to_lobby(
    addr: &str,
    hosted: &mut HostedServer,
    server_addr: &mut ServerAddr,
    next_state: &mut NextState<GameState>,
    browser: &mut LobbyBrowser,
    screen: &mut Screen,
) {
    if let Ok(sa) = addr.parse() {
        shutdown_session(None, None, hosted);
        server_addr.0 = sa;
        *screen = Screen::Root;
        next_state.set(GameState::Multiplayer);
        *browser = LobbyBrowser::default();
    }
}

fn show_browser_screen(
    ui: &mut egui::Ui,
    browser: &mut LobbyBrowser,
    hosted: &mut HostedServer,
    server_addr: &mut ServerAddr,
    next_state: &mut NextState<GameState>,
    screen: &mut Screen,
    loading_label: &str,
    empty_label: &str,
    refresh_label: &str,
    back_screen: Screen,
    fetch: fn() -> Result<Vec<LobbyInfo>, String>,
    draw_lobby: impl Fn(&mut egui::Ui, &LobbyInfo),
) {
    begin_browser_fetch(browser, fetch);
    poll_browser_fetch(browser);

    if browser.fetching {
        ui.spinner();
        ui.label(loading_label);
    } else if !browser.error.is_empty() {
        ui.colored_label(egui::Color32::RED, &browser.error);
    } else if browser.lobbies.is_empty() {
        ui.label(empty_label);
    } else {
        let mut connect_to: Option<String> = None;
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .show(ui, |ui| {
                for lobby in &browser.lobbies {
                    ui.horizontal(|ui| {
                        draw_lobby(ui, lobby);
                        if ui.button("Connect").clicked() {
                            connect_to = Some(lobby.host.clone());
                        }
                    });
                }
            });
        if let Some(addr) = connect_to {
            connect_to_lobby(&addr, hosted, server_addr, next_state, browser, screen);
        }
    }

    ui.add_space(4.0);
    if ui.button(refresh_label).clicked() {
        *browser = LobbyBrowser::default();
    }
    ui.add_space(4.0);
    if ui.button("Back").clicked() {
        *browser = LobbyBrowser::default();
        *screen = back_screen;
    }
}

fn show_matchmaking_screen(ui: &mut egui::Ui, screen: &mut Screen) {
    ui.label("Matchmaking coming soon.");
    ui.add_space(8.0);
    if ui.button("Back").clicked() {
        *screen = Screen::Multiplayer;
    }
}

fn show_map_gametype_grid(ui: &mut egui::Ui, id: &'static str, host: &mut HostState) {
    egui::Grid::new(format!("{id}_grid"))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Map");
            let map_label = host
                .maps
                .get(host.map_idx)
                .cloned()
                .unwrap_or_else(|| "—".into());
            egui::ComboBox::from_id_salt(format!("{id}_map_combo"))
                .selected_text(&map_label)
                .show_ui(ui, |ui| {
                    for i in 0..host.maps.len() {
                        let label = host.maps[i].clone();
                        ui.selectable_value(&mut host.map_idx, i, label);
                    }
                });
            ui.end_row();

            ui.label("Mode");
            let mode_label = host
                .gametypes
                .get(host.gametype_idx)
                .cloned()
                .unwrap_or_else(|| "—".into());
            egui::ComboBox::from_id_salt(format!("{id}_mode_combo"))
                .selected_text(&mode_label)
                .show_ui(ui, |ui| {
                    for i in 0..host.gametypes.len() {
                        let label = host.gametypes[i].clone();
                        ui.selectable_value(&mut host.gametype_idx, i, label);
                    }
                });
            ui.end_row();
        });
}

fn show_host_screen(
    ui: &mut egui::Ui,
    host: &mut HostState,
    hosted: &mut HostedServer,
    server_addr: &mut ServerAddr,
    next_state: &mut NextState<GameState>,
    screen: &mut Screen,
) {
    show_map_gametype_grid(ui, "host", host);
    egui::Grid::new("host_settings_grid")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Port");
            ui.text_edit_singleline(&mut host.port);
            ui.end_row();

            ui.label("Advertise");
            ui.checkbox(&mut host.advertise, "");
            ui.end_row();

            if host.advertise {
                ui.label("Lobby name");
                ui.text_edit_singleline(&mut host.name);
                ui.end_row();

                ui.label("Max players");
                ui.text_edit_singleline(&mut host.max_players);
                ui.end_row();
            }
        });

    ui.add_space(8.0);

    let port_ok = host.port.parse::<u16>().is_ok();
    let max_players_ok = !host.advertise || host.max_players.parse::<u8>().is_ok();
    let can_host = !host.maps.is_empty() && !host.gametypes.is_empty() && port_ok && max_players_ok;
    if ui
        .add_enabled(can_host, egui::Button::new("Start & Join"))
        .clicked()
    {
        shutdown_session(None, None, hosted);
        let port: u16 = host.port.parse().unwrap_or(42070);
        let map = format!("maps/{}.ron", host.maps[host.map_idx]);
        let gametype = gametype_path(&host.gametypes[host.gametype_idx]);
        let advertise = host.advertise.then(|| RegisterRequest {
            quic_port: port,
            name: host.name.clone(),
            max_players: host.max_players.parse().unwrap_or(8),
        });
        match start_hosted_server(hosted, port, &map, &gametype, advertise) {
            Ok(()) => {
                server_addr.0 = format!("127.0.0.1:{port}").parse().unwrap();
                *screen = Screen::Root;
                next_state.set(GameState::Multiplayer);
            }
            Err(e) => eprintln!("Failed to start gameserver: {e}"),
        }
    }
    ui.add_space(4.0);
    if ui.button("Back").clicked() {
        *screen = Screen::Multiplayer;
    }
}

fn credits_lines() -> &'static [&'static str] {
    &[
        "",
        "# Critical Mass",
        "",
        "# Programming",
        "Gage Howe",
        "",
        "# Audio",
        "Griffin Guge",
        "",
        "# Art",
        "your mother",
        "Various placeholder assets from kenney.nl, poly.pizza",
        "",
        "# Special Thanks",
        "Friends, testers, and contributors",
        "",
        "Thanks for playing!",
    ]
}

fn pause_menu(
    mut contexts: EguiContexts,
    mut next_game: ResMut<NextState<GameState>>,
    mut next_ui: ResMut<NextState<UiState>>,
    mut hosted: ResMut<HostedServer>,
    mut console_input: Local<String>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    show_fullscreen_menu(ctx, "pause_menu", |ui| {
        ui.set_min_width(200.0);
        ui.heading("Paused");
        ui.add_space(8.0);
        if ui.button("Resume").clicked() {
            next_ui.set(UiState::Playing);
        }
        ui.add_space(4.0);
        if ui.button("Settings").clicked() {
            next_ui.set(UiState::Settings);
        }
        ui.add_space(4.0);
        if ui
            .button("Quit to Menu (this will kick all players)")
            .clicked()
        {
            next_game.set(GameState::MainMenu);
            next_ui.set(UiState::Playing);
        }
        if hosted.child.is_some() {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label("Server console");
            let response = ui.text_edit_singleline(&mut *console_input);
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                hosted.send_command(&console_input.clone());
                console_input.clear();
            }
        }
    });
}

fn settings_menu(
    mut contexts: EguiContexts,
    mut next_ui: ResMut<NextState<UiState>>,
    mut settings: ResMut<Settings>,
    mut settings_section: Local<SettingsSection>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    show_fullscreen_menu(ctx, "settings_menu", |ui| {
        ui.set_min_width(250.0);
        ui.heading("Settings");
        ui.add_space(8.0);
        show_settings_ui(ui, &mut settings, &mut settings_section);
        ui.add_space(8.0);
        if ui.button("Back").clicked() {
            next_ui.set(UiState::Paused);
        }
    });
}
