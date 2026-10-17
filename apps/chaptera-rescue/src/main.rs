mod batch_ui;
#[cfg(windows)]
mod engine_job;
mod ui_theme;

use eframe::egui;
use pub_reader::{FailureIntakeClass, classify_failure_candidate};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

const APP_TITLE: &str = "Chaptera Rescue — Technical Preview";
const PRODUCT_ID: &str = "chaptera.rescue";
const PRODUCT_RECEIPT_VERSION: &str = "chaptera.rescue-product-validation.v1";

#[derive(Debug, Clone, Deserialize)]
struct FixtureIdentity {
    kind: String,
    source_sha256: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PrivacyState {
    source_free: bool,
    private_content_serialized: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct ProductValidationReceipt {
    receipt_version: String,
    product: String,
    producer_receipt_sha256: String,
    fixture: FixtureIdentity,
    outcome: String,
    source_immutable: bool,
    artifact_count: usize,
    known_loss: bool,
    native_pub_delivery_allowed: bool,
    fail_closed: bool,
    privacy: PrivacyState,
}

#[derive(Debug, Clone, Serialize)]
struct RescueDesktopReport {
    schema_version: &'static str,
    product_id: &'static str,
    source_sha256: String,
    source_immutable: bool,
    outcome: String,
    artifact_count: usize,
    known_loss: bool,
    native_pub_delivery_allowed: bool,
    producer_receipt_sha256: String,
    note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct SelfCheck {
    schema_version: &'static str,
    product_id: &'static str,
    canonical_executable: &'static str,
    editor_ui_present: bool,
    migration_batch_ui_present: bool,
    recovery_execution_embedded: bool,
}

#[derive(Default, PartialEq, Eq)]
enum RescueView {
    #[default]
    Batch,
    Evidence,
}

#[derive(Default)]
struct RescueApp {
    source_path: String,
    source_sha256: Option<String>,
    source_status: Option<String>,
    receipt_path: String,
    receipt: Option<ProductValidationReceipt>,
    receipt_status: Option<String>,
    report_path: String,
    report_status: Option<String>,
    batch_ui: batch_ui::BatchUi,
    view: RescueView,
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_product_receipt(
    receipt: &ProductValidationReceipt,
    source_sha256: &str,
) -> Result<(), String> {
    if receipt.receipt_version != PRODUCT_RECEIPT_VERSION {
        return Err(format!(
            "unsupported Rescue receipt version: {}",
            receipt.receipt_version
        ));
    }
    if receipt.product != "Chaptera Rescue" {
        return Err("receipt does not identify Chaptera Rescue".to_owned());
    }
    if receipt.fixture.source_sha256 != source_sha256 {
        return Err("receipt source identity does not match the selected PUB".to_owned());
    }
    if !matches!(
        receipt.fixture.kind.as_str(),
        "natural_partial_cfb" | "healthy_control" | "partial_control"
    ) {
        return Err("receipt fixture kind is outside the current Rescue contract".to_owned());
    }
    if !matches!(
        receipt.outcome.as_str(),
        "bounded_recovered"
            | "partially_recovered"
            | "manual_review"
            | "unsupported/no_safe_recovery"
    ) {
        return Err("receipt outcome is outside the current Rescue contract".to_owned());
    }
    if !receipt.source_immutable || !receipt.fail_closed {
        return Err("receipt does not prove immutable-source fail-closed handling".to_owned());
    }
    if !receipt.privacy.source_free || receipt.privacy.private_content_serialized {
        return Err("receipt violates the source-free public product boundary".to_owned());
    }
    if !is_lower_sha256(&receipt.producer_receipt_sha256) {
        return Err("producer_receipt_sha256 is invalid".to_owned());
    }
    Ok(())
}

fn load_product_receipt(
    path: &Path,
    source_sha256: &str,
) -> Result<ProductValidationReceipt, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let receipt: ProductValidationReceipt =
        serde_json::from_slice(&bytes).map_err(|error| format!("parse Rescue receipt: {error}"))?;
    validate_product_receipt(&receipt, source_sha256)?;
    Ok(receipt)
}

fn build_report(receipt: &ProductValidationReceipt, source_sha256: &str) -> RescueDesktopReport {
    RescueDesktopReport {
        schema_version: "chaptera.rescue-desktop-report.v1",
        product_id: PRODUCT_ID,
        source_sha256: source_sha256.to_owned(),
        source_immutable: true,
        outcome: receipt.outcome.clone(),
        artifact_count: receipt.artifact_count,
        known_loss: receipt.known_loss,
        native_pub_delivery_allowed: receipt.native_pub_delivery_allowed,
        producer_receipt_sha256: receipt.producer_receipt_sha256.clone(),
        note: "This report consumes a validated Rescue product receipt; it does not claim that this GUI executed recovery.",
    }
}

fn write_report(
    source_path: &Path,
    output_path: &Path,
    receipt: &ProductValidationReceipt,
    expected_source_sha256: &str,
) -> Result<(), String> {
    if source_path == output_path {
        return Err("report output must not overwrite the source PUB".to_owned());
    }
    let before = sha256_file(source_path)?;
    if before != expected_source_sha256 {
        return Err("source PUB identity changed before report export".to_owned());
    }

    let report = build_report(receipt, expected_source_sha256);
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("serialize Rescue report: {error}"))?;
    if let Some(parent) = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(output_path, [bytes.as_slice(), b"\n"].concat())
        .map_err(|error| format!("write {}: {error}", output_path.display()))?;

    let after = sha256_file(source_path)?;
    if after != before {
        return Err("source PUB identity changed during report export".to_owned());
    }
    Ok(())
}

impl RescueApp {
    fn inspect_source(&mut self) {
        let path = PathBuf::from(self.source_path.trim());
        self.receipt = None;
        self.receipt_status = None;
        self.report_status = None;

        if path
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            != Some("pub".to_owned())
        {
            self.source_sha256 = None;
            self.source_status =
                Some("Choose a .pub source file. Chaptera Rescue never overwrites it.".to_owned());
            return;
        }

        match sha256_file(&path) {
            Ok(hash) => {
                self.source_sha256 = Some(hash.clone());
                self.source_status = Some(format!("Source opened read-only. SHA-256: {hash}"));
                if self.report_path.trim().is_empty() {
                    self.report_path = format!("{}.chaptera-rescue-report.json", path.display());
                }
            }
            Err(error) => {
                self.source_sha256 = None;
                self.source_status = Some(error);
            }
        }
    }

    fn load_receipt(&mut self) {
        let Some(source_sha) = self.source_sha256.clone() else {
            self.receipt_status =
                Some("Inspect the source PUB before loading a recovery receipt.".to_owned());
            return;
        };
        let path = PathBuf::from(self.receipt_path.trim());
        match load_product_receipt(&path, &source_sha) {
            Ok(receipt) => {
                let outcome = receipt.outcome.clone();
                self.receipt = Some(receipt);
                self.receipt_status = Some(format!("Validated Chaptera Rescue outcome: {outcome}"));
            }
            Err(error) => {
                self.receipt = None;
                self.receipt_status = Some(error);
            }
        }
    }

    fn export_report(&mut self) {
        let (Some(source_sha), Some(receipt)) = (self.source_sha256.clone(), self.receipt.clone())
        else {
            self.report_status =
                Some("A matching validated Rescue receipt is required before export.".to_owned());
            return;
        };

        let source = PathBuf::from(self.source_path.trim());
        let output = PathBuf::from(self.report_path.trim());
        match write_report(&source, &output, &receipt, &source_sha) {
            Ok(()) => {
                self.report_status = Some(format!(
                    "Wrote source-preserving report: {}",
                    output.display()
                ))
            }
            Err(error) => self.report_status = Some(error),
        }
    }
}

impl RescueApp {
    fn draw_evidence(&mut self, ui: &mut egui::Ui) {
        use egui::RichText;
        ui_theme::kicker(ui, "PROVENANCE & VERIFICATION");
        ui_theme::headline(
            ui,
            "Validate evidence",
            "Inspect a local PUB and a matching receipt before exporting an independent report.",
        );
        ui.add_space(18.0);
        ui_theme::card(ui, |ui| {
            ui.label(
                RichText::new("01  /  ORIGINAL PUB")
                    .size(12.0)
                    .strong()
                    .color(ui_theme::accent()),
            );
            ui.label(
                RichText::new("Source identity")
                    .size(19.0)
                    .strong()
                    .color(ui_theme::ink()),
            );
            ui.label(RichText::new("Choose a local .pub file. This operation reads the source and computes its SHA-256; it never edits the file.").color(ui_theme::muted()));
            let width = ui.available_width();
            ui.add(
                egui::TextEdit::singleline(&mut self.source_path)
                    .hint_text("C:\\Documents\\original.pub")
                    .desired_width(width),
            );
            if ui.button("Inspect source").clicked() {
                self.inspect_source();
            }
            if let Some(status) = &self.source_status {
                ui.label(RichText::new(status).color(ui_theme::muted()));
            }
        });
        ui.add_space(14.0);
        ui_theme::card(ui, |ui| {
            ui.label(
                RichText::new("02  /  VALIDATION")
                    .size(12.0)
                    .strong()
                    .color(ui_theme::accent()),
            );
            ui.label(
                RichText::new("Verified Rescue receipt")
                    .size(19.0)
                    .strong()
                    .color(ui_theme::ink()),
            );
            ui.label(RichText::new("Only a source-matched canonical Rescue receipt is accepted. An uploaded or unverified result is not evidence.").color(ui_theme::muted()));
            let width = ui.available_width();
            ui.add(
                egui::TextEdit::singleline(&mut self.receipt_path)
                    .hint_text("C:\\Reports\\product-validation.json")
                    .desired_width(width),
            );
            if ui.button("Load validated receipt").clicked() {
                self.load_receipt();
            }
            if let Some(status) = &self.receipt_status {
                ui.label(RichText::new(status).color(ui_theme::muted()));
            }
            if let Some(receipt) = &self.receipt {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui_theme::pill(ui, "SOURCE MATCHED", ui_theme::accent());
                    ui.label(
                        RichText::new(format!("Outcome: {}", receipt.outcome))
                            .color(ui_theme::ink()),
                    );
                });
                ui.label(format!("Verified artifacts: {}", receipt.artifact_count));
                ui.label(format!("Known loss: {}", receipt.known_loss));
                ui.label(format!(
                    "Native PUB delivery allowed: {}",
                    receipt.native_pub_delivery_allowed
                ));
            }
        });
        ui.add_space(14.0);
        ui_theme::card(ui, |ui| {
            ui.label(
                RichText::new("03  /  EXPORT")
                    .size(12.0)
                    .strong()
                    .color(ui_theme::accent()),
            );
            ui.label(
                RichText::new("Source-preserving report")
                    .size(19.0)
                    .strong()
                    .color(ui_theme::ink()),
            );
            ui.label(RichText::new("Export creates a separate JSON report and verifies the original SHA before and after writing.").color(ui_theme::muted()));
            let width = ui.available_width();
            ui.add(
                egui::TextEdit::singleline(&mut self.report_path)
                    .hint_text("C:\\Reports\\rescue-report.json")
                    .desired_width(width),
            );
            if ui
                .add_enabled(
                    self.receipt.is_some() && self.source_sha256.is_some(),
                    egui::Button::new("Export Rescue report"),
                )
                .clicked()
            {
                self.export_report();
            }
            if let Some(status) = &self.report_status {
                ui.label(RichText::new(status).color(ui_theme::muted()));
            }
        });
        ui.add_space(14.0);
        ui.label(RichText::new(
            "Technical preview: this screen validates existing evidence. It does not create a repaired native PUB.",
        ).size(12.0).color(ui_theme::muted()));
    }
}

