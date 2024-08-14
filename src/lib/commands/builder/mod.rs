//! Bot command builders.
//!
//! # Overview
//!
//! ### Base command, subcommand and group creation:
//! ```text
//! fn command("name", "description") -> BaseCommandBuilder
//! fn sub("name", "description") -> CommandFunctionBuilder
//! fn group("name", "description") -> CommandGroupBuilder
//! ```
//!
//! ### Command parameter options:
//! ```text
//! fn bool("name", "description") -> ArgDesc
//! fn number("name", "description") -> NumberOptionBuilder
//! fn integer("name", "description") -> IntegerOptionBuilder
//! fn string("name", "description") -> StringOptionBuilder
//! fn channel("name", "description") -> ChannelOptionBuilder
//! fn message("name", "description") -> ArgDesc
//! fn attachment("name", "description") -> ArgDesc
//! fn user("name", "description") -> ArgDesc
//! fn role("name", "description") -> ArgDesc
//! fn mention("name", "description") -> ArgDesc
//! ```
//!

use std::collections::HashSet;
use std::sync::Arc;

use derive_more::{Display, IsVariant, Unwrap};
use thiserror::Error;
pub use twilight_model::channel::ChannelType;
pub use twilight_model::guild::Permissions;

use crate::commands::builder::twilight::{
    CommandValidationError, MessageCommand, SlashCommand, TwilightCommand, UserCommand,
};
use crate::commands::function::{
    ClassicFunction, Function, FunctionKind, IntoFunction, MessageFunction, SlashFunction,
    UserFunction,
};
use crate::commands::ResponseFuture;
use crate::utils::prelude::*;
use crate::Context;

pub mod twilight;

/// Create a new base command.
pub fn command(name: &'static str, description: &'static str) -> BaseCommandBuilder {
    BaseCommandBuilder::new(name, description)
}

/// Create a new subcommand.
pub const fn sub(name: &'static str, description: &'static str) -> CommandFunctionBuilder {
    CommandFunctionBuilder::new(name, description)
}

/// Create a new command group.
pub const fn group(name: &'static str, description: &'static str) -> CommandGroupBuilder {
    CommandGroupBuilder::new(name, description)
}

/// Create a new argument with kind `Bool`.
pub const fn bool(name: &'static str, description: &'static str) -> ArgDesc {
    ArgDesc::new(name, description, ArgKind::Bool)
}

/// Create a new argument with kind `Number`.
pub fn number(name: &'static str, description: &'static str) -> NumberOptionBuilder {
    NumberOptionBuilder::new(name, description)
}

/// Create a new argument with kind `Integer`.
pub fn integer(name: &'static str, description: &'static str) -> IntegerOptionBuilder {
    IntegerOptionBuilder::new(name, description)
}

/// Create a new argument with kind `String`.
pub fn string(name: &'static str, description: &'static str) -> StringOptionBuilder {
    StringOptionBuilder::new(name, description)
}

/// Create a new argument with kind `Channel`.
pub fn channel(name: &'static str, description: &'static str) -> ChannelOptionBuilder {
    ChannelOptionBuilder::new(name, description)
}

/// Create a new argument with kind `Message`.
pub const fn message(name: &'static str, description: &'static str) -> ArgDesc {
    ArgDesc::new(name, description, ArgKind::Message)
}

/// Create a new argument with kind `Attachment`.
pub const fn attachment(name: &'static str, description: &'static str) -> ArgDesc {
    ArgDesc::new(name, description, ArgKind::Attachment)
}

/// Create a new argument with kind `User`.
pub const fn user(name: &'static str, description: &'static str) -> ArgDesc {
    ArgDesc::new(name, description, ArgKind::User)
}

/// Create a new argument with kind `Role`.
pub const fn role(name: &'static str, description: &'static str) -> ArgDesc {
    ArgDesc::new(name, description, ArgKind::Role)
}

/// Create a new argument with kind `Mention`.
pub const fn mention(name: &'static str, description: &'static str) -> ArgDesc {
    ArgDesc::new(name, description, ArgKind::Mention)
}

