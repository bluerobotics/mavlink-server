use std::sync::Arc;

use eframe::egui::{self, Context};
use egui::mutex::Mutex;
use egui_autocomplete::AutoCompleteTextEdit;
use ehttp::{Request, Response};

use crate::messages_names::NAMES as mavlink_names;

const MAVLINK_HELPER: &str = "rest/helper";
const MAVLINK_POST: &str = "rest/mavlink";

pub struct HelperTab {
    pub search_query: String,
    pub mavlink_code: Arc<Mutex<String>>,
    pub mavlink_code_post_response: Arc<Mutex<String>>,
}

impl Default for HelperTab {
    fn default() -> Self {
        Self {
            search_query: "HEARTBEAT".to_string(),
            mavlink_code: Arc::new(Mutex::new(String::new())),
            mavlink_code_post_response: Arc::new(Mutex::new(String::new())),
        }
    }
}

impl HelperTab {
    pub fn show(&mut self, ctx: &Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.label("Search:");
                ui.add(AutoCompleteTextEdit::new(
                    &mut self.search_query,
                    mavlink_names,
                ));

                if ui.button("Find").clicked() {
                    let mavlink_code = self.mavlink_code.clone();
                    ehttp::fetch(
                        Request::get(format!("/{MAVLINK_HELPER}?name={}", self.search_query)),
                        move |res| {
                            let mut s = mavlink_code.lock();
                            *s = match res {
                                Ok(Response { bytes, .. }) => String::from_utf8(bytes).unwrap(),
                                Err(error) => format!("Error: {error:?}"),
                            };
                        },
                    );
                }
            });

            let mut theme =
                egui_extras::syntax_highlighting::CodeTheme::from_memory(ui.ctx(), ui.style());
            ui.collapsing("Theme", |ui| {
                ui.group(|ui| {
                    theme.ui(ui);
                    theme.clone().store_in_memory(ui.ctx());
                });
            });

            let mut layouter = |ui: &egui::Ui, string: &str, wrap_width: f32| {
                let mut layout_job = egui_extras::syntax_highlighting::highlight(
                    ui.ctx(),
                    ui.style(),
                    &theme,
                    string,
                    "js",
                );
                layout_job.wrap.max_width = wrap_width;
                ui.fonts(|f| f.layout_job(layout_job))
            };

            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut mavlink_code = self.mavlink_code.lock().to_owned();
                ui.add(
                    egui::TextEdit::multiline(&mut mavlink_code)
                        .font(egui::TextStyle::Monospace)
                        .code_editor()
                        .desired_rows(30)
                        .lock_focus(true)
                        .desired_width(500.0)
                        .layouter(&mut layouter),
                );
                *self.mavlink_code.lock() = mavlink_code;
            });

            ui.horizontal_top(|ui| {
                if ui.button("Send").clicked() {
                    let mavlink_code = self.mavlink_code.lock();

                    let mut request = Request::post(
                        format!("/{MAVLINK_POST}"),
                        mavlink_code.clone().into_bytes(),
                    );
                    request.headers.insert("Content-Type", "application/json");

                    let mavlink_code_post_response = self.mavlink_code_post_response.clone();
                    *mavlink_code_post_response.lock() = String::default();
                    ehttp::fetch(request, move |res| {
                        let mut s = mavlink_code_post_response.lock();
                        *s = match res {
                            Ok(Response { bytes, .. }) => String::from_utf8(bytes).unwrap(),
                            Err(error) => format!("Error: {error:?}"),
                        };
                    });
                }
                ui.label((*self.mavlink_code_post_response.lock()).to_string());
            });
        });
    }
}
