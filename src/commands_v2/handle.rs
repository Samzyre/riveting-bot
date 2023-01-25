use std::sync::Arc;

use tokio::task::JoinSet;
use twilight_model::application::command::CommandType;
use twilight_model::application::interaction::application_command::{
    CommandData, CommandOptionValue,
};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::Message;
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};

use crate::commands_v2::arg::{Arg, ArgValue, Args, Ref};
use crate::commands_v2::builder::{
    ArgKind, BaseCommand, CommandFunction, CommandGroup, CommandOption,
};
use crate::commands_v2::function::{Callable, ClassicFunction, SlashFunction};
use crate::commands_v2::prelude::*;
use crate::utils::prelude::*;
use crate::{parser, Context};

/// Handle interaction and execute command functions.
pub async fn application_command(
    ctx: &Context,
    inter: Interaction,
    data: CommandData,
) -> Result<(), CommandError> {
    let interaction = ctx.interaction();

    let resp = InteractionResponse {
        kind: InteractionResponseType::DeferredChannelMessageWithSource,
        data: Some(InteractionResponseData::default()),
    };

    // Acknowledge the interaction.
    interaction
        .create_response(inter.id, &inter.token, &resp)
        .await?;

    // Lookup command from context.
    let Some(base) = ctx.commands.get(data.name.as_str()) else {
        return Err(CommandError::NotFound(format!("Command '{}' does not exist", data.name)))
    };

    let base = Arc::clone(base);
    let inter = Arc::new(inter);
    let data = Arc::new(data);

    let result = {
        let ctx = ctx.clone();
        let inter = Arc::clone(&inter);
        // Spawn a task so that we can handle panics later.
        tokio::spawn(async move {
            // Process the command by kind.
            match data.kind {
                CommandType::ChatInput => process_slash(&ctx, base, inter, data).await,
                CommandType::Message => process_message(&ctx, base, inter, data).await,
                CommandType::User => process_user(&ctx, base, inter, data).await,
                CommandType::Unknown(n) => panic!("Unknown command kind: {n}"),
                other => panic!("Unhandled command kind: {other:?}"),
            }
        })
        .await
    };

    let clear = || async {
        // Clear deferred message response.
        interaction
            .delete_response(&inter.token)
            .await
            .context("Failed to clear interaction")
    };

    // Handle execution result.
    // Catch erroneous execution and clear dangling response.
    match result
        .context("Execution task error")
        .map(|k| k.map_err(|e| e.into())) // If task is ok, but result is not.
    {
        Ok(Ok(Response::None | Response::Clear)) => {
            clear().await?;
        },
        Ok(Ok(Response::CreateMessage(text))) => {
            interaction
                .update_response(&inter.token)
                .content(Some(&text))
                .context("Response message error")?
                .await
                .context("Failed to send response message")?;
        },
        Ok(Err(e)) | Err(e) => {
            clear().await?;
            return Err(e.into());
        },
    }

    Ok(())
}

async fn process_slash(
    ctx: &Context,
    base: Arc<BaseCommand>,
    inter: Arc<Interaction>,
    data: Arc<CommandData>,
) -> CommandResult {
    let mut args = Vec::new();
    let mut last = Lookup::Command(&base.command);
    let mut data_opts = data.options.to_vec();
    let mut lookup_opts; // Declared here for lifetime reasons.

    // Process interaction until last (sub)command is found.
    // This processes options in reverse, it is fine however,
    // because `CommandDataOption` is a nested structure and
    // only holds one type of options in the `value` field.
    while let Some(opt) = data_opts.pop() {
        match opt.value {
            CommandOptionValue::SubCommand(next) | CommandOptionValue::SubCommandGroup(next) => {
                lookup_opts = match last {
                    Lookup::Command(c) => c.options.to_vec(),
                    Lookup::Group(g) => g.to_options(),
                };

                // lookup option from lookup_opts
                let found = lookup_opts
                    .iter()
                    .filter_map(Lookup::from_option)
                    .find(|s| s.name() == opt.name);

                match found {
                    Some(sub) => {
                        data_opts = next.to_vec(); // Set next option to check.
                        last = sub; // Set last command or group found.
                    },
                    None => {
                        // TODO: This should return error.
                        error!("Subcommand or group not found: {}", opt.name);
                        panic!("Subcommand or group not found: {}", opt.name);
                    },
                }
            },
            arg => {
                // Convert argument.
                match arg.to_owned().try_into() {
                    Ok(arg) => {
                        // Args are still stored in reverse order.
                        args.push(Arg {
                            name: opt.name,
                            value: arg,
                        });
                    },
                    Err(e) => {
                        // TODO: This should return error.
                        error!("Could not process argument: '{}': {e}", arg.kind().kind());
                        panic!("Could not process argument: '{}': {e}", arg.kind().kind());
                    },
                }
            },
        }
    }

    println!("ARGS: {args:?}");

    let funcs = last
        .slash_functions()
        .context("Failed to get slash functions")?;

    let req = SlashRequest::new(base, Arc::clone(&inter), data, Args::from(args));

    execute(ctx, funcs, req)
        .await
        .context("Execution result")
        .map_err(Into::into)
}

