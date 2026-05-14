//! Hand-rolled ASCII PLY mesh decoder.

use crate::{DecodedMesh, DecodedModel, LoadError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ElementKind {
    Vertex,
    Face,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct VertexColumns {
    x: Option<usize>,
    y: Option<usize>,
    z: Option<usize>,
    nx: Option<usize>,
    ny: Option<usize>,
    nz: Option<usize>,
    count: usize,
}

impl VertexColumns {
    fn has_positions(self) -> bool {
        self.x.is_some() && self.y.is_some() && self.z.is_some()
    }

    fn has_normals(self) -> bool {
        self.nx.is_some() && self.ny.is_some() && self.nz.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Header {
    vertex_count: usize,
    face_count: usize,
    vertex_columns: VertexColumns,
    data_start_line: usize,
}

/// Decode an ASCII PLY model into a single geometry-only mesh.
pub fn decode_ply(bytes: &[u8]) -> Result<DecodedModel, LoadError> {
    let source = std::str::from_utf8(bytes).map_err(|err| LoadError::Decode(err.to_string()))?;
    let lines: Vec<&str> = source.lines().collect();
    let header = parse_header(&lines)?;
    let mesh = parse_data(&lines, &header)?;

    Ok(DecodedModel {
        meshes: vec![mesh],
        materials: Vec::new(),
    })
}

fn parse_header(lines: &[&str]) -> Result<Header, LoadError> {
    if lines.first().copied() != Some("ply") {
        return Err(LoadError::Decode("missing PLY magic header".into()));
    }

    let mut ascii = false;
    let mut vertex_count = None;
    let mut face_count = 0_usize;
    let mut current_element = ElementKind::Other;
    let mut vertex_columns = VertexColumns::default();

    for (line_index, line) in lines.iter().enumerate().skip(1) {
        let trimmed = line.trim();
        if trimmed == "end_header" {
            if !ascii {
                return Err(LoadError::Decode("PLY format must be ascii 1.0".into()));
            }
            let Some(vertex_count) = vertex_count else {
                return Err(LoadError::Decode("missing vertex element".into()));
            };
            if !vertex_columns.has_positions() {
                return Err(LoadError::Decode("missing x/y/z vertex properties".into()));
            }
            return Ok(Header {
                vertex_count,
                face_count,
                vertex_columns,
                data_start_line: line_index + 1,
            });
        }
        if trimmed.is_empty() || trimmed.starts_with("comment") {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        match parts.as_slice() {
            ["format", "ascii", "1.0"] => ascii = true,
            ["format", format, ..] => {
                return Err(LoadError::Decode(format!(
                    "unsupported PLY format '{format}'"
                )));
            }
            ["element", "vertex", count] => {
                vertex_count = Some(parse_usize(count, "vertex count")?);
                current_element = ElementKind::Vertex;
            }
            ["element", "face", count] => {
                face_count = parse_usize(count, "face count")?;
                current_element = ElementKind::Face;
            }
            ["element", ..] => current_element = ElementKind::Other,
            ["property", ..] if current_element == ElementKind::Vertex => {
                parse_vertex_property(&parts, &mut vertex_columns)?;
            }
            _ => {}
        }
    }

    Err(LoadError::Decode("missing end_header".into()))
}

fn parse_vertex_property(parts: &[&str], columns: &mut VertexColumns) -> Result<(), LoadError> {
    let ["property", property_type, name] = parts else {
        return Ok(());
    };
    if !matches!(
        *property_type,
        "float" | "float32" | "double" | "uchar" | "uint8" | "int" | "int32" | "uint" | "uint32"
    ) {
        columns.count += 1;
        return Ok(());
    }

    let column = columns.count;
    match *name {
        "x" => columns.x = Some(column),
        "y" => columns.y = Some(column),
        "z" => columns.z = Some(column),
        "nx" => columns.nx = Some(column),
        "ny" => columns.ny = Some(column),
        "nz" => columns.nz = Some(column),
        _ => {}
    }
    columns.count += 1;
    Ok(())
}

fn parse_data(lines: &[&str], header: &Header) -> Result<DecodedMesh, LoadError> {
    let mut positions = Vec::with_capacity(header.vertex_count * 3);
    let mut normals = if header.vertex_columns.has_normals() {
        Vec::with_capacity(header.vertex_count * 3)
    } else {
        Vec::new()
    };

    let vertex_lines = lines
        .get(header.data_start_line..header.data_start_line + header.vertex_count)
        .ok_or_else(|| LoadError::Decode("not enough vertex rows".into()))?;

    for line in vertex_lines {
        parse_vertex_line(line, header.vertex_columns, &mut positions, &mut normals)?;
    }

    let face_start = header.data_start_line + header.vertex_count;
    let face_lines = lines
        .get(face_start..face_start + header.face_count)
        .ok_or_else(|| LoadError::Decode("not enough face rows".into()))?;
    let mut indices = Vec::new();
    for line in face_lines {
        parse_face_line(line, &mut indices)?;
    }

    Ok(DecodedMesh {
        name: "ply_mesh".into(),
        positions,
        normals,
        uvs: Vec::new(),
        indices,
        material_index: None,
    })
}

fn parse_vertex_line(
    line: &str,
    columns: VertexColumns,
    positions: &mut Vec<f32>,
    normals: &mut Vec<f32>,
) -> Result<(), LoadError> {
    let values: Vec<&str> = line.split_whitespace().collect();
    let read = |column: Option<usize>, name: &str| -> Result<f32, LoadError> {
        let column = column.ok_or_else(|| LoadError::Decode(format!("missing {name} column")))?;
        let value = values
            .get(column)
            .ok_or_else(|| LoadError::Decode(format!("missing {name} value")))?;
        parse_f32(value, name)
    };

    positions.push(read(columns.x, "x")?);
    positions.push(read(columns.y, "y")?);
    positions.push(read(columns.z, "z")?);

    if columns.has_normals() {
        normals.push(read(columns.nx, "nx")?);
        normals.push(read(columns.ny, "ny")?);
        normals.push(read(columns.nz, "nz")?);
    }
    Ok(())
}

fn parse_face_line(line: &str, indices: &mut Vec<u32>) -> Result<(), LoadError> {
    let mut values = line.split_whitespace();
    let count = values
        .next()
        .ok_or_else(|| LoadError::Decode("missing face vertex count".into()))
        .and_then(|value| parse_usize(value, "face vertex count"))?;
    let face_indices = values
        .map(|value| parse_u32(value, "face index"))
        .collect::<Result<Vec<_>, _>>()?;
    if face_indices.len() < count {
        return Err(LoadError::Decode("face row has too few indices".into()));
    }
    for triangle in 1..count.saturating_sub(1) {
        indices.push(face_indices[0]);
        indices.push(face_indices[triangle]);
        indices.push(face_indices[triangle + 1]);
    }
    Ok(())
}

fn parse_f32(value: &str, context: &str) -> Result<f32, LoadError> {
    value
        .parse::<f32>()
        .map_err(|err| LoadError::Decode(format!("malformed {context}: {err}")))
}

fn parse_usize(value: &str, context: &str) -> Result<usize, LoadError> {
    value
        .parse::<usize>()
        .map_err(|err| LoadError::Decode(format!("malformed {context}: {err}")))
}

fn parse_u32(value: &str, context: &str) -> Result<u32, LoadError> {
    value
        .parse::<u32>()
        .map_err(|err| LoadError::Decode(format!("malformed {context}: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_minimal_triangle() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n0 1 0\n3 0 1 2\n";

        let model = decode_ply(ply).unwrap();

        assert_eq!(model.meshes[0].positions.len(), 9);
        assert_eq!(model.meshes[0].indices, vec![0, 1, 2]);
        assert!(model.meshes[0].normals.is_empty());
    }

    #[test]
    fn decode_quad_is_triangulated() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 4\nproperty float x\nproperty float y\nproperty float z\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0\n1 0 0\n1 1 0\n0 1 0\n4 0 1 2 3\n";

        let model = decode_ply(ply).unwrap();

        assert_eq!(model.meshes[0].indices, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn decode_with_normals() {
        let ply = b"ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty float nx\nproperty float ny\nproperty float nz\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 0 0 1\n1 0 0 0 0 1\n0 1 0 0 0 1\n3 0 1 2\n";

        let model = decode_ply(ply).unwrap();

        assert_eq!(
            model.meshes[0].normals,
            vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn decode_missing_vertex_element_errors() {
        let ply = b"ply\nformat ascii 1.0\nelement face 0\nend_header\n";

        assert!(matches!(decode_ply(ply), Err(LoadError::Decode(_))));
    }

    #[test]
    fn decode_binary_format_rejected() {
        let ply = b"ply\nformat binary_little_endian 1.0\nelement vertex 0\nend_header\n";

        assert!(matches!(decode_ply(ply), Err(LoadError::Decode(_))));
    }
}
