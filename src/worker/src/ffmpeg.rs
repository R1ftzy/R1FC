use crate::settings::Settings;
use std::ffi::OsString;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Png,
    Jpg,
    Webp,
    Bmp,
    Tiff,
    Gif,
    Mp4,
    Webm,
}

impl Target {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "png" => Some(Self::Png),
            "jpg" => Some(Self::Jpg),
            "webp" => Some(Self::Webp),
            "bmp" => Some(Self::Bmp),
            "tiff" => Some(Self::Tiff),
            "gif" => Some(Self::Gif),
            "mp4" => Some(Self::Mp4),
            "webm" => Some(Self::Webm),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Webp => "webp",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Gif => "gif",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }

    pub fn ext(self) -> &'static str {
        self.id()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Image,
    Video,
}

pub fn normalized_ext(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    Some(match extension.as_str() {
        "jpeg" => "jpg".to_string(),
        "tif" => "tiff".to_string(),
        _ => extension,
    })
}

pub fn source_kind(ext: &str) -> Option<SourceKind> {
    match ext {
        "png" | "jpg" | "webp" | "bmp" | "tiff" | "gif" | "ppm" | "pgm" | "pbm" | "pnm"
        | "ico" => Some(SourceKind::Image),
        "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" => Some(SourceKind::Video),
        _ => None,
    }
}

pub fn target_supported_for(target: Target, ext: &str) -> bool {
    let Some(kind) = source_kind(ext) else {
        return false;
    };

    if target.ext() == ext {
        return false;
    }

    match target {
        Target::Png | Target::Jpg | Target::Bmp | Target::Tiff => kind == SourceKind::Image,
        Target::Webp | Target::Gif => true,
        Target::Mp4 | Target::Webm => kind == SourceKind::Video || ext == "gif",
    }
}

pub fn find_ffmpeg(settings: &Settings) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    let explicit = settings.ffmpeg_path.trim();
    if !explicit.is_empty() {
        let path = PathBuf::from(explicit);
        candidates.push(path.clone());
        if path.is_dir() {
            candidates.push(path.join("ffmpeg.exe"));
        }
    }

    if let Some(value) = std::env::var_os("R1FC_FFMPEG") {
        candidates.push(PathBuf::from(value));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ffmpeg.exe"));
            candidates.push(dir.join("tools").join("ffmpeg.exe"));
        }
    }

    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        candidates.push(
            local
                .join("Microsoft")
                .join("WinGet")
                .join("Links")
                .join("ffmpeg.exe"),
        );

        let packages = local
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        if let Ok(entries) = std::fs::read_dir(&packages) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.starts_with("gyan.ffmpeg") {
                    if let Ok(children) = std::fs::read_dir(entry.path()) {
                        for child in children.flatten() {
                            candidates.push(child.path().join("bin").join("ffmpeg.exe"));
                        }
                    }
                }
            }
        }
    }

    candidates.push(PathBuf::from(r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"));
    candidates.push(PathBuf::from(r"C:\ffmpeg\bin\ffmpeg.exe"));

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            candidates.push(dir.join("ffmpeg.exe"));
        }
    }

    candidates.into_iter().find(|path| path.is_file())
}

