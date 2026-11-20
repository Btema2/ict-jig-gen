//! Locating the relevant layers inside an `EasyEDA` fabrication package (zip or directory).
//!
//! Layers are recognised by extension and by keywords in the file name, so both `EasyEDA`
//! Standard (`Gerber_BoardOutlineLayer.GKO`, `Drill_PTH_Through.DRL`) and Pro exports work.
//! Archives are read in memory only: nothing is extracted to disk, entry names are never used
//! as output paths, and every read is size-capped.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// A raw file taken from the package.
#[derive(Debug, Clone)]
pub struct Layer {
    /// File name (no directories).
    pub name: String,
    /// Raw contents.
    pub bytes: Vec<u8>,
}

/// The layers this tool consumes.
#[derive(Debug, Clone)]
pub struct Package {
    /// Display label of the input (path as given).
    pub source: String,
    /// Board outline (`.GKO` / `.GM1` / outline Gerber).
    pub outline: Layer,
    /// Plated through-hole drill files (merged by the caller).
    pub plated: Vec<Layer>,
    /// Non-plated drill files (mounting holes); may be empty.
    pub non_plated: Vec<Layer>,
    /// Pick & Place CSV found in the package, if any.
    pub pick_and_place: Option<Layer>,
}

/// Options for [`Package::open`].
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenOptions {
    /// Also treat `*Via*.DRL` files as probe-target drills.
    pub include_vias: bool,
}

/// What a file in the package is, judged by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Board outline Gerber.
    Outline,
    /// Plated drill.
    Plated,
    /// Plated via-only drill.
    Via,
    /// Non-plated drill.
    NonPlated,
    /// Pick & Place CSV.
    PickAndPlace,
}

/// Refuse to load any single file larger than this (guards against zip bombs).
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
/// Directory recursion limit when the input is a folder.
const MAX_DEPTH: usize = 3;

/// Classify a file name; `None` means "not relevant".
pub fn classify(file_name: &str) -> Option<Kind> {
    let name = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file_name)
        .to_ascii_lowercase();
    if name.starts_with("._") || name.starts_with('.') {
        return None;
    }
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    match ext {
        "drl" | "xln" | "drd" | "txt" if ext != "txt" || has(&["drill", "pth"]) => {
            if has(&["npth", "non-plated", "nonplated", "non_plated"]) {
                Some(Kind::NonPlated)
            } else if has(&["via"]) {
                Some(Kind::Via)
            } else {
                Some(Kind::Plated)
            }
        }
        "gko" | "gm1" | "gml" | "gm" => Some(Kind::Outline),
        "gbr" | "ger" if has(&["outline", "profile", "edge_cuts", "edgecuts", "boardedge"]) => {
            Some(Kind::Outline)
        }
        "csv"
            if has(&[
                "pickandplace",
                "pick_and_place",
                "pick-and-place",
                "pnp",
                "placement",
                "centroid",
            ]) =>
        {
            Some(Kind::PickAndPlace)
        }
        _ => None,
    }
}

/// Lower is better; `.GKO` is `EasyEDA`'s own outline layer.
fn outline_rank(name: &str) -> u8 {
    match name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("gko") => 0,
        Some("gm1") => 1,
        Some("gml" | "gm") => 2,
        _ => 3,
    }
}

enum Location {
    Zip(usize),
    Path(PathBuf),
}

struct Entry {
    name: String,
    kind: Kind,
    location: Location,
}

impl Package {
    /// Open a `.zip` archive or a directory and locate the layers.
    pub fn open(path: &Path, options: OpenOptions) -> Result<Self> {
        let source = path.display().to_string();
        let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
        if meta.is_dir() {
            let mut entries = Vec::new();
            walk(path, 0, &mut entries)?;
            Self::assemble(&source, options, entries, |loc| match loc {
                Location::Path(p) => read_capped(File::open(p).map_err(|e| Error::io(p, e))?, p),
                Location::Zip(_) => Err(Error::Config("internal: zip entry in directory".into())),
            })
        } else {
            let archive_err = |message: String| Error::Archive {
                path: path.to_path_buf(),
                message,
            };
            let file = File::open(path).map_err(|e| Error::io(path, e))?;
            let mut zip = zip::ZipArchive::new(file).map_err(|e| archive_err(e.to_string()))?;
            let mut entries = Vec::new();
            for index in 0..zip.len() {
                let entry = zip
                    .by_index(index)
                    .map_err(|e| archive_err(e.to_string()))?;
                if entry.is_dir() {
                    continue;
                }
                let full = entry.name().to_owned();
                let base = full.rsplit(['/', '\\']).next().unwrap_or(&full).to_owned();
                if full.split(['/', '\\']).any(|p| p == "__MACOSX") {
                    continue;
                }
                if let Some(kind) = classify(&base) {
                    entries.push(Entry {
                        name: base,
                        kind,
                        location: Location::Zip(index),
                    });
                }
            }
            Self::assemble(&source, options, entries, |loc| match loc {
                Location::Zip(i) => {
                    let entry = zip.by_index(*i).map_err(|e| archive_err(e.to_string()))?;
                    read_capped(entry, path)
                }
                Location::Path(_) => Err(Error::Config("internal: path entry in zip".into())),
            })
        }
    }

