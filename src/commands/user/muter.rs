use twilight_mention::parse::{MentionType, ParseMention};
use twilight_model::datetime::Timestamp;

use crate::commands::{CommandContext, CommandError, CommandResult};
use crate::utils::*;

/// Command: Silence voice users.
pub async fn muter(cc: CommandContext<'_>) -> CommandResult {
    Ok(())
}

/// Command: Disable communications(?) and mute a person for a minute.
pub async fn timeout(cc: CommandContext<'_>) -> CommandResult {
    let Some(guild_id) = cc.msg.guild_id else {
        return Err(CommandError::Disabled)
    };

    let timeout = 30;

    let now = chrono::Utc::now().timestamp();
    let until = Timestamp::from_secs(now + timeout).unwrap();

    println!("now: {:?}, until: {:?}", now, until.as_secs());

    let mention = MentionType::parse(cc.args.trim()).unwrap();
    let target_user_id = match mention {
        MentionType::User(user_id) => Ok(user_id),
        MentionType::Channel(_) => Err(CommandError::UnexpectedArgs(
            "Expected user tag, got channel".to_string(),
        )),
        MentionType::Emoji(_) => Err(CommandError::UnexpectedArgs(
            "Expected user tag, got emoji".to_string(),
        )),
        MentionType::Role(_) => Err(CommandError::UnexpectedArgs(
            "Expected user tag, got role".to_string(),
        )),
        MentionType::Timestamp(_) => Err(CommandError::UnexpectedArgs(
            "Expected user tag, got timestamp".to_string(),
        )),
        e => Err(CommandError::UnexpectedArgs(format!(
            "Expected user tag, got '{e}'"
        ))),
    }?;

    cc.http
        .update_guild_member(guild_id, target_user_id)
        .mute(true)
        // .communication_disabled_until(Some(until)) // This gives permissions error?
        // .unwrap()
        .exec()
        .await?;

    // println!(
    //     "dis: {:?}",
    //     cc.http
    //         .guild_member(guild_id, target_user_id)
    //         .send()
    //         .await?
    //         .communication_disabled_until
    // );

    tokio::time::sleep(std::time::Duration::from_secs(timeout as _)).await;

    cc.http
        .update_guild_member(guild_id, target_user_id)
        .mute(false)
        // .communication_disabled_until(None) // This gives permissions error?
        // .unwrap()
        .exec()
        .await?;

    Ok(())
}