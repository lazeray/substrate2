//! Disk caching utilities.

use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::Debug,
    fs::{create_dir_all, File, OpenOptions},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use fs2::FileExt;
use once_cell::sync::OnceCell;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

use crate::{error::Result, CacheHandle};

pub const MANIFEST_NAME: &str = "Cache.toml";
pub const ITEM_MANIFEST_NAME: &str = "CacheItem.toml";

pub struct DiskCache {
    root: PathBuf,
    manifest_path: PathBuf,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct ManifestData {
    items: HashSet<String>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct ItemManifestData {
    values: HashMap<String, ValueStatus>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ValueStatus {
    Loading,
    InUse,
    Evicting,
}

struct WriteLockedFile {
    path: PathBuf,
    file: File,
    format: Format,
}

struct ReadLockedFile {
    path: PathBuf,
    file: File,
    format: Format,
}

enum Generate {
    Yes(WriteLockedFile),
    No(ReadLockedFile),
}

#[derive(Debug, Clone, Copy)]
enum Format {
    Toml,
    Binary,
}

impl Format {
    fn read<T: DeserializeOwned>(&self, file: &mut File) -> Result<Option<T>> {
        let mut contents = Vec::new();
        file.read(&mut contents)?;
        Ok(match self {
            Format::Toml => toml::from_str(&std::str::from_utf8(&contents)?).ok(),
            Format::Binary => flexbuffers::from_slice(&contents).ok(),
        })
    }

    fn write<T: Serialize>(&self, file: &mut File, data: &T) -> Result<()> {
        file.write_all(&match self {
            Format::Toml => toml::to_string(data).unwrap().into_bytes(),
            Format::Binary => flexbuffers::to_vec(data).unwrap(),
        })?;
        Ok(())
    }
}

fn hash_serialize<T: Serialize>(obj: &T) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(&flexbuffers::to_vec(obj).unwrap());
    hasher.finalize()[..].into()
}

impl DiskCache {
    pub fn new(root: PathBuf) -> Result<Self> {
        let manifest_path = root.join(MANIFEST_NAME);
        std::fs::create_dir_all(&root)?;
        let cache = Self {
            root,
            manifest_path,
        };

        if !cache.manifest_path.exists() {
            let _ = WriteLockedFile::new_toml(&cache.manifest_path)?;
        }

        Ok(cache)
    }

    pub fn generate<
        K: Serialize + DeserializeOwned + Eq + Send + Sync,
        V: Serialize + DeserializeOwned + Send + Sync,
        E: Send + Sync,
    >(
        &mut self,
        id: String,
        key: K,
        generate_fn: impl FnOnce(&K) -> std::result::Result<V, E> + Send + 'static,
        panic_error: E,
    ) -> Result<CacheHandle<V, E>> {
        let mut manifest = WriteLockedFile::new_toml(&self.manifest_path)?;
        let id_hash = hash_serialize(&id);
        let item_path = self.root.join(hex::encode(&id_hash));

        let mut data = manifest.read_or_initialize(ManifestData::default)?;
        if data.items.contains(&id) {
            self.check_existing_item::<K, V>(&item_path, key)?;
        } else {
            data.items.insert(id);
            manifest.write(&data)?;
            self.check_existing_item::<K, V>(item_path, key)?;
        }

        Ok(CacheHandle(Arc::new(OnceCell::new())))
    }

    /// Once the item has been added to the main manifest, we check if its manifest exists.
    ///
    /// We also check if this specific key can be found in the manifest. If it is currently
    /// loading or in use, acquire a shared lock on the output file, blocking as necessary.
    ///
    /// If it is being evicted or not present, return that the result must be regenerated.
    fn check_existing_item<
        K: Serialize + Eq + Send + Sync,
        V: Send + Sync + Serialize + DeserializeOwned,
    >(
        &self,
        item_path: impl AsRef<Path>,
        key: K,
    ) -> Result<Generate> {
        let item_path = item_path.as_ref();
        let item_manifest_path = item_path.join(ITEM_MANIFEST_NAME);
        let mut item_manifest = WriteLockedFile::new_toml(&item_manifest_path)?;
        let key_hash = hex::encode(hash_serialize(&key));
        let value_path = item_path.join(&key_hash);
        let mut data = item_manifest.read_or_initialize(ItemManifestData::default)?;
        match data.values.entry(key_hash) {
            Entry::Vacant(v) => {
                v.insert(ValueStatus::Loading);
                let value_file = WriteLockedFile::new_binary(value_path)?;
                item_manifest.write(&data)?;
                Ok(Generate::Yes(value_file))
            }
            Entry::Occupied(o) => match o.get() {
                ValueStatus::InUse | ValueStatus::Loading => {
                    Ok(Generate::No(ReadLockedFile::new_binary(value_path)?))
                }
                ValueStatus::Evicting => {
                    *o.into_mut() = ValueStatus::Loading;
                    Ok(Generate::Yes(WriteLockedFile::new_binary(value_path)?))
                }
            },
        }
    }
}

impl WriteLockedFile {
    /// Exclusively locks the file and creates a [`WriteLockedFile`] object.
    fn new(path: impl AsRef<Path>, format: Format) -> Result<Self> {
        let path = path.as_ref();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        file.lock_exclusive()?;
        Ok(Self {
            path: path.into(),
            file,
            format,
        })
    }

    fn new_toml(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(path, Format::Toml)
    }

    fn new_binary(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(path, Format::Binary)
    }

    fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let mut contents = String::new();
        self.file.rewind()?;
        self.file.read_to_string(&mut contents)?;
        Ok(toml::from_str(&contents).ok())
    }

    fn read_or_initialize<T: Serialize + DeserializeOwned>(
        &mut self,
        initialize: impl FnOnce() -> T,
    ) -> Result<T> {
        Ok(self.read()?.unwrap_or_else(|| {
            let data = initialize();
            self.write(&data).unwrap();
            data
        }))
    }

    /// Writes the data stored in data.
    fn write<T: Serialize>(&mut self, data: &T) -> Result<()> {
        self.file.rewind()?;
        self.format.write(&mut self.file, data)?;
        Ok(())
    }

    /// Downgrades this write locked file to a read locked file. **Not atomic.**
    fn downgrade(self) -> Result<ReadLockedFile> {
        let path = self.path;
        drop(self.file);
        ReadLockedFile::new(path, self.format)
    }
}

impl ReadLockedFile {
    /// Acquires a shared lock to the file and creates a [`ReadLockedFile`] object.
    fn new(path: impl AsRef<Path>, format: Format) -> Result<Self> {
        let path = path.as_ref();
        let mut file = OpenOptions::new().read(true).open(path)?;
        file.lock_shared()?;
        Ok(Self {
            path: path.into(),
            file,
            format,
        })
    }

