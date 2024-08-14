use std::sync::Arc;

use riveting_bot::commands::prelude::*;
use riveting_bot::utils::prelude::*;
use songbird::input::{Input, YoutubeDl};
use songbird::tracks::Track;
use songbird::typemap::TypeMapKey;
use songbird::Call;
use tokio::sync::Mutex;
use twilight_gateway::Event;
use twilight_mention::Mention;
use twilight_model::channel::ChannelType;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};
use twilight_model::id::Id;

/// Command: Voice channel controls.
pub struct Voice;

impl Voice {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("voice", "Manage voice connection.")
            .attach(Self::classic)
            .attach(Self::slash)
            .option(
                sub("join", "Join the bot to a voice channel.")
                    .attach(Join::classic)
                    .attach(Join::slash)
                    .option(
                        channel("channel", "Voice channel to join.")
                            .types([ChannelType::GuildVoice, ChannelType::GuildStageVoice]),
                    ),
            )
            .option(
                sub("leave", "Disconnect the bot from a voice channel.")
                    .attach(Leave::classic)
                    .attach(Leave::slash),
            )
            .option(
                sub("play", "Play a sound or music on voice (queued).")
                    .attach(Play::classic)
                    .attach(Play::slash)
                    .option(string("url", "Youtube URL to play.").required()),
            )
            .option(
                sub("skip", "Go to the next track in queue.")
                    .attach(Skip::classic)
                    .attach(Skip::slash),
            )
    }

    async fn classic(_ctx: Context, _req: ClassicRequest) -> CommandResponse {
        todo!();
    }

    async fn slash(_ctx: Context, _req: SlashRequest) -> CommandResponse {
        todo!();
    }
}

/// Command: Tell the bot to connect to a voice channel.
struct Join;

impl Join {
    async fn uber(
        ctx: &Context,
        args: &Args,
        guild_id: Id<GuildMarker>,
        req_channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) -> AnyResult<Arc<Mutex<Call>>> {
        // If no arg was given, try to find user in voice channels, otherwise use channel id from the request itself.
        let channel_id = match args.channel("channel") {
            Ok(c) => c.id(),
            Err(e) => {
                debug!("{e}; Using fallback");
                match ctx.user_voice_channel(guild_id, user_id).await {
                    Ok(channel_id) => {
                        debug!("User '{user_id}' was found in voice channel '{channel_id}'");
                        channel_id
                    },
                    Err(v) => match ctx.channel_from(req_channel_id).await?.kind {
                        ChannelType::GuildVoice | ChannelType::GuildStageVoice => {
                            debug!("{e}; Using request channel");
                            req_channel_id
                        },
                        _ => return Err(e).context(v).map_err(Into::into),
                    },
                }
            },
        };

        if match ctx.voice.get(guild_id) {
            Some(call) => call.lock().await.current_channel().is_none(),
            None => true,
        } {
            let ctx = ctx.to_owned();
            tokio::spawn(async move {
                ctx.standby
                    .wait_for(guild_id, move |event: &Event| {
                        match event {
                            Event::GatewayClose(_) => true,
                            Event::VoiceStateUpdate(vsu) => {
                                // If the update is a disconnect and for the user who called join.
                                vsu.channel_id.is_none() && vsu.user_id == user_id
                            },
                            _ => false,
                        }
                    })
                    .await?;

                debug!("Autodisconnecting from voice");
                ctx.voice
                    .remove(guild_id)
                    .await
                    .with_context(|| format!("Failed to leave channel '{channel_id}'"))
                    .map(|_| info!("Disconnected from voice channel '{channel_id}'"))
            });
        }

        let call = ctx
            .voice
            .join(guild_id, channel_id)
            .await
            .with_context(|| format!("Failed to join channel '{channel_id}'"));

        match call {
            Ok(c) => {
                info!("Connected to voice channel '{channel_id}'");
                let mut call = c.lock().await;
                call.deafen(true).await.context("Failed to deafen")?;
                drop(call);
                Ok(c)
            },
            Err(e) => Err(e),
        }
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        Self::uber(
            &ctx,
            &req.args,
            req.message.guild_id.ok_or_else(|| CommandError::Disabled)?,
            req.message.channel_id,
            req.message.author.id,
        )
        .await
        .map(|_| Response::none())
        .map_err(Into::into)
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        match Self::uber(
            &ctx,
            &req.args,
            req.interaction
                .guild_id
                .ok_or_else(|| CommandError::Disabled)?,
            req.interaction
                .channel
                .as_ref()
                .map(|c| c.id)
                .context("No channel found")?,
            req.interaction.author_id().context("No user id found")?,
        )
        .await
        {
            Ok(c) => {
                if let Some(channel_id) = c.lock().await.current_channel() {
                    ctx.interaction()
                        .create_followup(&req.interaction.token)
                        .content(&format!(
                            "Joined channel {}",
                            ctx.channel_from(channel_id.0.into()).await?.mention()
                        ))?
                        .send()
                        .await?;
                }
                Ok(Response::none())
            },
            Err(e) => Err(e.into()),
        }
    }
}

/// Command: Tell the bot to disconnect from a voice channel.
struct Leave;

