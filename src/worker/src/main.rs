mod ffmpeg;
mod settings;
mod ui;

use ffmpeg::{SourceKind, Target};
use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use ui::UiState;

#[derive(Clone)]
struct Job {
    input: PathBuf,
    output: PathBuf,
    kind: SourceKind,
    target: Target,
}

struct RunSummary {
    converted: usize,
    failed: usize,
    skipped: usize,
    cancelled: bool,
    failures: Vec<String>,
}

impl RunSummary {
    fn detail(&self) -> String {
        let mut lines = vec![
            format!("Converted: {}", self.converted),
            format!("Failed: {}", self.failed),
            format!("Skipped: {}", self.skipped),
        ];

        if self.cancelled {
            lines.push("Cancelled.".to_string());
        }

        if !self.failures.is_empty() {
            lines.push(String::new());
            lines.push("Failures:".to_string());
            for failure in self.failures.iter().take(8) {
                lines.push(format!("- {failure}"));
            }
            if self.failures.len() > 8 {
                lines.push(format!("- ...and {} more", self.failures.len() - 8));
            }
        }

        lines.push(String::new());
        lines.push(format!("Log: {}", settings::log_path().display()));

        let mut text = lines.join("\n");
        if text.len() > 3000 {
            text.truncate(3000);
            text.push_str("\n...");
        }
        text
    }
}

fn main() {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.is_empty() {
        usage();
        return;
    }

    let command = args[0].to_string_lossy().to_ascii_lowercase();
    match command.as_str() {
        "--version" | "-v" => {
            println!("R1fc Convert {}", env!("CARGO_PKG_VERSION"));
        }
        "--settings" => open_settings(),
        "--self-test" => {
            let code = self_test();
            std::process::exit(code);
        }
        "--target" => run_from_args(&args),
        "--help" | "-h" | "/?" => usage(),
        _ => {
            eprintln!("Unknown argument: {}", args[0].to_string_lossy());
            usage();
            std::process::exit(2);
        }
    }
}

fn usage() {
    eprintln!(
        "R1fc Convert worker\n\
         \n\
         Usage:\n\
           r1fc-convert.exe --target <png|jpg|webp|bmp|tiff|gif|mp4|webm> [--response-file <path>] [files...]\n\
           r1fc-convert.exe --settings\n\
           r1fc-convert.exe --self-test\n\
           r1fc-convert.exe --version\n"
    );
}

fn run_from_args(args: &[OsString]) {
    if args.len() < 2 {
        ui::message_box_error("Missing target format.");
        return;
    }

    let target_text = args[1].to_string_lossy();
    let Some(target) = Target::from_id(&target_text) else {
        ui::message_box_error(&format!("Unknown target format: {target_text}"));
        return;
    };

    let mut files: Vec<PathBuf> = Vec::new();
    let mut response_file: Option<PathBuf> = None;
    let mut index = 2;

    while index < args.len() {
        let arg = args[index].to_string_lossy();
        if arg == "--response-file" {
            if index + 1 >= args.len() {
                ui::message_box_error("Missing value for --response-file.");
                return;
            }
            response_file = Some(PathBuf::from(&args[index + 1]));
            index += 2;
        } else if arg.starts_with("--") {
            ui::message_box_error(&format!("Unknown argument: {arg}"));
            return;
        } else {
            files.push(PathBuf::from(&args[index]));
            index += 1;
        }
    }

    if let Some(path) = response_file {
        match read_response_file(&path) {
            Ok(mut from_file) => files.append(&mut from_file),
            Err(error) => {
                let _ = fs::remove_file(&path);
                ui::message_box_error(&error);
                return;
            }
        }
        let _ = fs::remove_file(&path);
    }

    if files.is_empty() {
        ui::message_box_error("No files were selected.");
        return;
    }

    convert(target, files);
}