// TODO: To be implemented.
// TODO: See if any twilight resolved data can be used as objects instead of ids.
async fn process_message(
    _ctx: &Context,
    _base: Arc<BaseCommand>,
    _inter: Arc<Interaction>,
    data: Arc<CommandData>,
) -> CommandResult {
    // Message GUI commands.
    let data = data.resolved.as_ref().expect("Empty resolve error");
    println!("{data:#?}");

    // Globally.
    for _message in &data.messages {}

    // TODO: Should this create a modal to get missing args?
    todo!();
}

// TODO: To be implemented.
// TODO: See if any twilight resolved data can be used as objects instead of ids.
async fn process_user(
    _ctx: &Context,
    _base: Arc<BaseCommand>,
    _inter: Arc<Interaction>,
    data: Arc<CommandData>,
) -> CommandResult {
    // User GUI commands.
    let data = data.resolved.as_ref().expect("Empty resolve error");
    println!("{data:#?}");

    // Globally.
    for _user in &data.users {}

    // Guilds only.
    for _member in &data.members {}

    // TODO: Should this create a modal to get missing args?
    todo!();
}

/// Parse message and execute command functions.
pub async fn classic_command(ctx: &Context, msg: Arc<Message>) -> Result<(), CommandError> {
    // Unprefix the message contents.
    let prefix = ctx.classic_prefix(msg.guild_id);
    let Some((_, unprefixed)) =  parser::unprefix_with([prefix], &msg.content) else {
        return Err(CommandError::NotPrefixed);
    };

    // Get first possible command name.
    let (name, mut rest) = parser::split_once_whitespace(unprefixed);

    // Lookup command from context.
    let Some(base) = ctx.commands.get(name) else {
        return Err(CommandError::NotFound(format!("Command '{name}' does not exist")))
    };

    let base = Arc::new(base.to_owned());
    let mut lookup = Lookup::Command(&base.command);

    // Parse contents until last (sub)command is found.
    loop {
        let (name, next) = parser::split_once_whitespace(rest.unwrap_or(""));

        let found = match lookup {
            Lookup::Command(f) => f
                .options
                .iter()
                .filter_map(Lookup::from_option)
                .find(|t| t.name() == name),
            Lookup::Group(g) => g.subs.iter().find(|s| s.name == name).map(Lookup::Command),
        };

        if let Some(t) = found {
            lookup = t;
            rest = next;
            continue;
        }

        break;
    }

    let args = match lookup {
        Lookup::Command(c) => parse_classic_args(c, &msg, rest)?,
        Lookup::Group(g) => {
            return Err(CommandError::UnexpectedArgs(format!(
                "Expected command, found group '{}'",
                g.name
            )));
        },
    };

    let funcs = lookup
        .classic_functions()
        .context("Failed to get classic functions")?;

    trace!(
        "Creating classic request for '{name}' by user '{}'",
        msg.author.id
    );

    let req = ClassicRequest::new(Arc::clone(&base), Arc::clone(&msg), args);

    debug!("Executing '{name}' by user '{}'", msg.author.id);

    let response = execute(ctx, funcs, req).await.context("Execution result")?;

    trace!("Completing '{name}' by user '{}'", msg.author.id);

    // Handle execution result.
    match response {
        Response::None => (),
        Response::Clear => {
            ctx.http
                .delete_message(msg.channel_id, msg.id)
                .await
                .context("Failed to clear command message")?;
        },
        Response::CreateMessage(text) => {
            ctx.http
                .create_message(msg.channel_id)
                .reply(msg.id)
                .content(&format!("{text}\n"))
                .context("Response message error")?
                .await
                .context("Failed to send response message")?;
        },
    }

    Ok(())
}

