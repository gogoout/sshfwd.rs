use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedForward {
    pub remote_port: u16,
    pub local_port: u16,
}

type ForwardsFile = HashMap<String, Vec<PersistedForward>>;

fn forwards_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".sshfwd").join("forwards.json")
}

pub fn load_forwards(destination: &str) -> Vec<PersistedForward> {
    let path = forwards_path();
    let data = match fs::read_to_string(&path) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let file: ForwardsFile = match serde_json::from_str(&data) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    file.get(destination).cloned().unwrap_or_default()
}

pub fn save_forwards(destination: &str, forwards: &[PersistedForward]) {
    let path = forwards_path();

    // Read existing file to preserve other destinations
    let mut file: ForwardsFile = fs::read_to_string(&path)
        .ok()
        .and_then(|d| serde_json::from_str(&d).ok())
        .unwrap_or_default();

    if forwards.is_empty() {
        file.remove(destination);
    } else {
        file.insert(destination.to_string(), forwards.to_vec());
    }

    // Create directory if needed
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(json) = serde_json::to_string_pretty(&file) {
        let _ = fs::write(&path, json);
    }
}
