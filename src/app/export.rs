//! File → Export… (#9, Ctrl+S): the window (rows, columns, format and its options), the export
//! thread with progress and cancel, the result in the status bar and the last exported file.
//! The writers are in `crate::tabexport`; the window follows the table export of RusTrace.

use crate::app::*;
use crate::tabexport::{
    self, ColumnSet, ColumnSpec, Control, Scope, TableFormat, csv::CsvEncoding,
};

/// A running export.
pub(in crate::app) struct ExportJob {
    rx: mpsc::Receiver<Result<(usize, usize), Option<String>>>,
    ctl: Control,
    total: usize,
    target: PathBuf,
}

/// How long the result of an export stays in the status bar.
const RESULT_SECS: u64 = 10;

impl App {
    pub(in crate::app) fn open_export(&mut self) {
        self.show_export = true;
    }

    /// The rows of a scope, in table order. "All" ignores the switches and filters but uses the
    /// host names and the sort order of the table.
    fn export_rows(&mut self, scope: Scope) -> Vec<Row> {
        match scope {
            Scope::Visible => self.view.clone(),
            Scope::Selected => self.selected_rows().into_iter().cloned().collect(),
            Scope::All => {
                let cols = self.cfg.columns.visible();
                let now = now_ms();
                let mut rows: Vec<Row> = self.tracker.rows().cloned().collect();
                for r in &mut rows {
                    self.fill_host_names(r, &cols, now);
                }
                let sort = self.cfg.sort;
                rows.sort_by(|a, b| sort.compare(a, b));
                rows
            }
        }
    }

    fn export_count(&self, scope: Scope) -> usize {
        match scope {
            Scope::Visible => self.view.len(),
            Scope::Selected => self.sel.positions(&self.view_keys()).len(),
            Scope::All => self.tracker.len(),
        }
    }

    fn export_columns(&self, set: ColumnSet) -> Vec<ColumnId> {
        let all: Vec<ColumnId> = self.cfg.columns.0.iter().map(|c| c.id).collect();
        tabexport::export_columns(set, &self.cfg.columns.visible(), &all)
    }

    pub(in crate::app) fn export_running(&self) -> bool {
        self.export_job.is_some()
    }

