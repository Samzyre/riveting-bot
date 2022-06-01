#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::prelude::*;
use std::mem;
use std::path::Path;

use serde::{Deserialize, Serialize};
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use twilight_model::id::Id;

use crate::commands::admin::alias::Alias;
use crate::utils::prelude::*;

pub const CONFIG_FILE: &str = "./data/bot.json";
pub const GUILD_CONFIG_DIR: &str = "./data/guilds/";

/// General settings for the bot.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Settings {
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    #[serde(default)]
    pub perms: HashMap<String, PermissionMap>,
}

impl Settings {
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn aliases(&self) -> &HashMap<String, String> {
        &self.aliases
    }

    pub fn perms(&self) -> &HashMap<String, PermissionMap> {
        &self.perms
    }

    pub fn prefix_mut(&mut self) -> &mut String {
        &mut self.prefix
    }

    pub fn aliases_mut(&mut self) -> &mut HashMap<String, String> {
        &mut self.aliases
    }

    pub fn perms_mut(&mut self) -> &mut HashMap<String, PermissionMap> {
        &mut self.perms
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            prefix: "!".to_string(),
            aliases: HashMap::new(),
            perms: HashMap::new(),
        }
    }
}

/// Contains allowed or disallowed ids.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct PermissionMap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub users: Option<HashMap<Id<UserMarker>, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roles: Option<HashMap<Id<RoleMarker>, bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_channels: Option<HashSet<Id<ChannelMarker>>>,
}

impl PermissionMap {
    /// Get a user permission, if exists.
    pub fn user(&self, user_id: Id<UserMarker>) -> Option<bool> {
        self.users.as_ref()?.get(&user_id).copied()
    }

    /// Get a role permission, if exists.
    pub fn role(&self, role_id: Id<RoleMarker>) -> Option<bool> {
        self.roles.as_ref()?.get(&role_id).copied()
    }

    /// Set a permission rule for a user, returns replaced rule if there was any.
    pub fn set_user(&mut self, user_id: Id<UserMarker>, allow: bool) -> Option<bool> {
        self.users.get_or_insert_default().insert(user_id, allow)
    }

    /// Set a permission rule for a role, returns replaced rule if there was any.
    pub fn set_role(&mut self, role_id: Id<RoleMarker>, allow: bool) -> Option<bool> {
        self.roles.get_or_insert_default().insert(role_id, allow)
    }

    /// Remove a permission rule for a user, returns removed rule if there was any.
    pub fn remove_user(&mut self, user_id: Id<UserMarker>) -> Option<bool> {
        self.users.as_mut()?.remove(&user_id)
    }

    /// Remove a permission rule for a role, returns removed rule if there was any.
    pub fn remove_role(&mut self, role_id: Id<RoleMarker>) -> Option<bool> {
        self.roles.as_mut()?.remove(&role_id)
    }

    /// Returns `true` if channel is disabled.
    pub fn is_channel_disabled(&self, channel_id: Id<ChannelMarker>) -> bool {
        self.disabled_channels
            .as_ref()
            .map(|dc| dc.contains(&channel_id))
            .unwrap_or(false)
    }

    /// Add channel to the disabled channels list, returns `true` if it was not yet present.
    pub fn disable_channel(&mut self, channel_id: Id<ChannelMarker>) -> bool {
        self.disabled_channels
            .get_or_insert_default()
            .insert(channel_id)
    }

    /// Remove a channel from the disabled channels list, returns `true` if it was found and removed.
    pub fn enable_channel(&mut self, channel_id: Id<ChannelMarker>) -> bool {
        self.disabled_channels
            .as_mut()
            .map(|dc| dc.remove(&channel_id))
            .unwrap_or(false)
    }
}

/// Serializable bot configuration.
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
pub struct Config {
    pub global: Settings,
    // Guild settings are serialized to separate files.
    #[serde(skip_serializing, default)]
    pub guilds: HashMap<Id<GuildMarker>, Settings>,
}

