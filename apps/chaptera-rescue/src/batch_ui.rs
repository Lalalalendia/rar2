//! Optional local batch controls for Chaptera Rescue's desktop technical preview.
//!
//! A matching portable release auto-detects its sibling `rescue-engine.exe`;
//! a development checkout may explicitly select Python and the rar2 directory.
//! Both routes call the same source-verified worker, outside GUI memory.

use eframe::egui;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use crate::engine_job::EngineJob;

const BATCH_REPORT_SCHEMA: &str = "chaptera.rescue-desktop-batch-report.v1";
const MAX_REPORT_BYTES: u64 = 2 * 1024 * 1024;
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

// Unique run identity, not a security credential. Controllers must carry this
// exact token so delayed cancel/pause requests cannot affect a successor run.
fn next_run_token() -> String {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("gui-{:x}-{stamp:x}-{sequence:x}", std::process::id())
}

#[derive(Debug, Deserialize)]
struct BatchRow {
    sha256: String,
    status: String,
    outcome: Option<String>,
    reason: Option<String>,
    stream_count: Option<usize>,
    asset_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct BatchReport {
    schema_version: String,
    source_free: bool,
    unique_sources: usize,
    duplicate_paths: usize,
    counts: BTreeMap<String, usize>,
    entries: Vec<BatchRow>,
}

pub struct BatchUi {
    input_path: String,
    output_path: String,
    repo_root: String,
    python_path: String,
    asset_cli: String,
    workers: usize,
    active_action: Option<&'static str>,
    active_process: Option<Child>,
    control_process: Option<Child>,
    run_token: Option<String>,
    status: String,
    report: Option<BatchReport>,
    filter: String,
}

impl Default for BatchUi {
    fn default() -> Self {
        Self {
            input_path: String::new(),
            output_path: String::new(),
            repo_root: String::new(),
            python_path: "python".to_owned(),
            asset_cli: String::new(),
            workers: 2,
            active_action: None,
            active_process: None,
            control_process: None,
            run_token: None,
            status: "No local batch has been selected".to_owned(),
            report: None,
            filter: String::new(),
        }
    }
}

impl BatchUi {
    pub fn use_input(&mut self, path: &Path) {
        self.input_path = path.to_string_lossy().to_string();
    }

    fn engine_root(&self) -> Result<PathBuf, String> {
        let root = if self.repo_root.trim().is_empty() {
            std::env::current_dir().map_err(|_| "Select an existing rar2 checkout".to_owned())?
        } else {
            PathBuf::from(self.repo_root.trim())
        };
        if !root.join("tools/rescue_desktop_batch_v1.py").is_file()
            || !root.join("tools/rescue_salvage_bundle_v1.py").is_file()
        {
            return Err("Choose a local rar2 checkout with the Rescue batch engine".to_owned());
        }
        Ok(root)
    }

    fn portable_engine() -> Option<PathBuf> {
        let gui = std::env::current_exe().ok()?;
        let engine = gui.parent()?.join("rescue-engine.exe");
        if engine.is_file() && !engine.is_symlink() {
            Some(engine)
        } else {
            None
        }
    }

    fn command_for(&self, action: &str) -> Result<Command, String> {
        let packaged = if self.repo_root.trim().is_empty() {
            Self::portable_engine()
        } else {
            None
        };
        self.command_for_engine(action, packaged.as_deref())
    }

