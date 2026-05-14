//! Importer implementation and mesh adaptation helpers.

use std::collections::HashMap;
use std::sync::Arc;

use rig_assets::{
    AssetStore, IndexFormat, MaterialAsset, MaterialParams, MeshAsset, SamplerDescriptor,
    SamplerHandle, ShaderAsset, ShaderHandle, TextureAsset, TextureFormat, TextureHandle,
};
use rig_loader::{
    AssetPath, AssetSource, ColorSpace, DecodedImage, DecodedMaterial, DecodedMesh, Loader,
};
use rig_math::{BoundingSphere, Vec3};

use crate::{ImportError, ImportedMesh, LoadedModel, MeshConfig, TextureConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CachedHandle {
    Texture {
        texture: TextureHandle,
        sampler: SamplerHandle,
    },
    Shader(ShaderHandle),
}

/// Local importer with a short-lived path-to-handle cache.
pub struct Importer {
    loader: Loader,
    cache: HashMap<AssetPath, CachedHandle>,
}

impl Importer {
    /// Create an importer reading from `source`.
    pub fn new(source: impl AssetSource + 'static) -> Self {
        Self {
            loader: Loader::new(source),
            cache: HashMap::new(),
        }
    }

    /// Import a texture and register its [`TextureAsset`] plus default sampler.
    pub fn import_texture(
        &mut self,
        path: &AssetPath,
        config: &TextureConfig,
        store: &mut AssetStore,
    ) -> Result<TextureHandle, ImportError> {
        let (texture, _) = self.import_texture_slot(path, config, store)?;
        Ok(texture)
    }

    /// Import an OBJ mesh as adapted data without registering meshes/materials.
    ///
    /// Diffuse textures referenced by MTL materials are registered as a side
    /// effect because material assets need texture handles.
    pub fn import_mesh(
        &mut self,
        path: &AssetPath,
        config: &MeshConfig,
        shader: ShaderHandle,
        store: &mut AssetStore,
    ) -> Result<LoadedModel, ImportError> {
        let decoded = self.loader.read_mesh(path)?;
        let materials = decoded
            .materials
            .iter()
            .map(|material| self.import_material(path, material, shader, store))
            .collect::<Result<Vec<_>, _>>()?;

        let meshes = decoded
            .meshes
            .iter()
            .map(|mesh| import_decoded_mesh(mesh, config))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(LoadedModel { meshes, materials })
    }

    /// Import a WGSL shader and register it in the asset store.
    pub fn import_shader(
        &mut self,
        path: &AssetPath,
        store: &mut AssetStore,
    ) -> Result<ShaderHandle, ImportError> {
        if let Some(CachedHandle::Shader(handle)) = self.cache.get(path).copied() {
            return Ok(handle);
        }

        let shader = self.loader.read_shader(path)?;
        let handle = store.add_shader(ShaderAsset {
            source: Arc::from(shader.source),
        });
        self.cache
            .insert(path.clone(), CachedHandle::Shader(handle));
        Ok(handle)
    }

    fn import_texture_slot(
        &mut self,
        path: &AssetPath,
        config: &TextureConfig,
        store: &mut AssetStore,
    ) -> Result<(TextureHandle, SamplerHandle), ImportError> {
        if let Some(CachedHandle::Texture { texture, sampler }) = self.cache.get(path).copied() {
            return Ok((texture, sampler));
        }

        let decoded = self.loader.read_texture(path)?;
        let adapted = adapt_image(decoded, config);
        let format = match adapted.color_space {
            ColorSpace::Linear => TextureFormat::Rgba8Unorm,
            ColorSpace::Srgb => TextureFormat::Rgba8UnormSrgb,
        };
        let texture = store.add_texture(TextureAsset {
            width: adapted.width,
            height: adapted.height,
            format,
            data: Arc::from(adapted.data),
        });
        let sampler = store.add_sampler(SamplerDescriptor::default());
        self.cache
            .insert(path.clone(), CachedHandle::Texture { texture, sampler });
        Ok((texture, sampler))
    }

    fn import_material(
        &mut self,
        model_path: &AssetPath,
        material: &DecodedMaterial,
        shader: ShaderHandle,
        store: &mut AssetStore,
    ) -> Result<(MaterialAsset, String), ImportError> {
        let textures = if let Some(texture_name) = material.diffuse_texture.as_deref() {
            let texture_path = model_path.sibling(texture_name);
            let slot = self
                .import_texture_slot(&texture_path, &TextureConfig::default(), store)
                .map_err(|err| match err {
                    ImportError::Load(source) => ImportError::UnresolvedDependency {
                        path: texture_path,
                        source,
                    },
                    other => other,
                })?;
            vec![slot]
        } else {
            Vec::new()
        };

        Ok((
            MaterialAsset {
                shader,
                parameters: material_params(material),
                textures,
            },
            material.name.clone(),
        ))
    }
}

fn material_params(material: &DecodedMaterial) -> MaterialParams {
    MaterialParams {
        ambient: [
            material.diffuse[0] * 0.2,
            material.diffuse[1] * 0.2,
            material.diffuse[2] * 0.2,
            1.0,
        ],
        diffuse: [
            material.diffuse[0],
            material.diffuse[1],
            material.diffuse[2],
            1.0,
        ],
        specular: [
            material.specular[0],
            material.specular[1],
            material.specular[2],
            material.shininess,
        ],
        ..MaterialParams::default()
    }
}

fn adapt_image(mut image: DecodedImage, config: &TextureConfig) -> DecodedImage {
    if config.flip_y {
        flip_y(&mut image.data, image.width, image.height);
    }
    if config.premultiply_alpha {
        premultiply_alpha(&mut image.data);
    }
    if let Some(max_dimension) = config.max_dimension {
        image = resize_to_max_dimension(image, max_dimension.max(1));
    }
    image
}

fn flip_y(data: &mut [u8], width: u32, height: u32) {
    let row_bytes = width as usize * 4;
    for y in 0..height as usize / 2 {
        let top = y * row_bytes;
        let bottom = (height as usize - 1 - y) * row_bytes;
        for x in 0..row_bytes {
            data.swap(top + x, bottom + x);
        }
    }
}

fn premultiply_alpha(data: &mut [u8]) {
    for pixel in data.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        pixel[0] = ((u16::from(pixel[0]) * alpha) / 255) as u8;
        pixel[1] = ((u16::from(pixel[1]) * alpha) / 255) as u8;
        pixel[2] = ((u16::from(pixel[2]) * alpha) / 255) as u8;
    }
}

