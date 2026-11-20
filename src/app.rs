//! The end-to-end pipeline behind the CLI: package in, STL + JSON + wiring guide out.

use std::path::{Path, PathBuf};

use crate::board::Board;
use crate::cassette::{self, Cassette, Solids};
use crate::config::{ConfigFile, ImageSize, Settings};
use crate::error::{Error, Result};
use crate::mesh::Mesh;
use crate::package::{OpenOptions, Package};
use crate::pinmap::{ListaElementow, PinMap};
use crate::{pnp, report, stl};

/// Combined cassette (default mode).
pub const STL_NAME: &str = "cassette.stl";
/// Bottom half with `--split`.
pub const STL_BOTTOM_NAME: &str = "cassette_bottom.stl";
/// Top half with `--split`.
pub const STL_TOP_NAME: &str = "cassette_top.stl";
/// Pin-map file consumed by the companion app.
pub const JSON_NAME: &str = "lista_elementow.json";
/// Human-readable wiring guide.
pub const GUIDE_NAME: &str = "wiring_guide.txt";

/// What to generate.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    /// `.zip` or directory with the `EasyEDA` export.
    pub input: PathBuf,
    /// Output directory (created if missing).
    pub out_dir: PathBuf,
    /// Pick & Place file overriding the one in the package.
    pub pnp: Option<PathBuf>,
    /// Merged configuration (file + command line).
    pub config: ConfigFile,
}

/// Everything [`generate`] produced.
#[derive(Debug)]
pub struct Outputs {
    /// Written STL files, in the order of `cassette.solids.meshes()`: `cassette.stl`, or
    /// `cassette_bottom.stl` then `cassette_top.stl` with `--split`.
    pub stl_paths: Vec<PathBuf>,
    /// Written pin map.
    pub json_path: PathBuf,
    /// Written wiring guide.
    pub guide_path: PathBuf,
    /// The wiring guide text.
    pub guide: String,
    /// The parsed board.
    pub board: Board,
    /// The pin assignment.
    pub pin_map: PinMap,
    /// The generated cassette.
    pub cassette: Cassette,
    /// The exported element list.
    pub lista: ListaElementow,
    /// All warnings, in the order they arose.
    pub warnings: Vec<String>,
}

fn load_board(input: &Path, pnp_override: Option<&Path>, include_vias: bool) -> Result<Board> {
    let package = Package::open(input, OpenOptions { include_vias })?;
    let mut board = Board::from_package(&package)?;
    if let Some(path) = pnp_override {
        let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        board.set_components(pnp::parse(&bytes, &name)?, &name);
    }
    Ok(board)
}

/// Run the whole pipeline and write the outputs.
pub fn generate(req: &GenerateRequest) -> Result<Outputs> {
    let settings = Settings::resolve(req.config.clone())?;
    let board = load_board(&req.input, req.pnp.as_deref(), settings.include_vias)?;
    let pin_map = PinMap::build(&board, &settings)?;
    let size = settings.image.resolve()?;
    let cassette = cassette::build(&board, &pin_map.probes, &settings.mech)?;
    let lista = pin_map.to_lista(&board.bbox, size)?;

    let mut warnings = board.warnings.clone();
    warnings.extend(pin_map.warnings.iter().cloned());
    warnings.extend(cassette.warnings.iter().cloned());
    warnings.extend(aspect_warning(&board, size));

    let guide = report::wiring_guide(&board, &pin_map, &cassette, &settings.mech, &warnings);
    let stl_files: Vec<(&str, Vec<u8>)> = match &cassette.solids {
        Solids::Single(mesh) => vec![(STL_NAME, encode_stl(mesh, &req.out_dir)?)],
        Solids::Split { bottom, top } => vec![
            (STL_BOTTOM_NAME, encode_stl(bottom, &req.out_dir)?),
            (STL_TOP_NAME, encode_stl(top, &req.out_dir)?),
        ],
    };
    let json = lista.to_json()?;

    std::fs::create_dir_all(&req.out_dir).map_err(|e| Error::io(&req.out_dir, e))?;
    let put = |name: &str, data: &[u8]| -> Result<PathBuf> {
        let path = req.out_dir.join(name);
        std::fs::write(&path, data).map_err(|e| Error::io(&path, e))?;
        Ok(path)
    };
    let stl_paths = stl_files
        .iter()
        .map(|(name, bytes)| put(name, bytes))
        .collect::<Result<Vec<_>>>()?;
    Ok(Outputs {
        stl_paths,
        json_path: put(JSON_NAME, json.as_bytes())?,
        guide_path: put(GUIDE_NAME, guide.as_bytes())?,
        guide,
        board,
        pin_map,
        cassette,
        lista,
        warnings,
    })
}

fn encode_stl(mesh: &Mesh, out_dir: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stl::write(mesh, &mut bytes).map_err(|e| Error::io(out_dir, e))?;
    Ok(bytes)
}

/// Listing of candidate probe holes (the `holes` command).
pub fn list_holes(input: &Path, pnp_override: Option<&Path>, include_vias: bool) -> Result<String> {
    Ok(report::hole_listing(&load_board(
        input,
        pnp_override,
        include_vias,
    )?))
}

fn aspect_warning(board: &Board, size: ImageSize) -> Option<String> {
    let board_aspect = board.bbox.width() / board.bbox.height();
    let image_aspect = f64::from(size.width) / f64::from(size.height);
    ((board_aspect / image_aspect - 1.0).abs() > 0.05).then(|| {
        format!(
            "image is {}x{} (aspect {image_aspect:.2}) but the board is {:.1} x {:.1} mm (aspect \
             {board_aspect:.2}): pixel positions are stretched. Pass --image <photo.png> or \
             --image-size WxH matching the photo",
            size.width,
            size.height,
            board.bbox.width(),
            board.bbox.height()
        )
    })
}
