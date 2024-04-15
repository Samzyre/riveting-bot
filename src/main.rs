#![feature(associated_type_defaults)]
#![feature(decl_macro)]
#![feature(iter_intersperse)]
#![feature(iterator_try_collect)]
#![feature(iterator_try_reduce)]
#![feature(option_get_or_insert_default)]
#![feature(pattern)]
#![feature(trait_alias)]
//
#![allow(dead_code)]
#![allow(clippy::significant_drop_in_scrutinee)]

use std::sync::{Arc, Mutex};
use std::{env, fs};

use tokio::sync::mpsc::{self, UnboundedSender};
use tracing::Level;
use tracing_subscriber::EnvFilter;
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::stream::ShardEventStream;
use twilight_gateway::{
    stream, CloseFrame, ConfigBuilder, Event, EventTypeFlags, MessageSender, ShardId,
};
use twilight_http::client::InteractionClient;
use twilight_http::Client;
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::{Channel, Message};
use twilight_model::gateway::payload::incoming::{
    ChannelUpdate, Hello, MessageDelete, MessageDeleteBulk, MessageUpdate, Ready, RoleUpdate,
};
use twilight_model::gateway::payload::outgoing::update_presence::UpdatePresencePayload;
use twilight_model::gateway::presence::{ActivityType, MinimalActivity, Status};
use twilight_model::gateway::{GatewayReaction, Intents};
use twilight_model::guild::{Guild, Role};
use twilight_model::id::marker::{ChannelMarker, GuildMarker, RoleMarker, UserMarker};
use twilight_model::id::Id;
use twilight_model::oauth::Application;
use twilight_model::user::CurrentUser;
use twilight_model::voice::VoiceState;
use twilight_standby::Standby;

use crate::commands::{CommandError, Commands};
use crate::config::BotConfig;
use crate::utils::prelude::*;

mod commands;
mod config;
mod parser;
mod utils;

pub type BotEventSender = UnboundedSender<BotEvent>;

/// Shard id and channel.
#[derive(Debug, Clone)]
pub struct PartialShard {
    id: ShardId,
    sender: MessageSender,
}

/// Common bot context that contains field for managing and operating the bot.
#[derive(Clone)]
pub struct Context {
    /// Bot configuration.
    config: Arc<BotConfig>,
    /// Bot commands list.
    commands: Arc<Commands>,
    /// Bot events channel.
    events_tx: BotEventSender,
    /// Application http client.
    http: Arc<Client>,
    /// Application information.
    application: Arc<Application>,
    /// Application bot user.
    user: Arc<CurrentUser>,
    /// Caching of events.
    cache: Arc<InMemoryCache>,
    /// Standby event system.
    standby: Arc<Standby>,
    /// Shard associated with the event.
    shard: Option<PartialShard>,
    /// Songbird voice manager.
    #[cfg(feature = "voice")]
    voice: Arc<songbird::Songbird>,
}

impl Context {
    /// Shortcut for `self.http.interaction(self.application.id)`.
    pub fn interaction(&self) -> InteractionClient {
        self.http.interaction(self.application.id)
    }

    /// Get role objects with `ids` from cache or fetch from client.
    pub async fn roles_from(
        &self,
        guild_id: Id<GuildMarker>,
        ids: &[Id<RoleMarker>],
    ) -> AnyResult<Vec<Role>> {
        // Try to get the roles from cache.
        let cached_roles = ids
            .iter()
            .map(|id| self.cache.role(*id).map(|r| r.resource().to_owned()))
            .try_collect();

        // Use cached roles or otherwise fetch from client.
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

        // Manually update the cache.
        for role in fetch.iter().cloned() {
            self.cache.update(&RoleUpdate { guild_id, role });
        }

        fetch.retain(|r| ids.contains(&r.id));

        Ok(fetch)
    }

