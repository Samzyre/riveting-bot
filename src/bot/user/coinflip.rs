use rand::random;
use riveting_bot::commands::prelude::*;

/// Command: Coinflip.
pub struct Coinflip;

impl Coinflip {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("coinflip", "Flip a coin.").attach(Self::slash).dm()
    }

    async fn slash(ctx: Context, req: SlashRequest) -> CommandResponse {
        let flip = random::<bool>();
        let flip = if flip { ":coin: Heads" } else { "Tails :coin:" };

        ctx.interaction()
            .create_followup(&req.interaction.token)
            .content(flip)?
            .await?;

        Ok(Response::none())
    }
}
