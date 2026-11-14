//! Minimal Wavefront OBJ reader: vertex positions and triangulated faces only.

use thiserror::Error;

/// A triangle mesh in the file's own coordinates (Matterport's are Z-up metres).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<[f64; 3]>,
    pub triangles: Vec<[usize; 3]>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObjError {
    #[error("line {line}: malformed vertex")]
    BadVertex { line: usize },
    #[error("line {line}: malformed face index")]
    BadFace { line: usize },
    #[error("line {line}: face refers to a vertex that does not exist")]
    IndexOutOfRange { line: usize },
}

/// Parses `v` and `f` records. Texture coordinates, normals, materials, and
/// groups are ignored; polygons are fan-triangulated.
pub fn parse_obj(text: &str) -> Result<Mesh, ObjError> {
    let mut mesh = Mesh::default();
    let mut face = Vec::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let mut fields = raw.split_whitespace();
        match fields.next() {
            Some("v") => {
                let mut coordinate = || -> Result<f64, ObjError> {
                    fields
                        .next()
                        .and_then(|field| field.parse().ok())
                        .ok_or(ObjError::BadVertex { line })
                };
                mesh.positions
                    .push([coordinate()?, coordinate()?, coordinate()?]);
            }
            Some("f") => {
                face.clear();
                for field in fields {
                    let position = field.split('/').next().unwrap_or_default();
                    let value: i64 = position.parse().map_err(|_| ObjError::BadFace { line })?;
                    face.push(resolve_index(value, mesh.positions.len(), line)?);
                }
                if face.len() < 3 {
                    return Err(ObjError::BadFace { line });
                }
                for corner in 1..face.len() - 1 {
                    mesh.triangles
                        .push([face[0], face[corner], face[corner + 1]]);
                }
            }
            _ => {}
        }
    }

    Ok(mesh)
}

/// OBJ indices are 1-based; negative values count back from the latest vertex.
fn resolve_index(value: i64, vertex_count: usize, line: usize) -> Result<usize, ObjError> {
    let count = i64::try_from(vertex_count).map_err(|_| ObjError::IndexOutOfRange { line })?;
    let resolved = match value {
        0 => return Err(ObjError::BadFace { line }),
        positive if positive > 0 => positive - 1,
        negative => count + negative,
    };
    if resolved < 0 || resolved >= count {
        return Err(ObjError::IndexOutOfRange { line });
    }
    usize::try_from(resolved).map_err(|_| ObjError::IndexOutOfRange { line })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_triangles_quads_and_slashed_indices() {
        let mesh = parse_obj(
            "# comment\nmtllib a.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\n\
             f 1/1 2/1 3/1\nf 1//1 2//1 3//1 4//1\n",
        )
        .unwrap();
        assert_eq!(mesh.positions.len(), 4);
        assert_eq!(mesh.triangles, vec![[0, 1, 2], [0, 1, 2], [0, 2, 3]]);
    }

    #[test]
    fn resolves_negative_indices() {
        let mesh = parse_obj("v 0 0 0\nv 1 0 0\nv 1 1 0\nf -3 -2 -1\n").unwrap();
        assert_eq!(mesh.triangles, vec![[0, 1, 2]]);
    }

    #[test]
    fn rejects_missing_vertices() {
        assert_eq!(
            parse_obj("v 0 0 0\nf 1 2 3\n"),
            Err(ObjError::IndexOutOfRange { line: 2 })
        );
    }
}
