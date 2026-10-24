//! MP3 & SRT Maker — batch desktop app that turns video files into MP3 audio and/or
//! SRT closed captions (via Whisper, using whisper.cpp). Each table row is an
//! independent job that can be started, stopped, and removed.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

fn format_timestamp(ms: i64) -> String {
    let hrs = ms / 3_600_000;
    let mins = (ms % 3_600_000) / 60_000;
    let secs = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{hrs:02}:{mins:02}:{secs:02},{millis:03}")
}

/// Segment times are in milliseconds.
fn build_srt(segments: &[(i64, i64, String)]) -> String {
    segments
        .iter()
        .enumerate()
        .map(|(i, (start, end, text))| {
            format!(
                "{}\n{} --> {}\n{}\n",
                i + 1,
                format_timestamp(*start),
                format_timestamp(*end),
                text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Why a job ended early: the user pressed Stop, or something failed.
enum JobError {
    Cancelled,
    Failed(String),
}

impl<E: std::fmt::Display> From<E> for JobError {
    fn from(e: E) -> Self {
        JobError::Failed(e.to_string())
    }
}

type JobResult<T> = Result<T, JobError>;

/// job_id -> cancel flag
#[derive(Default)]
struct Jobs(Mutex<HashMap<String, Arc<AtomicBool>>>);

/// Serializes model downloads so concurrent rows don't fetch the same file twice.
static DOWNLOAD_LOCK: Mutex<()> = Mutex::new(());

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobArgs {
    job_id: String,
    video_path: String,
    output_dir: String,
    model_name: String,
    language: String,
    create_srt: bool,
    create_mp3: bool,
    new_folder: bool,
    move_video: bool,
}

#[derive(Serialize, Default)]
struct DoneData {
    job_id: String,
    status: &'static str,
    message: &'static str,
    srt_path: String,
    mp3_path: String,
    video_path: String,
    moved_video: bool,
    transcript: String,
    language: String,
    segments: usize,
    has_srt: bool,
    percent: f64,
}

/// Per-job context handed to the worker thread.
struct Job {
    app: AppHandle,
    id: String,
    cancel: Arc<AtomicBool>,
}

impl Job {
    fn progress(&self, status: &str, message: &str, percent: Option<f64>) {
        let _ = self.app.emit(
            "progress",
            serde_json::json!({
                "job_id": self.id, "status": status,
                "message": message, "percent": percent,
            }),
        );
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn check_cancel(&self) -> JobResult<()> {
        if self.cancelled() {
            Err(JobError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// Run an ffmpeg command, killing it if the job is stopped.
    fn run_ff(&self, args: &[&str]) -> JobResult<()> {
        let mut child = Command::new(ffmpeg_path())
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| JobError::Failed(format!("ffmpeg not found ({e}). Install ffmpeg first.")))?;
        loop {
            if let Some(status) = child.try_wait()? {
                self.check_cancel()?;
                if !status.success() {
                    return Err(JobError::Failed(format!("ffmpeg failed ({status})")));
                }
                return Ok(());
            }
            if self.cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(JobError::Cancelled);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Prefer the ffmpeg sidecar Tauri places next to the app executable. Failing
/// that, GUI apps on macOS don't inherit the shell PATH, so look in the usual
/// Homebrew locations before falling back to plain `ffmpeg`.
fn ffmpeg_path() -> PathBuf {
    let sidecar = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(format!("ffmpeg{}", std::env::consts::EXE_SUFFIX))));
    if let Some(p) = sidecar.filter(|p| p.is_file()) {
        return p;
    }
    for p in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"] {
        if Path::new(p).is_file() {
            return PathBuf::from(p);
        }
    }
    PathBuf::from("ffmpeg")
}

#[tauri::command]
fn start_job(app: AppHandle, jobs: State<Jobs>, args: JobArgs) {
    let cancel = Arc::new(AtomicBool::new(false));
    jobs.0.lock().unwrap().insert(args.job_id.clone(), cancel.clone());
    let job = Job { app, id: args.job_id.clone(), cancel };
    thread::spawn(move || run_job(job, args));
}

#[tauri::command]
fn stop_job(jobs: State<Jobs>, job_id: String) {
    if let Some(cancel) = jobs.0.lock().unwrap().get(&job_id) {
        cancel.store(true, Ordering::Relaxed);
    }
}

fn run_job(job: Job, args: JobArgs) {
    let mut temp_audio = None;
    let result = process(&job, &args, &mut temp_audio);
    if let Some(p) = temp_audio {
        let _ = fs::remove_file(p);
    }
    job.app.state::<Jobs>().0.lock().unwrap().remove(&job.id);

    let payload = match result {
        Ok(done) => serde_json::to_value(done).unwrap(),
        Err(JobError::Cancelled) => {
            serde_json::json!({"job_id": job.id, "status": "stopped", "message": "Stopped"})
        }
        Err(JobError::Failed(msg)) => {
            serde_json::json!({"job_id": job.id, "status": "error", "message": msg})
        }
    };
    let _ = job.app.emit("progress", payload);
}

fn process(job: &Job, args: &JobArgs, temp_audio: &mut Option<PathBuf>) -> JobResult<DoneData> {
    job.check_cancel()?;
    let video = PathBuf::from(&args.video_path);
    let stem = video
        .file_stem()
        .ok_or_else(|| JobError::Failed("Invalid video path".into()))?
        .to_string_lossy()
        .to_string();

    // Resolve output directory. Optionally nest in a folder named after the video file.
    let mut out_dir = if args.output_dir.is_empty() {
        video.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        PathBuf::from(&args.output_dir)
    };
    if args.new_folder {
        out_dir.push(&stem);
    }
    fs::create_dir_all(&out_dir)?;

    // Step 1: MP3.
    let mut mp3_path = None;
    if args.create_mp3 {
        let p = out_dir.join(format!("{stem}.mp3"));
        job.progress("extracting", "Creating MP3...", Some(4.0));
        job.run_ff(&[
            "-i", &args.video_path, "-vn", "-ar", "44100", "-ac", "2",
            "-b:a", "192k", &p.to_string_lossy(), "-y",
        ])?;
        mp3_path = Some(p);
    }

    // Step 2: SRT (optional). Skips the whole Whisper pipeline when off.
    let mut done = DoneData { job_id: job.id.clone(), language: "unknown".into(), ..Default::default() };
    if args.create_srt {
        job.check_cancel()?;
        // Whisper wants 16 kHz mono f32 PCM; decode to a raw temp file.
        let raw = std::env::temp_dir().join(format!("mp3srt-{}-{}.f32", std::process::id(), job.id));
        *temp_audio = Some(raw.clone());
        job.progress("extracting", "Extracting audio...", Some(6.0));
        job.run_ff(&[
            "-i", &args.video_path, "-vn", "-ar", "16000", "-ac", "1",
            "-f", "f32le", &raw.to_string_lossy(), "-y",
        ])?;
        let bytes = fs::read(&raw)?;
        let audio: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        if audio.is_empty() {
            return Err(JobError::Failed("Video has no audio".into()));
        }

        let model_path = ensure_model(job, &args.model_name)?;
        job.progress("loading", &format!("Loading model '{}'...", args.model_name), Some(10.0));
        job.check_cancel()?;
        let ctx = WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        )?;
        let mut state = ctx.create_state()?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let lang = if args.language.is_empty() { "auto" } else { args.language.as_str() };
        params.set_language(Some(lang));
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // transcribe (real progress from whisper.cpp, 10% -> 95%).
        let app = job.app.clone();
        let id = job.id.clone();
        params.set_progress_callback_safe(move |p: i32| {
            let pct = 10.0 + p as f64 * 0.85;
            let _ = app.emit(
                "progress",
                serde_json::json!({
                    "job_id": id, "status": "transcribing",
                    "message": format!("Transcribing... {}%", pct as i32), "percent": pct,
                }),
            );
        });
        let cancel = job.cancel.clone();
        params.set_abort_callback_safe(move || cancel.load(Ordering::Relaxed));

        job.progress("transcribing", "Transcribing...", Some(10.0));
        let res = state.full(params, &audio);
        job.check_cancel()?;
        res?;

        // write SRT
        job.progress("writing", "Writing SRT...", Some(96.0));
        let segments: Vec<(i64, i64, String)> = state
            .as_iter()
            .map(|s| {
                let text = s.to_str_lossy().map(|t| t.to_string()).unwrap_or_default();
                (s.start_timestamp() * 10, s.end_timestamp() * 10, text)
            })
            .collect();
        let srt_path = out_dir.join(format!("{stem}.srt"));
        fs::write(&srt_path, build_srt(&segments))?;

        done.transcript = segments.iter().map(|s| s.2.trim()).collect::<Vec<_>>().join(" ");
        if let Some(l) = whisper_rs::get_lang_str(state.full_lang_id_from_state()) {
            done.language = l.to_string();
        }
        done.segments = segments.len();
        done.has_srt = true;
        done.srt_path = srt_path.to_string_lossy().to_string();
    }

    // Optionally move the source video into the new folder, after the outputs
    // exist. Only applies when a new folder was created.
    done.video_path = args.video_path.clone();
    if args.move_video && args.new_folder {
        let dest = out_dir.join(video.file_name().unwrap());
        if fs::canonicalize(&video).ok() != fs::canonicalize(&dest).ok() {
            job.progress("moving", "Moving video...", Some(98.0));
            if fs::rename(&video, &dest).is_err() {
                // Cross-device: copy then delete.
                fs::copy(&video, &dest)?;
                fs::remove_file(&video)?;
            }
            done.video_path = dest.to_string_lossy().to_string();
            done.moved_video = true;
        }
    }

    done.status = "done";
    done.message = "Complete";
    done.mp3_path = mp3_path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    done.percent = 100.0;
    Ok(done)
}

/// Return the local path of a ggml Whisper model, downloading it on first use.
fn ensure_model(job: &Job, name: &str) -> JobResult<PathBuf> {
    let dir = job.app.path().app_data_dir()?.join("models");
    let path = dir.join(format!("ggml-{name}.bin"));
    if path.is_file() {
        return Ok(path);
    }

    job.progress("downloading", "Waiting for model download...", Some(8.0));
    let _guard = DOWNLOAD_LOCK.lock().unwrap();
    if path.is_file() {
        return Ok(path); // another row finished downloading it
    }
    fs::create_dir_all(&dir)?;

    let mut resp = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()?
        .get(format!("{MODEL_URL}/ggml-{name}.bin"))
        .send()?
        .error_for_status()?;
    let total = resp.content_length().unwrap_or(0);
    let part = path.with_extension("part");
    let mut file = fs::File::create(&part)?;
    let mut buf = vec![0u8; 1 << 16];
    let (mut got, mut last_pct) = (0u64, -1i64);
    loop {
        if job.cancelled() {
            drop(file);
            let _ = fs::remove_file(&part);
            return Err(JobError::Cancelled);
        }
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        got += n as u64;
        let pct = if total > 0 { (got * 100 / total) as i64 } else { 0 };
        if pct != last_pct {
            last_pct = pct;
            job.progress(
                "downloading",
                &format!("Downloading model '{name}'... {pct}%"),
                Some(8.0),
            );
        }
    }
    file.flush()?;
    drop(file);
    fs::rename(&part, &path)?;
    Ok(path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Jobs::default())
        .invoke_handler(tauri::generate_handler![start_job, stop_job])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(format_timestamp(0), "00:00:00,000");
        assert_eq!(format_timestamp(3_723_456), "01:02:03,456");
    }

    #[test]
    fn srt() {
        let s = build_srt(&[(0, 1500, " Hi ".into()), (1500, 3000, "there".into())]);
        assert_eq!(s, "1\n00:00:00,000 --> 00:00:01,500\nHi\n\n2\n00:00:01,500 --> 00:00:03,000\nthere\n");
    }
}
