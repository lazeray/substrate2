//! Disk caching utilities.

use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::{Read, Write},
    path::PathBuf,
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::error::Result;

pub const MANIFEST_NAME: &str = "Cache.toml";
pub const ITEM_MANIFEST_NAME: &str = "CacheItem.toml";

pub struct DiskCache {
    root: PathBuf,
    manifest_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ManifestData {
    items: HashSet<String>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct ItemManifestData {
    values: HashMap<Vec<u8>, ValueStatus>,
}

#[derive(Serialize, Deserialize)]
pub enum ValueStatus {
    Loading,
    InUse(u64),
    Evicting,
}

impl DiskCache {
    pub fn new(root: PathBuf) -> Result<Self> {
        let manifest_path = root.join(MANIFEST_NAME);
        std::fs::create_dir_all(&root)?;
        let cache = Self {
            root,
            manifest_path,
        };
        cache.create_manifest_if_absent()?;
        Ok(cache)
    }

    /// Loads the cache manifest, creating it if it does not exist.
    fn create_manifest_if_absent(&self) -> Result<()> {
        let mut manifest = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&self.manifest_path)?;
        manifest.lock_exclusive()?;
        let mut contents = String::new();
        manifest.read_to_string(&mut contents)?;
        if let Err(_) = toml::from_str::<ManifestData>(&contents) {
            manifest.write_all(&toml::to_string(&ManifestData::default())?.as_bytes())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::DiskCache;

    const BUILD_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "build");

    #[test]
    fn create_disk_cache_works() {
        let cache = DiskCache::new(PathBuf::from(BUILD_DIR));
    }
}
