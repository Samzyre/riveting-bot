use std::any;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use derive_more::{Deref, Display};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use twilight_model::channel::message::ReactionType;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker};
use twilight_model::id::Id;

use crate::config::storage::{Directory, Storage};
use crate::utils;
use crate::utils::prelude::*;

pub mod storage;

/// Returns a key which can be used to access reaction-roles mappings.
pub fn reaction_roles_key(channel_id: Id<ChannelMarker>, message_id: Id<MessageMarker>) -> String {
    format!("{channel_id}.{message_id}")
}

/// Custom data collection type.
pub type Custom = HashMap<String, serde_json::Value>;

/// Whitelist collection type.
pub type Whitelist = HashSet<Id<GuildMarker>>;

/// Global bot settings.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GlobalSettings {
    /// Global classic command prefix.
    #[serde(default)]
    pub prefix: Prefix,

    /// Whitelisted guilds, disabled if `None`.
    #[serde(default)]
    pub whitelist: Option<Whitelist>,
}

/// General guild settings.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GuildSettings {
    /// Guild specific classic command prefix.
    #[serde(default)]
    pub prefix: Prefix,

    // TODO: To be implemented.
    #[serde(default)]
    pub aliases: HashMap<String, String>,

    /// Guild reaction-role mappings.
    #[serde(default)]
    pub reaction_roles: HashMap<String, Vec<ReactionRole>>,
}

#[derive(Debug)]
pub struct BotConfig {
    storage: Storage,
}

impl BotConfig {
    /// Setup a new configuration.
    pub fn new() -> AnyResult<Self> {
        let mut storage = Storage::default();

        storage.bind::<GlobalSettings>("bot")?;
        storage.bind::<GuildSettings>("guild")?;
        storage.bind::<Custom>("custom")?;

        Ok(Self {
            storage: storage.validated()?,
        })
    }

    /// Return a reference to the inner storage type.
    pub const fn inner(&self) -> &Storage {
        &self.storage
    }

    /// Return general bot configuration directory.
    pub fn global(&self) -> Global {
        Global::new(self.storage.global())
    }

    /// Return guild configuration directory.
    pub fn guild(&self, guild_id: Id<GuildMarker>) -> Guild {
        Guild::new(self.storage.by_guild_id(guild_id), guild_id)
    }

    /// Modify global settings with a function.
    /// This method will save the changes to file and then returns
    /// with the return type of the closure.
    pub fn global_settings_with<R>(
        &self,
        f: impl Fn(&mut GlobalSettings) -> AnyResult<R>,
    ) -> AnyResult<R> {
        self.storage.global().save_with(f)
    }

    /// Modify guild settings with a function.
    /// This method will save the changes to file and then returns
    /// with the return type of the closure.
    pub fn guild_settings_with<R>(
        &self,
        guild_id: Id<GuildMarker>,
        f: impl Fn(&mut GuildSettings) -> AnyResult<R>,
    ) -> AnyResult<R> {
        self.storage.by_guild_id(guild_id).save_with(f)
    }

    /// Access custom data config.
    pub fn custom_entry(&self, guild_id: Option<Id<GuildMarker>>) -> CustomEntry {
        CustomEntry::new(self.directory(guild_id))
    }

    /// Return classic command prefix, either global prefix or a guild specific one.
    pub fn classic_prefix(&self, guild_id: Option<Id<GuildMarker>>) -> AnyResult<Prefix> {
        let global_prefix = || self.global().classic_prefix().map(ToOwned::to_owned);

        let guild_prefix = |guild_id| {
            self.guild(guild_id)
                .classic_prefix()
                .map(ToOwned::to_owned)
                .map_err(|e| debug!("{e}"))
                .or_else(|_| global_prefix())
        };

        guild_id.map_or_else(global_prefix, guild_prefix)
    }

    /// Returns global storage directory if `guild_id` is `None`,
    /// otherwise returns guild storage directory by guild id.
    fn directory(&self, guild_id: Option<Id<GuildMarker>>) -> Directory {
        guild_id.map_or_else(
            || self.storage.global(),
            |guild_id| self.storage.by_guild_id(guild_id),
        )
    }
}

/// Global data entry guard.
#[derive(Debug)]
pub struct Global<'a> {
    dir: Directory<'a>,
}

impl<'a> Global<'a> {
    /// Create a global access with a directory.
    pub const fn new(dir: Directory<'a>) -> Self {
        Self { dir }
    }

    /// Get global bot settings.
    pub fn bot_settings(&mut self) -> AnyResult<&GlobalSettings> {
        self.dir
            .load_or_default()
            .context("Failed to load bot settings")
    }

    /// Get guild whitelist.
    pub fn whitelist(&mut self) -> AnyResult<&Option<Whitelist>> {
        Ok(&self.bot_settings()?.whitelist)
    }

    /// Get global classic command prefix.
    pub fn classic_prefix(&mut self) -> AnyResult<&Prefix> {
        Ok(&self.bot_settings()?.prefix)
    }
}

/// Guild data entry guard.
#[derive(Debug)]
pub struct Guild<'a> {
    dir: Directory<'a>,
    guild_id: Id<GuildMarker>,
}

impl<'a> Guild<'a> {
    /// Create a guild access with a directory and guild id.
    pub const fn new(dir: Directory<'a>, guild_id: Id<GuildMarker>) -> Self {
        Self { dir, guild_id }
    }

    /// Get guild settings.
    pub fn settings(&mut self) -> AnyResult<&GuildSettings> {
        self.dir
            .load_or_default()
            .context("Failed to load settings")
    }

    /// Get guild classic command prefix.
    pub fn classic_prefix(&mut self) -> AnyResult<&Prefix> {
        Ok(&self.settings()?.prefix)
    }