impl eframe::App for RescueApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            let Some(path) = file.path else {
                continue;
            };
            if path.is_dir() {
                self.batch_ui.use_input(&path);
                continue;
            }
            if path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                == Some("pub".to_owned())
            {
                self.batch_ui.use_input(&path);
            }
            match path
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
            {
                Some(ext) if ext == "pub" => {
                    self.source_path = path.display().to_string();
                    self.inspect_source();
                }
                Some(ext) if ext == "json" => {
                    self.receipt_path = path.display().to_string();
                }
                _ => {}
            }
        }

        self.batch_ui.tick(ctx);

        egui::TopBottomPanel::top("chaptera-rescue-header")
            .exact_height(72.0)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .stroke(egui::Stroke::new(1.0, ui_theme::border()))
                    .inner_margin(egui::Margin::symmetric(24, 14)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("chaptera")
                            .size(23.0)
                            .strong()
                            .color(ui_theme::accent()),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new("RESCUE")
                            .size(14.0)
                            .strong()
                            .color(ui_theme::ink()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui_theme::pill(ui, "OFFLINE  ·  TECHNICAL PREVIEW", ui_theme::accent());
                    });
                });
            });

        egui::TopBottomPanel::bottom("chaptera-rescue-footer")
            .exact_height(34.0)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(24, 7)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Local processing  /  Sources remain unchanged")
                            .size(11.0)
                            .color(ui_theme::muted()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("No cloud upload")
                                .size(11.0)
                                .color(ui_theme::success()),
                        );
                    });
                });
            });

        egui::SidePanel::left("chaptera-rescue-navigation")
            .resizable(false)
            .exact_width(208.0)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::WHITE)
                    .stroke(egui::Stroke::new(1.0, ui_theme::border()))
                    .inner_margin(egui::Margin::symmetric(14, 20)),
            )
            .show(ctx, |ui| {
                ui_theme::kicker(ui, "WORKSPACES");
                ui.add_space(12.0);
                let batch_active = self.view == RescueView::Batch;
                let batch_label =
                    egui::RichText::new("Batch recovery")
                        .strong()
                        .color(if batch_active {
                            ui_theme::accent()
                        } else {
                            ui_theme::ink()
                        });
                if ui
                    .add_sized(
                        [ui.available_width(), 42.0],
                        egui::Button::new(batch_label)
                            .fill(if batch_active {
                                ui_theme::accent_soft()
                            } else {
                                egui::Color32::WHITE
                            })
                            .stroke(egui::Stroke::NONE),
                    )
                    .clicked()
                {
                    self.view = RescueView::Batch;
                }
                let evidence_active = self.view == RescueView::Evidence;
                let evidence_label =
                    egui::RichText::new("Validate evidence")
                        .strong()
                        .color(if evidence_active {
                            ui_theme::accent()
                        } else {
                            ui_theme::ink()
                        });
                if ui
                    .add_sized(
                        [ui.available_width(), 42.0],
                        egui::Button::new(evidence_label)
                            .fill(if evidence_active {
                                ui_theme::accent_soft()
                            } else {
                                egui::Color32::WHITE
                            })
                            .stroke(egui::Stroke::NONE),
                    )
                    .clicked()
                {
                    self.view = RescueView::Evidence;
                }
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);
                ui_theme::kicker(ui, "SAFETY BOUNDARY");
                ui.label(
                    egui::RichText::new("Read-only source intake")
                        .size(12.0)
                        .color(ui_theme::ink()),
                );
                ui.label(
                    egui::RichText::new("No native PUB repair is promised in this preview.")
                        .size(11.0)
                        .color(ui_theme::muted()),
                );
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ui_theme::canvas())
                    .inner_margin(egui::Margin::symmetric(22, 20)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        match self.view {
                            RescueView::Batch => self.batch_ui.draw(ui),
                            RescueView::Evidence => self.draw_evidence(ui),
                        }
                    });
            });
    }
}