    /// Get the channel object from cache or fetch from client.
    pub async fn channel_from(&self, channel_id: Id<ChannelMarker>) -> AnyResult<Channel> {
        match self.cache.channel(channel_id) {
            Some(chan) => {
                // Use cached channel.
                Ok(chan.to_owned())
            },
            None => {
                // Fetch channel from the http client.
                let chan = self.http.channel(channel_id).send().await?;

                // Manually update the cache.
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
                // FIXME: `voice_states` is empty.
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
    fn with_shard(mut self, id: ShardId, sender: MessageSender) -> Self {
        self.shard = Some(PartialShard { id, sender });
        self
    }
}

#[derive(Debug)]
pub enum BotEvent {
    Shutdown,
}

#[tracing::instrument]
#[tokio::main]
async fn main() -> AnyResult<()> {
    // Load environment variables from `./.env` file, if any exists.
    simple_env_load::load_env_from([".env"]);

    // Create data folder if it doesn't exist yet.
    std::fs::create_dir_all("./data/")
        .map_err(|e| anyhow::anyhow!("Failed to create data folder: {}", e))?;

    // Create a log file or truncate an existing one.
    let logfile = fs::File::create("./data/log.log")
        .map_err(|e| anyhow::anyhow!("Failed to create log file: {}", e))?;

    // Initialize the logger to use `RUST_LOG` environment variable.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(Level::DEBUG.into())
                .try_from_env()
                .with_context(|| {
                    format!(
                        "Problem with `RUST_LOG={}`",
                        env::var("RUST_LOG").unwrap_or_default()
                    )
                })?,
        )
        .with_ansi(false)
        .with_writer(Mutex::new(logfile))
        .compact()
        .init();

    // Setup bot configuration files.
    let config = Arc::new(BotConfig::new()?);

    // Initialize chat and interaction commands.
    let commands = Arc::new(commands::bot::create_commands()?);

    // Bot events channel.
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();

    // Spawn ctrl-c shutdown task.
    tokio::spawn(shutdown_task(events_tx.clone()));

    // Get discord bot token from environment variable.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Create an http client.
    let http = Arc::new(Client::new(token.to_owned()));

    // Get the application info, such as its id and owner.
    let application = Arc::new(http.current_user_application().send().await?);

    // Get the bot user info.
    let user = Arc::new(http.current_user().send().await?);

    // Create a cache.
    let cache = Arc::new(InMemoryCache::new());

    // Create a standby instance.
    let standby = Arc::new(Standby::new());

    // Create the shards.
    let mut shards = stream::create_recommended(
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

    let ctx = Context {
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
    };

    // Create an infinite stream over the shards' events.
    let mut stream = ShardEventStream::new(shards.iter_mut());

    loop {
        use futures::prelude::*;

        let (shard, event) = tokio::select! {
            Some(twilight_event) = stream.next() => twilight_event,
            Some(BotEvent::Shutdown) = events_rx.recv() => break,
            else => break,
        };

        // Process each event as they come in.
        let event = match event {
            Ok(event) => event,
            Err(source) => {
                eprintln!("Error receiving event: {:?}", source);
                if source.is_fatal() {
                    error!(?source, "Error receiving event");
                    break;
                } else {
                    warn!(?source, "Error receiving event");
                    continue;
                }
            },
        };

        // Update the cache with the event.
        ctx.cache.update(&event);

        // Update songbird if enabled.
        #[cfg(feature = "voice")]
        ctx.voice.process(&event).await;

        // Update standby events.
        let processed = ctx.standby.process(&event);
        log_processed(processed);

        // Handle event.
        tokio::spawn(handle_event(
            ctx.clone().with_shard(shard.id(), shard.sender()),
            event,
        ));
    }

    drop(stream);

    for shard in shards.iter_mut() {
        let _ = shard
            .close(CloseFrame::NORMAL)
            .await
            .map_err(|e| warn!("{e}"));
    }

    Ok(())
}

/// Ctrl-C shutdown task.
async fn shutdown_task(events_tx: BotEventSender) -> AnyResult<()> {
    tokio::signal::ctrl_c()
        .await
        .expect("Could not register ctrl+c handler");
    info!("Shutting down by ctrl-c");
    events_tx.send(BotEvent::Shutdown)?;
    println!("Ctrl-C");
    Ok(())
}

/// Main events handler.
#[tracing::instrument(name = "events", skip_all, fields(event = event.kind().name()))]
async fn handle_event(ctx: Context, event: Event) -> AnyResult<()> {
    let result = match event {
        Event::Ready(r) => handle_ready(&ctx, *r).await,
        Event::GuildCreate(g) => handle_guild_create(&ctx, g.0).await,
        Event::InteractionCreate(i) => handle_interaction_create(&ctx, i.0).await,
        Event::MessageCreate(mc) => handle_message_create(&ctx, mc.0).await,
        Event::MessageUpdate(mu) => handle_message_update(&ctx, *mu).await,
        Event::MessageDelete(md) => handle_message_delete(&ctx, md).await,
        Event::MessageDeleteBulk(mdb) => handle_message_delete_bulk(&ctx, mdb).await,
        Event::ReactionAdd(r) => handle_reaction_add(&ctx, r.0).await,
        Event::ReactionRemove(r) => handle_reaction_remove(&ctx, r.0).await,
        Event::VoiceStateUpdate(v) => handle_voice_state(&ctx, v.0).await,
        Event::CommandPermissionsUpdate(cpu) => {
            debug!(
                "Permissions update event: Command '{}' in guild '{}'",
                cpu.id, cpu.guild_id
            );
            Ok(())
        },

        // Gateway events.
        Event::GatewayHello(h) => handle_hello(&ctx, h).await,
        Event::GatewayHeartbeat(_)
        | Event::GatewayInvalidateSession(_)
        | Event::GatewayReconnect => {
            debug!("Gateway event: {:?}", event.kind());
            Ok(())
        },
        Event::GatewayHeartbeatAck => {
            trace!("Gateway event: {:?}", event.kind());
            Ok(())
        },

        Event::PresenceUpdate(p) => {
            trace!("Presence event: {:?}", p.user.id());
            Ok(())
        },

        // Other events here...
        event => {
            println!("Event: {:?}", event.kind());
            debug!("Event: {:?}", event.kind());
            Ok(())
        },
    };

    if let Err(e) = result {
        let chain = e.oneliner();
        eprintln!("Event error: {e:?}");
        error!("Event error: {chain}");

        if let Ok(id) = env::var("DISCORD_BOTDEV_CHANNEL") {
            // Send error as message on bot dev channel.
            let bot_dev = Id::new(id.parse()?);
            ctx.http
                .create_message(bot_dev)
                .content(&format!("{e:?}"))?
                .send()
                .await?;
        }
    }

    Ok(())
}

async fn handle_hello(ctx: &Context, h: Hello) -> AnyResult<()> {
    info!(
        "Connected on shard {} with a heartbeat of {}",
        ctx.shard
            .as_ref()
            .map(|s| s.id)
            .ok_or_else(|| anyhow::anyhow!("Missing shard id"))?,
        h.heartbeat_interval
    );
    Ok(())
}

async fn handle_ready(ctx: &Context, ready: Ready) -> AnyResult<()> {
    println!("Ready: '{}'", ready.user.name);
    info!("Ready: '{}'", ready.user.name);

    let commands = ctx.commands.twilight_commands()?;

    debug!("Creating {} global commands", commands.len());

    // Set global application commands.
    ctx.http
        .interaction(ctx.application.id)
        .set_global_commands(&commands)
        .send()
        .await?;

    Ok(())
}

async fn handle_guild_create(ctx: &Context, guild: Guild) -> AnyResult<()> {
    println!("Guild: {}", guild.name);
    info!("Guild: '{}'", guild.name);

    let whitelist = ctx.config.global().whitelist()?.to_owned();

    // If whitelist is enabled, check if this guild is in it.
    if let Some(whitelist) = whitelist {
        if !whitelist.contains(&guild.id) {
            info!("Leaving a non-whitelisted guild '{}'", guild.id);
            ctx.http.leave_guild(guild.id).await?;
        } else {
            debug!("Whitelisted guild: '{}'", guild.id)
        }
    }

    // ctx.http
    //     .interaction(ctx.application.id)
    //     .set_guild_commands(guild.id, &commands)
    //     .send()
    //     .await?;

    Ok(())
}

async fn handle_interaction_create(ctx: &Context, mut inter: Interaction) -> AnyResult<()> {
    // println!("{:#?}", inter);

    // Take interaction data from the interaction,
    // so that both can be passed forward without matching again.
    match inter.data.take() {
        Some(InteractionData::ApplicationCommand(d)) => {
            println!("{d:#?}");
            crate::commands::handle::application_command(ctx, inter, *d)
                .await
                .context("Failed to handle application command")?;
        },
        Some(InteractionData::MessageComponent(d)) => {
            println!("{d:#?}");
            //
        },
        Some(InteractionData::ModalSubmit(d)) => {
            println!("{d:#?}");
            //
        },
        Some(d) => {
            println!("{d:#?}");
            //
        },
        None => println!("{inter:#?}"),
    }

    Ok(())
}

async fn handle_message_create(ctx: &Context, msg: Message) -> AnyResult<()> {
    // Ignore bot users.
    if msg.author.bot {
        trace!("Message sender is a bot '{}'", msg.author.name);
        return Ok(());
    }

    let msg = Arc::new(msg);

    match crate::commands::handle::classic_command(ctx, Arc::clone(&msg)).await {
        Err(CommandError::NotPrefixed) => {
            // Message was not a classic command.

            if msg.mentions.iter().any(|mention| mention.id == ctx.user.id)
                && msg.referenced_message.is_none()
            {
                // Send bot help message.
                let about_msg = format!(
                    "Try `/about` or `{prefix}about` for general info, or `/help` or \
                     `{prefix}help` for commands.",
                    prefix = ctx.config.classic_prefix(msg.guild_id)?,
                );

                ctx.http
                    .create_message(msg.channel_id)
                    .content(&about_msg)?
                    .reply(msg.id)
                    .await?;
            }
            Ok(())
        },
        Err(CommandError::AccessDenied) => {
            ctx.http
                .create_message(msg.channel_id)
                .content("Rekt, you cannot use that. :melting_face:")?
                .reply(msg.id)
                .await?;
            Ok(())
        },
        res => res.context("Failed to handle classic command"),
    }
}

async fn handle_message_update(_ctx: &Context, _mu: MessageUpdate) -> AnyResult<()> {
    // TODO Check if updated message is something that should update content from the bot.
    Ok(())
}

async fn handle_message_delete(ctx: &Context, md: MessageDelete) -> AnyResult<()> {
    let Some(guild_id) = md.guild_id else {
        return Ok(());
    };

    // Remove reaction roles mappping, if deleted message was one.
    ctx.config
        .guild(guild_id)
        .remove_reaction_roles(md.channel_id, md.id)?;

    Ok(())
}

async fn handle_message_delete_bulk(ctx: &Context, mdb: MessageDeleteBulk) -> AnyResult<()> {
    let message_delete_with = |id| MessageDelete {
        channel_id: mdb.channel_id,
        guild_id: mdb.guild_id,
        id,
    };

    for id in mdb.ids {
        // Calls single delete handler for every deletion.
        handle_message_delete(ctx, message_delete_with(id)).await?;
    }

    Ok(())
}

async fn handle_reaction_add(ctx: &Context, reaction: GatewayReaction) -> AnyResult<()> {
    let Some(guild_id) = reaction.guild_id else {
        return Ok(());
    };

    let user = match reaction.member {
        Some(m) => m.user,
        None => match ctx.cache.user(reaction.user_id) {
            Some(m) => m.to_owned(),
            None => ctx.http.user(reaction.user_id).send().await?,
        },
    };

    // Ignore reactions from bots.
    if user.bot {
        return Ok(());
    }

    // Check if message is cached.
    if let Some(msg) = ctx.cache.message(reaction.message_id) {
        // Ignore if message is not from this bot.
        if msg.author() != ctx.user.id {
            return Ok(());
        }
    }

    let add_roles = match ctx
        .config
        .guild(guild_id)
        .reaction_roles(reaction.channel_id, reaction.message_id)
    {
        Ok(map) => map
            .iter()
            .filter(|rr| utils::reaction_type_eq(&rr.emoji, &reaction.emoji))
            .map(|rr| rr.role)
            .collect::<Vec<_>>(),
        Err(e) => {
            debug!("{e}");
            return Ok(());
        },
    };

    if add_roles.is_empty() {
        info!("No roles to add for '{}'", user.name);
    } else {
        info!("Adding roles for '{}'", user.name);
        for role_id in add_roles {
            ctx.http
                .add_guild_member_role(guild_id, reaction.user_id, role_id)
                .await?;
        }
    }

    Ok(())
}

async fn handle_reaction_remove(ctx: &Context, reaction: GatewayReaction) -> AnyResult<()> {
    let Some(guild_id) = reaction.guild_id else {
        return Ok(());
    };

    let user = match reaction.member {
        Some(m) => m.user,
        None => match ctx.cache.user(reaction.user_id) {
            Some(m) => m.to_owned(),
            None => ctx.http.user(reaction.user_id).send().await?,
        },
    };

    // Ignore reactions from bots.
    if user.bot {
        return Ok(());
    }

    // Check if message is cached.
    if let Some(msg) = ctx.cache.message(reaction.message_id) {
        // Ignore if message is not from this bot.
        if msg.author() != ctx.user.id {
            return Ok(());
        }
    }

    let remove_roles = match ctx
        .config
        .guild(guild_id)
        .reaction_roles(reaction.channel_id, reaction.message_id)
    {
        Ok(map) => map
            .iter()
            .filter(|rr| utils::reaction_type_eq(&rr.emoji, &reaction.emoji))
            .map(|rr| rr.role)
            .collect::<Vec<_>>(),
        Err(e) => {
            debug!("{e}");
            return Ok(());
        },
    };

    if remove_roles.is_empty() {
        info!("No roles to remove for '{}'", user.name);
    } else {
        info!("Removing roles for '{}'", user.name);
        for role_id in remove_roles {
            ctx.http
                .remove_guild_member_role(guild_id, reaction.user_id, role_id)
                .await?;
        }
    }

    Ok(())
}

async fn handle_voice_state(_ctx: &Context, _voice: VoiceState) -> AnyResult<()> {
    // println!("{voice:#?}",);
    Ok(())
}

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

fn event_type_flags() -> EventTypeFlags {
    EventTypeFlags::all()
        - EventTypeFlags::TYPING_START
        - EventTypeFlags::DIRECT_MESSAGE_TYPING
        - EventTypeFlags::GUILD_MESSAGE_TYPING
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
