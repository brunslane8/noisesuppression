use serde::{Deserialize, Serialize};

/// Everything the user can tweak. Defaults are the recommended setup.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Name of the physical mic, or empty for the Windows default.
    pub mic_device: String,
    /// Microphone volume in dB (-20..=20).
    pub mic_gain_db: f32,
    /// AI noise removal strength, percent (0..=100).
    pub noise_removal: f32,
    /// Steady background hum removal, 0 (off) to 4 (max).
    pub hum_level: u8,
    /// Echo removal strength, percent (0..=100). 0 turns echo removal off.
    pub echo_removal: f32,
    /// How firmly the mic is muted between words, percent (0..=100).
    pub silence_gate: f32,
    /// Keep the voice at a steady loudness.
    pub even_volume: bool,
    /// Play the processed mic through the headphones/speakers.
    pub hear_myself: bool,
    pub start_with_windows: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            mic_device: String::new(),
            mic_gain_db: 0.0,
            noise_removal: 85.0,
            hum_level: 2,
            echo_removal: 50.0,
            silence_gate: 30.0,
            even_volume: true,
            hear_myself: false,
            start_with_windows: false,
        }
    }
}

impl Settings {
    /// Reset the sliders but keep the choices about devices and startup.
    pub fn reset_sliders(&mut self) {
        let d = Settings::default();
        self.mic_gain_db = d.mic_gain_db;
        self.noise_removal = d.noise_removal;
        self.hum_level = d.hum_level;
        self.echo_removal = d.echo_removal;
        self.silence_gate = d.silence_gate;
        self.even_volume = d.even_volume;
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    fn path() -> std::path::PathBuf {
        let base = std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        base.join("Hush").join("settings.json")
    }
}
