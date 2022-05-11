#![feature(let_else)]
#![feature(decl_macro)]
#![feature(pattern)]
#![feature(associated_type_bounds)]
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::{env, fs};

use futures::stream::StreamExt;
use tracing::Level;
use tracing_subscriber::EnvFilter;
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::cluster::ShardScheme;
use twilight_gateway::{Cluster, Event};
use twilight_http::Client;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::Message;
use twilight_model::gateway::event::shard::Connected;
use twilight_model::gateway::payload::incoming::Ready;
use twilight_model::gateway::Intents;
use twilight_model::guild::Guild;
use twilight_model::id::Id;
use twilight_model::oauth::Application;
use twilight_model::user::CurrentUser;
use twilight_model::voice::VoiceState;
use twilight_standby::Standby;

use crate::commands::admin::scheduler::handle_timer;
use crate::commands::{ChatCommands, CommandError};
use crate::config::{BotConfig, Config};
use crate::utils::prelude::*;

mod commands;
mod config;
mod parser;
mod utils;

#[derive(Debug, Clone)]
pub struct Context {
    config: BotConfig,
    http: Arc<Client>,
    cluster: Arc<Cluster>,
    application: Arc<Application>,
    user: Arc<CurrentUser>,
    cache: Arc<InMemoryCache>,
    standby: Arc<Standby>,
    chat_commands: Arc<ChatCommands>,
    shard: Option<u64>,
}

impl Context {
    fn with_shard(mut self, id: u64) -> Self {
        self.shard = Some(id);
        self
    }
}

#[tokio::main]
async fn main() -> AnyResult<()> {
    // Load environment variables from `./.env` file, if any exists.
    dotenv::dotenv().ok();

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
                .from_env()?,
        )
        .with_ansi(false)
        .with_writer(Mutex::new(logfile))
        .compact()
        .init();

    // Load bot configuration file.
    let config = BotConfig::new(Config::load()?);

    // Get discord bot token from environment variable.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Create an http client.
    let http = Arc::new(Client::new(token.to_owned()));

    // Start a gateway connection, technically even a single shard would be enough for this, but hey.
    let (cluster, mut events) = Cluster::builder(token, intents())
        .shard_scheme(ShardScheme::Auto)
        .http_client(Arc::clone(&http))
        .build()
        .await?;
    let cluster = Arc::new(cluster);

    // Start up the cluster.
    {
        let cluster = Arc::clone(&cluster);
        tokio::spawn(async move {
            cluster.up().await;
        });
    }

    // Spawn ctrl-c shutdown task.
    {
        let cluster = Arc::clone(&cluster);
        tokio::spawn(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("Could not register ctrl+c handler");

            info!("Shutting down by ctrl-c");

            // cluster.down_resumable();
            cluster.down();

            println!("Ctrl-C");
        });
    }

    // Get the application info, such as its id and owner.
    let application = Arc::new(http.current_user_application().send().await?);

    // Get the bot user info.
    let user = Arc::new(http.current_user().send().await?);

    // Create a cache.
    let cache = Arc::new(InMemoryCache::new());

    // Create a standby instance.
    let standby = Arc::new(Standby::new());

    // Initialize chat commands.
    let chat_commands = Arc::new(ChatCommands::new());

    let ctx = Context {
        config,
        http,
        cluster,
        application,
        user,
        cache,
        standby,
        chat_commands,
        shard: None,
    };

    // Spawn community event handler.
    tokio::spawn(handle_timer(ctx.clone(), 60 * 5));

    // Process each event as they come in.
    while let Some((id, event)) = events.next().await {
        // Update the cache with the event.
        ctx.cache.update(&event);

        // Update standby events.
        let processed = ctx.standby.process(&event);
        println!("{processed:?}");

        tokio::spawn(handle_event(ctx.clone().with_shard(id), event));
    }

    Ok(())
}

