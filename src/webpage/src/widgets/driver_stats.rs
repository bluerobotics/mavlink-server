use chrono::{DateTime, Utc};
use eframe::egui::{self, CollapsingHeader};
use egui_extras::{Column, TableBuilder};
use uuid::Uuid;

use crate::stats::{
    ByteStatsHistorical, DelayStatsHistorical, MessageStatsHistorical, StatsInner,
    drivers_stats::{DriverStats, DriversStatsHistorical},
};

pub struct DriverStatsWidget<'a> {
    pub now: DateTime<Utc>,
    pub drivers_stats: &'a DriversStatsHistorical,
}

impl<'a> DriverStatsWidget<'a> {
    pub fn new(now: DateTime<Utc>, drivers_stats: &'a DriversStatsHistorical) -> Self {
        Self { now, drivers_stats }
    }

    pub fn show(&self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("scrolldrivers_stats")
            .show(ui, |ui| {
                let drivers_stats = &self.drivers_stats.drivers_stats;

                for (driver_uuid, driver_stats) in drivers_stats {
                    self.show_driver_stats(ui, driver_uuid, driver_stats);
                }
            });
    }

    fn show_driver_stats(
        &self,
        ui: &mut egui::Ui,
        driver_uuid: &Uuid,
        driver_stats: &DriverStats<
            ByteStatsHistorical,
            MessageStatsHistorical,
            DelayStatsHistorical,
        >,
    ) {
        let driver_name = &driver_stats.name;
        let driver_type = &driver_stats.driver_type;

        let drivers_stats_id_str = format!("driver_stats_{driver_uuid}");
        let drivers_stats_id_hash = ui.make_persistent_id(&drivers_stats_id_str);

        CollapsingHeader::new(format!("Driver Stats: {driver_name}"))
            .id_salt(drivers_stats_id_hash)
            .default_open(true)
            .show(ui, |ui| {
                let stats = &driver_stats.stats;

                let salt = ui.make_persistent_id(format!("driver_stats_info_table_{driver_uuid}"));
                TableBuilder::new(ui)
                    .id_salt(salt)
                    .striped(true)
                    .column(Column::auto().at_least(120.))
                    .column(Column::remainder().at_least(150.))
                    .body(|mut body| {
                        body.row(15., |mut row| {
                            row.col(|ui| {
                                ui.label("Name");
                            });
                            row.col(|ui| {
                                ui.label(driver_name);
                            });
                        });

                        body.row(15., |mut row| {
                            row.col(|ui| {
                                ui.label("Type");
                            });
                            row.col(|ui| {
                                ui.label(driver_type);
                            });
                        });

                        body.row(15., |mut row| {
                            row.col(|ui| {
                                ui.label("UUID");
                            });
                            row.col(|ui| {
                                ui.label(driver_uuid.to_string());
                            });
                        });
                    });

                self.show_stats_section(ui, "Input", driver_uuid, stats.input.as_ref());
                self.show_stats_section(ui, "Output", driver_uuid, stats.output.as_ref());
            });
    }

    fn show_stats_section(
        &self,
        ui: &mut egui::Ui,
        section_name: &str,
        driver_uuid: &Uuid,
        stats: Option<
            &StatsInner<ByteStatsHistorical, MessageStatsHistorical, DelayStatsHistorical>,
        >,
    ) {
        CollapsingHeader::new(section_name)
            .id_salt(ui.make_persistent_id(format!(
                "driver_stats_{}_{}",
                section_name.to_lowercase(),
                driver_uuid
            )))
            .default_open(true)
            .show(ui, |ui| {
                if let Some(stats) = stats {
                    let id_salt = ui.make_persistent_id(format!(
                        "driver_stats_{}_table_{}",
                        section_name.to_lowercase(),
                        driver_uuid
                    ));
                    TableBuilder::new(ui)
                        .id_salt(id_salt)
                        .striped(true)
                        .column(Column::auto().at_least(100.))
                        .column(Column::remainder().at_least(150.))
                        .body(|mut body| {
                            crate::app::add_label_and_plot_all_stats(&mut body, self.now, stats);
                        });
                }
            });
    }
}