fn read_response_file(path: &Path) -> Result<Vec<PathBuf>, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("Could not read the file list: {error}"))?;

    if bytes.len() % 2 != 0 {
        return Err("The file list is malformed.".to_string());
    }

    let mut units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();

    if units.first() == Some(&0xFEFF) {
        units.remove(0);
    }

    let mut result = Vec::new();
    let mut current: Vec<u16> = Vec::new();

    for unit in units {
        if unit == b'\r' as u16 || unit == b'\n' as u16 {
            if !current.is_empty() {
                result.push(PathBuf::from(OsString::from_wide(&current)));
                current.clear();
            }
        } else {
            current.push(unit);
        }
    }

    if !current.is_empty() {
        result.push(PathBuf::from(OsString::from_wide(&current)));
    }

    Ok(result)
}

fn convert(target: Target, files: Vec<PathBuf>) {
    let config = settings::load_or_create();
    let Some(ffmpeg) = ffmpeg::find_ffmpeg(&config) else {
        let message = format!(
            "R1fc Convert could not find ffmpeg.exe.\n\n\
             Install it with:\n  winget install Gyan.FFmpeg\n\n\
             Or set \"ffmpeg_path\" in:\n  {}\n\n\
             Then try again.",
            settings::settings_path().display()
        );
        settings::log("ffmpeg not found");
        ui::message_box_error(&message);
        return;
    };

    let mut skipped = 0usize;
    let jobs = build_jobs(target, &files, &mut skipped);
    if jobs.is_empty() {
        ui::message_box_error(&format!(
            "Nothing to convert.\n\nNo selected file can be converted to {}.\n\nLog: {}",
            target.id(),
            settings::log_path().display()
        ));
        return;
    }

    settings::log(&format!(
        "target={} files={} jobs={} skipped={} ffmpeg={}",
        target.id(),
        files.len(),
        jobs.len(),
        skipped,
        ffmpeg.display()
    ));

    if should_show_progress(&jobs, &config) {
        run_with_ui(jobs, ffmpeg, config, skipped);
    } else {
        let cancel = AtomicBool::new(false);
        let pid = AtomicU32::new(0);
        let summary = run_jobs(&jobs, &ffmpeg, &config, &cancel, &pid, None, skipped);
        if summary.failed > 0 || summary.cancelled {
            ui::message_box_error(&summary.detail());
        }
    }
}

fn build_jobs(target: Target, files: &[PathBuf], skipped: &mut usize) -> Vec<Job> {
    let mut jobs = Vec::new();
    let mut planned: HashSet<String> = HashSet::new();
    let mut seen: HashSet<String> = HashSet::new();

    for input in files {
        let key = input.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }

        if !input.is_file() {
            *skipped += 1;
            settings::log(&format!("skip (not a file): {}", input.display()));
            continue;
        }

        let Some(extension) = ffmpeg::normalized_ext(input) else {
            *skipped += 1;
            settings::log(&format!("skip (no extension): {}", input.display()));
            continue;
        };

        if !ffmpeg::target_supported_for(target, &extension) {
            *skipped += 1;
            settings::log(&format!(
                "skip ({} -> {}): {}",
                extension,
                target.id(),
                input.display()
            ));
            continue;
        }

        let Some(kind) = ffmpeg::source_kind(&extension) else {
            *skipped += 1;
            continue;
        };

        let output = unique_output(input, target.ext(), &mut planned);
        jobs.push(Job {
            input: input.clone(),
            output,
            kind,
            target,
        });
    }

    jobs
}

fn unique_output(input: &Path, extension: &str, planned: &mut HashSet<String>) -> PathBuf {
    let directory = input
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let stem = input
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "converted".to_string());

    let mut index = 0usize;
    loop {
        let name = if index == 0 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem} ({index}).{extension}")
        };
        let candidate = directory.join(name);
        let key = candidate.to_string_lossy().to_ascii_lowercase();
        if !candidate.exists() && !planned.contains(&key) {
            planned.insert(key);
            return candidate;
        }
        index += 1;
    }
}

fn temp_output(final_path: &Path, extension: &str) -> PathBuf {
    let directory = final_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);

    directory.join(format!(
        ".r1fc-{}-{}.{}",
        std::process::id(),
        stamp,
        extension
    ))
}

fn should_show_progress(jobs: &[Job], config: &settings::Settings) -> bool {
    if jobs.len() > 10 {
        return true;
    }

    let threshold = config.progress_threshold_mb.saturating_mul(1024 * 1024);
    jobs.iter().any(|job| {
        job.kind == SourceKind::Video
            || fs::metadata(&job.input)
                .map(|metadata| metadata.len() >= threshold)
                .unwrap_or(false)
    })
}

