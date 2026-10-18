use crate::audio::{self, Engine, Status};
use crate::settings::Settings;
use crate::winutil;
use eframe::egui::{self, Color32, RichText};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct HushApp {
    shared_settings: Arc<Mutex<Settings>>,
    s: Settings,
    saved: Settings,
    engine: Option<Engine>,
    mics: Vec<String>,
    cable: bool,
    last_scan: Instant,
    quitting: bool,
    start_minimized: bool,
}

/// VB-CABLE's installer sometimes makes "CABLE Input" the default speakers, which
/// silences the PC. Put a real speaker back.
fn fix_speakers() {
    if let (Some(cur), Some(cable_in)) = (winutil::default_speaker_id(), audio::cable_input_id()) {
        if cur == cable_in {
            if let Some(real) = audio::real_output_id() {
                winutil::set_default_endpoint(&real);
            }
        }
    }
}

/// Make the Hush microphone the Windows default, remembering the real one.
pub fn take_over(s: &mut Settings) -> bool {
    let Some(cable) = audio::cable_output_id() else {
        return false;
    };
    let cur = winutil::default_mic_id();
    if cur.as_deref() == Some(cable.as_str()) {
        return true;
    }
    if let Some(cur) = cur {
        s.original_mic_id = cur;
    }
    winutil::set_default_endpoint(&cable)
}

/// Give the user's real microphone back as the Windows default.
pub fn give_back(s: &Settings) {
    let Some(cable) = audio::cable_output_id() else {
        return;
    };
    if winutil::default_mic_id().as_deref() == Some(cable.as_str()) && !s.original_mic_id.is_empty() {
        winutil::set_default_endpoint(&s.original_mic_id);
    }
}

impl HushApp {
    pub fn new(start_minimized: bool) -> Self {
        let mut s = Settings::load();
        s.start_with_windows = winutil::autostart_enabled();
        let cable = audio::cable_installed();
        if cable {
            fix_speakers();
            if s.enabled && s.auto_default {
                take_over(&mut s);
            }
        }
        s.save();
        let mut app = Self {
            shared_settings: Arc::new(Mutex::new(s.clone())),
            saved: s.clone(),
            s,
            engine: None,
            mics: audio::list_mics(),
            cable,
            last_scan: Instant::now(),
            quitting: false,
            start_minimized,
        };
        app.restart_engine();
        app
    }

    fn restart_engine(&mut self) {
        self.engine = None; // stops the old one
        if self.s.enabled {
            self.engine = Some(Engine::start(self.shared_settings.clone()));
        }
    }
}

fn describe(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).weak().size(12.5));
    ui.add_space(10.0);
}

fn meter(ui: &mut egui::Ui, label: &str, level: f32) {
    ui.horizontal(|ui| {
        ui.add_sized([110.0, 16.0], egui::Label::new(label));
        // Show on a gentle curve so quiet voices still move the bar.
        let shown = level.clamp(0.0, 1.0).sqrt();
        ui.add(egui::ProgressBar::new(shown).desired_width(240.0));
    });
}

