//! Optional local batch controls for Chaptera Rescue's desktop technical preview.
//!
//! The executable intentionally does not bundle a Python runtime yet: the
//! operator chooses an existing rar2 checkout and interpreter. This delegates
//! recovery to the existing verified local salvage engine, not to GUI memory.

use eframe::egui;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

const BATCH_REPORT_SCHEMA: &str = "chaptera.rescue-desktop-batch-report.v1";
const MAX_REPORT_BYTES: u64 = 2 * 1024 * 1024;

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
    receiver: Option<Receiver<Result<i32, String>>>,
    control_receiver: Option<Receiver<Result<i32, String>>>,
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
            receiver: None,
            control_receiver: None,
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

    fn command_for(&self, action: &str) -> Result<Command, String> {
        let root = self.engine_root()?;
        let output = self.output_path.trim();
        if output.is_empty() {
            return Err("Set a dedicated output folder, outside the source tree".to_owned());
        }
        if !Path::new(output).is_absolute() {
            return Err("Use an absolute output folder path".to_owned());
        }
        let python = self.python_path.trim();
        if python.is_empty() {
            return Err("Set the local Python interpreter".to_owned());
        }
        let mut cmd = Command::new(python);
        cmd.current_dir(root)
            .arg("-m")
            .arg("tools.rescue_desktop_batch_v1")
            .arg(action)
            .arg("--output")
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
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
                if !self.asset_cli.trim().is_empty() {
                    if !Path::new(self.asset_cli.trim()).is_absolute() {
                        return Err("Use an absolute path to the optional asset CLI".to_owned());
                    }
                    cmd.arg("--pub-cli").arg(self.asset_cli.trim());
                }
            }
            "retry" | "pause" | "cancel" => {}
            _ => return Err("Unknown batch action".to_owned()),
        }
        Ok(cmd)
    }

    fn begin(&mut self, action: &'static str) {
        if self.receiver.is_some() {
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
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = command
                .status()
                .map(|exit| exit.code().unwrap_or(-1))
                .map_err(|_| "Cannot launch local Python Rescue engine".to_owned());
            let _ = sender.send(result);
        });
        self.receiver = Some(receiver);
        self.active_action = Some(action);
        self.status = format!("{action} started in a separate local process");
    }

    fn request_control(&mut self, action: &'static str) {
        if self.active_action != Some("run") || self.control_receiver.is_some() {
            return;
        }
        let mut command = match self.command_for(action) {
            Ok(command) => command,
            Err(error) => {
                self.status = error;
                return;
            }
        };
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = command
                .status()
                .map(|exit| exit.code().unwrap_or(-1))
                .map_err(|_| "Cannot send batch control command".to_owned());
            let _ = sender.send(result);
        });
        self.control_receiver = Some(receiver);
        self.status = format!("Requesting {action} for the active local batch");
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
        let received = self.receiver.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(v) => Some(v),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("Batch runner disconnected unexpectedly".to_owned()))
            }
        });
        if let Some(result) = received {
            let action = self.active_action.take().unwrap_or("command");
            self.receiver = None;
            self.status = match result {
                Ok(0) => format!("{action}: completed (see per-file outcomes below)"),
                Ok(1) => format!("{action}: incomplete, paused, or some files failed"),
                Ok(2) => format!("{action}: rejected unsafe input or invalid configuration"),
                Ok(code) => format!("{action}: local process exited with code {code}"),
                Err(error) => error,
            };
        }
        let controlled = self.control_receiver.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(v) => Some(v),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                Some(Err("Local control command disconnected".to_owned()))
            }
        });
        if let Some(result) = controlled {
            self.control_receiver = None;
            if !matches!(result, Ok(0)) {
                self.status = "Pause/cancel request could not be delivered".to_owned();
            }
        }
        self.refresh_report();
        if self.receiver.is_some() || self.control_receiver.is_some() {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }

    pub fn draw(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.poll(ctx);
        let busy = self.receiver.is_some();
        ui.group(|ui| {
            ui.strong("4. Local batch salvage (preview)");
            ui.label("Drag a PUB/folder or type a path. Requires a local rar2 checkout and Python; no cloud upload.");
            ui.horizontal(|ui| {
                ui.label("PUB / folder");
                ui.text_edit_singleline(&mut self.input_path);
            });
            ui.horizontal(|ui| {
                ui.label("Output folder");
                ui.text_edit_singleline(&mut self.output_path);
            });
            ui.horizontal(|ui| {
                ui.label("rar2 checkout");
                ui.text_edit_singleline(&mut self.repo_root);
            });
            ui.horizontal(|ui| {
                ui.label("Python");
                ui.text_edit_singleline(&mut self.python_path);
                ui.label("Optional pub CLI");
                ui.text_edit_singleline(&mut self.asset_cli);
            });
            ui.add(egui::Slider::new(&mut self.workers, 1..=4).text("Parallel files"));
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Scan / add folder")).clicked() {
                    self.begin("scan");
                }
                if ui.add_enabled(!busy, egui::Button::new("Run / resume")).clicked() {
                    self.begin("run");
                }
                if ui.add_enabled(!busy, egui::Button::new("Retry failed")).clicked() {
                    self.begin("retry");
                }
                if ui.add_enabled(self.active_action == Some("run"), egui::Button::new("Pause")).clicked() {
                    self.request_control("pause");
                }
                if ui.add_enabled(self.active_action == Some("run"), egui::Button::new("Cancel")).clicked() {
                    self.request_control("cancel");
                }
                if ui.button("Refresh").clicked() {
                    self.refresh_report();
                }
            });
            ui.label(&self.status);
            if let Some(report) = &self.report {
                ui.separator();
                ui.label(format!("Unique PUB: {}  |  Duplicate paths: {}", report.unique_sources, report.duplicate_paths));
                for (state, count) in &report.counts {
                    ui.label(format!("{state}: {count}"));
                }
                ui.horizontal(|ui| {
                    ui.label("Filter SHA or status");
                    ui.text_edit_singleline(&mut self.filter);
                });
                egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                    for entry in report.entries.iter().filter(|entry| {
                        self.filter.is_empty()
                            || entry.sha256.contains(self.filter.trim())
                            || entry.status.contains(self.filter.trim())
                    }).take(100) {
                        let suffix = entry.outcome.as_deref().or(entry.reason.as_deref()).unwrap_or("-");
                        let short_sha = &entry.sha256[..entry.sha256.len().min(12)];
                        ui.label(format!("{short_sha}  {}  {}  streams:{} assets:{}", entry.status, suffix,
                            entry.stream_count.unwrap_or(0), entry.asset_count.unwrap_or(0)));
                    }
                });
            }
        });
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
        surface.output_path = "C:/rescue-output".to_owned();
        surface.input_path = "C:/private-folder".to_owned();
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
