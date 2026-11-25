//! 3D LUTs in the `.cube` format, applied by the color correction filter.
//! Parsed once per path; the renderer finds them by `lut_id`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// A cube of `size³` RGB entries, red varying fastest (the `.cube` order).
#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    pub size: usize,
    pub data: Vec<[f32; 3]>,
}

pub const MAX_LUT_SIZE: usize = 128;

pub fn parse_cube(text: &str) -> Result<Lut, String> {
    let mut size = None;
    let mut domain_min = [0.0f32; 3];
    let mut domain_max = [1.0f32; 3];
    let mut data = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or_default();
        let numbers = |words: std::str::SplitWhitespace| -> Result<[f32; 3], String> {
            let v: Vec<f32> = words
                .map(|w| w.parse::<f32>().map_err(|_| format!("not a number: {w}")))
                .collect::<Result<_, _>>()?;
            v.try_into()
                .map_err(|_| format!("expected three numbers: {line}"))
        };
        match first {
            "TITLE" => {}
            "LUT_3D_SIZE" => {
                let n: usize = words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or("bad LUT_3D_SIZE")?;
                if !(2..=MAX_LUT_SIZE).contains(&n) {
                    return Err(format!("unsupported LUT size {n}"));
                }
                size = Some(n);
            }
            "LUT_1D_SIZE" => return Err("1D LUTs are not supported".into()),
            "DOMAIN_MIN" => domain_min = numbers(words)?,
            "DOMAIN_MAX" => domain_max = numbers(words)?,
            _ if first
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic()) => {}
            _ => {
                let mut all = vec![first];
                all.extend(words);
                let v: Vec<f32> = all
                    .iter()
                    .map(|w| w.parse::<f32>().map_err(|_| format!("not a number: {w}")))
                    .collect::<Result<_, _>>()?;
                let rgb: [f32; 3] = v
                    .try_into()
                    .map_err(|_| format!("expected three numbers: {line}"))?;
                data.push(rgb);
            }
        }
    }
    let size = size.ok_or("missing LUT_3D_SIZE")?;
    if data.len() != size * size * size {
        return Err(format!(
            "expected {} entries, found {}",
            size * size * size,
            data.len()
        ));
    }
    for rgb in &mut data {
        for i in 0..3 {
            let span = (domain_max[i] - domain_min[i]).max(1e-6);
            rgb[i] = (rgb[i] - domain_min[i]) / span;
        }
    }
    Ok(Lut { size, data })
}

struct Entry {
    path: PathBuf,
    lut: Option<Result<Arc<Lut>, String>>,
}

fn registry() -> &'static Mutex<HashMap<u64, Entry>> {
    static LUTS: OnceLock<Mutex<HashMap<u64, Entry>>> = OnceLock::new();
    LUTS.get_or_init(Default::default)
}

/// The id the renderer looks `path` up by; registers it.
pub fn lut_id(path: &Path) -> u64 {
    let id = crate::stable_hash::stable_hash(path.to_string_lossy().as_ref());
    registry()
        .lock()
        .unwrap()
        .entry(id)
        .or_insert_with(|| Entry {
            path: path.to_path_buf(),
            lut: None,
        });
    id
}

/// The LUT registered as `id`, read from disk the first time.
pub fn lut_for_id(id: u64) -> Option<Result<Arc<Lut>, String>> {
    let path = {
        let luts = registry().lock().unwrap();
        let entry = luts.get(&id)?;
        if let Some(lut) = &entry.lut {
            return Some(lut.clone());
        }
        entry.path.clone()
    };
    let lut = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| parse_cube(&text))
        .map(Arc::new);
    if let Some(entry) = registry().lock().unwrap().get_mut(&id) {
        entry.lut = Some(lut.clone());
    }
    Some(lut)
}

/// Reads and checks a `.cube` file: what the import button reports.
pub fn validate(path: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_cube(&text).map(|_| ())
}

#[cfg(test)]
#[path = "tests/lut.rs"]
mod tests;
