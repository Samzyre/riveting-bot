use std::fmt::Write;

use riveting_bot::commands::prelude::*;
use riveting_bot::utils::prelude::*;
use twilight_mention::Mention;
use twilight_util::builder::embed::{self, EmbedFieldBuilder, ImageSource};

// Useful: https://discord.com/developers/docs/reference#image-formatting-cdn-endpoints

/// Command: Get information about user.
pub struct UserInfo;

impl UserInfo {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("userinfo", "Get information about a user.")
            .attach(Self::slash)
            .option(user("user", "User to show information about."))
            .dm()
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        let Some(guild_id) = req.interaction.guild_id else {
            return Err(CommandError::Disabled);
        };

        // If no args provided, check own props
        let user_id = match req.args.user("user") {
            Ok(user) => user.id(),
            _ => req
                .interaction
                .author_id()
                .ok_or(CommandError::MissingArgs)?,
        };

        let member = ctx.http.guild_member(guild_id, user_id).send().await?;
        // Somewhat of a redundant call, but some data may be missing on `member.user`.
        let user = ctx.http.user(user_id).send().await?;

        // If no avatar for the user, get the default one
        let image_url = match member.avatar.or(user.avatar) {
            Some(avatar) => {
                format!("https://cdn.discordapp.com/avatars/{user_id}/{avatar}.png?size=4096")
            },
            _ => {
                let discriminator = user.discriminator % 5;
                format!("https://cdn.discordapp.com/embed/avatars/{discriminator}.png")
            },
            // _ => "https://cdn.discordapp.com/embed/avatars/0.png".to_string(),
        };

        let mut embed = embed::EmbedBuilder::new();

        if let Some(banner) = user.banner {
            embed = embed.thumbnail(ImageSource::url(format!(
                "https://cdn.discordapp.com/banners/{user_id}/{banner}.png?size=4096"
            ))?);
        }

        if let Some(nick) = member.nick {
            embed = embed.field(EmbedFieldBuilder::new("AKA", nick).inline());
        }

        let roles: String = member.roles.into_iter().fold(String::new(), |mut s, i| {
            let _ = write!(s, "{} ", i.mention());
            s
        });
        let roles = roles.trim();
        let roles = if roles.is_empty() { "-" } else { roles };

        let embed = embed
            .title(user.name)
            .color(user.accent_color.unwrap_or(0))
            .image(ImageSource::url(image_url)?)
            .field(EmbedFieldBuilder::new("Roles", roles).inline())
            .build();

        ctx.interaction()
            .update_response(&req.interaction.token)
            .embeds(Some(&[embed]))?
            .send()
            .await?;

        Ok(Response::none())
    }
}