fn parse_classic_args(
    c: &CommandFunction,
    msg: &Message,
    mut rest: Option<&str>,
) -> Result<Args, CommandError> {
    let mut parsed = Vec::new();
    let mut args = c.args();

    'parse: while let Some(arg) = args.next().filter(|a| a.required) {
        // Handle special arguments.
        'special: {
            let value = match arg.kind {
                ArgKind::Message => {
                    let Some(replied) = msg.referenced_message.as_ref() else {
                        break 'special; // Try to get the message through passed id.
                    };

                    ArgValue::Message(Ref::from_obj(replied.as_ref().to_owned()))
                },
                ArgKind::Attachment => {
                    let att = msg
                        .attachments
                        .first()
                        .ok_or(CommandError::MissingArgs)
                        .with_context(|| {
                            format!(
                                "Expected a required argument '{}' of type '{}' (upload)",
                                arg.name, arg.kind
                            )
                        })?;

                    ArgValue::Attachment(Ref::from_obj(att.to_owned()))
                },
                _ => break 'special, // If not a special arg.
            };

            parsed.push(Arg {
                name: arg.name.to_string(),
                value,
            });

            continue 'parse; // Go to next arg.
        }

        // Normal parsing.

        let unparsed = rest.ok_or(CommandError::MissingArgs).with_context(|| {
            format!(
                "Expected a required argument '{}' of type '{}'",
                arg.name, arg.kind
            )
        })?;

        let (value, next) =
            parser::maybe_quoted_arg(unparsed).context("Failed to parse next argument")?;
        rest = next;

        parsed.push(Arg::from_desc(arg, value)?);
    }

    Ok(Args::from(parsed))
}

enum Lookup<'a> {
    Command(&'a CommandFunction),
    Group(&'a CommandGroup),
}

impl<'a> Lookup<'a> {
    const fn from_option(opt: &'a CommandOption) -> Option<Self> {
        match opt {
            CommandOption::Sub(s) => Some(Self::Command(s)),
            CommandOption::Group(g) => Some(Self::Group(g)),
            _ => None,
        }
    }

    const fn name(&self) -> &str {
        match self {
            Lookup::Command(t) => t.name,
            Lookup::Group(t) => t.name,
        }
    }

    fn classic_functions(&self) -> AnyResult<Vec<Arc<dyn ClassicFunction>>> {
        match self {
            Lookup::Command(c) if c.has_classic() => Ok(c
                .functions
                .iter()
                .filter_map(|f| match f {
                    Function::Classic(f) => Some(Arc::clone(f)),
                    _ => None,
                })
                .collect()),
            Lookup::Command(c) => {
                anyhow::bail!("No classic commands found for command call: '{}'", c.name)
            },
            Lookup::Group(g) => {
                // TODO: This should be usage error or considered as an arg to previous command.
                anyhow::bail!("Expected a subcommand, found group: '{}'", g.name)
            },
        }
    }

    fn slash_functions(&self) -> AnyResult<Vec<Arc<dyn SlashFunction>>> {
        match self {
            Lookup::Command(c) if c.has_slash() => Ok(c
                .functions
                .iter()
                .filter_map(|f| match f {
                    Function::Slash(f) => Some(Arc::clone(f)),
                    _ => None,
                })
                .collect()),
            Lookup::Command(c) => {
                anyhow::bail!("No slash commands found for command call: '{}'", c.name)
            },
            Lookup::Group(g) => {
                anyhow::bail!("Expected a subcommand, found group: '{}'", g.name)
            },
        }
    }
}

/// Execute tasks.
async fn execute<F, R>(ctx: &Context, funcs: Vec<F>, req: R) -> CommandResult
where
    F: Callable<R>,
    R: Clone,
{
    let mut set = JoinSet::new();
    let mut results = Vec::with_capacity(funcs.len());

    for func in funcs {
        set.spawn(func.call(ctx.to_owned(), req.clone()));
    }

    // Wait for completion.
    while let Some(task) = set.join_next().await {
        let task = match task {
            Ok(k) => k,
            Err(e) => {
                eprintln!("Execution task join error: {e}");
                error!("Execution task join error: {e}");
                continue;
            },
        };

        results.push(task);
    }

    // This should not fail.
    let last = results.pop().expect("No results from command handlers");

    for r in results {
        // TODO: Collect all errors.
        // Prioritize returning errors immediately, for now.
        r?;
    }

    last
}