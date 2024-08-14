use thiserror::Error;
use twilight_model::application::command::{Command, CommandOption, CommandType};
use twilight_util::builder::command::*;

use crate::commands::builder::BaseCommand;
use crate::utils::prelude::*;

pub type TwilightCommand = Command;

/// Helper trait for twilight builders where the value may be optional.
/// This trait lets you apply the optional value if it is present,
/// otherwise preserve the builder default.
trait Optional: Sized {
    /// Apply a function only if `value` is `Some`.
    fn optional<F, A>(mut self, value: Option<A>, func: F) -> Self
    where
        F: Fn(Self, A) -> Self,
    {
        if let Some(value) = value {
            self = func(self, value);
        }
        self
    }
}

impl<T> Optional for T {}

pub trait CommandBuilderExt<Output = Command>: Sized {
    fn build_checked(self) -> AnyResult<Output>;
}

impl CommandBuilderExt for CommandBuilder {
    fn build_checked(self) -> AnyResult<Command> {
        let cmd = self.build();
        validate_command(&cmd)?;
        Ok(cmd)
    }
}

/// Creates shortcut functions for useful builders.
macro short_fn($v:vis fn $f:ident -> $t:ty) {
    /// Creates an argument builder.
    $v fn $f(name: impl ToString, description: impl ToString) -> $t {
        <$t>::new(name.to_string(), description.to_string())
    }
}

short_fn!(pub fn attachment -> AttachmentBuilder);
short_fn!(pub fn boolean -> BooleanBuilder);
short_fn!(pub fn mention -> MentionableBuilder);
short_fn!(pub fn role -> RoleBuilder);
short_fn!(pub fn user -> UserBuilder);
short_fn!(pub fn channel -> ChannelBuilder);
short_fn!(pub fn integer -> IntegerBuilder);
short_fn!(pub fn number -> NumberBuilder);
short_fn!(pub fn string -> StringBuilder);