    /// Get a reaction-roles configuration by channel and message ids.
    pub fn reaction_roles(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> AnyResult<Vec<ReactionRole>> {
        self.dir
            .load::<GuildSettings>()
            .and_then(|s| {
                let key = reaction_roles_key(channel_id, message_id);
                s.reaction_roles.get(&key).with_context(|| {
                    format!(
                        "No reaction-roles found for guild '{guild_id}' on channel '{channel_id}' \
                         with message '{message_id}'",
                        guild_id = self.guild_id
                    )
                })
            })
            .cloned()
    }

    /// Add a reaction-role configuration.
    pub fn add_reaction_roles(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
        map: Vec<ReactionRole>,
    ) -> AnyResult<()> {
        self.dir.save_with::<GuildSettings, _>(|s| {
            let key = reaction_roles_key(channel_id, message_id);
            s.reaction_roles.insert(key, map);
            Ok(())
        })
    }

    /// Remove a reaction-role configuration.
    pub fn remove_reaction_roles(
        &mut self,
        channel_id: Id<ChannelMarker>,
        message_id: Id<MessageMarker>,
    ) -> AnyResult<()> {
        self.dir.save_with::<GuildSettings, _>(|s| {
            let key = reaction_roles_key(channel_id, message_id);
            s.reaction_roles.remove(&key);
            Ok(())
        })
    }
}

/// Error for when data does not match type.
#[derive(Debug, Error)]
#[error("Custom data with name '{name}' is not compatible with type '{ty_name}'")]
struct IncompatibleTypeError {
    ty_name: &'static str,
    name: String,
}

impl IncompatibleTypeError {
    fn new<T>(name: &str) -> Self {
        Self {
            ty_name: any::type_name::<T>(),
            name: name.to_string(),
        }
    }
}

/// Custom data entry guard.
#[derive(Debug)]
pub struct CustomEntry<'a> {
    dir: Directory<'a>,
}

impl<'a> CustomEntry<'a> {
    /// Create a new custom entry guard.
    pub const fn new(dir: Directory<'a>) -> Self {
        Self { dir }
    }

    /// Get custom data, if exists.
    pub fn get<T>(&self, name: &str) -> AnyResult<T>
    where
        T: DeserializeOwned,
    {
        self.dir
            .get::<Custom>()
            .with_context(|| format!("Custom data with name '{name}' is not loaded"))
            .and_then(|c| {
                c.get(name)
                    .with_context(|| format!("No custom data with name '{name}' found"))
            })
            .and_then(|v| serde_json::from_value(v.to_owned()).map_err(Into::into))
    }

    /// Force save custom data.
    pub fn overwrite<T>(&mut self, name: String, data: T) -> AnyResult<()>
    where
        T: Serialize,
    {
        self.save_with(|c| {
            let value = serde_json::to_value(data)?;
            c.insert(name, value);
            Ok(())
        })
    }

    /// Save custom data.
    ///
    /// # Errors
    /// If previously saved data is not compatible with the type.
    pub fn save<T>(&mut self, name: String, data: T) -> AnyResult<()>
    where
        T: Serialize + DeserializeOwned,
    {
        self.save_with(|c| {
            if let Some(value) = c.remove(&name) {
                serde_json::from_value::<T>(value)
                    .with_context(|| IncompatibleTypeError::new::<T>(&name))?;
            }
            let value = serde_json::to_value(data)?;
            c.insert(name, value);
            Ok(())
        })
    }

    /// Load custom data.
    pub fn load<T>(&mut self, name: &str) -> AnyResult<T>
    where
        T: DeserializeOwned,
    {
        self.dir
            .load::<Custom>()
            .with_context(|| format!("Could not load custom data with name '{name}'"))
            .and_then(|c| {
                c.get(name)
                    .with_context(|| format!("No custom data with name '{name}' found"))
            })
            .and_then(|v| {
                serde_json::from_value(v.to_owned())
                    .with_context(|| IncompatibleTypeError::new::<T>(name))
            })
    }

    /// Load custom data or create default.
    ///
    /// # Errors
    /// If loaded data is not compatible with the type.
    pub fn load_or_default<T>(&mut self, name: String) -> AnyResult<T>
    where
        T: Default + Serialize + DeserializeOwned,
    {
        let value = match self
            .dir
            .load_or_default_mut::<Custom>()?
            .entry(name.to_string())
        {
            Entry::Occupied(o) => o.get().to_owned(),
            Entry::Vacant(v) => {
                let value = serde_json::to_value(T::default())?;
                v.insert(value.to_owned());
                self.dir.save_from_memory::<Custom>()?;
                value
            },
        };
        serde_json::from_value(value).with_context(|| IncompatibleTypeError::new::<T>(&name))
    }

    /// Modify custom data with a function.
    fn save_with<R>(&mut self, f: impl FnOnce(&mut Custom) -> AnyResult<R>) -> AnyResult<R> {
        self.dir.save_with(f)
    }
}

/// Bot classic command prefix.
#[derive(Debug, Clone, Deref, Display, Serialize, Deserialize)]
pub struct Prefix(String);

impl Prefix {
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Default for Prefix {
    fn default() -> Self {
        Self(String::from("!"))
    }
}

impl AsRef<str> for Prefix {
    fn as_ref(&self) -> &str {
        self
    }
}

/// Reaction-role mapping with the reaction type and role id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionRole {
    pub emoji: ReactionType,
    pub role: Id<RoleMarker>,
}

impl ReactionRole {
    pub const fn new(emoji: ReactionType, role: Id<RoleMarker>) -> Self {
        Self { emoji, role }
    }
}

impl PartialEq for ReactionRole {
    fn eq(&self, other: &Self) -> bool {
        utils::reaction_type_eq(&self.emoji, &other.emoji) && self.role == other.role
    }
}
