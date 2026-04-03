use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Read;
use std::path::Path;

fn default_jobs() -> usize {
    20
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Repos {
    pub enabled: bool,
    pub remote: String,
    pub branch: String,
    pub group: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GitppSetting {
    #[serde(default)]
    pub config: HashMap<String, String>,
    pub comments: HashMap<String, String>,
    #[serde(default = "default_jobs")]
    pub jobs: usize,
    pub repos: Vec<Repos>,
}

pub fn load(config_path: Option<&Path>) -> Result<GitppSetting, Box<dyn Error>> {
    let file_result = match config_path {
        Some(path) => File::open(path).map_err(|e| {
            std::io::Error::new(
                e.kind(),
                format!("Cannot open config file '{}': {e}", path.display()),
            )
        }),
        None => File::open("gitpp.yaml").or_else(|_| File::open("gitpp.yml")),
    };

    let mut file = match file_result {
        Ok(f) => f,
        Err(e) => {
            if config_path.is_some() {
                return Err(e.to_string().into());
            }
            return Err("gitpp.yaml (or gitpp.yml) not found.".into());
        }
    };

    let mut yaml_text = String::new();
    file.read_to_string(&mut yaml_text)?;

    if yaml_text.trim().is_empty() {
        return Err("gitpp.yaml is empty.".into());
    }

    let setting: GitppSetting = serde_yaml::from_str(&yaml_text)?;
    Ok(setting)
}
