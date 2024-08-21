#![feature(decl_macro)]
#![feature(iter_intersperse)]
#![feature(iterator_try_collect)]
#![feature(option_get_or_insert_default)]
#![feature(pattern)]
#![feature(trait_alias)]

use std::env;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::mpsc::UnboundedSender;
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::stream::ShardRef;
use twilight_gateway::{
    stream, ConfigBuilder, Event, EventTypeFlags, MessageSender, Shard, ShardId,
};
use twilight_http::client::InteractionClient;
use twilight_http::Client;
use twilight_model::channel::Channel;
use twilight_model::gateway::payload::incoming::{ChannelUpdate, RoleUpdate};
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;
use twilight_model::gateway::presence::{ActivityType, MinimalActivity, Status};
use twilight_model::gateway::Intents;
use twilight_model::guild::Role;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use twilight_model::id::Id;
use twilight_model::oauth::Application;
use twilight_model::user::CurrentUser;
use twilight_standby::Standby;

use crate::commands::Commands;
use crate::config::BotConfig;
use crate::utils::prelude::*;

pub mod commands;
pub mod config;
pub mod parser;
pub mod utils;

pub type BotEventSender = UnboundedSender<BotEvent>;

/// Shard id and channel.
#[derive(Debug, Clone)]
pub struct PartialShard {
    pub id: ShardId,
    pub sender: MessageSender,
}

/// Common bot context that contains field for managing and operating the bot.
#[derive(Clone)]
pub struct Context {
    /// Bot configuration.
    pub config: Arc<BotConfig>,
    /// Bot commands list.
    pub commands: Arc<Commands>,
    /// Bot events channel.
    pub events_tx: BotEventSender,
    /// Application http client.
    pub http: Arc<Client>,
    /// Application information.
    pub application: Arc<Application>,
    /// Application bot user.
    pub user: Arc<CurrentUser>,
    /// Caching of twilight events.
    pub cache: Arc<InMemoryCache>,
    /// Standby twilight event system.
    pub standby: Arc<Standby>,
    /// Shard associated with the event.
    pub shard: Option<PartialShard>,
    /// Songbird voice manager.
    #[cfg(feature = "voice")]
    pub voice: Arc<songbird::Songbird>,
}

impl Context {
    pub async fn new(
        events_tx: BotEventSender,
        commands: Commands,
    ) -> AnyResult<(Self, Vec<Shard>)> {
        let config = Arc::new(BotConfig::new()?);
        let commands = Arc::new(commands);
        let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");
        let http = Arc::new(Client::new(token.to_owned()));
        let application = Arc::new(http.current_user_application().send().await?);
        let user = Arc::new(http.current_user().send().await?);
        let cache = Arc::new(InMemoryCache::new());
        let standby = Arc::new(Standby::new());

        let shards = stream::create_recommended(
            &http,
            ConfigBuilder::new(token, intents())
                .event_types(event_type_flags())
                .presence(UpdatePresencePayload::new(
                    vec![
                        MinimalActivity {
                            kind: ActivityType::Watching,
                            name: "you".into(),
                            url: None,
                        }
                        .into(),
                    ],
                    false,
                    None,
                    Status::Online,
                )?)
                .build(),
            |_, builder| builder.build(),
        )
        .await?
        .collect::<Vec<_>>();

        #[cfg(feature = "voice")]
        let voice = {
            Arc::new(songbird::Songbird::twilight(
                Arc::new(songbird::shards::TwilightMap::new(
                    shards
                        .iter()
                        .map(|s| (s.id().number(), s.sender()))
                        .collect(),
                )),
                user.id,
            ))
        };

        Ok((
            Self {
                config,
                commands,
                events_tx,
                http,
                application,
                user,
                cache,
                standby,
                shard: None,
                #[cfg(feature = "voice")]
                voice,
            },
            shards,
        ))
    }

    pub async fn handle<Fut>(
        &self,
        shard: ShardRef<'_>,
        event: Event,
        handler: fn(Self, Event) -> Fut,
    ) where
        Fut: Future<Output = AnyResult<()>> + Send + 'static,
    {
        // Update the cache with the event.
        self.cache.update(&event);

        // Update songbird if enabled.
        #[cfg(feature = "voice")]
        self.voice.process(&event).await;

        // Update standby events.
        let processed = self.standby.process(&event);
        log_processed(processed);

        // Handle event.
        tokio::spawn(handler(
            self.clone().with_shard(shard.id(), shard.sender()),
            event,
        ));
    }

