//! Minimal reader for an extracted AC ini file: `[SECTION]`, `KEY=VALUE`, `;` comments.
//!
//! Not a port of AC's `INIReader`. Numbers are parsed as f64 and narrowed to f32; whether
//! `INIReader::getFloat` rounds the same way in every case has not been confirmed.

use std::collections::HashMap;
use std::path::Path;

pub struct Section {
    pub name: String,
    values: HashMap<String, String>,
}

pub struct Ini {
    sections: Vec<Section>,
}

impl Ini {
    pub fn load(path: &Path) -> Result<Ini, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Ini::parse(&String::from_utf8_lossy(&bytes)))
    }

    pub fn parse(text: &str) -> Ini {
        let mut sections: Vec<Section> = Vec::new();
        for raw in text.lines() {
            let line = raw.split(';').next().unwrap_or("").trim();
            if line.starts_with('[') && line.ends_with(']') {
                sections.push(Section {
                    name: line[1..line.len() - 1].trim().to_string(),
                    values: HashMap::new(),
                });
            } else if let (Some((key, value)), Some(sec)) =
                (line.split_once('='), sections.last_mut())
            {
                sec.values.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
        Ini { sections }
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }
}

impl Section {
    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn float_opt(&self, key: &str) -> Result<Option<f32>, String> {
        match self.values.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<f64>()
                .map(|x| Some(x as f32))
                .map_err(|_| format!("[{}] {key}={v}: not a number", self.name)),
        }
    }

    pub fn float(&self, key: &str) -> Result<f32, String> {
        self.float_opt(key)?.ok_or_else(|| format!("[{}] is missing {key}", self.name))
    }

    pub fn int(&self, key: &str) -> Result<i32, String> {
        Ok(self.float(key)? as i32)
    }
}
