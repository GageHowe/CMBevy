use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use crate::{GameState, UiState, ServerAddr, HostedServer};
use crate::settings::{show_settings_ui, Settings};

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
            advertise: false,
        }
    }
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
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let center = ctx.content_rect().center();
    let title = match *screen {
        Screen::Root        => "Critical Mass",
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
        .show(ctx, |ui| {
            ui.set_min_width(240.0);
            ui.vertical_centered(|ui| {
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
                            host.gametypes = scan_dir(&format!("{base}/gametypes"), "rhai");
                            host.map_idx = 0;
                            host.gametype_idx = 0;
                            *screen = Screen::Host;
                        }
                        ui.add_space(4.0);
                        if ui.button("Back").clicked() { *screen = Screen::Root; }
                    }
                    Screen::CustomGames => {
                        ui.label("No lobbies available.");
                        // TODO: fetch from beacon and list here
                        ui.add_space(8.0);
                        if ui.button("Back").clicked() { *screen = Screen::Multiplayer; }
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
                                ui.add_enabled(false, egui::Checkbox::new(&mut host.advertise, "(coming soon)"));
                                ui.end_row();
                            });

                        ui.add_space(8.0);

                        let port_ok = host.port.parse::<u16>().is_ok();
                        let can_host = !host.maps.is_empty() && !host.gametypes.is_empty() && port_ok;
                        if ui.add_enabled(can_host, egui::Button::new("Start & Join")).clicked() {
                            let port: u16 = host.port.parse().unwrap_or(42070);
                            let base = asset_base();
                            let map = format!("{base}/maps/{}.ron", host.maps[host.map_idx]);
                            let gametype = format!("{base}/gametypes/{}.rhai", host.gametypes[host.gametype_idx]);
                            match std::process::Command::new(gameserver_exe())
                                .args(["--port", &port.to_string(), "--map", &map, "--gametype", &gametype])
                                .stdin(std::process::Stdio::piped())
                                .spawn()
                            {
                                Ok(mut child) => {
                                    let stdin = child.stdin.take().map(std::io::BufWriter::new);
                                    hosted.child = Some(child);
                                    hosted.stdin = stdin;
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
        .show(ctx, |ui| {
            ui.set_min_width(200.0);
            ui.vertical_centered(|ui| {
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
        .show(ctx, |ui| {
            ui.set_min_width(250.0);
            show_settings_ui(ui, &mut settings);
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                if ui.button("Back").clicked() {
                    next_ui.set(UiState::Paused);
                }
            });
        });
}