    fn command_for_engine(&self, action: &str, packaged: Option<&Path>) -> Result<Command, String> {
        let output = self.output_path.trim();
        if output.is_empty() {
            return Err("Set a dedicated output folder, outside the source tree".to_owned());
        }
        if !Path::new(output).is_absolute() {
            return Err("Use an absolute output folder path".to_owned());
        }
        let mut cmd = if let Some(engine) = packaged {
            let mut command = Command::new(engine);
            if let Some(folder) = engine.parent() {
                command.current_dir(folder);
            }
            command
        } else {
            let root = self.engine_root()?;
            let python = self.python_path.trim();
            if python.is_empty() {
                return Err("Set the local Python interpreter".to_owned());
            }
            let mut command = Command::new(python);
            command
                .current_dir(root)
                .arg("-m")
                .arg("tools.rescue_desktop_batch_v1");
            command
        };
        cmd.arg(action)
            .arg("--output")
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            // Prevent a console flash for each frozen Python worker/control
            // process spawned by the GUI. Direct CLI users still see output.
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        match action {
            "scan" => {
                if self.input_path.trim().is_empty() {
                    return Err("Drop a PUB or folder, or provide its local path".to_owned());
                }
                if !Path::new(self.input_path.trim()).is_absolute() {
                    return Err("Use an absolute source file/folder path".to_owned());
                }
                cmd.arg("--input").arg(self.input_path.trim());
                if Path::new(output).join("queue-private.json").is_file() {
                    cmd.arg("--append");
                }
            }
            "run" => {
                cmd.arg("--workers")
                    .arg(self.workers.clamp(1, 4).to_string());
                let asset_cli = if !self.asset_cli.trim().is_empty() {
                    Some(PathBuf::from(self.asset_cli.trim()))
                } else {
                    packaged.and_then(|engine| {
                        let bundled_cli = engine.with_file_name("pub.exe");
                        bundled_cli.is_file().then_some(bundled_cli)
                    })
                };
                if let Some(asset_cli) = asset_cli {
                    if !asset_cli.is_absolute() {
                        return Err("Use an absolute path to the optional asset CLI".to_owned());
                    }
                    cmd.arg("--pub-cli").arg(asset_cli);
                }
            }
            "retry" => {}
            "pause" | "cancel" => {
                if let Some(token) = &self.run_token {
                    cmd.arg("--expected-run-token").arg(token);
                }
            }
            _ => return Err("Unknown batch action".to_owned()),
        }
        Ok(cmd)
    }

    fn begin(&mut self, action: &'static str) {
        if self.active_process.is_some() || self.control_process.is_some() {
            self.status = "A local batch command is already active".to_owned();
            return;
        }
        let mut command = match self.command_for(action) {
            Ok(command) => command,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let token = (action == "run").then(next_run_token);
        if let Some(token) = &token {
            command.arg("--run-token").arg(token);
        }
        // The process-wide GUI Job must be installed before CreateProcess, so
        // neither this child nor its grandchildren have any unguarded launch
        // window. The existing Python Job adds a nested per-run memory limit.
        #[cfg(windows)]
        if !EngineJob::gui_is_guarded() {
            self.status = "Refusing batch launch without GUI lifetime Job".to_owned();
            return;
        }
        let child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                self.status = "Cannot launch local Rescue engine".to_owned();
                return;
            }
        };
        self.active_process = Some(child);
        self.active_action = Some(action);
        self.run_token = token;
        self.status = format!("{action} started in a GUI-owned local process");
    }