    fn new_toml(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(path, Format::Toml)
    }

    fn new_binary(path: impl AsRef<Path>) -> Result<Self> {
        Self::new(path, Format::Binary)
    }

    fn read<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let mut contents = String::new();
        self.file.read_to_string(&mut contents)?;
        Ok(toml::from_str(&contents).ok())
    }

    /// Reads the file or initializes it to the desired value, upgrading to a write lock
    /// temporarily.
    fn read_or_initialize<T: Serialize + DeserializeOwned>(
        &mut self,
        initialize: impl FnOnce() -> T,
    ) -> Result<T> {
        Ok(if let Some(data) = self.read()? {
            data
        } else {
            self.file.unlock()?;
            let mut write_lock = WriteLockedFile::new(&self.path, self.format)?;
            if let Some(data) = write_lock.read()? {
                data
            } else {
                let data = initialize();
                write_lock.write(&data)?;
                *self = ReadLockedFile::new(&self.path, self.format)?;
                self.read()?.unwrap()
            }
        })
    }

    /// Upgrades this read locked file to a write locked file. **Not atomic.**
    fn upgrade(self) -> Result<WriteLockedFile> {
        let path = self.path;
        drop(self.file);
        WriteLockedFile::new(path, self.format)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use anyhow::anyhow;

    use super::DiskCache;

    const BUILD_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/build");

    #[test]
    fn create_disk_cache_works() {
        let mut cache = DiskCache::new(PathBuf::from(BUILD_DIR)).unwrap();
        let _ = cache.generate(
            "test".to_string(),
            (),
            |_| Ok(()),
            anyhow!("generation failed"),
        );
    }
}