fn resize_to_max_dimension(image: DecodedImage, max_dimension: u32) -> DecodedImage {
    let longest = image.width.max(image.height);
    if longest <= max_dimension {
        return image;
    }
    let scale = max_dimension as f32 / longest as f32;
    let new_width = ((image.width as f32 * scale).round() as u32).max(1);
    let new_height = ((image.height as f32 * scale).round() as u32).max(1);
    let mut data = vec![0_u8; new_width as usize * new_height as usize * 4];
    for y in 0..new_height {
        for x in 0..new_width {
            let src_x = (x as u64 * image.width as u64 / new_width as u64) as u32;
            let src_y = (y as u64 * image.height as u64 / new_height as u64) as u32;
            let src = ((src_y * image.width + src_x) * 4) as usize;
            let dst = ((y * new_width + x) * 4) as usize;
            data[dst..dst + 4].copy_from_slice(&image.data[src..src + 4]);
        }
    }
    DecodedImage {
        width: new_width,
        height: new_height,
        data,
        ..image
    }
}

fn import_decoded_mesh(
    mesh: &DecodedMesh,
    config: &MeshConfig,
) -> Result<ImportedMesh, ImportError> {
    let vertex_count = mesh.positions.len() / 3;
    if vertex_count == 0 {
        return Err(ImportError::MissingPositions {
            mesh: mesh.name.clone(),
        });
    }

    validate_indices(mesh, vertex_count)?;
    let indices = adapted_indices(&mesh.indices, config.reverse_winding);
    let normals = if mesh.normals.len() / 3 == vertex_count {
        mesh.normals.clone()
    } else if config.generate_normals {
        generate_smooth_normals(&mesh.positions, &indices)
    } else {
        vec![0.0; vertex_count * 3]
    };
    let vertex_data = interleave_vertices(&mesh.positions, &normals, &mesh.uvs, vertex_count);
    let (index_data, index_format) = pack_indices(&indices, vertex_count);

    Ok(ImportedMesh {
        mesh: MeshAsset {
            vertex_layout: rig_assets::standard_vertex_layout(),
            vertex_data: Arc::from(vertex_data),
            index_data: Arc::from(index_data),
            index_format,
            local_bounds: compute_bounding_sphere(&mesh.positions),
        },
        material_index: mesh.material_index,
        name: mesh.name.clone(),
    })
}

