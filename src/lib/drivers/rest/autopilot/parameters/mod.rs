use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

static ASSETS_DIR: Dir = include_dir!(
    "$CARGO_MANIFEST_DIR/src/lib/drivers/rest/autopilot/parameters/ardupilot_parameters",
    "**/*.json"
);

// A function that caches it return value, should return a map of vehicle type by version
pub fn parameters() -> HashMap<String, Vec<String>> {
    let mut parameters: HashMap<String, Vec<String>> = HashMap::new();
    for dir in ASSETS_DIR.dirs() {
        let directory = dir
            .path()
            .file_name()
            .expect("Files should exist in compile time")
            .to_str()
            .unwrap();

        if !directory.contains("-") {
            continue;
        }

        let vehicle_type = directory.split("-").next().unwrap();
        let version = directory.split("-").nth(1).unwrap();

        if parameters.contains_key(vehicle_type) {
            parameters
                .get_mut(vehicle_type)
                .unwrap()
                .push(version.to_string());
        } else {
            parameters.insert(vehicle_type.to_string(), vec![version.to_string()]);
        }
    }
    parameters
}

#[cached::proc_macro::cached]
pub fn get_parameters(vehicle_type: String, version: String) -> HashMap<String, Parameter> {
    let file = ASSETS_DIR
        .get_dir(format!("{}-{}", vehicle_type, version))
        .and_then(|dir| {
            dir.files()
                .filter(|file| file.path().extension().unwrap_or_default() == "json")
                .next()
        });

    if let Some(file) = file {
        let content = file.contents_utf8().unwrap();
        let mut json_value: serde_json::Value = serde_json::from_str(content).unwrap();

        // TODO: Deal with the version number
        if let Some(obj) = json_value.as_object_mut() {
            obj.remove("json");
        }

        let full: HashMap<String, HashMap<String, Parameter>> =
            serde_json::from_value(json_value).unwrap();
        let mut parameters: HashMap<String, Parameter> = HashMap::new();
        for (_key, value) in full {
            for (key2, value2) in value {
                parameters.insert(key2, value2);
            }
        }
        parameters
    } else {
        Default::default()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Parameter {
    #[serde(rename(serialize = "description", deserialize = "Description"))]
    pub description: String,
    #[serde(rename(serialize = "display_name", deserialize = "DisplayName"))]
    pub display_name: String,
    #[serde(
        rename(serialize = "increment", deserialize = "Increment"),
        default,
        deserialize_with = "str_to_f64_opt"
    )]
    pub increment: Option<f64>,
    #[serde(
        rename(serialize = "reboot_required", deserialize = "RebootRequired"),
        default,
        deserialize_with = "str_to_bool"
    )]
    pub reboot_required: bool,
    #[serde(rename(serialize = "range", deserialize = "Range"), default)]
    pub range: Option<Range>,
    #[serde(rename(serialize = "units", deserialize = "Units"), default)]
    pub units: Option<String>,
    #[serde(rename(serialize = "user_level", deserialize = "User"), default)]
    pub user_level: String,
    #[serde(rename(serialize = "values", deserialize = "Values"), default)]
    pub values: Option<HashMap<String, String>>,
    #[serde(rename(serialize = "bitmask", deserialize = "Bitmask"), default)]
    pub bitmask: Option<HashMap<String, String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Range {
    #[serde(deserialize_with = "str_to_f64")]
    pub high: f64,
    #[serde(deserialize_with = "str_to_f64")]
    pub low: f64,
}

fn str_to_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => s.to_lowercase().parse().map_err(serde::de::Error::custom),
        serde_json::Value::Bool(b) => Ok(b),
        other => Err(serde::de::Error::custom(format!(
            "Unexpected type: {:?}",
            other
        ))),
    }
}

fn str_to_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => s.parse().map_err(serde::de::Error::custom),
        serde_json::Value::Number(num) => num
            .as_f64()
            .ok_or_else(|| serde::de::Error::custom("Invalid number")),
        other => Err(serde::de::Error::custom(format!(
            "Unexpected type: {:?}",
            other
        ))),
    }
}

fn str_to_f64_opt<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(s) => s.parse().map(Some).map_err(serde::de::Error::custom),
        serde_json::Value::Number(n) => Ok(n.as_f64()),
        _ => Ok(None),
    }
}
