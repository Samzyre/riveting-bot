use std::any::{self, Any, TypeId};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;

use crate::utils::prelude::*;

struct Config;

impl Config {
    fn write<T>(value: &T, path: &Path) -> AnyResult<()>
    where
        T: Serialize,
    {
        let dir = path.parent().with_context(|| {
            format!(
                "Config path does not have a valid parent dir: '{}'",
                path.display()
            )
        })?;

        fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create dir: '{}'", dir.display()))?;

        let config = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .with_context(|| format!("Failed to open file: '{}'", path.display()))?;

        serde_json::to_writer_pretty(config, &value)
            .with_context(|| format!("Failed to serialize data: '{}'", path.display()))?;

        Ok(())
    }

    fn read<T>(path: &Path) -> AnyResult<T>
    where
        T: DeserializeOwned,
    {
        let mut value = String::new();
        {
            let mut config = OpenOptions::new()
                .read(true)
                .open(path)
                .with_context(|| format!("Failed to open path '{}'", path.display()))?;
            config.read_to_string(&mut value)?;
        }
        let value = serde_json::from_str::<T>(&value)?;
        Ok(value)
    }

    fn read_or_create<T>(path: &Path) -> AnyResult<T>
    where
        T: Default + Serialize + DeserializeOwned,
    {
        match Self::read::<T>(path) {
            Ok(value) => Ok(value),
            Err(e) => {
                debug!("Could not load config: {}", e);
                info!("Creating a default config: '{}'", path.display());
                Self::write(&T::default(), path).context("Failed to create config file")?;
                Ok(T::default())
            },
        }
    }

    const fn extension() -> &'static str {
        "json"
    }
}

pub trait Object = Any + Send + 'static;
pub trait Storable = Serialize + DeserializeOwned + Object;

type NameMap = HashMap<TypeId, &'static str>;
type DataMap = HashMap<TypeId, Box<dyn Object>>;
type PathMap = HashMap<PathBuf, DataMap>;

/// Configuration data storage.
#[derive(Debug, Default)]
pub struct Storage {
    names: NameMap,
    data: Mutex<PathMap>,
}

impl Storage {
    const GLOBAL: &'static str = "./data/global/";
    const GUILDS: &'static str = "./data/guilds/";

    /// Get global storage.
    ///
    /// # Notes
    /// Returned `Directory` holds a mutex lock to `self`.
    ///
    /// # Panics
    /// If something goes wrong with internal mutex.
    pub fn global(&self) -> Directory {
        Directory {
            dir: PathBuf::from(Self::GLOBAL),
            names: &self.names,
            data: self.data.lock().unwrap(),
        }
    }

    /// Get guild storage by id.  
    ///
    /// # Notes
    /// Returned `Directory` holds a mutex lock to `self`.
    ///
    /// # Panics
    /// If something goes wrong with internal mutex.
    pub fn by_guild_id(&self, guild_id: Id<GuildMarker>) -> Directory {
        Directory {
            dir: PathBuf::from(format!("{}{guild_id}/", Self::GUILDS)),
            names: &self.names,
            data: self.data.lock().unwrap(),
        }
    }

    /// Bind a type to a config name.
    ///
    /// # Errors
    /// If type is already bound to a name.
    pub fn bind<T: 'static>(&mut self, name: &'static str) -> AnyResult<()> {
        let id = TypeId::of::<T>();
        let ty_name = any::type_name::<T>();
        match self.names.entry(id) {
            Entry::Occupied(o) => Err(anyhow::anyhow!(
                "Cannot map config name '{name}' to type '{ty_name}', because the type is already \
                 mapped with a different name '{other}'",
                other = o.get()
            )),
            Entry::Vacant(v) => {
                v.insert(name);
                Ok(())
            },
        }
    }

    /// Returns self as a result of storage bindings validation.
    pub fn validated(self) -> AnyResult<Self> {
        let mut seen = HashSet::new();
        self.names
            .values()
            .find(|&n| !seen.insert(n.to_lowercase()))
            .map(|n| Err(anyhow::anyhow!("Duplicate config name found '{n}'")))
            .unwrap_or(Ok(self))
    }
}

#[derive(Debug, Error)]
#[error("Value not found for type '{0}'")]
struct ValueNotFoundError(&'static str);
impl ValueNotFoundError {
    fn new<T>() -> Self {
        Self(any::type_name::<T>())
    }
}

/// Represents a directory of configs on disk.
///
/// # Notes
/// This holds a mutex lock to the original storage.
#[derive(Debug)]
pub struct Directory<'a> {
    dir: PathBuf,
    names: &'a NameMap,
    data: MutexGuard<'a, PathMap>,
}

