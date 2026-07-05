use bevy_egui::egui;

use crate::settings::{
    DisplayMode, PhysicsInterp, Settings, ShadowQuality, SsaoQuality, VsyncMode,
};

pub fn show(ui: &mut egui::Ui, settings: &mut Settings) {
    egui::CollapsingHeader::new("Display")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Display mode")
                    .on_hover_text("Windowed is normal desktop mode. Borderless fullscreen is fullscreen without exclusive mode.");
                egui::ComboBox::from_id_salt("display_mode_combo")
                    .selected_text(match settings.display_mode {
                        DisplayMode::Windowed => "Windowed",
                        DisplayMode::BorderlessFullscreen => "Borderless fullscreen",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut settings.display_mode, DisplayMode::Windowed, "Windowed");
                        ui.selectable_value(&mut settings.display_mode, DisplayMode::BorderlessFullscreen, "Borderless fullscreen");
                    });
            });

            ui.horizontal(|ui| {
                ui.label("VSync").on_hover_text("Controls how frames are presented to the display.");
                egui::ComboBox::from_id_salt("vsync_combo")
                    .selected_text(match settings.vsync {
                        VsyncMode::AutoVsync => "Auto (VSync)",
                        VsyncMode::AutoNoVsync => "Auto (No VSync)",
                        VsyncMode::Fifo => "Fifo",
                        VsyncMode::FifoRelaxed => "Fifo Relaxed",
                        VsyncMode::Immediate => "Immediate",
                        VsyncMode::Mailbox => "Mailbox",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut settings.vsync, VsyncMode::AutoVsync, "Auto (VSync)");
                        ui.selectable_value(&mut settings.vsync, VsyncMode::AutoNoVsync, "Auto (No VSync)");
                        ui.selectable_value(&mut settings.vsync, VsyncMode::Fifo, "Fifo");
                        ui.selectable_value(&mut settings.vsync, VsyncMode::FifoRelaxed, "Fifo Relaxed");
                        ui.selectable_value(&mut settings.vsync, VsyncMode::Immediate, "Immediate");
                        ui.selectable_value(&mut settings.vsync, VsyncMode::Mailbox, "Mailbox");
                    });
            });

            ui.horizontal(|ui| {
                ui.label("UI size").on_hover_text("Scales the entire interface globally.");
                show_ui_scale_input(ui, settings);
            });

            ui.horizontal(|ui| {
                ui.label("Reticle size").on_hover_text("Scales the center crosshair and lead reticle.");
                ui.add(
                    egui::Slider::new(&mut settings.reticle_scale, 0.5..=2.0).fixed_decimals(2),
                );
            });

            ui.horizontal(|ui| {
                ui.label("FPS cap").on_hover_text("Limits the client update rate. Uncapped leaves frame pacing to VSync and hardware.");
                egui::ComboBox::from_id_salt("fps_cap_combo")
                    .selected_text(fps_cap_label(settings.fps_cap))
                    .show_ui(ui, |ui| {
                        for fps_cap in [0, 30, 60, 90, 120, 144, 165, 240] {
                            ui.selectable_value(&mut settings.fps_cap, fps_cap, fps_cap_label(fps_cap));
                        }
                    });
            });
            ui.horizontal(|ui| {
                ui.label("Shadow quality").on_hover_text("Controls directional shadow map quality. Off disables sun shadows entirely.");
                egui::ComboBox::from_id_salt("shadow_quality_combo")
                    .selected_text(match settings.shadow_quality {
                        ShadowQuality::Off => "Off",
                        ShadowQuality::Low => "Low",
                        ShadowQuality::Medium => "Medium",
                        ShadowQuality::High => "High",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut settings.shadow_quality, ShadowQuality::Off, "Off");
                        ui.selectable_value(&mut settings.shadow_quality, ShadowQuality::Low, "Low");
                        ui.selectable_value(&mut settings.shadow_quality, ShadowQuality::Medium, "Medium");
                        ui.selectable_value(&mut settings.shadow_quality, ShadowQuality::High, "High");
                    });
            });
        });

    egui::CollapsingHeader::new("Post Processing").default_open(true).show(ui, |ui| {
        ui.checkbox(&mut settings.anti_aliasing, "Anti-aliasing")
            .on_hover_text("Subpixel Morphological Anti-Aliasing (SMAA). Smoothes rough pixels.");

        ui.checkbox(&mut settings.auto_exposure, "Auto exposure").on_hover_text("Automatically adapts camera exposure to brightness.");

        ui.checkbox(&mut settings.bloom, "Bloom").on_hover_text("Adds glow around bright areas.");
        if settings.bloom {
            ui.horizontal(|ui| {
                ui.label("Bloom intensity").on_hover_text("Overall strength of the bloom effect.");
                ui.add(egui::Slider::new(&mut settings.bloom_intensity, 0.0..=2.0));
            });
            ui.horizontal(|ui| {
                ui.label("Bloom threshold").on_hover_text("Only pixels above this brightness contribute to bloom.");
                ui.add(egui::Slider::new(&mut settings.bloom_threshold, 0.0..=5.0));
            });
        }

        ui.checkbox(&mut settings.motion_blur, "Motion blur").on_hover_text("Uses per-object motion vectors to blur fast movement. Costs extra GPU time.");
        if settings.motion_blur {
            ui.horizontal(|ui| {
                ui.label("Shutter angle").on_hover_text("How wide the motion blur is. Higher values blur more. No performance cost.");
                ui.add(egui::Slider::new(&mut settings.motion_blur_shutter_angle, 0.0..=std::f32::consts::TAU));
            });
        }

        ui.horizontal(|ui| {
            ui.label("SSAO").on_hover_text("GTAO-like screen-space ambient occlusion. Adds depth and contact shadowing.");
            egui::ComboBox::from_id_salt("ssao_quality_combo")
                .selected_text(match settings.ssao_quality {
                    SsaoQuality::Off => "Off",
                    SsaoQuality::Medium => "Medium",
                    SsaoQuality::High => "High",
                    SsaoQuality::Ultra => "Ultra",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::Off, "Off");
                    ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::Medium, "Medium");
                    ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::High, "High");
                    ui.selectable_value(&mut settings.ssao_quality, SsaoQuality::Ultra, "Ultra");
                });
        });

        ui.horizontal(|ui| {
            ui.label("Gamma").on_hover_text("Nonlinear brightness shaping applied through Bevy color grading.");
            ui.add(egui::Slider::new(&mut settings.gamma, 0.5..=2.0).fixed_decimals(2));
        });

        ui.horizontal(|ui| {
            ui.label("Contrast").on_hover_text("Moves colors toward or away from neutral gray.");
            ui.add(egui::Slider::new(&mut settings.contrast, 0.5..=1.5).fixed_decimals(2));
        });

        ui.horizontal(|ui| {
            ui.label("Saturation").on_hover_text("Post-tonemap saturation. Lower values desaturate, higher values intensify color.");
            ui.add(egui::Slider::new(&mut settings.saturation, 0.0..=2.0).fixed_decimals(2));
        });

        ui.horizontal(|ui| {
            ui.label("Outline color").on_hover_text("Screen-space outline tint.");
            ui.add(egui::Slider::new(&mut settings.outline_red, 0.0..=1.0).text("R").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut settings.outline_green, 0.0..=1.0).text("G").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut settings.outline_blue, 0.0..=1.0).text("B").fixed_decimals(2));
        });

        ui.horizontal(|ui| {
            ui.label("Outline opacity").on_hover_text("Alpha of the screen-space outline overlay.");
            ui.add(egui::Slider::new(&mut settings.outline_opacity, 0.0..=1.0).fixed_decimals(2));
        });
    });

    egui::CollapsingHeader::new("Camera")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Field of view").on_hover_text("Horizontal field of view in degrees.");
                ui.add(egui::Slider::new(&mut settings.fov, 60.0..=160.0).suffix("°"));
            });

            ui.horizontal(|ui| {
                ui.label("Physics interpolation").on_hover_text("How visual positions are smoothed between physics ticks.");
                ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Off, "Off").on_hover_text("No smoothing. Objects snap to their physics position each tick.");
                ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Interpolate, "Interpolate").on_hover_text("Blends between the previous and current physics tick. Adds one tick of visual latency.");
                ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Extrapolate, "Extrapolate").on_hover_text("Predicts ahead using current velocity. No added latency but can overshoot.");
                ui.selectable_value(&mut settings.physics_interp, PhysicsInterp::Balanced, "Balanced").on_hover_text("Extrapolates position for responsiveness, but interpolates rotation for smoother aiming and camera motion.");
            });
        });
}

fn show_ui_scale_input(ui: &mut egui::Ui, settings: &mut Settings) {
    let id = ui.make_persistent_id("ui_scale_input");
    let mut text = ui
        .data_mut(|data| data.get_persisted::<String>(id))
        .unwrap_or_else(|| format!("{:.0}", settings.ui_scale * 100.0));
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(id)
            .desired_width(56.0)
            .hint_text("150"),
    );
    ui.label("%");
    if response.changed() {
        ui.data_mut(|data| data.insert_persisted(id, text.clone()));
    }
    if response.lost_focus() {
        if let Ok(percent) = text.trim().parse::<f32>() {
            let clamped_percent = percent.clamp(50.0, 250.0);
            settings.ui_scale = clamped_percent / 100.0;
            text = format!("{clamped_percent:.0}");
        } else {
            text = format!("{:.0}", settings.ui_scale * 100.0);
        }
        ui.data_mut(|data| data.insert_persisted(id, text));
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
