use riveting_bot::commands::prelude::*;
use riveting_bot::utils::prelude::*;
use twilight_gateway::Event;
use twilight_model::id::marker::{GuildMarker, UserMarker};
use twilight_model::id::Id;

const DEFAULT_MUTE: u64 = 60;

/// Command: Silence a voice user for a set amount of time.
pub struct Mute;

impl Mute {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("mute", "Silence someone in voice channel.")
            .attach(Self::classic)
            .attach(Self::slash)
            .attach(Self::user)
            .permissions(Permissions::ADMINISTRATOR)
            .option(user("user", "Who to mute.").required())
            .option(integer("seconds", "Duration of the mute.").min(0))
    }

    async fn uber(
        ctx: Context,
        guild_id: Option<Id<GuildMarker>>,
        user_id: Id<UserMarker>,
        duration: Option<u64>,
    ) -> CommandResult<()> {
        let Some(guild_id) = guild_id else {
            return Err(CommandError::Disabled);
        };

        let timeout = duration.unwrap_or(DEFAULT_MUTE);

        // This fails if the target user is not connected to a voice channel.
        if ctx
            .http
            .update_guild_member(guild_id, user_id)
            .mute(true)
            .await
            .is_err()
        {
            return Ok(()); // Nothing more to do here.
        }

        tokio::time::sleep(std::time::Duration::from_secs(timeout)).await;

        let unmute = || ctx.http.update_guild_member(guild_id, user_id).mute(false);
        // Stop trying after some attempts.
        for _ in 0..3 {
            // Try to unmute the target.
            if unmute().await.is_ok() {
                break;
            }

            // Otherwise, try again later when they trigger a voice channel event.
            ctx.standby
                .wait_for(guild_id, move |event: &Event| match event {
                    Event::VoiceStateUpdate(data) => {
                        data.member.as_ref().map_or(false, |m| m.user.id == user_id)
                    },
                    _ => false,
                })
                .await?;
        }

        Ok(())
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        req.clear(&ctx).await?; // Clear original beforehand.

        Self::uber(
            ctx,
            req.message.guild_id,
            req.args.user("user").map(|r| r.id())?,
            req.args.integer("seconds").map(|i| i as u64).ok(),
        )
        .await
        .map(|_| Response::none())
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        req.clear(&ctx).await?; // Clear original beforehand.

        Self::uber(
            ctx,
            req.interaction.guild_id,
            req.args.user("user").map(|r| r.id())?,
            req.args.integer("seconds").map(|i| i as u64).ok(),
        )
        .await
        .map(|_| Response::none())
    }

    async fn user(ctx: Context, req: UserRequest) -> CommandResponse {
        req.clear(&ctx).await?; // Clear original beforehand.

        Self::uber(
            ctx,
            req.interaction.guild_id,
            req.target_id,
            None, // TODO: Create modal for duration input.
        )
        .await
        .map(|_| Response::none())
    }
}
