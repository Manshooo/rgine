use std::{env, error::Error, fs, io, path::PathBuf};

use engine_core::{Reflect, TypeRegistry};
use engine_scene::{SceneTypeName, Truth, TruthObject, register_prefabs};
use serde::de::DeserializeOwned;
use uuid::Uuid;

#[path = "src/level_data.rs"]
mod level_data;
use level_data::{Kind, Piece};

const LEVEL: &str = "assets/level.scene.ron";
const PLATFORM: &str = "assets/platform.prefab.ron";
const COIN: &str = "assets/coin.prefab.ron";
const PLATFORM_GUID: Uuid = Uuid::from_u128(100);
const COIN_GUID: Uuid = Uuid::from_u128(101);

#[derive(Reflect)]
#[allow(dead_code)]
struct AuthoredPiece {
    kind: Kind,
    position: [f32; 3],
    size: [f32; 3],
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut registry = TypeRegistry::default();
    registry
        .register_reflect::<AuthoredPiece>()
        .insert_metadata(SceneTypeName("platformer.piece"));
    register_prefabs(&mut registry);
    let load = |path: &str| -> Result<Truth<'_>, Box<dyn Error>> {
        println!("cargo:rerun-if-changed={path}");
        let input = fs::read_to_string(path)?;
        Ok(Truth::from_ron(&registry, &input).map_err(|error| format!("{path}: {error}"))?)
    };
    let platform = load(PLATFORM)?;
    let coin = load(COIN)?;
    let level = load(LEVEL)?.resolve_prefabs(|guid| match guid {
        PLATFORM_GUID => Some(&platform),
        COIN_GUID => Some(&coin),
        _ => None,
    })?;
    let compiled = level.compile(|_, object, _| lower(object))?.into_compiled();
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(output.join("level.ron"), ron::to_string(&compiled)?)?;
    Ok(())
}

fn field<T: DeserializeOwned + 'static>(object: &TruthObject, name: &str) -> io::Result<T> {
    object
        .property(name)
        .ok_or_else(|| io::Error::other(format!("missing property {name}")))?
        .read()
        .map_err(|error| io::Error::other(format!("{name}: {error}")))
}

fn lower(object: &TruthObject) -> io::Result<Piece> {
    let piece = Piece {
        kind: field(object, "kind")?,
        position: field(object, "position")?,
        size: field(object, "size")?,
    };
    if piece.position.into_iter().all(f32::is_finite)
        && piece
            .size
            .into_iter()
            .all(|extent| extent.is_finite() && extent > 0.0)
    {
        Ok(piece)
    } else {
        Err(io::Error::other(
            "expected a finite position and positive, finite dimensions",
        ))
    }
}