/// Helper macro to implement common methods for data builder.
/// This assumes `data` type implements `Default`.
macro_rules! impl_data_builder {
    (
        $( #[$new_meta:meta] )*
        $vis:vis fn new(..) -> Self( $variant:ident ( $data:ty ) )
    ) => {
        $( #[$new_meta] )*
        $vis fn new(name: &'static str, description: &'static str) -> Self {
            Self(ArgDesc::new(
                name,
                description,
                ArgKind::$variant( <$data>::default() ) ,
            ))
        }

        /// Set argument to be required. All required arguments must be before any optional ones.
        $vis const fn required(mut self) -> Self {
            self.0.required = true;
            self
        }

        /// Finalize the argument.
        $vis fn build(self) -> ArgDesc {
            self.0
        }

        /// Get inner data struct.
        fn inner_mut(&mut self) -> &mut $data {
            let ArgKind::$variant(ref mut data) = self.0.kind else { unreachable!() };
            data
        }
    }
}

#[derive(Debug, Clone)]
pub struct NumberOptionBuilder(ArgDesc);

impl NumberOptionBuilder {
    impl_data_builder!(
        /// Create new number option builder.
        pub fn new(..) -> Self(Number(NumericalData<f64>))
    );

    /// Set minimum value.
    pub fn min(mut self, min: f64) -> Self {
        self.inner_mut().min = Some(min);
        self
    }

    /// Set maximum value.
    pub fn max(mut self, max: f64) -> Self {
        self.inner_mut().max = Some(max);
        self
    }

    /// Set number option choices.
    pub fn choices<T>(mut self, choices: impl IntoIterator<Item = (T, f64)>) -> Self
    where
        T: Into<String>,
    {
        self.inner_mut().choices = choices.into_iter().map(|(a, b)| (a.into(), b)).collect();
        self
    }
}

#[derive(Debug, Clone)]
pub struct IntegerOptionBuilder(ArgDesc);

impl IntegerOptionBuilder {
    impl_data_builder!(
        /// Create new integer option builder.
        pub fn new(..) -> Self(Integer(NumericalData<i64>))
    );

    /// Set minimum value.
    pub fn min(mut self, min: i64) -> Self {
        self.inner_mut().min = Some(min);
        self
    }

    /// Set maximum value.
    pub fn max(mut self, max: i64) -> Self {
        self.inner_mut().max = Some(max);
        self
    }

    /// Set integer option choices.
    pub fn choices<T>(mut self, choices: impl IntoIterator<Item = (T, i64)>) -> Self
    where
        T: Into<String>,
    {
        self.inner_mut().choices = choices.into_iter().map(|(a, b)| (a.into(), b)).collect();
        self
    }
}

#[derive(Debug, Clone)]
pub struct StringOptionBuilder(ArgDesc);

impl StringOptionBuilder {
    impl_data_builder!(
        /// Create new string option builder.
        pub fn new(..) -> Self(String(StringData))
    );

    /// Maximum allowed length. Must be at least `1` and at most `6000`.
    pub fn max_length(mut self, max: u16) -> Self {
        self.inner_mut().max_length = Some(max);
        self
    }

    /// Minimum allowed length. Must be at most `6000`.
    pub fn min_length(mut self, min: u16) -> Self {
        self.inner_mut().min_length = Some(min);
        self
    }

    /// Set string option choices as `(name, value)` pairs.
    pub fn choices<N, V>(mut self, choices: impl IntoIterator<Item = (N, V)>) -> Self
    where
        N: Into<String>,
        V: Into<String>,
    {
        self.inner_mut().choices = choices
            .into_iter()
            .map(|(a, b)| (a.into(), b.into()))
            .collect();
        self
    }
}

#[derive(Debug, Clone)]
pub struct ChannelOptionBuilder(ArgDesc);

impl ChannelOptionBuilder {
    impl_data_builder!(
        /// Create new channel option builder.
        pub fn new(..) -> Self(Channel(ChannelData))
    );

    /// Set channel types for the option.
    ///
    /// Restricts the channel choice to specific types.
    pub fn types(mut self, types: impl IntoIterator<Item = ChannelType>) -> Self {
        self.inner_mut().channel_types = types.into_iter().collect();
        self
    }
}

#[derive(Debug, Default, Clone)]
pub struct NumericalData<T> {
    pub min: Option<T>,
    pub max: Option<T>,
    pub choices: Vec<(String, T)>,
}

#[derive(Debug, Default, Clone)]
pub struct StringData {
    pub max_length: Option<u16>,
    pub min_length: Option<u16>,
    pub choices: Vec<(String, String)>,
}

#[derive(Debug, Default, Clone)]
pub struct ChannelData {
    pub channel_types: Vec<ChannelType>,
}

#[derive(Debug, Clone, Display)]
pub enum ArgKind {
    #[display(fmt = "bool")]
    Bool,

    #[display(fmt = "number")]
    Number(NumericalData<f64>),

    #[display(fmt = "integer")]
    Integer(NumericalData<i64>),

    #[display(fmt = "string")]
    String(StringData),

    #[display(fmt = "channel")]
    Channel(ChannelData),

    #[display(fmt = "message")]
    Message,

    #[display(fmt = "attachment")]
    Attachment, // TODO: Define if this should try to capture the object (eg. uploaded attachment or attachment in replied message)

    #[display(fmt = "user")]
    User, // TODO: Define if this should try to capture the object (eg. sender)

    #[display(fmt = "role")]
    Role,

    #[display(fmt = "mention")]
    Mention,
}

#[derive(Debug, Clone)]
pub struct ArgDesc {
    pub name: &'static str,
    pub description: &'static str,
    pub kind: ArgKind,
    pub required: bool,
}

impl ArgDesc {
    /// Create a new argument.
    const fn new(name: &'static str, description: &'static str, kind: ArgKind) -> Self {
        Self {
            name,
            description,
            kind,
            required: false,
        }
    }

    /// Set argument to be required. All required arguments must be before any optional ones.
    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }
}

/// This error type contains a collection of missing function errors found in a command.
#[derive(Debug, Error)]
struct MissingFunctionsError {
    errors: Vec<anyhow::Error>,
}

impl std::fmt::Display for MissingFunctionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        )
    }
}

