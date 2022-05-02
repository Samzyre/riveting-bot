use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;

use crate::commands::admin::alias::Alias;
use crate::utils::prelude::*;

pub const CONFIG_PATH: &str = "./data/bot.json";

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Data {
    pub prefix: String,
    pub aliases: HashMap<String, String>,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            prefix: "!".to_string(),
            aliases: HashMap::new(),
        }
    }
}

/// Serializable bot configuration.
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct Config {
    pub global: Data,
    pub guilds: HashMap<Id<GuildMarker>, Data>,
}

impl Config {
    /// Load the configuration file from `CONFIG_PATH`.
    pub fn load() -> AnyResult<Config> {
        info!("Loading config file");

        let mut cfg = String::new();
        {
            let mut config = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(CONFIG_PATH)?;

            config.read_to_string(&mut cfg)?;
        }

        match serde_json::from_str(&cfg) {
            Ok(c) => Ok(c),
            Err(e) => {
                debug!("Could not load config: {}", e);
                info!("Creating a default config file");

                let def = Config::default();
                def.write()?;

                Ok(def)
            },
        }
    }

    /// Force update `self` from file.
    pub fn reload(&mut self) -> AnyResult<()> {
        *self = Self::load()?;

        Ok(())
    }

    /// Write the configuration to a file in `CONFIG_PATH`.
    /// # Notes
    /// This will truncate and overwrite the file, any changes that are not in the new data will be lost.
    pub fn write(&self) -> AnyResult<()> {
        info!("Updating config file");

        let config = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(CONFIG_PATH)?;

        serde_json::to_writer_pretty(config, self)?;

        Ok(())
    }

    /// Get guild's config.
    pub fn guild(&self, guild_id: Id<GuildMarker>) -> Option<&Data> {
        self.guilds.get(&guild_id)
    }

    /// Set guild's custom prefix.
    pub fn set_prefix(&mut self, guild_id: Id<GuildMarker>, prefix: &str) {
        self.guilds.entry(guild_id).or_default().prefix = prefix.to_string();
    }

    /// Set an alias, return `Some(alias_command)` if it replaced one.
    pub fn set_alias(&mut self, guild_id: Id<GuildMarker>, alias: Alias) -> Option<String> {
        self.guilds
            .entry(guild_id)
            .or_default()
            .aliases
            .insert(alias.name, alias.command)
    }

    /// Remove an alias, returns `Some(alias_command)` if successful.
    pub fn remove_alias(&mut self, guild_id: Id<GuildMarker>, alias_name: &str) -> Option<String> {
        self.guilds
            .entry(guild_id)
            .or_default()
            .aliases
            .remove(alias_name)
    }
}

/// Thread safe bot configuration wrapper.
#[derive(Debug, Clone)]
pub struct BotConfig(Arc<Mutex<Config>>);

impl BotConfig {
    /// Wrap a `Config` into a new `BotConfig(Arc<Mutex<Config>>)`.
    pub fn new(cfg: Config) -> Self {
        Self(Arc::new(Mutex::new(cfg)))
    }
}

impl std::ops::Deref for BotConfig {
    type Target = Arc<Mutex<Config>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
