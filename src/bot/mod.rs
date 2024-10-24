/*!
Command template:
```
use riveting_bot::commands::prelude::*;

pub struct Command;

impl Command {
    pub fn command() -> impl Into<BaseCommand> {
        use riveting_bot::commands::builder::*;

        command("cmd", "Thing.")
            .attach(Self::classic)
            .attach(Self::slash)
            .attach(Self::message)
            .attach(Self::user)
    }

    async fn classic(_ctx: Context, _req: ClassicRequest) -> CommandResponse {
        todo!();
    }

    async fn slash(_ctx: Context, _req: SlashRequest) -> CommandResponse {
        todo!();
    }

    async fn message(_ctx: Context, _req: MessageRequest) -> CommandResponse {
        todo!();
    }

    async fn user(_ctx: Context, _req: UserRequest) -> CommandResponse {
        todo!();
    }
}
```
*/

use std::sync::Arc;

use riveting_bot::commands::{Commands, CommandsBuilder};
use riveting_bot::config::BotConfig;
use riveting_bot::utils::prelude::*;
use riveting_bot::BotEventSender;
use twilight_standby::Standby;

/// Generic commands.
pub mod meta;

/// Normal user commands.
#[cfg(feature = "user")]
pub mod user;

/// Administrator comands.
#[cfg(feature = "admin")]
pub mod admin;

/// Bot owner only commands.
#[cfg(feature = "owner")]
pub mod owner;

/// Create the list of bot commands.
pub fn create_commands() -> AnyResult<Commands> {
    let mut commands = CommandsBuilder::new();

    // Basic functionality.
    commands
        .bind(meta::essential::Ping::command())
        .bind(meta::essential::About::command())
        .bind(meta::essential::Help::command());

    #[cfg(feature = "voice")]
    commands.bind(meta::voice::Voice::command());

    // Extra utility.
    #[cfg(feature = "bulk-delete")]
    commands.bind(meta::bulk::BulkDelete::command());

    #[cfg(feature = "user")]
    commands
        .bind(user::fuel::Fuel::command())
        .bind(user::time::Time::command())
        .bind(user::joke::Joke::command())
        .bind(user::coinflip::Coinflip::command())
        .bind(user::user_info::UserInfo::command());

    // Moderation functionality.
    #[cfg(feature = "admin")]
    commands
        .bind(admin::bot::Bot::command())
        .bind(admin::roles::Roles::command())
        .bind(admin::silence::Mute::command());

    // Bot owner functionality.
    #[cfg(feature = "owner")]
    commands.bind(owner::Shutdown::command());

    add_commands_to_help(&mut commands);

    commands
        .validate()
        .context("Failed to validate commands list")?;

    Ok(commands.build())
}

// HACK: This really is an afterthought.
fn add_commands_to_help(cmds: &mut CommandsBuilder) {
    use riveting_bot::commands::builder::{ArgDesc, ArgKind, CommandOption, StringData};

    let names = cmds
        .list
        .iter()
        .map(|c| (c.command.name.to_string(), c.command.name.to_string()))
        .collect::<Vec<_>>();
    let choices = cmds
        .list
        .iter_mut()
        .find(|c| c.command.name == "help")
        .and_then(|c| {
            c.command.options.iter_mut().find_map(|a| match a {
                CommandOption::Arg(ArgDesc {
                    name: "command",
                    kind: ArgKind::String(StringData { choices, .. }),
                    ..
                }) => Some(choices),
                _ => None,
            })
        })
        .expect("No help command found");
    *choices = names;
}

pub struct _State {
    /// Bot configuration.
    config: Arc<BotConfig>,
    /// Bot commands list.
    commands: Arc<Commands>,
    /// Bot events channel.
    events_tx: BotEventSender,
    /// Standby twilight event system.
    standby: Arc<Standby>,
    /// Songbird voice manager.
    #[cfg(feature = "voice")]
    voice: Arc<songbird::Songbird>,
}