/// Base command type, contains meta information with the command itself.
#[derive(Debug, Clone)]
pub struct BaseCommand {
    /// The command structure.
    pub command: CommandFunction,
    /// Additional help for using the command. (not full usage help)
    pub help: String,
    /// If the command can be used in DMs.
    pub dm_enabled: bool,
    /// Default guild member permissions for the command.
    /// - `None`: Anyone,
    /// - `Some(Permissions::empty())`: Administrator,
    /// - `Some(Permissions::all())`: Administrator,
    /// - `Some(perms)`: User must satisfy all contained perms,
    pub member_permissions: Option<Permissions>,
}

impl BaseCommand {
    /// Generate commands to be integrated to discord.
    pub fn twilight_commands(
        &self,
    ) -> impl Iterator<Item = Result<TwilightCommand, CommandValidationError>> + '_ {
        let mut seen = HashSet::new();
        self.command
            .functions
            .iter()
            .filter(move |f| seen.insert(f.kind()))
            .filter_map(|f| match f {
                Function::Classic(_) => None,
                Function::Slash(_) => Some(SlashCommand::try_from(self.clone()).map(Into::into)),
                Function::Message(_) => {
                    Some(MessageCommand::try_from(self.clone()).map(Into::into))
                },
                Function::User(_) => Some(UserCommand::try_from(self.clone()).map(Into::into)),
            })
    }

    /// Validate the command.
    pub fn validate(&self) -> AnyResult<()> {
        self.check_missing_functions()?;

        // HACK: Mostly waste of cpu cycles.
        self.twilight_commands()
            .try_for_each(|c| c.map(|_| ()))
            .with_context(|| format!("Failed to validate command '{}'", self.command.name))
            .map_err(Into::into)
    }

    /// Generate usage help text.
    pub fn generate_help(&self) -> String {
        let types = {
            let mut types = Vec::with_capacity(4);
            if self.command.has_classic() {
                types.push("Classic");
            }
            if self.command.has_slash() {
                types.push("Slash");
            }
            if self.command.has_message() {
                types.push("Message");
            }
            if self.command.has_user() {
                types.push("User");
            }
            types.join(", ")
        };

        let dm = if self.dm_enabled { "Yes" } else { "No" };

        let perms = match self.member_permissions {
            None => "None".to_string(),
            Some(mp) if mp.contains(Permissions::ADMINISTRATOR) || mp.is_empty() => {
                "Administrator".to_string()
            },
            Some(mp) => format!("{mp:?}"),
        };

        let help_spacer = if self.help.is_empty() { "" } else { "\n" };

        let text = indoc::formatdoc! {"
            ```yaml
            {cmd}
            {help_spacer}{help}
            Permissions required: {perms}
            Enabled in DMs: {dm}
            Types: {types}
            ```",
            cmd = self.command.generate_help(0),
            help = self.help,
        };

        text
    }

    /// Checks that the base command contains all function types that are present in subcommands.
    fn check_missing_functions(&self) -> Result<(), MissingFunctionsError> {
        fn check_sub(
            errors: &mut Vec<anyhow::Error>,
            base_name: &str,
            base: &[FunctionKind],
            sub: &CommandFunction,
        ) {
            for kind in sub.functions.iter().map(|f| f.kind()) {
                if !base.contains(&kind) {
                    errors.push(anyhow::anyhow!(
                        "Base command '{base_name}' does not map to a function of a kind \
                         '{kind:?}', but the subcommand '{sub_name}' does",
                        sub_name = sub.name
                    ));
                }
            }
        }

        let base_funcs: Vec<_> = self.command.functions.iter().map(|f| f.kind()).collect();
        let mut errors = Vec::new();

        for opt in self.command.options.iter() {
            match opt {
                CommandOption::Arg(_) => break,
                CommandOption::Sub(s) => check_sub(&mut errors, self.command.name, &base_funcs, s),
                CommandOption::Group(g) => g
                    .subs
                    .iter()
                    .for_each(|s| check_sub(&mut errors, self.command.name, &base_funcs, s)),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(MissingFunctionsError { errors })
        }
    }
}

