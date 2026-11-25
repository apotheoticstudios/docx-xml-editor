#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod package;
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, FontId, RichText};
use package::{Package, APP, CORE};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const INK: Color32 = Color32::from_rgb(226, 234, 244);
const MUTED: Color32 = Color32::from_rgb(155, 172, 192);
const TEAL: Color32 = Color32::from_rgb(111, 225, 203);
const BUTTON: Color32 = Color32::from_rgb(0, 106, 113);
const SURFACE: Color32 = Color32::from_rgb(24, 34, 49);
const BACKGROUND: Color32 = Color32::from_rgb(16, 24, 37);
const TEAL_PALE: Color32 = Color32::from_rgb(30, 63, 66);
const NAVY: Color32 = Color32::from_rgb(11, 18, 29);
const BORDER: Color32 = Color32::from_rgb(49, 65, 84);
const RED: Color32 = Color32::from_rgb(255, 143, 146);
const FIELDS: [(&str, &str, &str); 8] = [
    ("Title", CORE, "title"),
    ("Subject", CORE, "subject"),
    ("Author", CORE, "creator"),
    ("Keywords", CORE, "keywords"),
    ("Description", CORE, "description"),
    ("Last modified by", CORE, "lastModifiedBy"),
    ("Revision counter", CORE, "revision"),
    ("Editing time", APP, "TotalTime"),
];
#[derive(Clone)]
enum PendingAction {
    OpenDialog,
    OpenPath(PathBuf),
    Close,
}
#[derive(Default)]
struct Editor {
    package: Option<Package>,
    fields: Vec<String>,
    baseline: Vec<String>,
    saved: BTreeMap<String, Vec<u8>>,
    checked: Option<BTreeMap<String, Vec<u8>>>,
    output: Option<PathBuf>,
    xml_tab: bool,
    selected: Option<String>,
    buffer: String,
    find: String,
    part_filter: String,
    match_index: usize,
    search_selection: Option<(usize, usize)>,
    wrap_xml: bool,
    status: String,
    error: Option<String>,
    pending: Option<PendingAction>,
    allow_close: bool,
    logo: Option<egui::TextureHandle>,
}
impl Editor {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = egui::Style {
            visuals: egui::Visuals::dark(),
            ..Default::default()
        };
        style.visuals.override_text_color = Some(INK);
        style.visuals.panel_fill = BACKGROUND;
        style.visuals.window_fill = SURFACE;
        style.visuals.extreme_bg_color = Color32::from_rgb(18, 28, 43);
        style.visuals.faint_bg_color = Color32::from_rgb(31, 43, 60);
        style.visuals.selection.bg_fill = Color32::from_rgb(34, 88, 89);
        style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, TEAL);
        style.visuals.widgets.inactive.bg_fill = SURFACE;
        style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
        style.visuals.widgets.inactive.corner_radius = 6.into();
        style.visuals.widgets.hovered.bg_fill = TEAL_PALE;
        style.visuals.widgets.hovered.weak_bg_fill = TEAL_PALE;
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, TEAL);
        style.visuals.widgets.hovered.corner_radius = 6.into();
        style.visuals.widgets.active.corner_radius = 6.into();
        style.spacing.item_spacing = egui::vec2(10., 9.);
        style.spacing.button_padding = egui::vec2(16., 10.);
        style.spacing.interact_size.y = 34.;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(23.));
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.set_style(style);
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        let logo = image::load_from_memory(include_bytes!("../assets/logo.png"))
            .ok()
            .map(|image| {
                let image = image.to_rgba8();
                cc.egui_ctx.load_texture(
                    "logo",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    ),
                    Default::default(),
                )
            });
        Self {
            logo,
            wrap_xml: true,
            status: "Ready · Choose a DOCX to begin".into(),
            ..Default::default()
        }
    }
    fn pending_edits(&self) -> bool {
        self.package.as_ref().is_some_and(|p| {
            self.fields != self.baseline
                || self
                    .selected
                    .as_ref()
                    .is_some_and(|s| p.text(s).is_ok_and(|t| t != self.buffer))
        })
    }
    fn dirty(&self) -> bool {
        self.pending_edits()
            || self
                .package
                .as_ref()
                .is_some_and(|p| p.changes != self.saved)
    }
    fn validated(&self) -> bool {
        !self.pending_edits()
            && self
                .package
                .as_ref()
                .is_some_and(|p| self.checked.as_ref() == Some(&p.changes))
    }
    fn load_fields(&mut self) -> Result<()> {
        if let Some(package) = &self.package {
            self.fields = FIELDS
                .iter()
                .map(|(_, part, key)| {
                    if package.data(part).is_err() {
                        Ok(String::new())
                    } else {
                        package.property(part, key)
                    }
                })
                .collect::<Result<_>>()?;
            self.baseline = self.fields.clone();
        }
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        let Some(package) = &self.package else {
            return Ok(());
        };
        let mut staged = package.clone();
        if let Some(part) = &self.selected {
            staged.set_text(part, self.buffer.clone())?;
        }
        for (i, (label, part, key)) in FIELDS.iter().enumerate() {
            if self.fields[i] != self.baseline[i] {
                if *key == "revision" || *key == "TotalTime" {
                    self.fields[i].parse::<u32>().with_context(|| {
                        format!("{label} needs a whole number of zero or more.")
                    })?;
                }
                staged.set_property(part, key, &self.fields[i])?;
            }
        }
        staged.validate()?;
        if let Some(part) = &self.selected {
            self.buffer = staged.text(part)?.to_owned();
        }
        self.checked = Some(staged.changes.clone());
        self.package = Some(staged);
        self.load_fields()
    }
    fn open_path(&mut self, path: &Path) -> Result<()> {
        let package = Package::open(path)?;
        // Read the properties before replacing the current document.
        let fields = FIELDS
            .iter()
            .map(|(_, part, key)| {
                if package.data(part).is_err() {
                    Ok(String::new())
                } else {
                    package.property(part, key)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        self.fields = fields;
        self.baseline = self.fields.clone();
        self.package = Some(package);
        self.saved.clear();
        self.checked = Some(BTreeMap::new());
        self.selected = None;
        self.buffer.clear();
        self.part_filter.clear();
        self.find.clear();
        self.output = None;
        self.xml_tab = false;
        self.status = "Document opened · Package checks passed".into();
        Ok(())
    }
    fn open_dialog(&mut self) -> Result<()> {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Word document", &["docx"])
            .pick_file()
        {
            self.open_path(&path)?;
        }
        Ok(())
    }
    fn save(&mut self) -> Result<bool> {
        self.commit()?;
        let Some(package) = &self.package else {
            return Ok(false);
        };
        let suggested = self
            .output
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                format!(
                    "{} - Edited.docx",
                    package
                        .source
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                )
            });
        let dialog = rfd::FileDialog::new()
            .add_filter("Word document", &["docx"])
            .set_directory(package.source.parent().unwrap_or(Path::new(".")))
            .set_file_name(suggested);
        if let Some(mut path) = dialog.save_file() {
            if path.extension().is_none() {
                path.set_extension("docx");
            }
            let backup = package.save(&path)?;
            self.saved = package.changes.clone();
            self.output = Some(path.clone());
            self.status = format!(
                "Saved and verified {}{}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                if backup.is_some() {
                    " · Previous file backed up"
                } else {
                    ""
                }
            );
            return Ok(true);
        }
        Ok(false)
    }
    fn report(&mut self, result: Result<()>) {
        if let Err(error) = result {
            self.error = Some(format!("{error:#}"));
        }
    }
    fn perform(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::OpenDialog => {
                let result = self.open_dialog();
                self.report(result);
            }
            PendingAction::OpenPath(path) => {
                let result = self.open_path(&path);
                self.report(result);
            }
            PendingAction::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    fn request(&mut self, action: PendingAction, ctx: &egui::Context) {
        if self.dirty() {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }
    fn check(&mut self) {
        let result = self.commit();
        if result.is_ok() {
            self.status =
                "Checks passed · XML, namespaces, archive structure and relationships".into();
        }
        self.report(result);
    }
    fn set_tab(&mut self, xml: bool) {
        let result = self.commit();
        if result.is_ok() {
            self.xml_tab = xml;
            if xml && self.selected.is_none() {
                if let Some(part) = self
                    .package
                    .as_ref()
                    .and_then(|p| p.xml_parts().into_iter().find(|s| s == CORE))
                {
                    self.select_part(&part);
                }
            }
        }
        self.report(result);
    }
    fn select_part(&mut self, part: &str) {
        let result = self.commit();
        if result.is_ok() {
            if let Some(package) = &self.package {
                match package.text(part) {
                    Ok(value) => {
                        self.buffer = value.to_owned();
                        self.selected = Some(part.into());
                        self.match_index = 0;
                    }
                    Err(error) => self.error = Some(format!("{error:#}")),
                }
            }
        }
        self.report(result);
    }
    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(24, 16))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(logo) = &self.logo {
                        ui.image((logo.id(), egui::vec2(40., 40.)));
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("DOCX XML Editor").size(18.).strong());
                        ui.label(
                            RichText::new("Document properties & XML")
                                .size(12.)
                                .color(MUTED),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(self.package.is_some(), primary("Save a copy…"))
                            .on_hover_text("Validate and save a rebuilt DOCX · ⌘/Ctrl+S")
                            .clicked()
                        {
                            let result = self.save().map(|_| ());
                            self.report(result);
                        }
                        if ui
                            .add_enabled(
                                self.package.is_some(),
                                egui::Button::new("Check document"),
                            )
                            .on_hover_text("Check XML syntax and package integrity")
                            .clicked()
                        {
                            self.check();
                        }
                        if ui
                            .button("Open…")
                            .on_hover_text("Open a DOCX · ⌘/Ctrl+O")
                            .clicked()
                        {
                            self.request(PendingAction::OpenDialog, ctx);
                        }
                    });
                });
            });
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("navigation")
            .exact_width(228.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(NAVY)
                    .inner_margin(egui::Margin::symmetric(18, 22)),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(219, 229, 241));
                ui.label(
                    RichText::new("WORKSPACE")
                        .size(10.)
                        .strong()
                        .color(Color32::from_rgb(134, 156, 182)),
                );
                ui.add_space(10.);
                if let Some(package) = &self.package {
                    let filename = package
                        .source
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    ui.add(
                        egui::Label::new(
                            RichText::new(filename)
                                .size(14.)
                                .strong()
                                .color(Color32::from_rgb(225, 235, 246)),
                        )
                        .wrap(),
                    )
                    .on_hover_text(package.source.display().to_string());
                    ui.add_space(6.);
                    ui.label(
                        RichText::new(format!("{} XML parts", package.xml_parts().len()))
                            .size(12.)
                            .color(Color32::from_rgb(159, 180, 203)),
                    );
                    ui.add_space(12.);
                    let (label, color) = if self.dirty() {
                        ("Unsaved changes", Color32::from_rgb(255, 200, 113))
                    } else if self.output.is_some() {
                        ("Copy saved", Color32::from_rgb(125, 224, 195))
                    } else {
                        ("Ready to edit", Color32::from_rgb(125, 224, 195))
                    };
                    ui.horizontal(|ui| {
                        let (dot, _) =
                            ui.allocate_exact_size(egui::vec2(8., 12.), egui::Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3., color);
                        ui.label(RichText::new(label).size(12.).color(color));
                    });
                } else {
                    ui.label(
                        RichText::new("No document open")
                            .strong()
                            .color(Color32::from_rgb(225, 235, 246)),
                    );
                    ui.label(
                        RichText::new("Open or drop a .docx file")
                            .size(12.)
                            .color(Color32::from_rgb(159, 180, 203)),
                    );
                }
                ui.add_space(24.);
                let nav = [
                    (false, "Properties", "Edit common document fields"),
                    (true, "XML editor", "Inspect individual package parts"),
                ];
                for (xml, title, subtitle) in nav {
                    let active = self.xml_tab == xml;
                    let fill = if active {
                        Color32::from_rgb(35, 62, 80)
                    } else {
                        NAVY
                    };
                    let response = egui::Frame::new()
                        .fill(fill)
                        .corner_radius(8)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(title).size(14.).strong().color(if active {
                                Color32::from_rgb(142, 234, 214)
                            } else {
                                Color32::from_rgb(222, 232, 242)
                            }));
                            ui.label(
                                RichText::new(subtitle)
                                    .size(11.)
                                    .color(Color32::from_rgb(155, 178, 199)),
                            );
                        })
                        .response;
                    if ui
                        .interact(
                            response.rect,
                            response.id.with("navigation"),
                            egui::Sense::click(),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        self.set_tab(xml);
                    }
                    ui.add_space(5.);
                }
                ui.add_space(24.);
                ui.label(
                    RichText::new("PROCESSING")
                        .size(10.)
                        .strong()
                        .color(Color32::from_rgb(134, 156, 182)),
                );
                ui.label(
                    RichText::new("Files are processed on this computer.")
                        .size(12.)
                        .color(Color32::from_rgb(159, 180, 203)),
                );
                if let Some(path) = &self.output {
                    ui.add_space(20.);
                    ui.label(
                        RichText::new("LAST SAVED COPY")
                            .size(10.)
                            .strong()
                            .color(Color32::from_rgb(134, 156, 182)),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(path.file_name().unwrap_or_default().to_string_lossy())
                                .size(12.),
                        )
                        .wrap(),
                    )
                    .on_hover_text(path.display().to_string());
                }
            });
    }
    fn welcome(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(58.);
        let width = ui.available_width().min(650.);
        ui.horizontal(|ui| {
            ui.add_space(((ui.available_width() - width) / 2.).max(0.));
            ui.vertical(|ui| {
                ui.set_width(width);
                card(ui, "welcome-card", |ui| {
                    if let Some(logo) = &self.logo {
                        ui.image((logo.id(), egui::vec2(72., 72.)));
                    }
                    ui.add_space(12.);
                    ui.label(RichText::new("Open a Word document").size(28.).strong());
                    ui.label(
                        RichText::new(
                            "Edit its properties or inspect the XML inside the DOCX package.",
                        )
                        .size(15.)
                        .color(MUTED),
                    );
                    ui.add_space(22.);
                    if ui.add(primary("Choose a DOCX file…")).clicked() {
                        self.request(PendingAction::OpenDialog, ctx);
                    }
                    ui.label(
                        RichText::new("or drag a .docx file into this window")
                            .size(12.)
                            .color(MUTED),
                    );
                    ui.add_space(28.);
                    for (number, title, detail) in [
                        ("01", "Open", "Load a document and check its package."),
                        ("02", "Edit", "Use labelled fields or the XML editor."),
                        ("03", "Save a copy", "Validate and rebuild a separate DOCX."),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(number).size(13.).color(TEAL).strong());
                            ui.vertical(|ui| {
                                ui.label(RichText::new(title).strong());
                                ui.label(RichText::new(detail).size(12.).color(MUTED));
                            });
                        });
                        ui.add_space(9.);
                    }
                });
            });
        });
    }
    fn field(&mut self, ui: &mut egui::Ui, index: usize, hint: &str) {
        let (label, part, _) = FIELDS[index];
        let enabled = self.package.as_ref().is_some_and(|p| p.data(part).is_ok());
        let label_response = ui.label(RichText::new(label).size(12.).strong());
        let response = ui.add_enabled(
            enabled,
            if index == 4 {
                egui::TextEdit::multiline(&mut self.fields[index]).desired_rows(3)
            } else {
                egui::TextEdit::singleline(&mut self.fields[index])
            }
            .id_salt(("property", index))
            .hint_text(hint)
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(10., 9.)),
        );
        response.labelled_by(label_response.id);
        if !enabled {
            ui.label(
                RichText::new("This document has no corresponding metadata part.")
                    .size(11.)
                    .color(MUTED),
            );
        }
        if enabled
            && index >= 6
            && self.fields[index] != self.baseline[index]
            && self.fields[index].parse::<u32>().is_err()
        {
            ui.label(
                RichText::new("Enter a whole number of zero or more.")
                    .size(11.)
                    .color(RED),
            );
        }
    }
    fn properties(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading("Document properties");
                ui.label(
                    RichText::new("Edit the fields below, then save a rebuilt copy.").color(MUTED),
                );
            });
            if self.validated() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    badge(ui, "Checks passed", TEAL_PALE, TEAL);
                });
            }
        });
        ui.add_space(20.);
        card(ui, "details", |ui| {
            section(
                ui,
                "Document details",
                "The title and description stored inside the file.",
            );
            self.field(ui, 0, "Give the document a title");
            ui.add_space(8.);
            ui.columns(2, |columns| {
                self.field(&mut columns[0], 1, "Topic or subject");
                self.field(&mut columns[1], 3, "Keywords separated by commas");
            });
            ui.add_space(8.);
            self.field(ui, 4, "Add a short description");
        });
        ui.add_space(14.);
        card(ui, "people", |ui| {
            section(
                ui,
                "People",
                "Author and last editor recorded in the document.",
            );
            ui.columns(2, |columns| {
                self.field(&mut columns[0], 2, "Author name");
                self.field(&mut columns[1], 5, "Last editor name");
            });
        });
        ui.add_space(14.);
        card(ui, "statistics", |ui| {
            section(
                ui,
                "Editing statistics",
                "Stored counters that Word may update when it next saves.",
            );
            ui.columns(2, |columns| {
                self.field(&mut columns[0], 6, "0");
                columns[0].label(
                    RichText::new("A counter, not a detailed revision history.")
                        .size(11.)
                        .color(MUTED),
                );
                self.field(&mut columns[1], 7, "Whole minutes");
                if let Ok(minutes) = self.fields[7].parse::<u32>() {
                    columns[1].label(
                        RichText::new(format!(
                            "{} h {} min  ·  {:.2} hours",
                            minutes / 60,
                            minutes % 60,
                            minutes as f64 / 60.
                        ))
                        .size(12.)
                        .color(TEAL),
                    );
                }
            });
        });
        ui.add_space(16.);
        ui.label(RichText::new("Saving checks the package and verifies its contents. Arbitrary XML changes may still need review in Word.").size(12.).color(MUTED));
    }
    fn xml_editor(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading("XML editor");
                ui.label(
                    RichText::new("Select a package part to inspect or edit its source.")
                        .color(MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                badge(
                    ui,
                    "Advanced",
                    Color32::from_rgb(64, 49, 28),
                    Color32::from_rgb(242, 194, 117),
                );
            });
        });
        ui.add_space(18.);
        let height = (ui.available_height() - 8.).max(240.);
        let parts = self
            .package
            .as_ref()
            .map(Package::xml_parts)
            .unwrap_or_default();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(238.);
                egui::Frame::new()
                    .fill(SURFACE)
                    .corner_radius(10)
                    .stroke(egui::Stroke::new(1.0_f32, BORDER))
                    .inner_margin(14)
                    .show(ui, |ui| {
                        ui.set_width(210.);
                        ui.label(
                            RichText::new("PACKAGE PARTS")
                                .size(10.)
                                .strong()
                                .color(MUTED),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.part_filter)
                                .hint_text("Filter parts…")
                                .desired_width(210.)
                                .margin(egui::vec2(8., 7.)),
                        );
                        ui.add_space(4.);
                        egui::ScrollArea::vertical()
                            .id_salt("part-list")
                            .max_height(height - 100.)
                            .show(ui, |ui| {
                                for part in parts
                                    .iter()
                                    .filter(|p| {
                                        p.to_lowercase().contains(&self.part_filter.to_lowercase())
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>()
                                {
                                    let modified = self
                                        .package
                                        .as_ref()
                                        .is_some_and(|p| p.changes.contains_key(&part))
                                        || (self.selected.as_ref() == Some(&part)
                                            && self.pending_edits());
                                    let label =
                                        format!("{}{}", if modified { "• " } else { "" }, part);
                                    if ui
                                        .add(
                                            egui::Button::selectable(
                                                self.selected.as_ref() == Some(&part),
                                                RichText::new(label).size(12.),
                                            )
                                            .min_size(egui::vec2(206., 28.)),
                                        )
                                        .on_hover_text(&part)
                                        .clicked()
                                    {
                                        self.select_part(&part);
                                    }
                                }
                            });
                    });
            });
            ui.add_space(2.);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                egui::Frame::new()
                    .fill(SURFACE)
                    .corner_radius(10)
                    .stroke(egui::Stroke::new(1.0_f32, BORDER))
                    .inner_margin(16)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_min_height(height - 34.);
                        ui.label(
                            RichText::new(self.selected.as_deref().unwrap_or("Select a part"))
                                .monospace()
                                .size(12.)
                                .strong(),
                        );
                        let mut keep_find_focus = false;
                        ui.horizontal(|ui| {
                            let response = ui.add(
                                egui::TextEdit::singleline(&mut self.find)
                                    .id_salt("xml-find")
                                    .hint_text("Find in this part…")
                                    .desired_width(190.)
                                    .margin(egui::vec2(8., 6.)),
                            );
                            if ctx.input_mut(|i| {
                                i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)
                            }) {
                                response.request_focus();
                            }
                            let matches: Vec<_> = if self.find.is_empty() {
                                Vec::new()
                            } else {
                                self.buffer
                                    .match_indices(&self.find)
                                    .map(|(i, _)| i)
                                    .collect()
                            };
                            if response.changed() {
                                self.match_index = 0;
                            }
                            keep_find_focus = response.has_focus();
                            let mut jump = response.changed() && !matches.is_empty();
                            let previous = ui
                                .add_enabled(!matches.is_empty(), egui::Button::new("‹"))
                                .on_hover_text("Previous match")
                                .clicked();
                            let next = ui
                                .add_enabled(!matches.is_empty(), egui::Button::new("›"))
                                .on_hover_text("Next match")
                                .clicked();
                            if !matches.is_empty() {
                                if previous {
                                    self.match_index =
                                        (self.match_index + matches.len() - 1) % matches.len();
                                    jump = true;
                                }
                                if next
                                    || (response.has_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    self.match_index = (self.match_index + 1) % matches.len();
                                    jump = true;
                                }
                                self.match_index = self.match_index.min(matches.len() - 1);
                                ui.label(
                                    RichText::new(format!(
                                        "{} / {}",
                                        self.match_index + 1,
                                        matches.len()
                                    ))
                                    .size(11.)
                                    .color(MUTED),
                                );
                                if jump {
                                    let byte = matches[self.match_index];
                                    let start = self.buffer[..byte].chars().count();
                                    self.search_selection =
                                        Some((start, start + self.find.chars().count()));
                                }
                            } else if !self.find.is_empty() {
                                ui.label(RichText::new("No matches").size(11.).color(MUTED));
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.wrap_xml, "Wrap lines");
                            ui.label(RichText::new("UTF-8").size(11.).color(MUTED));
                        });
                        ui.separator();
                        if self.selected.is_some() {
                            egui::ScrollArea::both()
                                .id_salt("xml-scroll")
                                .max_height(height - 175.)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let mut output = egui::TextEdit::multiline(&mut self.buffer)
                                        .id_salt("xml-source")
                                        .font(egui::TextStyle::Monospace)
                                        .code_editor()
                                        .desired_width(if self.wrap_xml {
                                            ui.available_width()
                                        } else {
                                            f32::INFINITY
                                        })
                                        .desired_rows(28)
                                        .show(ui);
                                    if let Some((start, end)) = self.search_selection.take() {
                                        output.state.cursor.set_char_range(Some(
                                            egui::text::CCursorRange::two(
                                                egui::text::CCursor::new(start),
                                                egui::text::CCursor::new(end),
                                            ),
                                        ));
                                        output.state.store(ctx, output.response.id);
                                        let cursor_rect = output
                                            .galley
                                            .pos_from_cursor(egui::text::CCursor::new(end));
                                        ui.scroll_to_rect(
                                            cursor_rect.translate(output.galley_pos.to_vec2()),
                                            Some(egui::Align::Center),
                                        );
                                        if !keep_find_focus {
                                            output.response.request_focus();
                                        }
                                    }
                                });
                        } else {
                            ui.label(
                                RichText::new("Choose an XML file from the package list.")
                                    .color(MUTED),
                            );
                        }
                    });
            });
        });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.pending.clone() {
            egui::Modal::new(egui::Id::new("unsaved-changes")).show(ctx, |ui| {
                ui.set_width(400.);
                ui.label(RichText::new("Save your changes?").size(22.).strong());
                ui.label("Your document has unsaved edits. Save a copy before continuing, or discard these edits.");
                ui.add_space(16.);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() { self.pending = None; }
                    if ui.button("Discard edits").clicked() { self.pending = None; self.perform(action.clone(), ctx); }
                    if ui.add(primary("Save a copy…")).clicked() {
                        match self.save() {
                            Ok(true) => { self.pending = None; self.perform(action.clone(), ctx); }
                            Ok(false) => {},
                            Err(error) => { self.pending = None; self.error = Some(format!("{error:#}")); }
                        }
                    }
                });
            });
        }
        if let Some(message) = self.error.clone() {
            egui::Modal::new(egui::Id::new("operation-error")).show(ctx, |ui| {
                ui.set_width(480.);
                ui.label(
                    RichText::new("The document needs attention")
                        .size(21.)
                        .strong()
                        .color(RED),
                );
                egui::ScrollArea::vertical()
                    .max_height(240.)
                    .show(ui, |ui| {
                        ui.label(&message);
                    });
                ui.add_space(10.);
                if ui.add(primary("Return to editor")).clicked() {
                    self.error = None;
                }
            });
        }
    }
}
// These small presentation helpers keep the screens visually consistent.
fn primary(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
        .fill(BUTTON)
        .stroke(egui::Stroke::NONE)
}
fn card(ui: &mut egui::Ui, id: &str, contents: impl FnOnce(&mut egui::Ui)) {
    ui.push_id(id, |ui| {
        egui::Frame::new()
            .fill(SURFACE)
            .corner_radius(12)
            .stroke(egui::Stroke::new(1.0_f32, BORDER))
            .inner_margin(22)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                contents(ui);
            });
    });
}
fn section(ui: &mut egui::Ui, title: &str, detail: &str) {
    ui.label(RichText::new(title).size(16.).strong());
    ui.label(RichText::new(detail).size(12.).color(MUTED));
    ui.add_space(12.);
}
fn badge(ui: &mut egui::Ui, label: &str, fill: Color32, ink: Color32) {
    egui::Frame::new()
        .fill(fill)
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.).strong().color(ink));
        });
}
impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending = Some(PendingAction::Close);
        }
        if self.pending.is_none() && self.error.is_none() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O)) {
                self.request(PendingAction::OpenDialog, ctx);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
                && self.package.is_some()
            {
                let result = self.save().map(|_| ());
                self.report(result);
            }
            let dropped = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
            if let Some(path) = dropped {
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("docx"))
                {
                    self.request(PendingAction::OpenPath(path), ctx);
                } else {
                    self.error = Some("Choose a Word document with a .docx extension.".into());
                }
            }
        }
        self.toolbar(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(20, 9))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER)),
            )
            .show(ctx, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&self.status).size(12.).color(MUTED)).truncate(),
                )
                .on_hover_text(&self.status);
            });
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(28, 24)),
            )
            .show(ctx, |ui| {
                if self.package.is_none() {
                    self.welcome(ui, ctx);
                } else if self.xml_tab {
                    self.xml_editor(ui, ctx);
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("properties-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.properties(ui);
                        });
                }
            });
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            egui::Area::new(egui::Id::new("drop-overlay"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, [0., 0.])
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(TEAL_PALE)
                        .corner_radius(14)
                        .stroke(egui::Stroke::new(2.0_f32, TEAL))
                        .inner_margin(30)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Drop a DOCX to open it")
                                    .size(22.)
                                    .strong()
                                    .color(TEAL),
                            );
                        });
                });
        }
        self.dialogs(ctx);
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--check") {
        let path = args
            .get(2)
            .context("Usage: docx-xml-editor --check input.docx")?;
        Package::open(Path::new(path))?;
        println!("Validation passed.");
        return Ok(());
    }
    if args.get(1).is_some_and(|a| a == "--roundtrip") {
        anyhow::ensure!(
            args.len() == 4,
            "Usage: docx-xml-editor --roundtrip input.docx output.docx"
        );
        Package::open(Path::new(&args[2]))?.save(Path::new(&args[3]))?;
        println!("Rebuilt and verified.");
        return Ok(());
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png"))?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180., 820.])
            .with_min_inner_size([1000., 700.])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "DOCX XML Editor",
        options,
        Box::new(|cc| Ok(Box::new(Editor::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn editor() -> Editor {
        let mut editor = Editor {
            package: Some(package::tests::fixture()),
            checked: Some(BTreeMap::new()),
            ..Default::default()
        };
        editor.load_fields().unwrap();
        editor
    }
    #[test]
    fn field_edits_update_dirty_and_validation_state() {
        let mut editor = editor();
        assert!(!editor.dirty());
        assert!(editor.validated());
        editor.fields[0] = "Edited title".into();
        assert!(editor.dirty());
        assert!(!editor.validated());
        editor.commit().unwrap();
        assert!(editor.dirty());
        assert!(editor.validated());
        editor.saved = editor.package.as_ref().unwrap().changes.clone();
        assert!(!editor.dirty());
    }
    #[test]
    fn invalid_field_does_not_partially_commit_other_edits() {
        let mut editor = editor();
        let original = editor
            .package
            .as_ref()
            .unwrap()
            .data(CORE)
            .unwrap()
            .to_vec();
        editor.fields[0] = "Pending title".into();
        editor.fields[7] = "1.5".into();
        assert!(editor.commit().is_err());
        assert_eq!(
            editor.package.as_ref().unwrap().data(CORE).unwrap(),
            original
        );
        assert!(editor.dirty());
    }
    #[test]
    fn xml_edits_synchronize_back_to_properties() {
        let mut editor = editor();
        let mut changed = editor.package.as_ref().unwrap().clone();
        changed
            .set_property(CORE, "title", "Title from XML")
            .unwrap();
        editor.selected = Some(CORE.into());
        editor.buffer = changed.text(CORE).unwrap().into();
        assert!(editor.dirty());
        editor.commit().unwrap();
        assert_eq!(editor.fields[0], "Title from XML");
        assert!(editor.validated());
    }
}