    fn request_control(&mut self, action: &'static str) {
        if self.active_action != Some("run")
            || self.active_process.is_none()
            || self.control_process.is_some()
        {
            return;
        }
        let mut command = match self.command_for(action) {
            Ok(command) => command,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        match command.spawn() {
            Ok(child) => {
                self.control_process = Some(child);
                self.status = format!("Requesting {action} for this exact batch run");
            }
            Err(_) => self.status = "Cannot send batch control command".to_owned(),
        }
    }

    fn refresh_report(&mut self) {
        if self.output_path.trim().is_empty() {
            self.report = None;
            return;
        }
        let path = Path::new(self.output_path.trim()).join("summary.json");
        let Ok(meta) = fs::metadata(&path) else {
            return;
        };
        if meta.len() > MAX_REPORT_BYTES || !meta.is_file() {
            self.status = "Batch summary exceeds local read limit".to_owned();
            return;
        }
        let Ok(raw) = fs::read(&path) else {
            return;
        };
        if let Ok(report) = serde_json::from_slice::<BatchReport>(&raw) {
            if report.source_free
                && report.schema_version == BATCH_REPORT_SCHEMA
                && report.entries.len() <= 10000
                && report.entries.iter().all(|entry| {
                    entry.sha256.len() == 64
                        && entry.sha256.bytes().all(|value| value.is_ascii_hexdigit())
                })
            {
                self.report = Some(report);
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let received = self
            .active_process
            .as_mut()
            .and_then(|child| match child.try_wait() {
                Ok(Some(exit)) => Some(Ok(exit.code().unwrap_or(-1))),
                Ok(None) => None,
                Err(_) => Some(Err("Cannot inspect local batch engine".to_owned())),
            });
        if let Some(result) = received {
            let action = self.active_action.take().unwrap_or("command");
            if let Some(mut child) = self.active_process.take() {
                if result.is_err() {
                    let _ = child.kill();
                }
                let _ = child.wait();
            }
            self.run_token = None;
            self.status = match result {
                Ok(0) => format!("{action}: completed (see per-file outcomes below)"),
                Ok(1) => format!("{action}: incomplete, paused, or some files failed"),
                Ok(2) => format!("{action}: rejected unsafe input or invalid configuration"),
                Ok(code) => format!("{action}: local process exited with code {code}"),
                Err(error) => error,
            };
        }

        let controlled = self
            .control_process
            .as_mut()
            .and_then(|child| match child.try_wait() {
                Ok(Some(exit)) => Some(Ok(exit.code().unwrap_or(-1))),
                Ok(None) => None,
                Err(_) => Some(Err("Cannot inspect local control command".to_owned())),
            });
        if let Some(result) = controlled {
            if let Some(mut child) = self.control_process.take() {
                if result.is_err() {
                    let _ = child.kill();
                }
                let _ = child.wait();
            }
            // A late control failure must not overwrite the final result of a
            // batch that already finished. It never counts as cancellation.
            if !matches!(result, Ok(0)) && self.active_process.is_some() {
                self.status = "Pause/cancel request could not be delivered".to_owned();
            }
        }
        self.refresh_report();
        if self.active_process.is_some() || self.control_process.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use crate::ui_theme;
        use egui::RichText;

        self.poll(ctx);
        let busy = self.active_process.is_some() || self.control_process.is_some();
        let can_control = self.active_action == Some("run")
            && self.active_process.is_some()
            && self.control_process.is_none();

        ui_theme::kicker(ui, "OFFLINE WORKSPACE / FILE RECOVERY");
        ui_theme::headline(
            ui,
            "Batch recovery",
            "Scan Publisher files, extract proven data and review exactly what was recovered.",
        );
        ui.add_space(18.0);

        ui_theme::card(ui, |ui| {
            ui.label(RichText::new("01  /  SELECT FILES").size(12.0).strong().color(ui_theme::accent()));
            ui.label(RichText::new("Your originals stay untouched").size(18.0).strong().color(ui_theme::ink()));
            ui.label(
                RichText::new("Drop a PUB or folder into the window, or enter an absolute local path.")
                    .size(12.0).color(ui_theme::muted()),
            );
            ui.add_space(6.0);
            ui.label(RichText::new("SOURCE PUB OR FOLDER").size(11.0).strong().color(ui_theme::muted()));
            let width = ui.available_width();
            ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.input_path)
                    .desired_width(width)
                    .hint_text("C:\\Documents\\Publications"),
            );
            ui.label(RichText::new("OUTPUT FOLDER").size(11.0).strong().color(ui_theme::muted()));
            let width = ui.available_width();
            if ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.output_path)
                    .desired_width(width)
                    .hint_text("C:\\Rescue\\Results"),
            ).changed() {
                // Never show a cached receipt for another output directory.
                self.report = None;
            }
            ui.label(
                RichText::new("Results go to a separate folder outside the input tree.")
                    .size(11.0).color(ui_theme::muted()),
            );
        });