impl From<BaseCommandBuilder> for BaseCommand {
    fn from(value: BaseCommandBuilder) -> Self {
        value.build()
    }
}

#[derive(Debug, Clone)]
pub struct BaseCommandBuilder(BaseCommand);

impl BaseCommandBuilder {
    pub fn new(name: &'static str, description: &'static str) -> Self {
        Self(BaseCommand {
            command: CommandFunctionBuilder::new(name, description).into(),
            help: String::new(),
            dm_enabled: false,
            member_permissions: None,
        })
    }

    /// Additional help to show with usage.
    pub fn help(mut self, text: String) -> Self {
        self.0.help = text;
        self
    }

    /// Set command to be available in DMs.
    pub const fn dm(mut self) -> Self {
        self.0.dm_enabled = true;
        self
    }

    /// Set default guild member permissions for the command.
    pub const fn permissions(mut self, permissions: Permissions) -> Self {
        self.0.member_permissions = Some(permissions);
        self
    }

    // NOTE: Technically this should work with just `function: impl IntoFunction<R>` as parameter.
    // Though, without the additional bounds the compiler can sometimes generate "false" errors,
    // even if the problem is actually somewhere else. (Maybe related to incomplete features that are in use)
    /// Add a function to this base command. Functions get called on the command event.
    pub fn attach<F, R, Fut>(mut self, function: F) -> Self
    where
        F: Fn(Context, R) -> Fut + IntoFunction<R> + Send + Sync + 'static,
        Fut: ResponseFuture + 'static,
    {
        self.0.command.functions.push(function.into_function());
        self
    }

    /// Add an option to the command.
    pub fn option(mut self, option: impl Into<CommandOption>) -> Self {
        self.0.command.options.push(option.into());
        self
    }

