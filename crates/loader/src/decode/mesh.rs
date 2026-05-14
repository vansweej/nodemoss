//! OBJ mesh decoder.

use std::io::Cursor;
use std::path::Path;

use crate::{DecodedMaterial, DecodedMesh, DecodedModel, LoadError};

/// Decode OBJ bytes and any referenced MTL files.
///
/// The `mtl_loader` callback receives the MTL path exactly as referenced by the
/// OBJ. It returns raw MTL bytes; path resolution is intentionally left to the
/// caller so this decoder remains source-agnostic.
pub fn decode_obj<F>(bytes: &[u8], mtl_loader: F) -> Result<DecodedModel, LoadError>
where
    F: Fn(&Path) -> Result<Vec<u8>, LoadError>,
{
    let mut reader = Cursor::new(bytes);
    let options = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };

    let (models, materials) = tobj::load_obj_buf(&mut reader, &options, |path| {
        let bytes = mtl_loader(path).map_err(|_| tobj::LoadError::OpenFileFailed)?;
        let mut reader = Cursor::new(bytes);
        tobj::load_mtl_buf(&mut reader)
    })
    .map_err(|err| LoadError::Decode(err.to_string()))?;
    let materials = materials.map_err(|err| LoadError::Decode(err.to_string()))?;

    Ok(DecodedModel {
        meshes: models.into_iter().map(decoded_mesh).collect(),
        materials: materials.into_iter().map(decoded_material).collect(),
    })
}

fn decoded_mesh(model: tobj::Model) -> DecodedMesh {
    DecodedMesh {
        name: model.name,
        positions: model.mesh.positions,
        normals: model.mesh.normals,
        uvs: model.mesh.texcoords,
        indices: model.mesh.indices,
        material_index: model.mesh.material_id,
    }
}

fn decoded_material(material: tobj::Material) -> DecodedMaterial {
    DecodedMaterial {
        name: material.name,
        diffuse: material.diffuse.unwrap_or([0.8, 0.8, 0.8]),
        specular: material.specular.unwrap_or([1.0, 1.0, 1.0]),
        shininess: material.shininess.unwrap_or(32.0),
        diffuse_texture: material.diffuse_texture,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJ: &str = r#"
mtllib cube.mtl
o tri
v 0 0 0
v 1 0 0
v 0 1 0
vt 0 0
vt 1 0
vt 0 1
vn 0 0 1
usemtl mat
f 1/1/1 2/2/1 3/3/1
"#;

    const MTL: &str = r#"
newmtl mat
Kd 0.25 0.5 0.75
Ks 1.0 0.5 0.25
Ns 16
map_Kd checker.png
"#;

    #[test]
    fn decodes_obj_mesh_and_material() {
        let model = decode_obj(OBJ.as_bytes(), |_| Ok(MTL.as_bytes().to_vec())).unwrap();

        assert_eq!(model.meshes.len(), 1);
        assert_eq!(model.meshes[0].positions.len(), 9);
        assert_eq!(model.meshes[0].indices, vec![0, 1, 2]);
        assert_eq!(model.meshes[0].material_index, Some(0));
        assert_eq!(model.materials[0].diffuse, [0.25, 0.5, 0.75]);
        assert_eq!(
            model.materials[0].diffuse_texture.as_deref(),
            Some("checker.png")
        );
    }

    #[test]
    fn reports_decode_error_for_bad_obj() {
        assert!(matches!(
            decode_obj(b"f nope", |_| Ok(Vec::new())),
            Err(LoadError::Decode(_))
        ));
    }
}