    fn assemble(
        source: &str,
        options: OpenOptions,
        mut entries: Vec<Entry>,
        mut read: impl FnMut(&Location) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut load = |e: &Entry| -> Result<Layer> {
            Ok(Layer {
                name: e.name.clone(),
                bytes: read(&e.location)?,
            })
        };
        let outline = entries
            .iter()
            .filter(|e| e.kind == Kind::Outline)
            .min_by_key(|e| outline_rank(&e.name))
            .map(&mut load)
            .transpose()?
            .ok_or_else(|| Error::MissingLayer {
                what: "board outline layer",
                input: source.to_owned(),
                hint: "Gerber_BoardOutlineLayer.GKO",
            })?;
        let plated = entries
            .iter()
            .filter(|e| e.kind == Kind::Plated || (options.include_vias && e.kind == Kind::Via))
            .map(&mut load)
            .collect::<Result<Vec<_>>>()?;
        if plated.is_empty() {
            return Err(Error::MissingLayer {
                what: "plated through-hole drill file",
                input: source.to_owned(),
                hint: "Drill_PTH_Through.DRL",
            });
        }
        let non_plated = entries
            .iter()
            .filter(|e| e.kind == Kind::NonPlated)
            .map(&mut load)
            .collect::<Result<Vec<_>>>()?;
        let pick_and_place = entries
            .iter()
            .find(|e| e.kind == Kind::PickAndPlace)
            .map(&mut load)
            .transpose()?;
        Ok(Self {
            source: source.to_owned(),
            outline,
            plated,
            non_plated,
            pick_and_place,
        })
    }
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<Entry>) -> Result<()> {
    for item in std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))? {
        let item = item.map_err(|e| Error::io(dir, e))?;
        let path = item.path();
        let ftype = item.file_type().map_err(|e| Error::io(&path, e))?;
        if ftype.is_dir() && depth < MAX_DEPTH {
            walk(&path, depth + 1, out)?;
        } else if ftype.is_file() {
            let name = item.file_name().to_string_lossy().into_owned();
            if let Some(kind) = classify(&name) {
                out.push(Entry {
                    name,
                    kind,
                    location: Location::Path(path),
                });
            }
        }
    }
    Ok(())
}

fn read_capped(reader: impl Read, path: &Path) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    reader
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(|e| Error::io(path, e))?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        return Err(Error::Archive {
            path: path.to_path_buf(),
            message: format!(
                "a file is larger than {} MiB; refusing to load it",
                MAX_FILE_BYTES >> 20
            ),
        });
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_easyeda_standard_names() {
        use Kind::{NonPlated, Outline, PickAndPlace, Plated, Via};
        for (name, kind) in [
            ("Gerber_BoardOutlineLayer.GKO", Some(Outline)),
            ("Drill_PTH_Through.DRL", Some(Plated)),
            ("Drill_NPTH_Through.DRL", Some(NonPlated)),
            ("Drill_PTH_Through_Via.DRL", Some(Via)),
            ("PickAndPlace_PCB1_2024.csv", Some(PickAndPlace)),
            ("board.gm1", Some(Outline)),
            ("Board-Edge_Cuts.gbr", Some(Outline)),
            ("holes_PTH.TXT", Some(Plated)),
            ("notes.txt", None),
            ("Gerber_TopLayer.GTL", None),
            ("Gerber_TopSilkscreenLayer.GTO", None),
            ("BOM.csv", None),
            ("._Drill_PTH_Through.DRL", None),
            ("dir/sub\\Drill_NPTH.drl", Some(NonPlated)),
        ] {
            assert_eq!(classify(name), kind, "{name}");
        }
    }

    #[test]
    fn missing_input_is_an_io_error() {
        let e = Package::open(
            Path::new("/definitely/not/here.zip"),
            OpenOptions::default(),
        );
        assert!(matches!(e, Err(Error::Io { .. })));
    }
}