    /// Validate the command.
    pub fn validate(&self) -> AnyResult<()> {
        self.0.validate()
    }

    /// Finalize the command.
    pub fn build(self) -> BaseCommand {
        self.0
    }
}

/// Command that maps to a function.
#[derive(Debug, Clone)]
pub struct CommandFunction {
    pub name: &'static str,
    pub description: &'static str,
    pub functions: Vec<Function>,
    pub options: Vec<CommandOption>,
}

impl CommandFunction {
    /// Returns true if the command has classic functions.
    pub fn has_classic(&self) -> bool {
        self.functions.iter().any(Function::is_classic)
    }

    /// Returns true if the command has slash functions.
    pub fn has_slash(&self) -> bool {
        self.functions.iter().any(Function::is_slash)
    }

    /// Returns true if the command has message functions.
    pub fn has_message(&self) -> bool {
        self.functions.iter().any(Function::is_message)
    }

    /// Returns true if the command has user functions.
    pub fn has_user(&self) -> bool {
        self.functions.iter().any(Function::is_user)
    }

    /// Returns an iterator of attached classic functions.
    pub fn classic(&self) -> impl Iterator<Item = ClassicFunction> + '_ {
        self.functions.iter().filter_map(|f| match f {
            Function::Classic(f) => Some(Arc::clone(f)),
            _ => None,
        })
    }

    /// Returns an iterator of attached slash functions.
    pub fn slash(&self) -> impl Iterator<Item = SlashFunction> + '_ {
        self.functions.iter().filter_map(|f| match f {
            Function::Slash(f) => Some(Arc::clone(f)),
            _ => None,
        })
    }

    /// Returns an iterator of attached message functions.
    pub fn message(&self) -> impl Iterator<Item = MessageFunction> + '_ {
        self.functions.iter().filter_map(|f| match f {
            Function::Message(f) => Some(Arc::clone(f)),
            _ => None,
        })
    }

    /// Returns an iterator of attached user functions.
    pub fn user(&self) -> impl Iterator<Item = UserFunction> + '_ {
        self.functions.iter().filter_map(|f| match f {
            Function::User(f) => Some(Arc::clone(f)),
            _ => None,
        })
    }

    /// Returns an iterator of command arguments.
    pub fn args(&self) -> impl Iterator<Item = &ArgDesc> {
        self.options.iter().filter_map(|o| o.arg())
    }

    /// Generate usage help text.
    fn generate_help(&self, indent: usize) -> String {
        let mut opt_help = String::new();
        for opt in self.options.iter() {
            opt_help.push('\n');
            opt_help.push_str(&"\t".repeat(indent + 1));
            opt_help.push_str(&opt.generate_help(indent + 1));
        }
        format!("{:<16} {}{opt_help}", self.name, self.description)
    }
}

impl From<CommandFunctionBuilder> for CommandFunction {
    fn from(value: CommandFunctionBuilder) -> Self {
        value.build()
    }
}

#[derive(Debug, Clone)]
pub struct CommandFunctionBuilder(CommandFunction);

