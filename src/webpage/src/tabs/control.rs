use eframe::egui::{self, CollapsingHeader, Context};
use egui_extras::{Column, TableBuilder};
use ehttp::Request;
use std::collections::BTreeMap;

const CONTROL_VEHICLES: &str = "rest/vehicles";

#[derive(Default)]
pub struct ControlTab {
    pub editor_open: bool,
    pub editor_parameter_name: String,
    pub editor_new_value: String,
    pub search_query: String,
}


impl ControlTab {
    pub fn show(
        &mut self,
        ctx: &Context,
        vehicles: &serde_json::Value,
        parameters: &BTreeMap<String, serde_json::Value>,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Control");
            if ui.button("Arm").clicked() {
                let mut request = Request::post(
                    format!("/{CONTROL_VEHICLES}/arm"),
                    "{
                    \"system_id\": 1,
                    \"component_id\": 1,
                    \"force\": true
                }"
                    .into(),
                );
                request.headers.insert("Content-Type", "application/json");
                ehttp::fetch(request, |res| {
                    log::info!("Arm response: {res:?}");
                });
            }
            if ui.button("Disarm").clicked() {
                let mut request = Request::post(
                    format!("/{CONTROL_VEHICLES}/disarm"),
                    "{
                    \"system_id\": 1,
                    \"component_id\": 1,
                    \"force\": true
                }"
                    .into(),
                );
                request.headers.insert("Content-Type", "application/json");
                ehttp::fetch(request, |res| {
                    log::info!("Disarm response: {res:?}");
                });
            }

            if !vehicles.is_object() {
                return;
            }

            let system_id = vehicles["vehicle_id"].as_u64().unwrap_or(0) as u8;
            let message_header = CollapsingHeader::new(format!("Vehicle: {system_id}",))
                .default_open(true)
                .id_salt(ui.make_persistent_id(format!("vehicle_control_{system_id}")));

            message_header.show(ui, |ui| {
                let id_salt = ui.make_persistent_id(format!("vehicle_table_control_{system_id}"));
                TableBuilder::new(ui)
                    .id_salt(id_salt)
                    .striped(true)
                    .column(Column::auto().at_least(100.))
                    .column(Column::remainder().at_least(150.))
                    .body(|mut body| {
                        if let Some(values) = vehicles.as_object() {
                            for (key, value) in values {
                                if key == "parameters" || key == "components" {
                                    continue;
                                }
                                if !value.is_object() {
                                    body.row(15., |mut row| {
                                        row.col(|ui| {
                                            let _label = ui.label(key);
                                        });
                                        row.col(|ui| {
                                            let _label = ui.label(value.to_string());
                                        });
                                    });
                                } else {
                                    body.row(15., |mut row| {
                                        row.col(|ui| {
                                            let _label = ui.label(key);
                                        });
                                    });
                                    for (key, value) in value.as_object().unwrap() {
                                        body.row(10., |mut row| {
                                            row.col(|ui| {
                                                let _label = ui.label(format!("\t{key}"));
                                            });

                                            row.col(|ui| {
                                                let _label = ui.label(format!("\t{value}"));
                                            });
                                        });
                                    }
                                }
                            }
                        }
                    });


                let components = vehicles["components"].as_object().unwrap();
                for (key, value) in components {
                    let component_id = key;
                    let typ = value["type"].as_str().unwrap_or("Unknown");
                    let component_header = CollapsingHeader::new(format!("Component {typ}: {component_id}"))
                        .default_open(true)
                        .id_salt(ui.make_persistent_id(format!("vehicle_control_{system_id}_{component_id}_{key}")));
                    component_header.show(ui, |ui| {
                        for (key, value) in value.as_object().unwrap() {
                                let id_salt = ui.make_persistent_id(format!("vehicle_table_table_{system_id}_{component_id}_{key}"));
                                TableBuilder::new(ui)
                                    .id_salt(id_salt)
                                    .striped(true)
                                    .column(Column::auto().at_least(100.))
                                    .column(Column::remainder().at_least(150.))
                                    .body(|mut body| {
                                        if !value.is_object() {
                                            body.row(15., |mut row| {
                                                row.col(|ui| {
                                                    let _label = ui.label(key);
                                                });
                                                row.col(|ui| {
                                                    let _label = ui.label(value.to_string());
                                                });
                                            });
                                        } else {
                                            body.row(15., |mut row| {
                                                row.col(|ui| {
                                                    let _label = ui.label(key);
                                                });
                                            });
                                            for (key, value) in value.as_object().unwrap() {
                                                body.row(10., |mut row| {
                                                    row.col(|ui| {
                                                        let _label = ui.label(format!("\t{key}"));
                                                    });

                                                    row.col(|ui| {
                                                        let _label = ui.label(format!("\t{value}"));
                                                    });
                                                });
                                            }
                                        }
                                    });
                        }
                    });
                }

                ui.horizontal_top(|ui| {
                    ui.strong("Search parameter:");
                    ui.text_edit_singleline(&mut self.search_query);
                });
                let id_salt =
                    ui.make_persistent_id(format!("vehicle_table_control_parameters{system_id}"));
                TableBuilder::new(ui)
                    .id_salt(id_salt)
                    .column(Column::auto_with_initial_suggestion(200.0).resizable(true))
                    .column(Column::auto_with_initial_suggestion(200.0).resizable(true))
                    .column(Column::remainder())
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.label("Name");
                        });
                        header.col(|ui| {
                            ui.label("Value");
                        });
                        header.col(|ui| {
                            ui.label("Description");
                        });
                    })
                    .body(|body| {
                        let filtered_params: Vec<_> = parameters
                            .iter()
                            .filter(|(key, _)| {
                                self.search_query.is_empty()
                                    || key
                                        .to_lowercase()
                                        .contains(&self.search_query.to_lowercase())
                            })
                            .collect();

                        body.rows(20.0, filtered_params.len(), |mut row| {
                            let (key, value) = filtered_params[row.index()];
                            row.col(|ui| {
                                if ui.button(key).clicked() {
                                    self.editor_open = true;
                                    self.editor_parameter_name = key.clone();
                                    self.editor_new_value = value["parameter"]["value"].to_string();
                                }
                            });
                            row.col(|ui| {
                                ui.label(value["parameter"]["value"].to_string());
                            });
                            row.col(|ui| {
                                ui.label(value["metadata"]["description"].to_string());
                            });
                        });
                    });

                egui::Window::new("Parameter Editor")
                    .open(&mut self.editor_open)
                    .show(ctx, |ui| {
                        ui.label(format!("Name: {}", self.editor_parameter_name));
                        let parameter = &parameters[&self.editor_parameter_name];
                        ui.label(format!(
                            "Current value: {}",
                            parameter["parameter"]["value"].to_string()
                        ));
                        ui.separator();
                        ui.label(format!(
                            "Type: {}",
                            parameter["parameter"]["param_type"].to_string()
                        ));
                        ui.label(format!(
                            "Increment: {}",
                            parameter["metadata"]["increment"].to_string()
                        ));
                        ui.label(format!(
                            "Reboot required: {}",
                            parameter["metadata"]["reboot_required"].to_string()
                        ));
                        ui.label(format!(
                            "Max/Min: {} / {}",
                            parameter["metadata"]["range"]["high"].to_string(),
                            parameter["metadata"]["range"]["low"].to_string()
                        ));
                        ui.label(format!(
                            "Units: {}",
                            parameter["metadata"]["unit"].to_string()
                        ));
                        ui.label(format!(
                            "User level: {}",
                            parameter["metadata"]["user_level"].to_string()
                        ));
                        ui.separator();
                        ui.text_edit_singleline(&mut self.editor_new_value);
                        if ui.button("Apply").clicked() {
                            let mut request = Request::post(
                                "/rest/vehicles/set_parameter",
                                serde_json::json!({
                                    "vehicle_id": 1,
                                    "component_id": 1,
                                    "parameter_name": self.editor_parameter_name,
                                    "value": self.editor_new_value.parse::<f64>().unwrap_or(0.0)
                                })
                                .to_string()
                                .into_bytes(),
                            );
                            request.headers.insert("Content-Type", "application/json");
                            ehttp::fetch(request, |res| {
                                log::info!("Apply parameter response: {res:?}");
                            });
                        }
                    });
            });
        });
    }
}