fn self_check() -> SelfCheck {
    SelfCheck {
        schema_version: "chaptera.rescue-self-check.v1",
        product_id: PRODUCT_ID,
        canonical_executable: "chaptera-rescue.exe",
        editor_ui_present: false,
        migration_batch_ui_present: false,
        recovery_execution_embedded: false,
    }
}

fn rescue_handoff_admitted_bytes(bytes: &[u8]) -> bool {
    matches!(
        classify_failure_candidate(bytes).class,
        FailureIntakeClass::PubDamaged
    )
}

fn run_cli(args: &[String]) -> Result<Option<i32>, String> {
    if args.iter().any(|arg| arg == "--self-check") {
        println!(
            "{}",
            serde_json::to_string_pretty(&self_check())
                .map_err(|error| format!("serialize self-check: {error}"))?
        );
        return Ok(Some(0));
    }

    if let Some(index) = args.iter().position(|arg| arg == "--handoff-accept-v1") {
        let values = args.get(index + 1..index + 3).ok_or_else(|| {
            "usage: chaptera-rescue --handoff-accept-v1 PACKET.json ACCEPTANCE.json".to_owned()
        })?;
        if values.len() != 2 {
            return Err(
                "usage: chaptera-rescue --handoff-accept-v1 PACKET.json ACCEPTANCE.json".to_owned(),
            );
        }
        let packet_path = PathBuf::from(&values[0]);
        let output = PathBuf::from(&values[1]);
        let validated = chaptera_suite_handoff::load_for_receiver(
            &packet_path,
            chaptera_suite_handoff::RESCUE_PRODUCT_ID,
        )?;
        let receiver_admitted = rescue_handoff_admitted_bytes(validated.source_bytes());
        let receipt = chaptera_suite_handoff::finish_acceptance(validated, receiver_admitted)?;
        chaptera_suite_handoff::write_acceptance(&receipt, &output)?;
        println!(
            "{}",
            serde_json::to_string(&receipt)
                .map_err(|error| format!("serialize suite handoff acceptance: {error}"))?
        );
        return Ok(Some(0));
    }

    if let Some(index) = args.iter().position(|arg| arg == "--acceptance-v1") {
        let values = args.get(index + 1..index + 4).ok_or_else(|| {
            "usage: chaptera-rescue --acceptance-v1 SOURCE.pub PRODUCT-RECEIPT.json REPORT.json"
                .to_owned()
        })?;
        if values.len() != 3 {
            return Err(
                "usage: chaptera-rescue --acceptance-v1 SOURCE.pub PRODUCT-RECEIPT.json REPORT.json"
                    .to_owned(),
            );
        }
        let source = PathBuf::from(&values[0]);
        let receipt_path = PathBuf::from(&values[1]);
        let output = PathBuf::from(&values[2]);
        let source_sha = sha256_file(&source)?;
        let receipt = load_product_receipt(&receipt_path, &source_sha)?;
        write_report(&source, &output, &receipt, &source_sha)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&build_report(&receipt, &source_sha))
                .map_err(|error| format!("serialize acceptance report: {error}"))?
        );
        return Ok(Some(0));
    }

    Ok(None)
}