impl Directory<'_> {
    /// Returns a reference to a type from memory, if it exists.
    pub fn get<T>(&self) -> Option<&T>
    where
        T: Storable,
    {
        let id = TypeId::of::<T>();
        self.data
            .get(&self.dir)
            .and_then(|d| d.get(&id))
            .and_then(|d| d.downcast_ref())
    }

    /// Returns a mutable reference to a type from memory, if it exists.
    pub fn get_mut<T>(&mut self) -> Option<&mut T>
    where
        T: Storable,
    {
        let id = TypeId::of::<T>();
        self.data
            .get_mut(&self.dir)
            .and_then(|d| d.get_mut(&id))
            .and_then(|d| d.downcast_mut())
    }

    /// Get file path of the config, if valid.
    pub fn path<T>(&self) -> AnyResult<PathBuf>
    where
        T: Storable,
    {
        let id = TypeId::of::<T>();
        let ty_name = any::type_name::<T>();
        let mut path = self
            .names
            .get(&id)
            .with_context(|| format!("Missing config file name for '{ty_name}'"))
            .map(|name| self.dir.join(name))?;
        path.set_extension(Config::extension());
        Ok(path)
    }

    /// Save a type value and write config.
    pub fn save<T>(&mut self, value: T) -> AnyResult<()>
    where
        T: Storable,
    {
        self.path::<T>()
            .and_then(|path| Config::write(&value, &path))?;
        let id = TypeId::of::<T>();
        self.data
            .entry(self.dir.to_owned())
            .or_default()
            .insert(id, Box::new(value));
        Ok(())
    }

    /// Write config from memory, if present.
    pub fn save_from_memory<T>(&self) -> AnyResult<()>
    where
        T: Default + Storable,
    {
        Config::write(
            self.get::<T>()
                .with_context(|| ValueNotFoundError::new::<T>())?,
            &self.path::<T>()?,
        )
    }

    /// Modify a type value with a function and write config.
    pub fn save_with<T, R>(&mut self, f: impl FnOnce(&mut T) -> AnyResult<R>) -> AnyResult<R>
    where
        T: Default + Storable,
    {
        self.load_or_default_mut::<T>().and_then(f).and_then(|r| {
            self.save_from_memory::<T>()?;
            Ok(r)
        })
    }

    /// Access a type value with a function.
    pub fn read_with<T, R>(&mut self, f: impl Fn(&T) -> AnyResult<R>) -> AnyResult<R>
    where
        T: Storable,
    {
        self.load::<T>().and_then(f)
    }

    /// Get a type from memory, otherwise try load from config file.
    pub fn load<T>(&mut self) -> AnyResult<&T>
    where
        T: Storable,
    {
        self.load_with::<T, &T>(|path| Config::read::<T>(&path), |s| s.get::<T>())
    }

    /// Get a type from memory, otherwise try load from config file.
    /// If not found, create default.
    pub fn load_or_default<T>(&mut self) -> AnyResult<&T>
    where
        T: Default + Storable,
    {
        self.load_with::<T, &T>(|path| Config::read_or_create::<T>(&path), |s| s.get::<T>())
    }

    /// Get a type from memory, otherwise try load from config file.
    /// If not found, create default.
    pub fn load_or_default_mut<T>(&mut self) -> AnyResult<&mut T>
    where
        T: Default + Storable,
    {
        self.load_with::<T, &mut T>(
            |path| Config::read_or_create::<T>(&path),
            |s| s.get_mut::<T>(),
        )
    }

    /// Load using a function to get the value.
    fn load_with<'a, T, R>(
        &'a mut self,
        reader: impl Fn(PathBuf) -> AnyResult<T>,
        out: impl Fn(&'a mut Self) -> Option<R>,
    ) -> AnyResult<R>
    where
        T: Storable,
    {
        if self.get::<T>().is_none() {
            let path = self.path::<T>()?;
            let value = reader(path).context("Failed to read config file")?;
            let id = TypeId::of::<T>();
            self.data
                .entry(self.dir.to_owned())
                .or_default()
                .insert(id, Box::new(value));
        }
        out(self).with_context(|| ValueNotFoundError::new::<T>())
    }
}
