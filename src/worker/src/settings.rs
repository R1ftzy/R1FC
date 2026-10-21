use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Optional explicit path to ffmpeg.exe, or a directory containing it.
    pub ffmpeg_path: String,
    /// ffmpeg mjpeg qscale. Lower is better quality. 2 is the default.
    pub jpg_qscale: i32,
    /// Quality for still WebP output.
    pub webp_quality: i32,
    /// Quality for animated WebP output.
    pub webp_animated_quality: i32,
    /// Frame rate for animated WebP output.
    pub webp_animated_fps: i32,
    /// Maximum width for animated WebP output. Images are never upscaled.
    pub webp_max_width: i32,
    /// Frame rate for GIF output.
    pub gif_fps: i32,
    /// Maximum width for GIF output. Images are never upscaled.
    pub gif_max_width: i32,
    /// Background used when flattening transparency into JPEG. "white", "black", or #rrggbb.
    pub flatten_color: String,
    /// Show the progress window when any input is at least this large, in MB.
    pub progress_threshold_mb: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ffmpeg_path: String::new(),
            jpg_qscale: 2,
            webp_quality: 90,
            webp_animated_quality: 80,
            webp_animated_fps: 15,
            webp_max_width: 1280,
            gif_fps: 12,
            gif_max_width: 800,
            flatten_color: "white".to_string(),
            progress_threshold_mb: 20,
        }
    }
}

impl Settings {
    pub fn sanitized(mut self) -> Self {
        self.jpg_qscale = self.jpg_qscale.clamp(1, 31);
        self.webp_quality = self.webp_quality.clamp(0, 100);
        self.webp_animated_quality = self.webp_animated_quality.clamp(0, 100);
        self.webp_animated_fps = self.webp_animated_fps.clamp(1, 60);
        self.webp_max_width = self.webp_max_width.clamp(16, 8192);
        self.gif_fps = self.gif_fps.clamp(1, 60);
        self.gif_max_width = self.gif_max_width.clamp(16, 8192);
        self.progress_threshold_mb = self.progress_threshold_mb.min(1_000_000);
        self.ffmpeg_path = self.ffmpeg_path.trim().trim_matches('"').to_string();

        let color = self.flatten_color.trim().to_ascii_lowercase();
        self.flatten_color = if color == "black" {
            "black".to_string()
        } else if color == "white" {
            "white".to_string()
        } else if color.starts_with('#')
            && (color.len() == 4 || color.len() == 7)
            && color[1..].chars().all(|c| c.is_ascii_hexdigit())
        {
            color
        } else {
            "white".to_string()
        };

        self
    }
}

pub fn local_app_data() -> PathBuf {
    if let Some(value) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(value);
    }
    if let Some(value) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(value).join("AppData").join("Local");
    }
    PathBuf::from(".")
}

pub fn r1fc_dir() -> PathBuf {
    local_app_data().join("R1fc")
}

pub fn settings_path() -> PathBuf {
    r1fc_dir().join("settings.json")
}

pub fn log_path() -> PathBuf {
    r1fc_dir().join("logs").join("convert.log")
}

pub fn load_or_create() -> Settings {
    let path = settings_path();
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => settings.sanitized(),
            Err(error) => {
                log(&format!(
                    "Could not parse settings file {}: {error}. Using defaults.",
                    path.display()
                ));
                Settings::default()
            }
        },
        Err(_) => {
            let settings = Settings::default();
            let _ = save(&settings);
            settings
        }
    }
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(settings)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    fs::write(path, text)
}

pub fn log(message: &str) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{seconds}] {message}");
    }
}
