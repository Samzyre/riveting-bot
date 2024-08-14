use riveting_bot::commands::prelude::*;
use riveting_bot::utils::prelude::*;
use twilight_model::id::marker::{ChannelMarker, MessageMarker};
use twilight_model::id::Id;

const MAX_DELETE: i64 = 100;

/// Command: Delete a bunch of messages at once.
pub struct BulkDelete {}

impl BulkDelete {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("bulk-delete", "Delete many of messages.")
            .attach(Self::classic)
            .attach(Self::slash)
            .permissions(Permissions::ADMINISTRATOR)
            .option(
                integer("amount", "Number of messages to delete.")
                    .required()
                    .min(0)
                    .max(100),
            )
    }

    async fn uber(
        ctx: &Context,
        args: &Args,
        timestamp: i64,
        channel_id: Option<Id<ChannelMarker>>,
        message_id: Option<Id<MessageMarker>>,
    ) -> CommandResult<()> {
        const TWO_WEEKS_SECS: i64 = 60 * 60 * 24 * 7 * 2;
        let two_weeks_ago = timestamp - TWO_WEEKS_SECS;
        let count = args.integer("amount")?;

        let Ok(delete_count) = count.min(MAX_DELETE).try_into() else {
            return Err(CommandError::UnexpectedArgs(format!(
                "Could not parse delete count: '{count}'"
            )));
        };

        if delete_count == 0 {
            return Ok(());
        }

        let Some(channel_id) = channel_id else {
            return Err(CommandError::MissingArgs);
        };

        let message_id = match message_id {
            Some(id) => id,
            None => {
                match ctx
                    .http
                    .channel_messages(channel_id)
                    .limit(1)?
                    .send()
                    .await?
                    .pop()
                {
                    Some(m) => m.id,
                    None => return Err(CommandError::MissingArgs),
                }
            },
        };

        // Fetch and filter messages that are not older than two weeks.
        let msgs: Vec<_> = ctx
            .http
            .channel_messages(channel_id)
            .before(message_id)
            .limit(delete_count)?
            .send()
            .await?
            .into_iter()
            .filter(|m| two_weeks_ago < m.timestamp.as_secs())
            .map(|m| m.id)
            .collect();

        debug!("Deleting {} messages", msgs.len());

        // Delete the messages.
        if msgs.len() > 1 {
            // Bulk delete must have 2 to 100 messages.
            let _ = ctx
                .http
                .delete_messages(channel_id, &msgs)
                .context("Failed to delete multiple messages")?
                .await?;
        } else if let Some(msg) = msgs.first() {
            ctx.http.delete_message(channel_id, *msg).await?;
        }

        Ok(())
    }

    async fn classic(ctx: Context, req: ClassicRequest) -> CommandResponse {
        Self::uber(
            &ctx,
            &req.args,
            req.message.timestamp.as_secs(),
            Some(req.message.channel_id),
            Some(req.message.id),
        )
        .await?;

        Ok(Response::clear(ctx, req))
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        Self::uber(
            &ctx,
            &req.args,
            chrono::Utc::now().timestamp(),
            req.interaction.channel.as_ref().map(|c| c.id),
            None,
        )
        .await?;

        Ok(Response::clear(ctx, req))
    }
}