impl CommandFunctionBuilder {
    /// Create a new command builder.
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self(CommandFunction {
            name,
            description: if description.is_empty() {
                "-" // Empty description.
            } else {
                description
            },
            functions: Vec::new(),
            options: Vec::new(),
        })
    }

    // NOTE: Technically this should work with just `function: impl IntoFunction<R>` as parameter.
    // Though, without the additional bounds the compiler can sometimes generate "false" errors,
    // even if the problem is actually somewhere else. (Maybe related to incomplete features that are in use)
    /// Add a function to this (sub)command. Functions get called on the command event.
    pub fn attach<F, R, Fut>(mut self, function: F) -> Self
    where
        F: Fn(Context, R) -> Fut + IntoFunction<R> + Send + Sync + 'static,
        Fut: ResponseFuture + 'static,
    {
        self.0.functions.push(function.into_function());
        self
    }

    /// Add an option to the command.
    pub fn option(mut self, option: impl Into<CommandOption>) -> Self {
        self.0.options.push(option.into());
        self
    }

    /// Finalize the command.
    pub fn build(self) -> CommandFunction {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct CommandGroup {
    pub name: &'static str,
    pub description: &'static str,
    pub subs: Vec<CommandFunction>,
}

impl CommandGroup {
    pub fn to_options(&self) -> Vec<CommandOption> {
        self.subs.iter().cloned().map(CommandOption::Sub).collect()
    }
}

impl From<CommandGroupBuilder> for CommandGroup {
    fn from(value: CommandGroupBuilder) -> Self {
        value.build()
    }
}

#[derive(Debug, Clone)]
pub struct CommandGroupBuilder(CommandGroup);

impl CommandGroupBuilder {
    /// Create a new command group builder.
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self(CommandGroup {
            name,
            description,
            subs: Vec::new(),
        })
    }

    /// Add subcommands to this group.
    pub fn subs<I>(mut self, subs: impl IntoIterator<Item = I>) -> Self
    where
        I: Into<CommandFunction>,
    {
        self.0.subs.extend(subs.into_iter().map(Into::into));
        self
    }

    /// Add a subcommand to this group.
    pub fn option(mut self, sub: impl Into<CommandFunction>) -> Self {
        self.0.subs.push(sub.into());
        self
    }

    /// Finalize the command group.
    pub fn build(self) -> CommandGroup {
        self.0
    }
}

/// Command option types.
#[derive(Debug, Clone, IsVariant, Unwrap)]
pub enum CommandOption {
    Arg(ArgDesc),
    Sub(CommandFunction),
    Group(CommandGroup),
}

impl CommandOption {
    impl_variant_option!(
        pub fn arg(&self: Arg(val)) -> &ArgDesc;
        pub fn sub(&self: Sub(val)) -> &CommandFunction;
        pub fn group(&self: Group(val)) -> &CommandGroup;
    );

    /// Get the option name.
    pub const fn name(&self) -> &str {
        match self {
            Self::Arg(a) => a.name,
            Self::Sub(s) => s.name,
            Self::Group(g) => g.name,
        }
    }

    /// Generate usage help text.
    fn generate_help(&self, indent: usize) -> String {
        match self {
            Self::Arg(a) => {
                let brackets = if a.required { ['<', '>'] } else { ['[', ']'] };
                let name = format!("{}{}{}", brackets[0], a.name, brackets[1]);
                format!("{name:<16} {}", a.description)
            },
            Self::Sub(s) => s.generate_help(indent),
            Self::Group(g) => {
                let mut sub_help = format!("{:<16} {}", g.name, g.description);
                for sub in g.subs.iter() {
                    sub_help.push('\n');
                    sub_help.push_str(&"\t".repeat(indent + 1));
                    sub_help.push_str(&sub.generate_help(indent + 1));
                }
                sub_help
            },
        }
    }
}

impl From<NumberOptionBuilder> for CommandOption {
    fn from(value: NumberOptionBuilder) -> Self {
        value.build().into()
    }
}

impl From<IntegerOptionBuilder> for CommandOption {
    fn from(value: IntegerOptionBuilder) -> Self {
        value.build().into()
    }
}

impl From<StringOptionBuilder> for CommandOption {
    fn from(value: StringOptionBuilder) -> Self {
        value.build().into()
    }
}

impl From<ChannelOptionBuilder> for CommandOption {
    fn from(value: ChannelOptionBuilder) -> Self {
        value.build().into()
    }
}

impl From<ArgDesc> for CommandOption {
    fn from(value: ArgDesc) -> Self {
        Self::Arg(value)
    }
}

impl From<CommandFunction> for CommandOption {
    fn from(value: CommandFunction) -> Self {
        Self::Sub(value)
    }
}

impl From<CommandFunctionBuilder> for CommandOption {
    fn from(value: CommandFunctionBuilder) -> Self {
        Self::Sub(value.into())
    }
}

impl From<CommandGroup> for CommandOption {
    fn from(value: CommandGroup) -> Self {
        Self::Group(value)
    }
}