    /// Get role objects with `ids` from cache or fetch from client.
    pub async fn roles_from(
        &self,
        guild_id: Id<GuildMarker>,
        ids: &[Id<RoleMarker>],
    ) -> AnyResult<Vec<Role>> {
        let cached_roles = ids
            .iter()
            .map(|id| self.cache.role(*id).map(|r| r.resource().to_owned()))
            .try_collect();
        match cached_roles {
            Some(r) => Ok(r),
            None => self.fetch_roles_from(guild_id, ids).await,
        }
    }

    /// Fetch role objects with `ids` from client without cache.
    pub async fn fetch_roles_from(
        &self,
        guild_id: Id<GuildMarker>,
        ids: &[Id<RoleMarker>],
    ) -> AnyResult<Vec<Role>> {
        let mut fetch = self.http.roles(guild_id).send().await?;
        for role in fetch.iter().cloned() {
            self.cache.update(&RoleUpdate { guild_id, role });
        }
        fetch.retain(|r| ids.contains(&r.id));
        Ok(fetch)
    }

    /// Get the channel object from cache or fetch from client.
    pub async fn channel_from(&self, channel_id: Id<ChannelMarker>) -> AnyResult<Channel> {
        match self.cache.channel(channel_id) {
            Some(chan) => Ok(chan.to_owned()),
            None => {
                let chan = self.http.channel(channel_id).send().await?;
                self.cache.update(&ChannelUpdate(chan.clone()));
                Ok(chan)
            },
        }
    }

    /// Search for a voice channel that a user is connected to in a guild.
    pub async fn user_voice_channel(
        &self,
        guild_id: Id<GuildMarker>,
        user_id: Id<UserMarker>,
    ) -> AnyResult<Id<ChannelMarker>> {
        match self.cache.voice_state(user_id, guild_id) {
            Some(s) => Some(s.channel_id()),
            None => {
                // `voice_states` is empty in some cases?
                let g = self.http.guild(guild_id).send().await?;
                g.voice_states
                    .into_iter()
                    .filter_map(|v| Some((v.member?.user.id, v.channel_id?)))
                    .find(|(u, _)| *u == user_id)
                    .map(|(_, c)| c)
            },
        }
        .with_context(|| {
            format!("User '{user_id}' was not found in voice channels of guild '{guild_id}'")
        })
    }

    /// This context with the provided shard id.
    pub fn with_shard(mut self, id: ShardId, sender: MessageSender) -> Self {
        self.shard = Some(PartialShard { id, sender });
        self
    }

    /// Shortcut for `self.http.interaction(self.application.id)`.
    pub fn interaction(&self) -> InteractionClient {
        self.http.interaction(self.application.id)
    }
}

#[derive(Debug)]
pub enum BotEvent {
    Shutdown,
}

fn log_processed(p: twilight_standby::ProcessResults) {
    if p.dropped() + p.fulfilled() + p.matched() + p.sent() > 0 {
        debug!(
            "Standby: {{ m: {}, d: {}, f: {}, s: {} }}",
            p.matched(),
            p.dropped(),
            p.fulfilled(),
            p.sent(),
        );
    }
}

/// Discord permission intents.
fn intents() -> Intents {
    #[cfg(feature = "all-intents")]
    {
        Intents::all()
    }

    #[cfg(not(feature = "all-intents"))]
    {
        Intents::MESSAGE_CONTENT
            | Intents::GUILDS
            | Intents::GUILD_MESSAGES
            | Intents::GUILD_MESSAGE_REACTIONS
            | Intents::GUILD_MEMBERS
            | Intents::GUILD_PRESENCES
            | Intents::GUILD_VOICE_STATES
            | Intents::DIRECT_MESSAGES
            | Intents::DIRECT_MESSAGE_REACTIONS
    }
}

/// Subscribed events from Discord.
fn event_type_flags() -> EventTypeFlags {
    EventTypeFlags::all()
        - EventTypeFlags::TYPING_START
        - EventTypeFlags::DIRECT_MESSAGE_TYPING
        - EventTypeFlags::GUILD_MESSAGE_TYPING
}