        ui.add_space(14.0);
        ui_theme::card(ui, |ui| {
            ui.label(RichText::new("02  /  PROCESS").size(12.0).strong().color(ui_theme::accent()));
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Scan / add folder")).clicked() {
                    self.begin("scan");
                }
                if ui.add_enabled(
                    !busy,
                    egui::Button::new(
                        RichText::new("Run / resume").strong().color(egui::Color32::WHITE),
                    ).fill(ui_theme::accent()),
                ).clicked() {
                    self.begin("run");
                }
                if ui.add_enabled(!busy, egui::Button::new("Retry failed")).clicked() {
                    self.begin("retry");
                }
                if ui.add_enabled(can_control, egui::Button::new("Pause")).clicked() {
                    self.request_control("pause");
                }
                if ui.add_enabled(can_control, egui::Button::new("Cancel")).clicked() {
                    self.request_control("cancel");
                }
                if ui.button("Refresh results").clicked() {
                    self.refresh_report();
                }
            });
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if busy {
                    ui.spinner();
                    ui_theme::pill(ui, "PROCESSING", ui_theme::accent());
                } else {
                    ui_theme::pill(ui, "LOCAL ENGINE", ui_theme::success());
                }
                ui.label(RichText::new(&self.status).size(12.0).color(ui_theme::ink()));
            });
        });

        ui.add_space(14.0);
        if let Some(report) = &self.report {
            ui_theme::card(ui, |ui| {
                ui.label(RichText::new("03  /  RESULTS").size(12.0).strong().color(ui_theme::accent()));
                ui.label(RichText::new("Verified queue overview").size(18.0).strong().color(ui_theme::ink()));
                ui.add_space(4.0);
                let partial = *report.counts.get("done_partial").unwrap_or(&0);
                let unsupported = *report.counts.get("done_unsupported").unwrap_or(&0);
                let failed = *report.counts.get("failed").unwrap_or(&0)
                    + *report.counts.get("cancelled").unwrap_or(&0);
                ui.columns(4, |cols| {
                    ui_theme::metric(&mut cols[0], "Unique PUB", report.unique_sources, ui_theme::ink());
                    ui_theme::metric(&mut cols[1], "Partial recovery", partial, ui_theme::success());
                    ui_theme::metric(&mut cols[2], "Unsupported", unsupported, ui_theme::warning());
                    ui_theme::metric(&mut cols[3], "Needs attention", failed, ui_theme::danger());
                });
                let finished = partial + unsupported + failed;
                let progress = if report.unique_sources == 0 {
                    0.0
                } else {
                    (finished as f32 / report.unique_sources as f32).clamp(0.0, 1.0)
                };
                ui.add(
                    egui::ProgressBar::new(progress)
                        .fill(ui_theme::accent())
                        .show_percentage(),
                );
                ui.label(
                    RichText::new(format!(
                        "{finished} of {} files in a terminal state  ·  {} duplicate paths",
                        report.unique_sources, report.duplicate_paths,
                    )).size(11.0).color(ui_theme::muted()),
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("RESULTS  /  SHA-256").size(11.0).strong().color(ui_theme::muted()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text("Filter hash or status")
                            .desired_width(220.0),
                    );
                });
                egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    for entry in report.entries.iter().filter(|entry| {
                        self.filter.trim().is_empty()
                            || entry.sha256.contains(self.filter.trim())
                            || entry.status.contains(self.filter.trim())
                    }).take(100) {
                        let short_hash = &entry.sha256[..entry.sha256.len().min(12)];
                        let outcome = entry.outcome.as_deref()
                            .or(entry.reason.as_deref()).unwrap_or("Not processed");
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(short_hash).monospace().size(12.0)
                                    .color(ui_theme::ink()),
                            );
                            let tone = match entry.status.as_str() {
                                "done_partial" => ui_theme::success(),
                                "done_unsupported" => ui_theme::warning(),
                                "failed" | "cancelled" => ui_theme::danger(),
                                "running" => ui_theme::accent(),
                                _ => ui_theme::muted(),
                            };
                            ui.label(RichText::new(&entry.status).strong().size(12.0).color(tone));
                            ui.label(
                                RichText::new(format!(
                                    "{}  ·  {} streams  ·  {} assets",
                                    outcome, entry.stream_count.unwrap_or(0),
                                    entry.asset_count.unwrap_or(0),
                                )).size(12.0).color(ui_theme::muted()),
                            );
                        });
                        ui.separator();
                    }
                });
            });
        } else {
            ui_theme::card(ui, |ui| {
                ui.label(RichText::new("03  /  RESULTS").size(12.0).strong().color(ui_theme::accent()));
                ui.label(RichText::new("Your results will appear here").size(18.0).strong().color(ui_theme::ink()));
                ui.label(
                    RichText::new("Run a scan to see verified SHA identities, outcomes, streams and errors. No results are inferred before the local engine reports them.")
                        .size(12.0).color(ui_theme::muted()),
                );
            });
        }

        ui.add_space(14.0);
        ui_theme::card(ui, |ui| {
            egui::CollapsingHeader::new("Advanced  /  local engine settings")
                .default_open(false)
                .show(ui, |ui| {
                    if self.repo_root.trim().is_empty() && Self::portable_engine().is_some() {
                        ui.label("Portable engine detected. Python and checkout are not needed.");
                    } else {
                        ui.label("Development mode: choose a rar2 checkout and Python interpreter.");
                    }
                    ui.label(RichText::new("RAR2 CHECKOUT").size(11.0).strong().color(ui_theme::muted()));
                    let width = ui.available_width();
                    ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.repo_root)
                        .desired_width(width).hint_text("Optional checkout path"));
                    ui.label(RichText::new("PYTHON").size(11.0).strong().color(ui_theme::muted()));
                    let width = ui.available_width();
                    ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.python_path)
                        .desired_width(width));
                    ui.label(RichText::new("OPTIONAL PUB CLI").size(11.0).strong().color(ui_theme::muted()));
                    let width = ui.available_width();
                    ui.add_enabled(!busy, egui::TextEdit::singleline(&mut self.asset_cli)
                        .desired_width(width));
                    ui.add_enabled(!busy, egui::Slider::new(&mut self.workers, 1..=4)
                        .text("Parallel files"));
                    ui.label(
                        RichText::new("All processing is offline; no source data is uploaded.")
                            .size(11.0).color(ui_theme::muted()),
                    );
                });
        });
    }
}