impl eframe::App for HushApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        if std::mem::take(&mut self.start_minimized) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        // Closing the window just tucks it into the taskbar so the mic keeps working.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        let mut restart = false;
        if self.last_scan.elapsed() > Duration::from_secs(3) {
            self.mics = audio::list_mics();
            let cable = audio::cable_installed();
            if cable && !self.cable {
                // The driver just appeared: finish setting everything up.
                fix_speakers();
                if self.s.enabled && self.s.auto_default {
                    take_over(&mut self.s);
                }
                restart = true;
            }
            self.cable = cable;
            self.last_scan = Instant::now();
        }

        let (status, in_level, out_level) = match &self.engine {
            Some(e) => {
                let (i, o) = e.shared().levels();
                (Some(e.shared().status()), i, o)
            }
            None => (None, 0.0, 0.0),
        };

        egui::CentralPanel::default().show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.heading("Hush");
                    ui.label(RichText::new("clean, clear microphone").weak());
                });
                ui.add_space(6.0);

                let label = if self.s.enabled { "Noise suppression is ON" } else { "Noise suppression is OFF" };
                let btn = egui::Button::new(RichText::new(label).size(17.0).strong())
                    .fill(if self.s.enabled { Color32::from_rgb(34, 139, 84) } else { Color32::from_gray(80) })
                    .min_size(egui::vec2(ui.available_width(), 40.0));
                if ui.add(btn).clicked() {
                    self.s.enabled = !self.s.enabled;
                    if self.s.auto_default {
                        if self.s.enabled {
                            take_over(&mut self.s);
                        } else {
                            give_back(&self.s);
                        }
                    }
                    restart = true;
                }
                ui.add_space(4.0);

                match status {
                    _ if !self.s.enabled => {
                        ui.label("Paused. Your apps are using your normal microphone again.");
                    }
                    Some(Status::NoCable) => {
                        ui.colored_label(Color32::from_rgb(230, 160, 40), "Hush's audio driver (VB-CABLE) is missing.");
                        describe(ui, "Run the Hush installer again, or install the free VB-CABLE yourself and restart your PC.");
                        if ui.button("Download VB-CABLE (free)").clicked() {
                            let _ = std::process::Command::new("cmd").args(["/C", "start", "https://vb-audio.com/Cable/"]).spawn();
                        }
                    }
                    Some(Status::Problem(m)) => {
                        ui.colored_label(Color32::from_rgb(220, 80, 70), m);
                    }
                    Some(Status::Running) if self.s.auto_default => {
                        ui.colored_label(Color32::from_rgb(60, 180, 110), "Working in all your apps. Nothing else to set up.");
                    }
                    Some(Status::Running) => {
                        ui.colored_label(Color32::from_rgb(60, 180, 110), "Working! In your apps, choose \"CABLE Output\" as the microphone.");
                    }
                    _ => {
                        ui.label("Starting...");
                    }
                }
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label("Microphone:");
                    let current = if self.s.mic_device.is_empty() { "Windows default".to_string() } else { self.s.mic_device.clone() };
                    let before = self.s.mic_device.clone();
                    egui::ComboBox::from_id_salt("mic").width(260.0).selected_text(current).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.s.mic_device, String::new(), "Windows default");
                        for m in &self.mics {
                            ui.selectable_value(&mut self.s.mic_device, m.clone(), m);
                        }
                    });
                    if before != self.s.mic_device {
                        restart = true;
                    }
                });
                ui.add_space(4.0);
                meter(ui, "Your mic", in_level);
                meter(ui, "What others hear", out_level);
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                ui.label(RichText::new("Microphone volume").strong());
                ui.add(egui::Slider::new(&mut self.s.mic_gain_db, -20.0..=20.0).suffix(" dB").fixed_decimals(0));
                describe(ui, "How loud your voice is. Turn it up if people say you are too quiet, down if you sound too loud or distorted.");

                ui.label(RichText::new("Noise removal").strong());
                ui.add(egui::Slider::new(&mut self.s.noise_removal, 0.0..=100.0).suffix("%").fixed_decimals(0));
                describe(ui, "Removes sudden sounds like typing, clicks, dogs, traffic and other voices far away. Higher removes more, but too high can make your voice sound a bit robotic.");

                ui.label(RichText::new("Background hum removal").strong());
                let names = ["Off", "Low", "Medium", "High", "Max"];
                let mut hum = self.s.hum_level as f32;
                ui.add(egui::Slider::new(&mut hum, 0.0..=4.0).step_by(1.0).custom_formatter(move |v, _| names[(v as usize).min(4)].to_string()).custom_parser(|_| None));
                self.s.hum_level = hum as u8;
                describe(ui, "Removes steady sounds that never stop, like a fan, air conditioner, computer buzz or electrical hum.");

                ui.label(RichText::new("Echo removal").strong());
                ui.add(egui::Slider::new(&mut self.s.echo_removal, 0.0..=100.0).suffix("%").fixed_decimals(0));
                describe(ui, "Stops other people from hearing themselves (or the game or video you are playing) coming back through your microphone when you use speakers. If you wear headphones you can turn this down. 0 turns it off.");

                ui.label(RichText::new("Silence between words").strong());
                ui.add(egui::Slider::new(&mut self.s.silence_gate, 0.0..=100.0).suffix("%").fixed_decimals(0));
                describe(ui, "Quietly mutes your microphone when you are not talking, so people hear nothing instead of faint background sound. If it ever cuts off the start of your words, turn it down.");

                ui.checkbox(&mut self.s.even_volume, RichText::new("Even out my volume").strong());
                describe(ui, "Keeps your voice at a steady level, whether you lean in close or sit back.");

                ui.separator();
                ui.add_space(4.0);
                ui.checkbox(&mut self.s.hear_myself, "Let me hear myself (use headphones!)");
                describe(ui, "Plays your cleaned-up voice in your headphones so you can fine-tune the sliders by ear.");

                if ui.checkbox(&mut self.s.start_with_windows, "Start Hush when Windows starts").changed() {
                    winutil::set_autostart(self.s.start_with_windows);
                }
                if ui.checkbox(&mut self.s.auto_default, "Use Hush in all my apps automatically").changed() {
                    if self.s.auto_default {
                        take_over(&mut self.s);
                    } else {
                        give_back(&self.s);
                    }
                }
                describe(ui, "Makes Hush your Windows microphone, so every app uses it with no setup. Your normal mic comes back when you quit Hush.");
                if ui.button("Reset to recommended").clicked() {
                    self.s.reset_sliders();
                }
                ui.add_space(8.0);
                if ui.button("Quit Hush").clicked() {
                    if self.s.auto_default {
                        give_back(&self.s);
                    }
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        });

        // Push changes to the audio thread and save them.
        if self.s != self.saved {
            *self.shared_settings.lock().unwrap() = self.s.clone();
            self.s.save();
            self.saved = self.s.clone();
        }
        if restart {
            self.restart_engine();
        }
        ctx.request_repaint_after(Duration::from_millis(60));
    }
}