pub fn version(ffmpeg: &Path) -> Result<String, String> {
    let output = Command::new(ffmpeg)
        .arg("-version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("Could not run ffmpeg: {error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("ffmpeg")
        .to_string())
}

pub struct RunOutput {
    pub ok: bool,
    pub stderr: String,
}

pub fn run(ffmpeg: &Path, args: &[OsString], pid: &AtomicU32) -> Result<RunOutput, String> {
    let mut command = Command::new(ffmpeg);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);

    let child = command
        .spawn()
        .map_err(|error| format!("Could not start ffmpeg: {error}"))?;

    pid.store(child.id(), Ordering::SeqCst);
    let output = child.wait_with_output();
    pid.store(0, Ordering::SeqCst);

    let output = output.map_err(|error| format!("Could not wait for ffmpeg: {error}"))?;

    Ok(RunOutput {
        ok: output.status.success(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn push(args: &mut Vec<OsString>, value: impl Into<OsString>) {
    args.push(value.into());
}

pub fn build_args(
    target: Target,
    input: &Path,
    output: &Path,
    kind: SourceKind,
    settings: &Settings,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();

    for value in ["-hide_banner", "-loglevel", "error", "-nostdin", "-n"] {
        push(&mut args, value);
    }
    push(&mut args, "-i");
    args.push(input.as_os_str().to_os_string());
    for value in ["-map_metadata", "-1"] {
        push(&mut args, value);
    }

    match target {
        Target::Png => {
            for value in ["-frames:v", "1", "-c:v", "png"] {
                push(&mut args, value);
            }
        }
        Target::Jpg => {
            let filter = format!(
                "[0:v]split=2[fg][ref];color=c={}:s=1x1[bg];\
                 [bg][ref]scale2ref=w=iw:h=ih[bg2][ref2];[ref2]nullsink;\
                 [bg2][fg]overlay=format=auto:shortest=1,format=yuvj420p[out]",
                settings.flatten_color
            );
            push(&mut args, "-filter_complex");
            push(&mut args, filter);
            for value in ["-map", "[out]", "-frames:v", "1", "-c:v", "mjpeg"] {
                push(&mut args, value);
            }
            push(&mut args, "-q:v");
            push(&mut args, settings.jpg_qscale.to_string());
            for value in ["-pix_fmt", "yuvj420p"] {
                push(&mut args, value);
            }
        }
        Target::Bmp => {
            for value in ["-frames:v", "1", "-c:v", "bmp"] {
                push(&mut args, value);
            }
        }
        Target::Tiff => {
            for value in ["-frames:v", "1", "-c:v", "tiff"] {
                push(&mut args, value);
            }
        }
        Target::Webp => {
            if kind == SourceKind::Image {
                for value in ["-frames:v", "1", "-c:v", "libwebp", "-lossless", "0"] {
                    push(&mut args, value);
                }
                push(&mut args, "-q:v");
                push(&mut args, settings.webp_quality.to_string());
            } else {
                let filter = format!(
                    "fps={},scale='min({},iw)':-2:flags=lanczos",
                    settings.webp_animated_fps, settings.webp_max_width
                );
                for value in ["-loop", "0", "-c:v", "libwebp", "-lossless", "0"] {
                    push(&mut args, value);
                }
                push(&mut args, "-q:v");
                push(&mut args, settings.webp_animated_quality.to_string());
                push(&mut args, "-vf");
                push(&mut args, filter);
                push(&mut args, "-an");
            }
        }
        Target::Gif => {
            if kind == SourceKind::Image {
                let filter = "split[s0][s1];[s0]palettegen=max_colors=256[p];\
                              [s1][p]paletteuse=dither=sierra2_4a";
                push(&mut args, "-filter_complex");
                push(&mut args, filter);
                for value in ["-frames:v", "1", "-c:v", "gif"] {
                    push(&mut args, value);
                }
            } else {
                let filter = format!(
                    "fps={},scale='min({},iw)':-2:flags=lanczos,\
                     split[s0][s1];[s0]palettegen=max_colors=256:stats_mode=diff[p];\
                     [s1][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
                    settings.gif_fps, settings.gif_max_width
                );
                push(&mut args, "-loop");
                push(&mut args, "0");
                push(&mut args, "-filter_complex");
                push(&mut args, filter);
                for value in ["-an", "-c:v", "gif"] {
                    push(&mut args, value);
                }
            }
        }
        Target::Mp4 => {
            for value in [
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "20",
                "-pix_fmt",
                "yuv420p",
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
                "-map_chapters",
                "-1",
            ] {
                push(&mut args, value);
            }
        }
        Target::Webm => {
            for value in [
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "32",
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-deadline",
                "good",
                "-cpu-used",
                "2",
                "-c:a",
                "libopus",
                "-b:a",
                "128k",
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2",
            ] {
                push(&mut args, value);
            }
        }
    }

    args.push(output.as_os_str().to_os_string());
    args
}