impl Drop for BatchUi {
    fn drop(&mut self) {
        if self.active_action == Some("run") && self.active_process.is_some() {
            // Ask the canonical engine to stop; never write its private queue
            // or control JSON directly from Rust. Wait a bounded grace period.
            if let Ok(mut command) = self.command_for("cancel") {
                if let Ok(mut control) = command.spawn() {
                    let deadline = Instant::now() + Duration::from_secs(2);
                    while Instant::now() < deadline {
                        let done = self
                            .active_process
                            .as_mut()
                            .and_then(|child| child.try_wait().ok())
                            .flatten()
                            .is_some();
                        if done {
                            break;
                        }
                        thread::sleep(Duration::from_millis(25));
                    }
                    if control.try_wait().ok().flatten().is_none() {
                        let _ = control.kill();
                    }
                    let _ = control.wait();
                }
            }
        }
        // On Windows the static GUI Job persists until process exit, so all
        // grandchildren are bounded even if orderly cancellation fails.
        // The engine's own memory Job closes on engine exit and reaps workers.
        if let Some(mut child) = self.active_process.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        // POSIX development mode has only a cooperative engine cancellation
        // plus parent kill fallback; unlike Windows it does not guarantee
        // crash-time termination of children in their own process sessions.
        if let Some(mut control) = self.control_process.take() {
            if control.try_wait().ok().flatten().is_none() {
                let _ = control.kill();
            }
            let _ = control.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gui_batch_command_uses_explicit_local_python_engine() {
        let mut surface = BatchUi::default();
        surface.repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .to_string_lossy()
            .to_string();
        surface.output_path = std::env::temp_dir()
            .join("rescue-output")
            .to_string_lossy()
            .to_string();
        surface.input_path = std::env::temp_dir()
            .join("private-folder")
            .to_string_lossy()
            .to_string();
        let command = surface.command_for("scan").expect("local engine command");
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args[0], "-m");
        assert_eq!(args[1], "tools.rescue_desktop_batch_v1");
        assert_eq!(args[2], "scan");
        assert!(args.contains(&"--input".to_owned()));
        assert!(args.contains(&"--output".to_owned()));
        assert!(!args.contains(&"--pub-cli".to_owned()));
    }

    #[test]
    fn packaged_engine_bypasses_python_and_checkout() {
        let mut surface = BatchUi::default();
        surface.output_path = std::env::temp_dir()
            .join("rescue-portable-output")
            .to_string_lossy()
            .to_string();
        surface.input_path = std::env::temp_dir()
            .join("test.pub")
            .to_string_lossy()
            .to_string();
        surface.python_path.clear();
        let engine = std::env::temp_dir()
            .join("rescuedir")
            .join("rescue-engine.exe");
        let command = surface
            .command_for_engine("scan", Some(&engine))
            .expect("bundled engine command");
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(command.get_program(), engine.as_os_str());
        assert_eq!(args[0], "scan");
        assert!(args.contains(&"--output".to_owned()));
        assert!(!args.contains(&"-m".to_owned()));
    }

    #[test]
    fn cancel_command_is_pinned_to_the_active_run() {
        let mut surface = BatchUi::default();
        surface.output_path = std::env::temp_dir()
            .join("rescue-control-token")
            .to_string_lossy()
            .to_string();
        surface.run_token = Some("gui-run-pinned-0001".to_owned());
        let engine = std::env::temp_dir().join("rescue-engine.exe");
        let command = surface
            .command_for_engine("cancel", Some(&engine))
            .expect("control command");
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert!(
            args.windows(2).any(|pair| {
                pair[0] == "--expected-run-token" && pair[1] == "gui-run-pinned-0001"
            })
        );
    }

    #[test]
    fn run_tokens_are_distinct_between_invocations() {
        assert_ne!(next_run_token(), next_run_token());
    }

    #[test]
    fn cannot_start_without_explicit_output() {
        let mut surface = BatchUi::default();
        surface.repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .to_string_lossy()
            .to_string();
        assert!(surface.command_for("run").is_err());
    }

    #[test]
    fn source_free_batch_summary_is_only_admitted_schema() {
        let sample = r#"{"schema_version":"chaptera.rescue-desktop-batch-report.v1","source_free":true,"unique_sources":1,"duplicate_paths":0,"counts":{"pending":1},"entries":[]}"#;
        let report: BatchReport = serde_json::from_str(sample).expect("known report schema");
        assert!(report.source_free);
        assert_eq!(report.schema_version, BATCH_REPORT_SCHEMA);
        assert_eq!(report.unique_sources, 1);
    }
}
