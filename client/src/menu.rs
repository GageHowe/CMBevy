use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use crate::{GameState, UiState, ServerAddr, HostedServer};
use crate::settings::{show_settings_ui, Settings};
use common::config::BEACON_URL;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, main_menu.run_if(in_state(GameState::MainMenu)));
        app.add_systems(EguiPrimaryContextPass, pause_menu.run_if(in_state(UiState::Paused)));
        app.add_systems(EguiPrimaryContextPass, settings_menu.run_if(in_state(UiState::Settings)));
    }
}

#[derive(Default, PartialEq, Clone, Copy)]
enum Screen {
    #[default]
    Root,
    SinglePlayer,
    Multiplayer,
    CustomGames,
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

// Matches beacon's LobbyInfo for the browser
#[derive(serde::Deserialize, Clone)]
struct Lobby {
    // id: String,
    name: String,
    host: String,
    player_count: u8,
    max_players: u8,
}

// Matches beacon's RegisterRequest / RegisterResponse
#[derive(serde::Serialize)]
struct RegisterRequest {
    quic_port: u16,
    name: String,
    max_players: u8,
}

#[derive(serde::Deserialize)]
struct RegisterResponse {
    id: String,
}

#[derive(Default)]
struct LobbyBrowser {
    rx: Option<std::sync::mpsc::Receiver<Result<Vec<Lobby>, String>>>,
    lobbies: Vec<Lobby>,
    error: String,
    fetching: bool,
}

fn beacon_register(req: RegisterRequest, id_slot: std::sync::Arc<std::sync::Mutex<Option<String>>>) {
    std::thread::spawn(move || {
        if let Ok(resp) = ureq::post(&format!("{BEACON_URL}/lobbies/register")).send_json(&req) {
            if let Ok(r) = resp.into_json::<RegisterResponse>() {
                *id_slot.lock().unwrap() = Some(r.id);
            }
        }
    });
}

fn fetch_lobbies() -> Result<Vec<Lobby>, String> {
    ureq::get(&format!("{BEACON_URL}/lobbies"))
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())
}

fn asset_base() -> &'static str {
    if std::path::Path::new("assets").exists() { "assets" } else { "../assets" }
}

fn scan_dir(dir: &str, ext: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |x| x == ext))
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

fn gameserver_exe() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    let name = if cfg!(windows) { "gameserver.exe" } else { "gameserver" };
    dir.join(name)
}