/// Validates options in the command.
pub fn validate_command(cmd: &Command) -> Result<(), CommandValidationError> {
    use twilight_validate::command as validate;

    /// Checks for local multiples of same option names.
    fn validate_options(options: &[CommandOption]) -> Result<(), CommandValidationError> {
        options.iter().enumerate().try_for_each(|(idx, opt)| {
            // Check for local ambiguity:
            // All the rest of the options must not have this name.
            if let Some(slice) = options.get(idx + 1..) {
                if slice.iter().any(|c| c.name == opt.name) {
                    return Err(CommandValidationError::AmbiguousName(format!(
                        "Duplicate name '{}' in option of kind '{}'",
                        opt.name,
                        opt.kind.kind()
                    )));
                }
            }

            // Recursively check suboptions.
            if let Some(options) = &opt.options {
                validate_options(options)?;
            }

            Ok(())
        })
    }

    if matches!(cmd.kind, CommandType::User | CommandType::Message) {
        // Check with twilight's validations.
        validate::name(&cmd.name).context("Twilight GUI-command name error")?;

        // Other manual checks.
        if !cmd.options.is_empty() {
            return Err(CommandValidationError::GuiOptions);
        }
    } else {
        // Check with twilight's validations.
        // Does not check for options.
        validate::command(cmd).context("Twilight validation error")?;

        // This checks for order, limit, name and description validity (recursively).
        // Does not check for ambiguity.
        validate::options(&cmd.options).context("Twilight options error")?;

        // Other manual checks.
        validate_options(&cmd.options).context("Custom validation error")?;
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum CommandValidationError {
    /// Multiple uses of same option name.
    #[error("Option names must be locally unique: {0}")]
    AmbiguousName(String),

    /// Options in GUI-based commands.
    #[error("GUI-based commands cannot have options")]
    GuiOptions,

    /// Twilight's validation error.
    #[error(transparent)]
    Twilight(#[from] twilight_validate::command::CommandValidationError),

    /// Other errors that are or can be converted to `anyhow::Error`.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct SlashCommand(Command);

impl TryFrom<BaseCommand> for SlashCommand {
    type Error = CommandValidationError;

    fn try_from(value: BaseCommand) -> Result<Self, Self::Error> {
        let mut cmd = CommandBuilder::new(
            value.command.name,
            value.command.description,
            CommandType::ChatInput,
        )
        .dm_permission(value.dm_enabled);

        for opt in value.command.options {
            if let Ok(opt) = CommandOption::try_from(opt) {
                cmd = cmd.option(opt);
            }
        }

        let mut cmd = cmd.build();

        cmd.default_member_permissions = value.member_permissions;

        validate_command(&cmd).context("Failed to validate slash command")?;

        Ok(Self(cmd))
    }
}

impl From<SlashCommand> for Command {
    fn from(value: SlashCommand) -> Self {
        value.0
    }
}

pub struct MessageCommand(Command);

impl TryFrom<BaseCommand> for MessageCommand {
    type Error = CommandValidationError;

    fn try_from(value: BaseCommand) -> Result<Self, Self::Error> {
        let mut cmd = CommandBuilder::new(value.command.name, "", CommandType::Message)
            .dm_permission(value.dm_enabled)
            .build();

        cmd.default_member_permissions = value.member_permissions;

        validate_command(&cmd).context("Failed to validate message command")?;

        Ok(Self(cmd))
    }
}

impl From<MessageCommand> for Command {
    fn from(value: MessageCommand) -> Self {
        value.0
    }
}

pub struct UserCommand(Command);

impl TryFrom<BaseCommand> for UserCommand {
    type Error = CommandValidationError;

    fn try_from(value: BaseCommand) -> Result<Self, Self::Error> {
        let mut cmd = CommandBuilder::new(value.command.name, "", CommandType::User)
            .dm_permission(value.dm_enabled)
            .build();

        cmd.default_member_permissions = value.member_permissions;

        validate_command(&cmd).context("Failed to validate user command")?;

        Ok(Self(cmd))
    }
}

impl From<UserCommand> for Command {
    fn from(value: UserCommand) -> Self {
        value.0
    }
}

impl TryFrom<super::CommandOption> for CommandOption {
    type Error = &'static str;

    fn try_from(value: super::CommandOption) -> Result<Self, Self::Error> {
        match value {
            super::CommandOption::Arg(arg) => Ok(arg.into()),
            super::CommandOption::Sub(sub) => sub.try_into(),
            super::CommandOption::Group(group) => Ok(group.into()),
        }
    }
}

impl TryFrom<super::CommandFunction> for CommandOption {
    type Error = &'static str;

    fn try_from(value: super::CommandFunction) -> Result<Self, Self::Error> {
        if value.has_slash() || value.has_message() || value.has_user() {
            let mut sub = SubCommandBuilder::new(value.name, value.description).build();
            // Flatmap to ignore other than application command functions.
            let iter = value.options.into_iter().flat_map(TryInto::try_into);
            sub.options.get_or_insert_default().extend(iter);
            Ok(sub)
        } else {
            Err("Not application command function")
        }
    }
}

impl From<super::CommandGroup> for CommandOption {
    fn from(value: super::CommandGroup) -> Self {
        let mut group = SubCommandGroupBuilder::new(value.name, value.description).build();
        // Flatmap to ignore other than application command functions.
        let iter = value.subs.into_iter().flat_map(TryInto::try_into);
        group.options.get_or_insert_default().extend(iter);
        group
    }
}

impl From<super::ArgDesc> for CommandOption {
    fn from(value: super::ArgDesc) -> Self {
        match value.kind {
            super::ArgKind::Bool => BooleanBuilder::new(value.name, value.description)
                .required(value.required)
                .build(),
            super::ArgKind::Number(d) => NumberBuilder::new(value.name, value.description)
                .required(value.required)
                .choices(d.choices)
                .optional(d.min, |b, v| b.min_value(v))
                .optional(d.max, |b, v| b.max_value(v))
                .build(),
            super::ArgKind::Integer(d) => IntegerBuilder::new(value.name, value.description)
                .required(value.required)
                .choices(d.choices)
                .optional(d.min, |b, v| b.min_value(v))
                .optional(d.max, |b, v| b.max_value(v))
                .build(),
            super::ArgKind::String(d) => StringBuilder::new(value.name, value.description)
                .required(value.required)
                .choices(d.choices)
                .optional(d.min_length, |b, v| b.min_length(v))
                .optional(d.max_length, |b, v| b.max_length(v))
                .build(),
            super::ArgKind::Channel(d) => ChannelBuilder::new(value.name, value.description)
                .required(value.required)
                .channel_types(d.channel_types)
                .build(),
            super::ArgKind::Message => StringBuilder::new(value.name, value.description)
                .required(value.required)
                .min_length(1)
                .max_length(32)
                .build(),
            super::ArgKind::Attachment => AttachmentBuilder::new(value.name, value.description)
                .required(value.required)
                .build(),
            super::ArgKind::User => UserBuilder::new(value.name, value.description)
                .required(value.required)
                .build(),
            super::ArgKind::Role => RoleBuilder::new(value.name, value.description)
                .required(value.required)
                .build(),
            super::ArgKind::Mention => MentionableBuilder::new(value.name, value.description)
                .required(value.required)
                .build(),
        }
    }
}
