use regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ParsedReward {
    pub item_name: String,
    pub quantity: i64,
    pub raw_line: String,
}

pub struct LogParser {
    patterns: Vec<Regex>,
}

impl LogParser {
    pub fn new() -> Self {
        let pattern_strings = vec![
            // "received ItemName x2"
            r"(?i)\breceived\s+([A-Za-z][A-Za-z0-9\s'\-]+?)\s+[xX]\s*(\d+)",
            // "reward: ItemName x2"
            r"(?i)\brewards?\s*[:\s]+([A-Za-z][A-Za-z0-9\s'\-]+?)\s+[xX]\s*(\d+)",
            // "Adding item: /path/ItemName x1"
            r"(?i)adding item.*?/([A-Za-z][A-Za-z0-9\s'\-]+?)\s+[xX]\s*(\d+)",
            // "ItemName x2" after mission/fissure keyword
            r"(?i)(?:mission|fissure|syndicate|foundry)[^\n]*?([A-Za-z][A-Za-z0-9\s'\-]{3,40}?)\s+[xX]\s*(\d+)",
            // "You received: ItemName x1"
            r"(?i)you received[:\s]+([A-Za-z][A-Za-z0-9\s'\-]+?)\s+[xX]\s*(\d+)",
        ];

        let patterns = pattern_strings
            .iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect();

        Self { patterns }
    }

    pub fn parse_line(&self, line: &str) -> Option<ParsedReward> {
        for pattern in &self.patterns {
            if let Some(caps) = pattern.captures(line) {
                if let (Some(name), Some(qty_str)) = (caps.get(1), caps.get(2)) {
                    let item_name = name.as_str().trim().to_string();
                    let quantity: i64 = qty_str.as_str().parse().unwrap_or(1);

                    if item_name.len() < 3 || item_name.len() > 80 {
                        continue;
                    }

                    return Some(ParsedReward {
                        item_name,
                        quantity,
                        raw_line: line.to_string(),
                    });
                }
            }
        }
        None
    }

    pub fn parse_file_from_offset(&self, path: &Path, offset: u64) -> (Vec<ParsedReward>, u64) {
        let file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return (vec![], offset),
        };

        let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

        // File was rotated (Warframe restarted) — start from beginning
        let actual_offset = if offset > file_size { 0 } else { offset };

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(actual_offset)).is_err() {
            return (vec![], actual_offset);
        }

        let mut rewards = vec![];
        let mut new_offset = actual_offset;

        for line in reader.lines() {
            match line {
                Ok(l) => {
                    new_offset += l.len() as u64 + 1;
                    if let Some(reward) = self.parse_line(&l) {
                        rewards.push(reward);
                    }
                }
                Err(_) => break,
            }
        }

        (rewards, new_offset)
    }
}

pub fn get_default_log_path() -> Option<String> {
    dirs::data_local_dir()
        .map(|data_dir| {
            #[cfg(target_os = "linux")]
            {
                linux_log_path(&data_dir)
            }
            #[cfg(not(target_os = "linux"))]
            {
                data_dir.join("Warframe").join("EE.log")
            }
        })
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg(target_os = "linux")]
fn linux_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("Steam/steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log")
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    #[test]
    fn builds_proton_log_path_from_linux_data_dir() {
        let path = linux_log_path(Path::new("/home/player/.local/share"));
        assert_eq!(
            path,
            Path::new("/home/player/.local/share/Steam/steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log")
        );
    }
}