impl Leave {
    async fn uber(ctx: &Context, guild_id: Option<Id<GuildMarker>>) -> AnyResult<()> {
        let guild_id = guild_id.ok_or_else(|| CommandError::Disabled)?;

        let channel_id = match ctx.voice.get(guild_id) {
            Some(call) => match call.lock().await.current_channel() {
                Some(channel_id) => channel_id,
                None => return Ok(()),
            },
            None => return Ok(()),
        };

        ctx.voice
            .remove(guild_id)
            .await
            .with_context(|| format!("Failed to leave channel '{channel_id}'"))
            .map(|_| info!("Disconnected from voice channel '{channel_id}'"))
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        Self::uber(&ctx, req.message.guild_id)
            .await
            .map(|_| Response::none())
            .map_err(Into::into)
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        Self::uber(&ctx, req.interaction.guild_id)
            .await
            .map(|_| Response::clear(ctx, req))
            .map_err(Into::into)
    }
}

/// Command: Play a sound or music in voice.
struct Play;

impl Play {
    async fn uber(
        ctx: &Context,
        args: &Args,
        guild_id: Id<GuildMarker>,
        req_channel_id: Id<ChannelMarker>,
        user_id: Id<UserMarker>,
    ) -> AnyResult<Option<String>> {
        // If not connected, try to join.
        let call = match ctx.voice.get(guild_id) {
            Some(call) => call,
            None => {
                info!("Bot is not connected to voice in guild '{guild_id}'; Trying to join");
                match Join::uber(ctx, args, guild_id, req_channel_id, user_id).await {
                    Ok(c) => c,
                    Err(e) => {
                        info!("{e}");
                        return Ok(None);
                    },
                }
            },
        };

        let url = args.string("url")?;
        let client = reqwest::Client::new();
        let mut input = Input::from(YoutubeDl::new(client, url.into_string()));
        let meta = input.aux_metadata().await;
        let track = Track::new(input).volume(0.5);

        let (is_empty, handle) = {
            let mut call = call.lock().await;
            let empty = call.queue().is_empty();
            (empty, call.enqueue(track).await)
        };

        if let Err(e) = handle.make_playable_async().await {
            info!("Cannot play audio track: {e}");
            return Ok(Some("I can't play that 🔇".to_string()));
        }

        let content = match meta {
            Ok(m) => {
                trace!("Metadata: {m:?}");

                let track = m
                    .title
                    .or(m.track)
                    .unwrap_or_else(|| "<UNKNOWN>".to_string());
                let artist = m.artist.unwrap_or_else(|| "<UNKNOWN>".to_string());
                let content = track_message(is_empty, &track, &artist);
                handle
                    .typemap()
                    .write()
                    .await
                    .insert::<Meta>(Meta { track, artist });
                content
            },
            Err(e) => {
                info!("Metadata error: {e}");
                info!("Something without metadata is playing");
                "What is that even? 🔉".to_string()
            },
        };

        Ok(Some(content))
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        match Self::uber(
            &ctx,
            &req.args,
            req.message.guild_id.ok_or(CommandError::Disabled)?,
            req.message.channel_id,
            req.message.author.id,
        )
        .await
        {
            Ok(Some(content)) => {
                ctx.http
                    .create_message(req.message.channel_id)
                    .content(&content)?
                    .await?;
                Ok(Response::none())
            },
            Ok(None) => Ok(Response::none()),
            Err(e) => Err(e.into()),
        }
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        match Self::uber(
            &ctx,
            &req.args,
            req.interaction.guild_id.ok_or(CommandError::Disabled)?,
            req.interaction
                .channel
                .as_ref()
                .map(|c| c.id)
                .context("No channel found")?,
            req.interaction.author_id().context("No user id found")?,
        )
        .await
        {
            Ok(Some(content)) => {
                ctx.interaction()
                    .create_followup(&req.interaction.token)
                    .content(&content)?
                    .await?;
                Ok(Response::none())
            },
            Ok(None) => Ok(Response::clear(ctx, req)),
            Err(e) => Err(e.into()),
        }
    }
}

/// Command: Go to the next track in queue.
struct Skip;

impl Skip {
    async fn uber(ctx: &Context, guild_id: Id<GuildMarker>) -> AnyResult<Option<String>> {
        match ctx.voice.get(guild_id) {
            Some(c) => {
                let call = c.lock().await;
                let queue = call.queue().current_queue();
                let result = Ok(Some(match queue.get(1) {
                    Some(t) => t.typemap().read().await.get::<Meta>().map_or_else(
                        || "What is that even? 🔉".to_string(),
                        |m| track_message(true, &m.track, &m.artist),
                    ),
                    None => "Queue is empty 🔇".to_string(),
                }));
                call.queue().skip().context("Failed to skip audio track")?;
                result
            },
            None => {
                info!("No voice connection found for '{guild_id}'");
                Ok(None)
            },
        }
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        match Self::uber(&ctx, req.message.guild_id.ok_or(CommandError::Disabled)?).await {
            Ok(Some(content)) => {
                ctx.http
                    .create_message(req.message.channel_id)
                    .content(&content)?
                    .await?;
                Ok(Response::none())
            },
            Ok(None) => Ok(Response::none()),
            Err(e) => Err(e.into()),
        }
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        match Self::uber(
            &ctx,
            req.interaction.guild_id.ok_or(CommandError::Disabled)?,
        )
        .await
        {
            Ok(Some(content)) => {
                ctx.interaction()
                    .create_followup(&req.interaction.token)
                    .content(&content)?
                    .await?;
                Ok(Response::none())
            },
            Ok(None) => Ok(Response::clear(ctx, req)),
            Err(e) => Err(e.into()),
        }
    }
}

fn track_message(playing: bool, track: &str, artist: &str) -> String {
    format!(
        "{} **{track}** by **{artist}**",
        if playing {
            "🔊 Playing"
        } else {
            "⏳ Queued"
        }
    )
}

struct Meta {
    track: String,
    artist: String,
}

impl TypeMapKey for Meta {
    type Value = Self;
}