fn validate_indices(mesh: &DecodedMesh, vertex_count: usize) -> Result<(), ImportError> {
    for &index in &mesh.indices {
        if index as usize >= vertex_count {
            return Err(ImportError::IndexOverflow {
                mesh: mesh.name.clone(),
                index,
                vertex_count,
            });
        }
    }
    Ok(())
}

fn adapted_indices(indices: &[u32], reverse_winding: bool) -> Vec<u32> {
    let mut adapted = indices.to_vec();
    if reverse_winding {
        for triangle in adapted.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }
    adapted
}

fn interleave_vertices(
    positions: &[f32],
    normals: &[f32],
    uvs: &[f32],
    vertex_count: usize,
) -> Vec<u8> {
    let mut floats = Vec::with_capacity(vertex_count * 8);
    for index in 0..vertex_count {
        floats.extend_from_slice(&positions[index * 3..index * 3 + 3]);
        floats.extend_from_slice(&normals[index * 3..index * 3 + 3]);
        if uvs.len() / 2 == vertex_count {
            floats.extend_from_slice(&uvs[index * 2..index * 2 + 2]);
        } else {
            floats.extend_from_slice(&[0.0, 0.0]);
        }
    }
    bytemuck::cast_slice(&floats).to_vec()
}

fn generate_smooth_normals(positions: &[f32], indices: &[u32]) -> Vec<f32> {
    let vertex_count = positions.len() / 3;
    let mut normals = vec![Vec3::ZERO; vertex_count];
    for triangle in indices.chunks_exact(3) {
        let i0 = triangle[0] as usize;
        let i1 = triangle[1] as usize;
        let i2 = triangle[2] as usize;
        let p0 = read_position(positions, i0);
        let p1 = read_position(positions, i1);
        let p2 = read_position(positions, i2);
        let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        normals[i0] += normal;
        normals[i1] += normal;
        normals[i2] += normal;
    }
    normals
        .into_iter()
        .flat_map(|normal| normal.normalize_or_zero().to_array())
        .collect()
}

fn read_position(positions: &[f32], index: usize) -> Vec3 {
    Vec3::new(
        positions[index * 3],
        positions[index * 3 + 1],
        positions[index * 3 + 2],
    )
}

fn pack_indices(indices: &[u32], vertex_count: usize) -> (Vec<u8>, IndexFormat) {
    if vertex_count <= u16::MAX as usize {
        let packed: Vec<u16> = indices.iter().map(|&index| index as u16).collect();
        (bytemuck::cast_slice(&packed).to_vec(), IndexFormat::Uint16)
    } else {
        (bytemuck::cast_slice(indices).to_vec(), IndexFormat::Uint32)
    }
}

fn compute_bounding_sphere(positions: &[f32]) -> BoundingSphere {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for position in positions
        .chunks_exact(3)
        .map(|p| Vec3::new(p[0], p[1], p[2]))
    {
        min = min.min(position);
        max = max.max(position);
    }
    let center = (min + max) * 0.5;
    let radius = positions
        .chunks_exact(3)
        .map(|p| Vec3::new(p[0], p[1], p[2]).distance(center))
        .fold(0.0, f32::max);
    BoundingSphere { center, radius }
}

