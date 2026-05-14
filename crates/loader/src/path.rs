//! Asset paths and byte sources.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use crate::LoadError;

/// Logical asset path used by loaders and importers.
///
/// Paths are stored as UTF-8 strings so they can identify assets from any
/// source, not only the local filesystem. Use [`AssetPath::sibling`] to resolve
/// dependencies such as MTL files and texture names relative to a model path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetPath(String);

impl AssetPath {
    /// Create a new logical asset path.
    ///
    /// # Examples
    ///
    /// ```
    /// use rig_loader::AssetPath;
    ///
    /// let path = AssetPath::new("models/cube.obj");
    /// assert_eq!(path.as_str(), "models/cube.obj");
    /// ```
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// Return the path as a UTF-8 string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return the file extension without the leading dot.
    pub fn extension(&self) -> Option<&str> {
        Path::new(&self.0).extension().and_then(|ext| ext.to_str())
    }

    /// Resolve `name` next to this path's parent directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use rig_loader::AssetPath;
    ///
    /// let obj = AssetPath::new("assets/models/cube.obj");
    /// assert_eq!(obj.sibling("cube.mtl").as_str(), "assets/models/cube.mtl");
    /// ```
    pub fn sibling(&self, name: impl AsRef<str>) -> Self {
        let child = Path::new(name.as_ref());
        if child.is_absolute() {
            return Self::new(child.to_string_lossy().into_owned());
        }

        let base = Path::new(&self.0);
        let joined = base
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from(child), |parent| parent.join(child));
        Self::new(joined.to_string_lossy().into_owned())
    }
}

impl fmt::Display for AssetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for AssetPath {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for AssetPath {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Byte source for logical asset paths.
pub trait AssetSource {
    /// Read all bytes for `path` or return a [`LoadError`].
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, LoadError>;
}

/// Filesystem-backed [`AssetSource`].
///
/// The default root is the current working directory. Use
/// [`FilesystemSource::with_root`] to load from a specific asset directory.
#[derive(Clone, Debug)]
pub struct FilesystemSource {
    root: PathBuf,
}

impl FilesystemSource {
    /// Create a source rooted at `root`.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the filesystem root used by this source.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Default for FilesystemSource {
    fn default() -> Self {
        Self::with_root(".")
    }
}

impl AssetSource for FilesystemSource {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, LoadError> {
        let full_path = self.root.join(path.as_str());
        fs::read(full_path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                LoadError::NotFound
            } else {
                LoadError::Io(err)
            }
        })
    }
}

/// In-memory [`AssetSource`] for tests and embedded examples.
#[derive(Clone, Debug, Default)]
pub struct MemorySource {
    assets: HashMap<AssetPath, Vec<u8>>,
}

impl MemorySource {
    /// Create an empty memory source.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert bytes for `path`, replacing any previous entry.
    pub fn insert(&mut self, path: impl Into<AssetPath>, bytes: impl Into<Vec<u8>>) {
        self.assets.insert(path.into(), bytes.into());
    }

    /// Builder-style insertion helper.
    pub fn with(mut self, path: impl Into<AssetPath>, bytes: impl Into<Vec<u8>>) -> Self {
        self.insert(path, bytes);
        self
    }
}

impl AssetSource for MemorySource {
    fn read(&self, path: &AssetPath) -> Result<Vec<u8>, LoadError> {
        self.assets.get(path).cloned().ok_or(LoadError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_resolves_relative_to_parent() {
        let path = AssetPath::new("models/cube.obj");

        assert_eq!(path.sibling("cube.mtl").as_str(), "models/cube.mtl");
    }

    #[test]
    fn memory_source_returns_inserted_bytes() {
        let source = MemorySource::new().with("shader.wgsl", b"shader".to_vec());

        assert_eq!(
            source.read(&AssetPath::new("shader.wgsl")).unwrap(),
            b"shader"
        );
    }

    #[test]
    fn memory_source_reports_missing_path() {
        let source = MemorySource::new();

        assert!(matches!(
            source.read(&AssetPath::new("missing")),
            Err(LoadError::NotFound)
        ));
    }
}