fn run_with_ui(
    jobs: Vec<Job>,
    ffmpeg: PathBuf,
    config: settings::Settings,
    skipped: usize,
) {
    let state = Arc::new(Mutex::new(UiState::new()));
    let cancel = Arc::new(AtomicBool::new(false));
    let pid = Arc::new(AtomicU32::new(0));
    let summary_slot: Arc<Mutex<Option<RunSummary>>> = Arc::new(Mutex::new(None));

    let work = {
        let state = state.clone();
        let cancel = cancel.clone();
        let pid = pid.clone();
        let summary_slot = summary_slot.clone();

        move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_jobs(
                    &jobs,
                    &ffmpeg,
                    &config,
                    &cancel,
                    &pid,
                    Some(&state),
                    skipped,
                )
            }));

            let summary = match result {
                Ok(summary) => summary,
                Err(_) => {
                    settings::log("internal panic in conversion thread");
                    RunSummary {
                        converted: 0,
                        failed: 1,
                        skipped,
                        cancelled: false,
                        failures: vec!["internal error (see log)".to_string()],
                    }
                }
            };

            if let Ok(mut ui_state) = state.lock() {
                ui_state.done = true;
                ui_state.success = summary.failed == 0 && !summary.cancelled;
                ui_state.cancelled = summary.cancelled;
                ui_state.detail = summary.detail();
                ui_state.status = if summary.cancelled {
                    "Cancelled".to_string()
                } else if ui_state.success {
                    "Done".to_string()
                } else {
                    "Finished with errors".to_string()
                };
            }

            if let Ok(mut slot) = summary_slot.lock() {
                *slot = Some(summary);
            }
        }
    };

    ui::run_with_progress(state, cancel, pid, work);

    let finished = summary_slot.lock().ok().and_then(|mut slot| slot.take());
    if let Some(summary) = finished {
        settings::log(&format!(
            "finished: converted={} failed={} skipped={} cancelled={}",
            summary.converted, summary.failed, summary.skipped, summary.cancelled
        ));
    }
}

fn run_jobs(
    jobs: &[Job],
    ffmpeg: &Path,
    config: &settings::Settings,
    cancel: &AtomicBool,
    pid: &AtomicU32,
    state: Option<&Arc<Mutex<UiState>>>,
    skipped: usize,
) -> RunSummary {
    let mut summary = RunSummary {
        converted: 0,
        failed: 0,
        skipped,
        cancelled: false,
        failures: Vec::new(),
    };

    let total = jobs.len();

    for (index, job) in jobs.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            summary.cancelled = true;
            break;
        }

        let name = job
            .input
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| job.input.display().to_string());

        set_status(
            state,
            format!("Converting {} of {}: {}", index + 1, total, name),
        );

        settings::log(&format!(
            "[{}/{}] {} -> {}",
            index + 1,
            total,
            job.input.display(),
            job.output.display()
        ));

        match convert_one(job, ffmpeg, config, cancel, pid) {
            Ok(()) => {
                summary.converted += 1;
                settings::log("  ok");
            }
            Err(error) => {
                if cancel.load(Ordering::SeqCst) {
                    summary.cancelled = true;
                    settings::log("  cancelled");
                    break;
                }
                summary.failed += 1;
                settings::log(&format!("  failed: {error}"));
                summary.failures.push(format!("{name}: {error}"));
            }
        }
    }

    if cancel.load(Ordering::SeqCst) {
        summary.cancelled = true;
    }

    summary
}

fn set_status(state: Option<&Arc<Mutex<UiState>>>, status: String) {
    if let Some(state) = state {
        if let Ok(mut value) = state.lock() {
            value.status = status;
        }
    }
}