impl From<CommandGroupBuilder> for CommandOption {
    fn from(value: CommandGroupBuilder) -> Self {
        Self::Group(value.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::function::mock;

    static COMMANDS: std::sync::OnceLock<Vec<BaseCommand>> = std::sync::OnceLock::new();

    fn commands() -> &'static [BaseCommand] {
        COMMANDS.get_or_init(|| {
            let mut commands = Vec::with_capacity(8);

            commands.push(
                command("message", "test")
                    .attach(mock::classic)
                    .attach(mock::slash)
                    .attach(mock::message)
                    .attach(mock::user)
                    .dm()
                    .option(message("message", "description")),
            );

            commands.push(command("a", "description"));

            commands.push(
                command("b", "description")
                    .attach(mock::message)
                    .attach(mock::user),
            );

            commands.push(
                command("c", "description")
                    .attach(mock::classic)
                    .permissions(Permissions::SEND_MESSAGES)
                    .option(bool("ca", "description").required())
                    .option(bool("cb", "description")),
            );

            commands.push(
                command("d", "")
                    .attach(mock::classic)
                    .attach(mock::slash)
                    .attach(mock::message)
                    .attach(mock::user)
                    .permissions(Permissions::empty())
                    .option(
                        number("da", "description")
                            .required()
                            .min(0.0)
                            .max(100.0)
                            .choices([("daa", 24.0), ("dab", 42.0)]),
                    )
                    .option(
                        integer("db", "description")
                            .required()
                            .min(0)
                            .max(100)
                            .choices([("dba", 24), ("dbb", 42)]),
                    )
                    .option(
                        string("dc", "description")
                            .required()
                            .choices([("dca", 1234.to_string())]),
                    )
                    .option(
                        channel("dd", "description")
                            .required()
                            .types([ChannelType::GuildText]),
                    )
                    .option(bool("de", "description").required())
                    .option(user("df", "description").required())
                    .option(role("dg", "description").required())
                    .option(message("dh", "description").required())
                    .option(mention("di", "description").required())
                    .option(attachment("dj", "description")),
            );

            commands.push(
                command("e", "description")
                    .attach(mock::classic)
                    .attach(mock::slash)
                    .attach(mock::message)
                    .attach(mock::user)
                    .dm()
                    .permissions(Permissions::all())
                    .option(sub("ea", "description"))
                    .option(
                        sub("eb", "description")
                            .attach(mock::classic)
                            .attach(mock::slash)
                            .option(
                                number("eaa", "description")
                                    .min(0.0)
                                    .max(100.0)
                                    .choices([("eaaa", 24.0), ("eaab", 42.0)]),
                            )
                            .option(
                                integer("eab", "description")
                                    .min(0)
                                    .max(100)
                                    .choices([("eaba", 24), ("eabb", 42)]),
                            )
                            .option(string("eac", "description").choices([("foo", "bar")]))
                            .option(channel("ead", "description").types([ChannelType::GuildText]))
                            .option(bool("eae", "description"))
                            .option(user("eaf", "description"))
                            .option(role("eag", "description"))
                            .option(message("eah", "description"))
                            .option(mention("eai", "description"))
                            .option(attachment("eaj", "description")),
                    )
                    .option(
                        group("ec", "description")
                            .option(sub("eca", "description"))
                            .option(
                                sub("ecb", "description")
                                    .attach(mock::classic)
                                    .attach(mock::slash)
                                    .option(bool("ecba", "description").required())
                                    .option(bool("ecbb", "description")),
                            ),
                    ),
            );

            commands.into_iter().map(|c| c.build()).collect::<Vec<_>>()
        })
    }

    #[test]
    fn valid_commands() {
        // FIXME: Numerical choices must be in range of min and max, this should give some warning at least
        commands()
            .iter()
            .filter_map(|c| Some((c.validate().err()?, c)))
            .for_each(|(e, c)| panic!("\n{c:#?}\n\n{e}"));
    }

    #[test]
    fn commands_help() {
        commands()
            .iter()
            .for_each(|c| println!("{}\n", c.generate_help()))
    }
}
