use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Read;

fn default_jobs() -> usize {
    20
}

#[derive(Debug, Deserialize, Serialize)]
pub struct User {
    pub name: String,
    pub email: String,
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
    pub user: User,
    pub comments: HashMap<String, String>,
    #[serde(default = "default_jobs")]
    pub jobs: usize,
    pub repos: Vec<Repos>,
}

pub fn load() -> Result<GitppSetting, Box<dyn Error>> {
    let file_result = File::open("gitpp.yaml").or_else(|_| File::open("gitpp.yml"));

    let mut file = match file_result {
        Ok(f) => f,
        Err(_) => {
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