fn convert_one(
    job: &Job,
    ffmpeg: &Path,
    config: &settings::Settings,
    cancel: &AtomicBool,
    pid: &AtomicU32,
) -> Result<(), String> {
    if job.output.exists() {
        return Err("output already exists".to_string());
    }

    let temporary = temp_output(&job.output, job.target.ext());
    let args = ffmpeg::build_args(job.target, &job.input, &temporary, job.kind, config);
    let result = ffmpeg::run(ffmpeg, &args, pid);

    if cancel.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&temporary);
        return Err("cancelled".to_string());
    }

    let output = match result {
        Ok(output) => output,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };

    if !output.ok {
        let _ = fs::remove_file(&temporary);
        return Err(trim_ffmpeg_error(&output.stderr));
    }

    let valid = fs::metadata(&temporary)
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false);
    if !valid {
        let _ = fs::remove_file(&temporary);
        return Err("ffmpeg did not produce an output file".to_string());
    }

    if job.output.exists() {
        let _ = fs::remove_file(&temporary);
        return Err("output already exists".to_string());
    }

    fs::rename(&temporary, &job.output).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("could not move output into place: {error}")
    })?;

    Ok(())
}

fn trim_ffmpeg_error(stderr: &str) -> String {
    let text = stderr.trim();
    if text.is_empty() {
        return "ffmpeg reported an error".to_string();
    }

    let mut result = text.lines().take(8).collect::<Vec<_>>().join("\n");
    if result.len() > 800 {
        result.truncate(800);
        result.push_str("...");
    }
    result
}

fn open_settings() {
    let config = settings::load_or_create();
    let _ = settings::save(&config);

    let path = settings::settings_path();
    if let Err(error) = Command::new("notepad.exe").arg(&path).spawn() {
        ui::message_box_error(&format!(
            "Could not open the settings file.\n\n{error}\n\n{}",
            path.display()
        ));
    }
}