    pub(in crate::app) fn export_window(&mut self, ctx: &egui::Context) {
        if !self.show_export {
            return;
        }
        let mut open = true;
        let mut start = false;
        let mut changed = false;
        egui::Window::new(t!("export.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                changed |= self.export_choices_ui(ui);
                ui.separator();
                changed |= self.export_format_ui(ui);
                ui.separator();
                let e = &self.cfg.export;
                let rows = self.export_count(e.scope);
                let cols = self.export_columns(e.columns).len();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            rows > 0 && !self.export_running(),
                            egui::Button::new(t!("export.button")),
                        )
                        .clicked()
                    {
                        start = true;
                    }
                    if self.export_running() {
                        self.export_progress(ui);
                    } else if rows == 0 {
                        ui.weak(t!("export.nothing"));
                    } else {
                        ui.weak(t!("export.summary", rows = rows, cols = cols));
                    }
                });
                changed |= ui
                    .checkbox(&mut self.cfg.export.open_after, t!("export.open_after"))
                    .changed();
                if let Some(last) = self.export_last.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(t!("export.last", path = last.display()));
                        if ui.small_button(t!("export.last.open")).clicked() {
                            self.open_exported(&last, false);
                        }
                        if ui.small_button(t!("export.last.folder")).clicked() {
                            self.open_exported(&last, true);
                        }
                        if ui.small_button(t!("export.last.copy")).clicked() {
                            ui.ctx().copy_text(last.display().to_string());
                        }
                    });
                }
            });
        if changed {
            self.settings_changed();
        }
        if start {
            self.start_export();
        }
        self.show_export = open;
    }

    /// Which rows and which columns, with their counts.
    fn export_choices_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.label(RichText::new(t!("export.rows")).strong());
        for (scope, key) in [
            (Scope::Visible, "export.scope.visible"),
            (Scope::Selected, "export.scope.selected"),
            (Scope::All, "export.scope.all"),
        ] {
            let count = self.export_count(scope);
            let enabled = count > 0 || scope != Scope::Selected;
            if ui
                .add_enabled(
                    enabled,
                    egui::RadioButton::new(self.cfg.export.scope == scope, t!(key, count = count)),
                )
                .on_hover_text(t!(&format!("{key}.tip")))
                .clicked()
            {
                self.cfg.export.scope = scope;
                changed = true;
            }
        }
        ui.add_space(4.0);
        ui.label(RichText::new(t!("export.columns")).strong());
        for (set, key) in [
            (ColumnSet::Visible, "export.columns.visible"),
            (ColumnSet::All, "export.columns.all"),
        ] {
            let count = self.export_columns(set).len();
            if ui
                .radio(self.cfg.export.columns == set, t!(key, count = count))
                .clicked()
            {
                self.cfg.export.columns = set;
                changed = true;
            }
        }
        changed
    }

    /// The format and the options that belong to it.
    fn export_format_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let e = &mut self.cfg.export;
        ui.label(RichText::new(t!("export.format")).strong());
        ui.horizontal_wrapped(|ui| {
            for f in TableFormat::ALL {
                if ui
                    .radio(e.format == f, format_name(f))
                    .on_hover_text(t!(&format!("export.format.{}.tip", f.extension())))
                    .clicked()
                {
                    e.format = f;
                    changed = true;
                }
            }
        });
        match e.format {
            TableFormat::Csv => {
                ui.horizontal(|ui| {
                    ui.label(t!("export.csv.separator"));
                    let system = tabexport::csv::system_list_separator();
                    let name = |c: Option<char>| match c {
                        None => t!("export.csv.separator.system", sep = sep_name(system)),
                        Some(c) => sep_name(c),
                    };
                    egui::ComboBox::from_id_salt("export_csv_separator")
                        .selected_text(name(e.csv_separator))
                        .show_ui(ui, |ui| {
                            for c in [None, Some(','), Some(';'), Some('\t'), Some('|')] {
                                changed |= ui
                                    .selectable_value(&mut e.csv_separator, c, name(c))
                                    .changed();
                            }
                        });
                    ui.label(t!("export.csv.encoding"));
                    egui::ComboBox::from_id_salt("export_csv_encoding")
                        .selected_text(encoding_name(e.csv_encoding))
                        .show_ui(ui, |ui| {
                            for enc in [
                                CsvEncoding::Utf8Bom,
                                CsvEncoding::Utf8,
                                CsvEncoding::Utf16Le,
                            ] {
                                changed |= ui
                                    .selectable_value(&mut e.csv_encoding, enc, encoding_name(enc))
                                    .changed();
                            }
                        });
                });
                changed |= ui
                    .checkbox(&mut e.safe_for_excel, t!("export.safe_for_excel"))
                    .on_hover_text(t!("export.safe_for_excel.tip"))
                    .changed();
            }
            TableFormat::Xlsx | TableFormat::Html => {
                changed |= ui
                    .checkbox(&mut e.colors, t!("export.colors"))
                    .on_hover_text(t!("export.colors.tip"))
                    .changed();
            }
            TableFormat::Jsonl => {}
        }
        if e.format != TableFormat::Jsonl {
            changed |= ui.checkbox(&mut e.header, t!("export.header")).changed();
        }
        changed
    }

    /// Asks for the file and starts the export thread.
    fn start_export(&mut self) {
        if self.export_running() {
            return;
        }
        let e = self.cfg.export.clone();
        let rows = self.export_rows(e.scope);
        if rows.is_empty() {
            return;
        }
        let ext = e.format.extension();
        let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let mut dlg = rfd::FileDialog::new()
            .add_filter(format_name(e.format), &[ext])
            .set_file_name(format!("connections_{stamp}.{ext}"));
        if let Some(dir) = self.export_last.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        }
        let Some(target) = dlg.save_file() else {
            return;
        };
        // A known extension typed into the dialog wins over the chosen format.
        let format = TableFormat::from_path(&target).unwrap_or(e.format);
        let sep = e
            .csv_separator
            .unwrap_or_else(tabexport::csv::system_list_separator);
        let now = now_ms();
        let job = tabexport::Job {
            cols: self
                .export_columns(e.columns)
                .into_iter()
                .map(ColumnSpec::new)
                .collect(),
            format,
            header: e.header,
            now,
            csv: tabexport::csv::CsvOptions::new(sep, e.csv_encoding, e.safe_for_excel),
            xlsx: tabexport::xlsx::XlsxOptions {
                colors: e.colors,
                sheet_name: t!("export.sheet"),
                ..Default::default()
            },
            html_title: t!("export.html.title", host = computer_name()),
            html_info: self.export_info(rows.len(), now),
            html_colors: e.colors,
            rows,
        };
        let ctl = Control::default();
        let total = job.rows.len();
        let (tx, rx) = mpsc::channel();
        let repaint = self.ctx.clone();
        let (thread_ctl, path) = (ctl.clone(), target.clone());
        let spawned = std::thread::Builder::new()
            .name("mmportlens-export".into())
            .spawn(move || {
                let res = tabexport::run(&job, &path, &thread_ctl).map_err(|e| {
                    (e.kind() != std::io::ErrorKind::Interrupted).then(|| e.to_string())
                });
                let _ = tx.send(res);
                repaint.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.export_job = Some(ExportJob {
                    rx,
                    ctl,
                    total,
                    target,
                })
            }
            Err(e) => self.export_message(t!("export.error", error = e), true),
        }
    }

    /// Lines under the heading of the HTML report: time, rows, the filters in use.
    fn export_info(&self, rows: usize, now: u64) -> Vec<String> {
        let mut info = vec![
            t!("export.html.time", time = crate::timefmt::date_time(now)),
            t!("export.html.rows", rows = rows),
        ];
        let mut filters: Vec<String> = Vec::new();
        if !self.filter.trim().is_empty() {
            filters.push(format!("\"{}\"", self.filter.trim()));
        }
        for (c, text) in self.column_filter_list() {
            filters.push(format!("{}: {text}", c.title()));
        }
        if !filters.is_empty() && self.cfg.export.scope != Scope::All {
            info.push(t!("export.html.filters", filters = filters.join(", ")));
        }
        info
    }

    fn export_message(&mut self, text: String, error: bool) {
        self.export_result = Some((
            text,
            error,
            Instant::now() + Duration::from_secs(RESULT_SECS),
        ));
    }

    /// Takes over the result of the export thread (every frame).
    pub(in crate::app) fn check_export(&mut self) {
        let Some(job) = &self.export_job else {
            return;
        };
        match job.rx.try_recv() {
            Ok(res) => {
                let target = job.target.clone();
                self.export_job = None;
                match res {
                    Ok((rows, cut)) => {
                        let msg = if cut > 0 {
                            t!(
                                "export.done.truncated",
                                rows = rows,
                                path = target.display(),
                                cells = cut
                            )
                        } else {
                            t!("export.done", rows = rows, path = target.display())
                        };
                        self.export_message(msg, false);
                        if self.cfg.export.open_after {
                            self.open_exported(&target, false);
                        }
                        self.export_last = Some(target);
                    }
                    Err(None) => self.export_message(t!("export.cancelled"), false),
                    Err(Some(e)) => self.export_message(t!("export.error", error = e), true),
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.export_job = None;
                self.export_message(t!("export.error", error = "internal error"), true);
            }
        }
    }

    /// Progress of a running export with a cancel button.
    fn export_progress(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.export_job else {
            return;
        };
        let done = job.ctl.done.load(Ordering::Relaxed).min(job.total);
        let frac = done as f32 / job.total.max(1) as f32;
        ui.spinner();
        ui.add(
            egui::ProgressBar::new(frac)
                .text(t!("export.progress", done = done, total = job.total))
                .desired_width(160.0),
        );
        if ui.small_button(t!("export.cancel")).clicked() {
            job.ctl.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The status bar: a running export, then its result for a few seconds.
    pub(in crate::app) fn export_status(&mut self, ui: &mut egui::Ui) {
        if self.export_running() {
            ui.separator();
            self.export_progress(ui);
            return;
        }
        let Some((text, error, until)) = &self.export_result else {
            return;
        };
        if Instant::now() > *until {
            self.export_result = None;
            return;
        }
        ui.separator();
        let color = if *error {
            ui.visuals().error_fg_color
        } else {
            ui.visuals().text_color()
        };
        ui.colored_label(color, text.as_str());
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    /// The last export: open it, or show it in its folder.
    fn open_exported(&mut self, path: &std::path::Path, folder: bool) {
        let res = if folder {
            show_in_folder(path)
        } else {
            open::that_detached(path)
        };
        if let Err(e) = res {
            self.export_message(
                t!("export.open_error", path = path.display(), error = e),
                true,
            );
        }
    }
}

fn format_name(f: TableFormat) -> String {
    t!(&format!("export.format.{}", f.extension()))
}

fn sep_name(c: char) -> String {
    match c {
        '\t' => t!("export.csv.tab"),
        c => format!("{c}"),
    }
}

fn encoding_name(e: CsvEncoding) -> String {
    match e {
        CsvEncoding::Utf8Bom => t!("export.csv.utf8bom"),
        CsvEncoding::Utf8 => "UTF-8".into(),
        CsvEncoding::Utf16Le => "UTF-16 LE".into(),
    }
}

/// Name of this computer for the heading of the HTML report.
fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
}

/// Opens Explorer with the file selected (other systems: its folder) – from RusTrace.
pub(in crate::app) fn show_in_folder(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `/select,"path"` must reach Explorer as one argument with the quotes.
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        open::that_detached(path.parent().unwrap_or(path))
    }
}