fn rescue_gui_options() -> eframe::NativeOptions {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([860.0, 640.0]),
        ..Default::default()
    };
    #[cfg(windows)]
    {
        // DX12 supports both hardware adapters and WARP (software rendering)
        // on Windows 10/11; avoid an unconditional OpenGL 2.0 requirement.
        options.renderer = eframe::Renderer::Wgpu;
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
        {
            setup.instance_descriptor.backends = eframe::wgpu::Backends::DX12;
        }
    }
    options
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();
    match run_cli(&args) {
        Ok(Some(code)) => std::process::exit(code),
        Ok(None) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }

    // Install Windows process-lifetime containment before any GUI batch child
    // can start. Children inherit the Job at CreateProcess time, closing the
    // otherwise unavoidable spawn-to-attach race. CLI-only operations remain
    // outside this GUI lifecycle boundary.
    #[cfg(windows)]
    if let Err(error) = engine_job::EngineJob::protect_gui_process() {
        eprintln!("Rescue GUI cannot safely start: {error}");
        std::process::exit(2);
    }

    let options = rescue_gui_options();
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|creation_context| {
            ui_theme::configure(&creation_context.egui_ctx);
            Ok(Box::<RescueApp>::default())
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(source_sha: &str) -> ProductValidationReceipt {
        ProductValidationReceipt {
            receipt_version: PRODUCT_RECEIPT_VERSION.to_owned(),
            product: "Chaptera Rescue".to_owned(),
            producer_receipt_sha256: "a".repeat(64),
            fixture: FixtureIdentity {
                kind: "natural_partial_cfb".to_owned(),
                source_sha256: source_sha.to_owned(),
            },
            outcome: "partially_recovered".to_owned(),
            source_immutable: true,
            artifact_count: 1,
            known_loss: true,
            native_pub_delivery_allowed: false,
            fail_closed: true,
            privacy: PrivacyState {
                source_free: true,
                private_content_serialized: false,
            },
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_rescue_gui_selects_dx12_wgpu_instead_of_glow() {
        let options = rescue_gui_options();
        assert!(matches!(options.renderer, eframe::Renderer::Wgpu));
        match options.wgpu_options.wgpu_setup {
            eframe::egui_wgpu::WgpuSetup::CreateNew(setup) => {
                assert_eq!(
                    setup.instance_descriptor.backends,
                    eframe::wgpu::Backends::DX12
                );
            }
            eframe::egui_wgpu::WgpuSetup::Existing(_) => {
                panic!("Rescue GUI must configure a new DX12 device");
            }
        }
    }

    #[test]
    fn product_receipt_requires_exact_source_identity() {
        let good = "b".repeat(64);
        validate_product_receipt(&receipt(&good), &good).expect("matching source");
        let error = validate_product_receipt(&receipt(&good), &"c".repeat(64))
            .expect_err("mismatch must fail closed");
        assert!(error.contains("does not match"));
    }

    #[test]
    fn private_product_receipt_is_rejected() {
        let source = "b".repeat(64);
        let mut value = receipt(&source);
        value.privacy.private_content_serialized = true;
        assert!(validate_product_receipt(&value, &source).is_err());
    }

    #[test]
    fn generic_editor_or_migration_identity_is_absent() {
        let cargo = include_str!("../Cargo.toml");
        assert!(!cargo.contains("pub-editor"));
        assert!(!cargo.contains("pub-interaction"));
        assert!(!cargo.contains("chaptera-desktop"));
        let check = self_check();
        assert!(!check.editor_ui_present);
        assert!(!check.migration_batch_ui_present);
        assert!(!check.recovery_execution_embedded);
        assert_eq!(check.canonical_executable, "chaptera-rescue.exe");
    }

    #[test]
    fn handoff_admission_reclassifies_damaged_source_independently() {
        let path = env::temp_dir().join(format!(
            "chaptera-rescue-handoff-damaged-{}.pub",
            std::process::id()
        ));
        let mut bytes = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        bytes.extend_from_slice(b"Microsoft Publisher");
        fs::write(&path, &bytes).expect("write damaged classifier witness");
        let admitted = fs::read(&path).expect("read damaged classifier witness");
        assert!(rescue_handoff_admitted_bytes(&admitted));
        fs::remove_file(path).ok();
    }

    #[test]
    fn handoff_admission_rejects_unknown_unsupported_source() {
        let path = env::temp_dir().join(format!(
            "chaptera-rescue-handoff-unknown-{}.pub",
            std::process::id()
        ));
        fs::write(
            &path,
            [0x00, 0xFF, 0x10, 0x80, 0x00, 0x7F, 0xAA, 0x55, 0x13, 0x37],
        )
        .expect("write unknown witness");
        let admitted = fs::read(&path).expect("read unknown classifier witness");
        assert!(!rescue_handoff_admitted_bytes(&admitted));
        fs::remove_file(path).ok();
    }

    #[test]
    fn report_is_explicit_that_gui_did_not_execute_recovery() {
        let source = "b".repeat(64);
        let report = build_report(&receipt(&source), &source);
        assert!(report.note.contains("does not claim"));
        assert_eq!(report.product_id, "chaptera.rescue");
    }
}