fn self_test() -> i32 {
    let config = settings::load_or_create();
    let Some(ffmpeg) = ffmpeg::find_ffmpeg(&config) else {
        println!("FAIL: ffmpeg.exe was not found.");
        return 1;
    };

    println!("ffmpeg: {}", ffmpeg.display());
    match ffmpeg::version(&ffmpeg) {
        Ok(version) => println!("version: {version}"),
        Err(error) => {
            println!("FAIL: {error}");
            return 1;
        }
    }

    let root = env::temp_dir().join(format!("r1fc-selftest-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);

    let tests: Vec<(&str, Target)> = vec![
        ("png_to_jpg", Target::Jpg),
        ("ppm_to_png", Target::Png),
        ("mp4_to_webp", Target::Webp),
        ("alpha_to_jpg", Target::Jpg),
    ];

    let mut all_ok = true;

    for (name, target) in tests {
        let directory = root.join(name);
        if let Err(error) = fs::create_dir_all(&directory) {
            println!("FAIL {name}: {error}");
            all_ok = false;
            continue;
        }

        let source = match name {
            "png_to_jpg" => directory.join("source.png"),
            "ppm_to_png" => directory.join("source.ppm"),
            "mp4_to_webp" => directory.join("source.mp4"),
            "alpha_to_jpg" => directory.join("source.png"),
            _ => directory.join("source.png"),
        };

        let generated = match name {
            "png_to_jpg" => run_simple(
                &ffmpeg,
                &[
                    OsStr::new("-y"),
                    OsStr::new("-hide_banner"),
                    OsStr::new("-loglevel"),
                    OsStr::new("error"),
                    OsStr::new("-f"),
                    OsStr::new("lavfi"),
                    OsStr::new("-i"),
                    OsStr::new("color=c=red:s=64x64:d=1"),
                    OsStr::new("-frames:v"),
                    OsStr::new("1"),
                    source.as_os_str(),
                ],
            ),
            "ppm_to_png" => {
                let png = directory.join("source.png");
                run_simple(
                    &ffmpeg,
                    &[
                        OsStr::new("-y"),
                        OsStr::new("-hide_banner"),
                        OsStr::new("-loglevel"),
                        OsStr::new("error"),
                        OsStr::new("-f"),
                        OsStr::new("lavfi"),
                        OsStr::new("-i"),
                        OsStr::new("color=c=blue:s=64x64:d=1"),
                        OsStr::new("-frames:v"),
                        OsStr::new("1"),
                        png.as_os_str(),
                    ],
                )
                .and_then(|_| {
                    run_simple(
                        &ffmpeg,
                        &[
                            OsStr::new("-y"),
                            OsStr::new("-hide_banner"),
                            OsStr::new("-loglevel"),
                            OsStr::new("error"),
                            OsStr::new("-i"),
                            png.as_os_str(),
                            source.as_os_str(),
                        ],
                    )
                })
            }
            "mp4_to_webp" => run_simple(
                &ffmpeg,
                &[
                    OsStr::new("-y"),
                    OsStr::new("-hide_banner"),
                    OsStr::new("-loglevel"),
                    OsStr::new("error"),
                    OsStr::new("-f"),
                    OsStr::new("lavfi"),
                    OsStr::new("-i"),
                    OsStr::new("testsrc=size=320x240:rate=10:duration=2"),
                    OsStr::new("-pix_fmt"),
                    OsStr::new("yuv420p"),
                    source.as_os_str(),
                ],
            ),
            "alpha_to_jpg" => run_simple(
                &ffmpeg,
                &[
                    OsStr::new("-y"),
                    OsStr::new("-hide_banner"),
                    OsStr::new("-loglevel"),
                    OsStr::new("error"),
                    OsStr::new("-f"),
                    OsStr::new("lavfi"),
                    OsStr::new("-i"),
                    OsStr::new("color=c=red:s=32x32:d=1"),
                    OsStr::new("-f"),
                    OsStr::new("lavfi"),
                    OsStr::new("-i"),
                    OsStr::new("color=c=black@0.0:s=64x64:d=1,format=rgba"),
                    OsStr::new("-filter_complex"),
                    OsStr::new("[1:v][0:v]overlay=16:16:format=auto,format=rgba"),
                    OsStr::new("-frames:v"),
                    OsStr::new("1"),
                    source.as_os_str(),
                ],
            ),
            _ => Ok(()),
        };

        if let Err(error) = generated {
            println!("FAIL {name}: could not create test input: {error}");
            all_ok = false;
            continue;
        }

        let mut skipped = 0usize;
        let jobs = build_jobs(target, std::slice::from_ref(&source), &mut skipped);
        let Some(job) = jobs.first() else {
            println!("FAIL {name}: no job was created");
            all_ok = false;
            continue;
        };

        let cancel = AtomicBool::new(false);
        let pid = AtomicU32::new(0);
        match convert_one(job, &ffmpeg, &config, &cancel, &pid) {
            Ok(()) if job.output.is_file() => {
                if name == "alpha_to_jpg" {
                    match jpeg_corner_is_white(&ffmpeg, &job.output) {
                        Ok(true) => println!("PASS {name}"),
                        Ok(false) => {
                            println!("FAIL {name}: transparent area is not white");
                            all_ok = false;
                        }
                        Err(error) => {
                            println!("FAIL {name}: {error}");
                            all_ok = false;
                        }
                    }
                } else {
                    println!("PASS {name} -> {}", job.output.display());
                }
            }
            Ok(()) => {
                println!("FAIL {name}: output was not created");
                all_ok = false;
            }
            Err(error) => {
                println!("FAIL {name}: {error}");
                all_ok = false;
            }
        }
    }

    if all_ok {
        let _ = fs::remove_dir_all(&root);
        0
    } else {
        println!("Test files kept at: {}", root.display());
        1
    }
}

fn run_simple(ffmpeg: &Path, args: &[&OsStr]) -> Result<(), String> {
    let output = Command::new(ffmpeg)
        .args(args)
        .creation_flags(ffmpeg::CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("could not run ffmpeg: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn jpeg_corner_is_white(ffmpeg: &Path, path: &Path) -> Result<bool, String> {
    let output = Command::new(ffmpeg)
        .args([
            OsStr::new("-v"),
            OsStr::new("error"),
            OsStr::new("-i"),
            path.as_os_str(),
            OsStr::new("-vf"),
            OsStr::new("format=rgb24,crop=1:1:0:0"),
            OsStr::new("-f"),
            OsStr::new("rawvideo"),
            OsStr::new("-"),
        ])
        .creation_flags(ffmpeg::CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("could not inspect output: {error}"))?;

    if !output.status.success() || output.stdout.len() < 3 {
        return Err("could not inspect output pixels".to_string());
    }

    Ok(output.stdout[0] > 200 && output.stdout[1] > 200 && output.stdout[2] > 200)
}
