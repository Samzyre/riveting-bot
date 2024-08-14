use indoc::formatdoc;
use riveting_bot::commands::prelude::*;
use riveting_bot::utils::prelude::*;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker};
use twilight_model::id::Id;

/// Command: Ping Pong!
pub struct Ping;

impl Ping {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("ping", "Ping the bot.")
            .attach(Self::classic)
            .attach(Self::slash)
            .dm()
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        ctx.http
            .create_message(req.message.channel_id)
            .reply(req.message.id)
            .content("Pong!")?
            .await?;

        Ok(Response::none())
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        ctx.interaction()
            .create_followup(&req.interaction.token)
            .content("Pong!")?
            .await?;

        Ok(Response::none())
    }
}

/// Command: Info about the bot.
pub struct About {
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Option<Id<ChannelMarker>>,
    message_id: Option<Id<MessageMarker>>,
}

impl About {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("about", "Display info about the bot.")
            .attach(Self::classic)
            .attach(Self::slash)
            .dm()
    }

    fn uber(self, ctx: &Context) -> String {
        formatdoc!(
            "I am a RivetingBot!
            You can list my commands with `/help` or `{prefix}help` command.
            My current version *(allegedly)* is `{version}`.
            My source is available at <{link}>
            ",
            prefix = ctx.config.classic_prefix(self.guild_id).unwrap_or_default(),
            version = env!("CARGO_PKG_VERSION"),
            link = env!("CARGO_PKG_REPOSITORY"),
        )
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        let about_msg = Self {
            guild_id: req.message.guild_id,
            channel_id: Some(req.message.channel_id),
            message_id: Some(req.message.id),
        }
        .uber(&ctx);

        ctx.http
            .create_message(req.message.channel_id)
            .reply(req.message.id)
            .content(&about_msg)?
            .await?;

        Ok(Response::none())
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        let about_msg = Self {
            guild_id: req.interaction.guild_id,
            channel_id: req.interaction.channel.as_ref().map(|c| c.id),
            message_id: None,
        }
        .uber(&ctx);

        ctx.interaction()
            .create_followup(&req.interaction.token)
            .content(&about_msg)?
            .await?;

        Ok(Response::none())
    }
}

/// Command: Help for using the bot, commands and usage.
pub struct Help {
    args: Args,
    guild_id: Option<Id<GuildMarker>>,
    channel_id: Option<Id<ChannelMarker>>,
    message_id: Option<Id<MessageMarker>>,
}

impl Help {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("help", "List bot commands.")
            .attach(Self::classic)
            .attach(Self::slash)
            .option(string("command", "Get help on a command.")) // Choices added here after other binds.
            .dm()
    }

    fn uber(self, ctx: &Context) -> AnyResult<String> {
        Ok(if let Ok(value) = self.args.string("command") {
            ctx.commands.get(&value).map_or_else(
                || format!("Command `{value}` not found :|"),
                |cmd| cmd.generate_help(),
            )
        } else {
            formatdoc! {"
                ```yaml
                Prefix: '/' or '{prefix}'
                Commands:
                {commands}
                ```",
                prefix = ctx.config.classic_prefix(self.guild_id).unwrap_or_default(),
                commands = ctx.commands.display(ctx, self.guild_id)?
            }
        })
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        let help_msg = Self {
            args: req.args,
            guild_id: req.message.guild_id,
            channel_id: Some(req.message.channel_id),
            message_id: Some(req.message.id),
        }
        .uber(&ctx)?;

        ctx.http
            .create_message(req.message.channel_id)
            .reply(req.message.id)
            .content(&help_msg)?
            .await?;

        Ok(Response::none())
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        let help_msg = Self {
            args: req.args,
            guild_id: req.interaction.guild_id,
            channel_id: req.interaction.channel.as_ref().map(|c| c.id),
            message_id: None,
        }
        .uber(&ctx)?;

        ctx.interaction()
            .create_followup(&req.interaction.token)
            .content(&help_msg)?
            .await?;

        Ok(Response::none())
    }
}