fn main_menu(
    mut contexts: EguiContexts,
    mut next_state: ResMut<NextState<GameState>>,
    mut server_addr: ResMut<ServerAddr>,
    mut hosted: ResMut<HostedServer>,
    mut screen: Local<Screen>,
    mut host: Local<HostState>,
    mut browser: Local<LobbyBrowser>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let center = ctx.content_rect().center();
    let title = match *screen {
        Screen::Root         => "Critical Mass",
        Screen::SinglePlayer => "Singleplayer",
        Screen::Multiplayer  => "Multiplayer",
        Screen::CustomGames  => "Custom Games",
        Screen::Matchmaking  => "Matchmaking",
        Screen::Host         => "Host",
    };
    egui::Window::new(title)
        .default_pos(center)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ui.set_min_width(280.0);
            ui.vertical_centered(|ui| {
                ui.heading(title);
                ui.add_space(8.0);
                match *screen {
                    Screen::Root => {
                        if ui.button("Singleplayer").clicked() { *screen = Screen::SinglePlayer; }
                        ui.add_space(4.0);
                        if ui.button("Multiplayer").clicked() { *screen = Screen::Multiplayer; }
                        ui.add_space(4.0);
                        if ui.button("Exit").clicked() { std::process::exit(0); }
                    }
                    Screen::SinglePlayer => {
                        if ui.button("Quick Start").clicked() {
                            *screen = Screen::Root;
                            next_state.set(GameState::SinglePlayer);
                        }
                        ui.add_space(4.0);
                        if ui.button("Back").clicked() { *screen = Screen::Root; }
                    }
                    Screen::Multiplayer => {
                        if ui.button("Custom Games").clicked() { *screen = Screen::CustomGames; }
                        ui.add_space(4.0);
                        if ui.button("Matchmaking").clicked() { *screen = Screen::Matchmaking; }
                        ui.add_space(4.0);
                        if ui.button("Host").clicked() {
                            let base = asset_base();
                            host.maps = scan_dir(&format!("{base}/maps"), "ron");
                            host.gametypes = scan_dir(&format!("{base}/gametypes"), "lua");
                            host.map_idx = 0;
                            host.gametype_idx = 0;
                            *screen = Screen::Host;
                        }
                        ui.add_space(4.0);
                        if ui.button("Back").clicked() { *screen = Screen::Root; }
                    }
                    Screen::CustomGames => {
                        // Start fetch when entering with no pending request
                        if !browser.fetching && browser.rx.is_none() {
                            let (tx, rx) = std::sync::mpsc::channel();
                            browser.rx = Some(rx);
                            browser.fetching = true;
                            browser.error.clear();
                            std::thread::spawn(move || { let _ = tx.send(fetch_lobbies()); });
                        }

                        // Drain result
                        if let Some(rx) = &browser.rx {
                            match rx.try_recv() {
                                Ok(Ok(lobbies)) => {
                                    browser.lobbies = lobbies;
                                    browser.rx = None;
                                    browser.fetching = false;
                                }
                                Ok(Err(e)) => {
                                    browser.error = e;
                                    browser.lobbies.clear();
                                    browser.rx = None;
                                    browser.fetching = false;
                                }
                                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                                Err(_) => { browser.rx = None; browser.fetching = false; }
                            }
                        }

                        if browser.fetching {
                            ui.spinner();
                            ui.label("Looking for lobbies…");
                        } else if !browser.error.is_empty() {
                            ui.colored_label(egui::Color32::RED, &browser.error);
                        } else if browser.lobbies.is_empty() {
                            ui.label("No lobbies found.");
                        } else {
                            let mut connect_to: Option<String> = None;
                            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                for lobby in &browser.lobbies {
                                    ui.horizontal(|ui| {
                                        ui.label(format!("{} · {} ({}/{})", lobby.name, lobby.host, lobby.player_count, lobby.max_players));
                                        if ui.button("Connect").clicked() {
                                            connect_to = Some(lobby.host.clone());
                                        }
                                    });
                                }
                            });
                            if let Some(addr) = connect_to {
                                if let Ok(sa) = addr.parse() {
                                    server_addr.0 = sa;
                                    *screen = Screen::Root;
                                    next_state.set(GameState::Multiplayer);
                                    *browser = LobbyBrowser::default();
                                }
                            }
                        }

                        ui.add_space(4.0);
                        if ui.button("Refresh").clicked() { *browser = LobbyBrowser::default(); }
                        ui.add_space(4.0);
                        if ui.button("Back").clicked() {
                            *browser = LobbyBrowser::default();
                            *screen = Screen::Multiplayer;
                        }
                    }
                    Screen::Matchmaking => {
                        ui.label("Matchmaking coming soon.");
                        ui.add_space(8.0);
                        if ui.button("Back").clicked() { *screen = Screen::Multiplayer; }
                    }
                    Screen::Host => {
                        egui::Grid::new("host_grid")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Map");
                                let map_label = host.maps.get(host.map_idx).cloned().unwrap_or_else(|| "—".into());
                                egui::ComboBox::from_id_salt("map_combo")
                                    .selected_text(&map_label)
                                    .show_ui(ui, |ui| {
                                        for i in 0..host.maps.len() {
                                            let label = host.maps[i].clone();
                                            ui.selectable_value(&mut host.map_idx, i, label);
                                        }
                                    });
                                ui.end_row();

                                ui.label("Mode");
                                let mode_label = host.gametypes.get(host.gametype_idx).cloned().unwrap_or_else(|| "—".into());
                                egui::ComboBox::from_id_salt("mode_combo")
                                    .selected_text(&mode_label)
                                    .show_ui(ui, |ui| {
                                        for i in 0..host.gametypes.len() {
                                            let label = host.gametypes[i].clone();
                                            ui.selectable_value(&mut host.gametype_idx, i, label);
                                        }
                                    });
                                ui.end_row();

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
                        if ui.add_enabled(can_host, egui::Button::new("Start & Join")).clicked() {
                            let port: u16 = host.port.parse().unwrap_or(42070);
                            let base = asset_base();
                            let map = format!("{base}/maps/{}.ron", host.maps[host.map_idx]);
                            let gametype = format!("{base}/gametypes/{}.lua", host.gametypes[host.gametype_idx]);
                            match std::process::Command::new(gameserver_exe())
                                .args(["--port", &port.to_string(), "--map", &map, "--gametype", &gametype])
                                .stdin(std::process::Stdio::piped())
                                .spawn()
                            {
                                Ok(mut child) => {
                                    let stdin = child.stdin.take().map(std::io::BufWriter::new);
                                    hosted.child = Some(child);
                                    hosted.stdin = stdin;
                                    if host.advertise {
                                        let req = RegisterRequest {
                                            quic_port: port,
                                            name: host.name.clone(),
                                            max_players: host.max_players.parse().unwrap_or(8),
                                        };
                                        beacon_register(req, std::sync::Arc::clone(&hosted.beacon_id));
                                    }
                                    server_addr.0 = format!("127.0.0.1:{port}").parse().unwrap();
                                    *screen = Screen::Root;
                                    next_state.set(GameState::Multiplayer);
                                }
                                Err(e) => eprintln!("Failed to start gameserver: {e}"),
                            }
                        }
                        ui.add_space(4.0);
                        if ui.button("Back").clicked() { *screen = Screen::Multiplayer; }
                    }
                }
            });
        });
}

fn pause_menu(
    mut contexts: EguiContexts,
    mut next_game: ResMut<NextState<GameState>>,
    mut next_ui: ResMut<NextState<UiState>>,
    mut hosted: ResMut<HostedServer>,
    mut console_input: Local<String>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let center = ctx.content_rect().center();
    egui::Window::new("Paused")
        .default_pos(center)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ui.set_min_width(200.0);
            ui.vertical_centered(|ui| {
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
                if ui.button("Quit to Menu (this will kick all players)").clicked() {
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
        });
}

fn settings_menu(
    mut contexts: EguiContexts,
    mut next_ui: ResMut<NextState<UiState>>,
    mut settings: ResMut<Settings>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let center = ctx.content_rect().center();
    egui::Window::new("Settings")
        .default_pos(center)
        .pivot(egui::Align2::CENTER_CENTER)
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .title_bar(false)
        .show(ctx, |ui| {
            ui.set_min_width(250.0);
            ui.heading("Settings");
            ui.add_space(8.0);
            show_settings_ui(ui, &mut settings);
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if ui.button("Back").clicked() {
                    next_ui.set(UiState::Paused);
                }
            });
        });
}
