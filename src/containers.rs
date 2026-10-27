//! Native account identities. Each identity owns a whole engine profile and process.
//! Registry edits never rename storage directories or remove stored site data.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::OpenOptions,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    #[default]
    Blue,
    Orange,
    Green,
    Pink,
    Purple,
    Red,
    Turquoise,
}
impl Color {
    pub const ALL: [Self; 7] = [
        Self::Blue,
        Self::Orange,
        Self::Green,
        Self::Pink,
        Self::Purple,
        Self::Red,
        Self::Turquoise,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Blue => "Blue",
            Self::Orange => "Orange",
            Self::Green => "Green",
            Self::Pink => "Pink",
            Self::Purple => "Purple",
            Self::Red => "Red",
            Self::Turquoise => "Turquoise",
        }
    }
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Self::Blue => [80, 155, 255],
            Self::Orange => [225, 145, 45],
            Self::Green => [45, 175, 105],
            Self::Pink => [225, 100, 170],
            Self::Purple => [170, 120, 240],
            Self::Red => [235, 100, 100],
            Self::Turquoise => [35, 175, 185],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub color: Color,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registry {
    version: u32,
    pub containers: Vec<Container>,
}
impl Default for Registry {
    fn default() -> Self {
        Self {
            version: 1,
            containers: [
                ("personal", "Personal", Color::Blue),
                ("work", "Work", Color::Orange),
                ("banking", "Banking", Color::Green),
                ("shopping", "Shopping", Color::Pink),
            ]
            .into_iter()
            .map(|(id, name, color)| Container {
                id: id.into(),
                name: name.into(),
                color,
            })
            .collect(),
        }
    }
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.trim() == name
        && name.chars().count() <= 64
        && !name.chars().any(char::is_control)
}
pub fn random_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
impl Registry {
    pub fn load(root: &Path) -> Result<Self, String> {
        let bytes = match fs::read(root.join("containers.json")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("Cannot read containers: {e}")),
        };
        let registry: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Cannot read containers: {e}. Existing data is unchanged."))?;
        registry.validate()?;
        Ok(registry)
    }
    fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        if self.version != 1
            || self.containers.len() > 32
            || self
                .containers
                .iter()
                .any(|c| !valid_id(&c.id) || !valid_name(&c.name) || !ids.insert(&c.id))
        {
            return Err("Invalid container registry; existing data is unchanged".into());
        }
        Ok(())
    }
    pub fn find(&self, id: &str) -> Result<&Container, String> {
        self.containers
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("Unknown container: {id}"))
    }
    /// Merge concurrent window edits under a short-lived lock, then atomically save.
    pub fn update(root: &Path, container: Container) -> Result<Self, String> {
        Self::edit(root, |registry| {
            if let Some(existing) = registry
                .containers
                .iter_mut()
                .find(|c| c.id == container.id)
            {
                *existing = container;
            } else {
                registry.containers.push(container);
            }
        })
    }
    pub fn remove(root: &Path, id: &str) -> Result<Self, String> {
        Self::edit(root, |registry| registry.containers.retain(|c| c.id != id))
    }
    fn edit(root: &Path, apply: impl FnOnce(&mut Self)) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("containers.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|_| "Containers are being edited in another window. Try again.".to_owned())?;
        let mut registry = Self::load(root)?;
        apply(&mut registry);
        registry.validate()?;
        crate::storage::atomic_write(
            &root.join("containers.json"),
            &serde_json::to_vec_pretty(&registry).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("Cannot save containers: {e}"))?;
        Ok(registry)
    }
}
/// The default profile is never repurposed or copied into a container. Reject
/// symlinked children so an identity cannot accidentally alias another profile.
pub fn profile_directory(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("Invalid container ID".into());
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut directory = root;
    for part in ["containers", id] {
        directory.push(part);
        match fs::symlink_metadata(&directory) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err("Container storage must be a separate directory, not a link".into());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&directory).map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(directory)
}

#[cfg(test)]
#[path = "containers_tests.rs"]
mod tests;
