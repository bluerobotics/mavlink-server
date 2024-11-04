use std::sync::Arc;

use egui::mutex::Mutex;
use serde::{Deserialize, Serialize};
use url::Url;
use web_sys::window;

#[derive(Debug, Serialize, Deserialize)]
pub struct Frequency {
    pub frequency: f32,
}

pub fn stats_frequency(stats_frequency: &Arc<Mutex<f32>>) {
    let location = window().unwrap().location();
    let host = location.host().unwrap();
    let protocol = location.protocol().unwrap();
    let url = Url::parse(&format!("{protocol}//{host}/stats/frequency"))
        .unwrap()
        .to_string();

    let mut request = ehttp::Request::get(url);
    request.headers.insert("Content-Type", "application/json");

    let stats_frequency = stats_frequency.clone();
    ehttp::fetch(
        request,
        move |result: ehttp::Result<ehttp::Response>| match result {
            Ok(response) => {
                if let Ok(Frequency { frequency }) = response.json() {
                    *stats_frequency.lock() = frequency;
                }
            }
            Err(error) => log::error!("Status code: {error:?}"),
        },
    );
}

pub fn set_stats_frequency(stats_frequency: &Arc<Mutex<f32>>, new_stats_frequency: f32) {
    *stats_frequency.lock() = new_stats_frequency;

    let location = window().unwrap().location();
    let host = location.host().unwrap();
    let protocol = location.protocol().unwrap();
    let url = Url::parse(&format!("{protocol}//{host}/stats/frequency"))
        .unwrap()
        .to_string();
    let body = serde_json::json!({
        "frequency": new_stats_frequency,
    })
    .to_string()
    .as_bytes()
    .to_vec();

    let mut request = ehttp::Request::post(url, body);
    request.headers.insert("Content-Type", "application/json");

    let stats_frequency = stats_frequency.clone();
    ehttp::fetch(
        request,
        move |result: ehttp::Result<ehttp::Response>| match result {
            Ok(response) => {
                if let Some(json) = response.text() {
                    if let Ok(Frequency { frequency }) = serde_json::from_str(json) {
                        *stats_frequency.lock() = frequency;
                    }
                }
            }
            Err(error) => log::error!("Status code: {error:?}"),
        },
    );
}