#[allow(dead_code)]
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, RgbaImage};
    use rig_assets::AssetStore;
    use rig_loader::MemorySource;

    use super::*;

    fn png_bytes() -> Vec<u8> {
        let image = RgbaImage::from_raw(2, 1, vec![255, 0, 0, 128, 0, 255, 0, 255]).unwrap();
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .unwrap();
        cursor.into_inner()
    }

    fn source_with_textured_obj() -> MemorySource {
        let obj = b"mtllib cube.mtl\no tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl mat\nf 1/1 2/2 3/3\n".to_vec();
        let mtl = b"newmtl mat\nKd 0.5 0.25 0.75\nmap_Kd checker.png\n".to_vec();
        MemorySource::new()
            .with("models/cube.obj", obj)
            .with("models/cube.mtl", mtl)
            .with("models/checker.png", png_bytes())
    }

    #[test]
    fn import_texture_registers_and_deduplicates_texture() {
        let mut importer = Importer::new(MemorySource::new().with("checker.png", png_bytes()));
        let mut store = AssetStore::new();

        let first = importer
            .import_texture(
                &AssetPath::new("checker.png"),
                &TextureConfig::default(),
                &mut store,
            )
            .unwrap();
        let second = importer
            .import_texture(
                &AssetPath::new("checker.png"),
                &TextureConfig::default(),
                &mut store,
            )
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            store.texture(first).unwrap().format,
            TextureFormat::Rgba8UnormSrgb
        );
    }

    #[test]
    fn import_shader_registers_and_deduplicates_shader() {
        let mut importer =
            Importer::new(MemorySource::new().with("shader.wgsl", b"shader".to_vec()));
        let mut store = AssetStore::new();

        let first = importer
            .import_shader(&AssetPath::new("shader.wgsl"), &mut store)
            .unwrap();
        let second = importer
            .import_shader(&AssetPath::new("shader.wgsl"), &mut store)
            .unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn import_mesh_loads_material_texture_dependency() {
        let mut importer = Importer::new(source_with_textured_obj());
        let mut store = AssetStore::new();
        let shader = store.add_shader(ShaderAsset {
            source: Arc::from("shader"),
        });

        let model = importer
            .import_mesh(
                &AssetPath::new("models/cube.obj"),
                &MeshConfig::default(),
                shader,
                &mut store,
            )
            .unwrap();

        assert_eq!(model.meshes.len(), 1);
        assert_eq!(model.materials.len(), 1);
        assert_eq!(model.materials[0].0.textures.len(), 1);
        assert_eq!(model.materials[0].0.textures[0].0.index(), 0);
    }

    #[test]
    fn missing_positions_returns_import_error() {
        let mesh = DecodedMesh {
            name: "empty".into(),
            positions: vec![],
            normals: vec![],
            uvs: vec![],
            indices: vec![],
            material_index: None,
        };

        assert!(matches!(
            import_decoded_mesh(&mesh, &MeshConfig::default()),
            Err(ImportError::MissingPositions { mesh }) if mesh == "empty"
        ));
    }

    #[test]
    fn index_overflow_returns_import_error() {
        let mesh = DecodedMesh {
            name: "bad".into(),
            positions: vec![0.0, 0.0, 0.0],
            normals: vec![],
            uvs: vec![],
            indices: vec![1],
            material_index: None,
        };

        assert!(matches!(
            import_decoded_mesh(&mesh, &MeshConfig::default()),
            Err(ImportError::IndexOverflow {
                index: 1,
                vertex_count: 1,
                ..
            })
        ));
    }

    #[test]
    fn texture_config_transforms_image_data() {
        let image = DecodedImage {
            width: 1,
            height: 2,
            channels: 4,
            color_space: ColorSpace::Srgb,
            data: vec![10, 0, 0, 255, 100, 0, 0, 128],
        };

        let adapted = adapt_image(
            image,
            &TextureConfig {
                flip_y: true,
                premultiply_alpha: true,
                max_dimension: Some(1),
            },
        );

        assert_eq!(adapted.width, 1);
        assert_eq!(adapted.height, 1);
        assert_eq!(adapted.data, vec![50, 0, 0, 128]);
    }
}