impl Config {
    /// Load the configuration file from `CONFIG_FILE`.
    pub fn load() -> AnyResult<Config> {
        info!("Loading config file");

        let mut cfg = String::new();
        {
            let mut config = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(CONFIG_FILE)?;

            config.read_to_string(&mut cfg)?;
        }

        match serde_json::from_str::<Config>(&cfg) {
            Ok(mut c) => {
                c.load_guild_settings()?;

                Ok(c)
            },
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

    /// Write the configuration to a file in `CONFIG_FILE`.
    /// # Notes
    /// This will truncate and overwrite the file, any changes that are not in the new data will be lost.
    pub fn write(&self) -> AnyResult<()> {
        info!("Updating config file");

        let config = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(CONFIG_FILE)?;

        serde_json::to_writer_pretty(config, self)?;

        // Write guild configuration files.
        self.write_guild_settings()?;

        Ok(())
    }

    /// Get guild's config.
    pub fn guild(&self, guild_id: Id<GuildMarker>) -> Option<&Settings> {
        self.guilds.get(&guild_id)
    }

    /// Get mutable reference to guild's config, if exists.
    pub fn guild_mut(&mut self, guild_id: Id<GuildMarker>) -> Option<&mut Settings> {
        self.guilds.get_mut(&guild_id)
    }

    /// Get mutable reference to guild's config. Creates default if not yet found.
    pub fn guild_or_default(&mut self, guild_id: Id<GuildMarker>) -> &mut Settings {
        self.guilds.entry(guild_id).or_default()
    }

    /// Set guild's custom prefix, returns previously set prefix.
    pub fn set_prefix(&mut self, guild_id: Id<GuildMarker>, prefix: &str) -> String {
        mem::replace(
            &mut self.guild_or_default(guild_id).prefix,
            prefix.to_string(),
        )
    }

    /// Add an alias, returns `Some(alias_command)` if it replaced one.
    pub fn add_alias(&mut self, guild_id: Id<GuildMarker>, alias: Alias) -> Option<String> {
        self.guild_or_default(guild_id)
            .aliases_mut()
            .insert(alias.name, alias.command)
    }

    /// Remove an alias, returns the removed alias value `Some(alias_command)` if successful.
    pub fn remove_alias(&mut self, guild_id: Id<GuildMarker>, alias_name: &str) -> Option<String> {
        self.guild_mut(guild_id)?.aliases_mut().remove(alias_name)
    }

    /// Look up all guild configurations in `GUILD_CONFIG_DIR` and save them to `self`.
    fn load_guild_settings(&mut self) -> AnyResult<()> {
        fs::create_dir_all(GUILD_CONFIG_DIR)
            .map_err(|e| anyhow::anyhow!("Failed to create guilds dir: {}", e))?;

        let paths = fs::read_dir(GUILD_CONFIG_DIR)?.flatten().map(|p| p.path());

        for path in paths {
            let content = fs::read_to_string(&path)?;
            let settings = serde_json::from_str::<Settings>(&content)?;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("Invalid file name"))?;

            match name.parse() {
                Ok(id) => {
                    self.guilds.insert(id, settings);
                },
                Err(e) => {
                    let path = path.display();
                    warn!("Could not parse guild config file name '{path}': {e}");
                },
            }
        }

        Ok(())
    }

    /// Save guild configurations in `self` to `GUILD_CONFIG_DIR`.
    fn write_guild_settings(&self) -> AnyResult<()> {
        fs::create_dir_all(GUILD_CONFIG_DIR)
            .map_err(|e| anyhow::anyhow!("Failed to create guilds dir: {}", e))?;

        for (id, settings) in self.guilds.iter() {
            let file_name = format!("{id}.json");

            let guild_config = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(Path::new(GUILD_CONFIG_DIR).join(file_name))?;

            serde_json::to_writer_pretty(guild_config, settings)?;
        }

        Ok(())
    }
}
