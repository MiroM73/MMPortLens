//! View → Country and ASN database… (#11): the offline database files of `crate::geo`, their
//! state, and where to download them. Loading runs in the background; the table is rebuilt
//! when it finishes (`App::receive` watches `geo::generation`).

use crate::app::*;
use crate::geo::Status;

/// Where the supported files come from (shown as links).
const ASN_URL: &str = "https://iptoasn.com/";
const COUNTRY_URL: &str = "https://db-ip.com/db/download/ip-to-country-lite";

/// Which file a button changes.
#[derive(Clone, Copy)]
enum Which {
    Asn,
    Country,
}

impl App {
    /// Loads the database files of the settings in the background (at start and after a change).
    pub(in crate::app) fn load_geo(&mut self) {
        let ctx = self.ctx.clone();
        crate::geo::start_loading(&self.cfg.geo, move || ctx.request_repaint());
    }

    pub(in crate::app) fn geo_window(&mut self, ctx: &egui::Context) {
        if !self.show_geo {
            return;
        }
        let mut open = true;
        let mut choose: Option<Which> = None;
        let mut clear: Option<Which> = None;
        let mut reload = false;
        let status = crate::geo::status();
        let geo = self.cfg.geo.clone();
        egui::Window::new(t!("geo.title"))
            .id(egui::Id::unique("geo_window"))
            .open(&mut open)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.label(t!("geo.intro"));
                ui.add_space(6.0);
                egui::Grid::new("geo_files")
                    .num_columns(4)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        for (which, label, path, url) in [
                            (Which::Asn, t!("geo.asn_file"), &geo.asn_file, ASN_URL),
                            (
                                Which::Country,
                                t!("geo.country_file"),
                                &geo.country_file,
                                COUNTRY_URL,
                            ),
                        ] {
                            ui.label(label);
                            match path {
                                Some(p) => ui.monospace(p.display().to_string()),
                                None => ui.weak(t!("geo.no_file")),
                            };
                            ui.horizontal(|ui| {
                                if ui.button(t!("geo.choose")).clicked() {
                                    choose = Some(which);
                                }
                                if path.is_some()
                                    && ui.button("✖").on_hover_text(t!("geo.clear")).clicked()
                                {
                                    clear = Some(which);
                                }
                            });
                            ui.hyperlink_to(t!("geo.download"), url);
                            ui.end_row();
                        }
                    });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (text, error) = match &status {
                        Status::None => (t!("geo.status.none"), false),
                        Status::Loading => (t!("geo.status.loading"), false),
                        Status::Ready(asn, country) => {
                            (t!("geo.status.ready", asn = asn, country = country), false)
                        }
                        Status::Failed(e) => (t!("geo.error", error = e), true),
                    };
                    if error {
                        ui.colored_label(ui.visuals().error_fg_color, text);
                    } else {
                        ui.label(text);
                    }
                    if geo.any()
                        && status != Status::Loading
                        && ui.button(t!("geo.reload")).clicked()
                    {
                        reload = true;
                    }
                });
                ui.add_space(4.0);
                ui.weak(t!("geo.columns_hint"));
            });
        if !open {
            self.show_geo = false;
        }
        let mut changed = false;
        if let Some(which) = choose {
            let dlg = rfd::FileDialog::new()
                .add_filter(t!("geo.file_type"), &["tsv", "csv", "gz", "txt"])
                .add_filter("*", &["*"]);
            if let Some(path) = dlg.pick_file() {
                match which {
                    Which::Asn => self.cfg.geo.asn_file = Some(path),
                    Which::Country => self.cfg.geo.country_file = Some(path),
                }
                changed = true;
            }
        }
        if let Some(which) = clear {
            match which {
                Which::Asn => self.cfg.geo.asn_file = None,
                Which::Country => self.cfg.geo.country_file = None,
            }
            changed = true;
        }
        if changed {
            self.settings_changed();
            // The columns are what the database is for: show them once a file is chosen.
            if self.cfg.geo.any() {
                self.cfg.columns.set_visible(ColumnId::Country, true);
                self.cfg.columns.set_visible(ColumnId::Asn, true);
            }
        }
        if changed || reload {
            self.load_geo();
        }
    }
}