/// Main events handler.
async fn handle_event(ctx: Context, event: Event) -> AnyResult<()> {
    match event {
        Event::ShardConnected(c) => handle_shard_connected(&ctx, c).await,
        Event::Ready(r) => handle_ready(&ctx, *r).await,
        Event::GuildCreate(g) => handle_guild_create(&ctx, g.0).await,
        Event::InteractionCreate(i) => handle_interaction_create(&ctx, i.0).await,
        Event::MessageCreate(msg) => handle_message_create(&ctx, msg.0).await,
        Event::VoiceStateUpdate(v) => handle_voice_state(&ctx, v.0).await,

        // Other events here...
        event => {
            println!("Event: {:?}", event.kind());
            debug!("Event: {:?}", event.kind());

            Ok(())
        },
    }
}

async fn handle_shard_connected(_ctx: &Context, connected: Connected) -> AnyResult<()> {
    info!(
        "Connected on shard {} with a heartbeat of {}",
        connected.shard_id, connected.heartbeat_interval
    );

    Ok(())
}

async fn handle_ready(_ctx: &Context, ready: Ready) -> AnyResult<()> {
    println!("Ready: '{}'", ready.user.name);
    info!("Ready: '{}'", ready.user.name);

    Ok(())
}

async fn handle_guild_create(_ctx: &Context, guild: Guild) -> AnyResult<()> {
    println!("Guild: {:#?}", guild);
    info!("Guild: '{}'", guild.name);

    Ok(())
}

async fn handle_interaction_create(_ctx: &Context, _inter: Interaction) -> AnyResult<()> {
    // use twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
    // match inter {
    //     Interaction::Ping(p) => {
    //         println!("{:#?}", p);
    //     },
    //     Interaction::ApplicationCommand(c) => {
    //         println!("{:#?}", c);
    //     },
    //     Interaction::ApplicationCommandAutocomplete(a) => {
    //         println!("{:#?}", a);
    //     },
    //     Interaction::MessageComponent(m) => {
    //         println!("{:#?}", m);
    //         let inter = ctx.http.interaction(ctx.application.id);
    //         let resp = InteractionResponse {
    //             kind: InteractionResponseType::UpdateMessage,
    //             data: Some(
    //                 twilight_util::builder::InteractionResponseDataBuilder::new()
    //                     .content("newcontent".to_string())
    //                     .build(),
    //             ),
    //         };
    //         inter.create_response(m.id, &m.token, &resp).exec().await?;

    //         println!("{:#?}", "DONE");
    //     },
    //     Interaction::ModalSubmit(s) => {
    //         println!("{:#?}", s);
    //     },
    //     i => todo!("not yet implemented: {:?}", i),
    // };

    Ok(())
}

async fn handle_message_create(ctx: &Context, msg: Message) -> AnyResult<()> {
    // Ignore bot users.
    anyhow::ensure!(!msg.author.bot, "Message sender is a bot");

    // Handle chat commands.
    if let Err(e) = ctx.chat_commands.process(ctx, &msg).await {
        match e {
            // Message was not meant for us.
            CommandError::NotPrefixed => (),

            // Log processing errors.
            e => {
                eprintln!("Error processing command: {}", e);
                error!("Error processing command: {}", e);

                if let Ok(id) = env::var("DISCORD_BOTDEV_CHANNEL") {
                    // On a bot dev channel, reply with error message.
                    let bot_dev = Id::new(id.parse()?);

                    if msg.channel_id == bot_dev {
                        ctx.http
                            .create_message(bot_dev)
                            .reply(msg.id)
                            .content(&e.to_string())?
                            .send()
                            .await?;
                    }
                }
            },
        }
    }

    Ok(())
}

async fn handle_voice_state(_ctx: &Context, _voice: VoiceState) -> AnyResult<()> {
    Ok(())
}

fn intents() -> Intents {
    #[cfg(feature = "all-intents")]
    return Intents::all();
    Intents::MESSAGE_CONTENT
        | Intents::GUILD_MESSAGES
        | Intents::GUILD_MESSAGE_REACTIONS
        | Intents::GUILD_MEMBERS
        | Intents::GUILD_VOICE_STATES
        | Intents::DIRECT_MESSAGES
        | Intents::DIRECT_MESSAGE_REACTIONS
}
