#![allow(clippy::redundant_pub_crate)]
#![allow(clippy::significant_drop_in_scrutinee)]

use std::sync::{Arc, Mutex};
use std::{env, fs};

use riveting_bot::commands::{CommandError, handle};
use riveting_bot::utils::prelude::*;
use riveting_bot::utils::{self};
use riveting_bot::{BotEvent, BotEventSender, Context};
use tokio::sync::mpsc;
use tracing::Level;
use tracing_subscriber::EnvFilter;
use twilight_gateway::stream::ShardEventStream;
use twilight_gateway::{CloseFrame, Event};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::Message;
use twilight_model::gateway::GatewayReaction;
use twilight_model::gateway::payload::incoming::{
    Hello, MessageDelete, MessageDeleteBulk, MessageUpdate, Ready,
};
use twilight_model::guild::Guild;
use twilight_model::id::Id;
use twilight_model::voice::VoiceState;

mod bot;

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

    // Bot events channel.
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();

    // Spawn ctrl-c shutdown task.
    tokio::spawn(shutdown_task(events_tx.clone()));

    let (ctx, mut shards) = Context::new(events_tx, bot::create_commands()?).await?;

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

        ctx.handle(shard, event, handle_event).await;
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
            handle::application_command(ctx, inter, *d)
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

    match handle::classic_command(ctx, Arc::clone(&msg)).await {
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
