//! Command types:
//! - Classic: Chat text message, can reference another message.
//! - Slash: Semi-text-based interaction, cannot reference another message directly.
//! - Message: GUI-based interaction, no options, some data may be resolved by Discord or Twilight.
//! - User: GUI-based interaction, no options, some data may be resolved by Discord or Twilight.
//!
//! ```text
//!     ┌─────────────────┐           ┌─────────────┐       ┌──────────────┐
//!     │Interaction Event│        ┌─►│Slash Command├────┬─►│Create Request│
//!     └──────────┬──────┘        │  └─────────────┘    │  └───────┬──────┘
//!                │               │                     │          │
//!                │               │  ┌───────────────┐  │          ▼
//!                ▼               ├─►│Message Command├──┤  ┌──────────────────────────┐
//!         ┌───────────────────┐  │  └───────────────┘  │  │Execute Attached Functions│
//!         │Application Command├──┤                     │  └───────┬──────────────────┘
//!         └───────────────────┘  │  ┌────────────┐     │          │
//!                                └─►│User Command├─────┤          ▼
//!                                   └────────────┘     │  ┌─────────────────────┐
//!                                                      │  │Handle Command Result│
//!     ┌──────────────────┐          ┌───────────────┐  │  └───────┬─────────────┘
//!     │Chat Message Event├─────────►│Classic Command├──┘          │
//!     └──────────────────┘          └───────────────┘             │
//!                                                                 │
//!                                       ┌────────────────────┐    │
//!                                    ┌──┤Interaction Response│◄───┤
//!         ┌───────────────────────┐  │  └────────────────────┘    │
//!         │ Clear / Update / None │◄─┤                            │
//!         └───────────────────────┘  │  ┌─────────────────────┐   │
//!                                    └──┤Original Chat Message│◄──┘
//!                                       └─────────────────────┘
//! ```
//!

use std::collections::{BTreeMap, HashSet};
use std::fmt::Write;
use std::mem;
use std::pin::Pin;
use std::sync::Arc;

use derive_more::{Deref, DerefMut, Index, IntoIterator};
use futures::Future;
use thiserror::Error;
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;

use crate::commands::builder::twilight::{CommandValidationError, TwilightCommand};
use crate::commands::builder::BaseCommand;
use crate::commands::request::Request;
use crate::utils::prelude::*;
use crate::{BotEvent, Context};

pub mod arg;
pub mod builder;
pub mod function;
pub mod handle;
pub mod request;

/// Prelude module for command things.
pub mod prelude {
    pub use crate::commands::arg::{ArgValueExt, Args};
    pub use crate::commands::builder::BaseCommand;
    pub use crate::commands::request::{ClassicRequest, MessageRequest, SlashRequest, UserRequest};
    pub use crate::commands::{
        AsyncResponse, CommandError, CommandFuture, CommandResponse, CommandResult, Response,
        ResponseFuture,
    };
    pub use crate::Context;
}

#[derive(Debug, Error)]
pub enum CommandError {
    /// A command prefix is needed.
    #[error("Message did not start with a command prefix")]
    NotPrefixed,

    /// A command does not exist.
    #[error("Command not found: {0}")]
    NotFound(String),

    /// A resource does not exist.
    #[error("Resource not found: {0}")]
    UnknownResource(String),

    /// Work-in-progress.
    #[error("Command or action not yet implemented")]
    NotImplemented,

    /// The sender must reply to a message.
    #[error("Expected reply reference missing")]
    MissingReply,

    /// The sender must provide some arguments.
    #[error("Expected arguments missing")]
    MissingArgs,

    /// The argument was found, but of a different type.
    #[error("Argument type mismatch")]
    ArgsMismatch,

    /// Some arguments are wrong, invalid or unexpected.
    #[error("Arguments unexpected or failed to process: {0}")]
    UnexpectedArgs(String),

    /// Error while parsing command or argument.
    #[error("Failed to parse command or argument: {0}")]
    ParseError(String),

    /// The command or action is not available in this context.
    #[error("Command or action disabled")]
    Disabled,

    /// The sender does not have permissions needed.
    #[error("Permission requirements not met")]
    AccessDenied,

    /// Other errors that are or can be converted to `anyhow::Error`.
    #[error(transparent)]
    Other(#[from] anyhow::Error), // Source and Display delegate to `anyhow::Error`
}

impl PartialEq for CommandError {
    fn eq(&self, other: &Self) -> bool {
        mem::discriminant(self) == mem::discriminant(other) // Close enough.
    }
}

impl From<&'static str> for CommandError {
    fn from(s: &'static str) -> Self {
        Self::Other(anyhow::anyhow!(s))
    }
}

macro impl_into_command_error($out:ident; $t:ty) {
    impl From<$t> for CommandError {
        fn from(other: $t) -> Self {
            Self::$out(other.into())
        }
    }
}

impl_into_command_error!(Other; std::fmt::Error);
impl_into_command_error!(Other; reqwest::Error);
impl_into_command_error!(Other; serde_json::Error);
impl_into_command_error!(Other; tokio::sync::mpsc::error::SendError<BotEvent>);
impl_into_command_error!(Other; twilight_gateway::error::SendError);
impl_into_command_error!(Other; twilight_http::Error);
impl_into_command_error!(Other; twilight_http::response::DeserializeBodyError);
impl_into_command_error!(Other; twilight_standby::future::Canceled);
impl_into_command_error!(Other; twilight_util::builder::embed::image_source::ImageSourceUrlError);
impl_into_command_error!(Other; twilight_validate::message::MessageValidationError);
impl_into_command_error!(Other; twilight_validate::request::ValidationError);

// Function -> CommandResponse -> Response -> CommandResult

/// Trait alias for a command response future.
pub trait ResponseFuture = Future<Output = CommandResponse> + Send;

/// Trait alias for a command result future.
pub trait CommandFuture = Future<Output = CommandResult<()>> + Send;

/// Non-generic return type for async command functions.
pub type AsyncResponse = Pin<Box<dyn ResponseFuture>>;

/// Result with a response from a command function.
pub type CommandResponse = Result<Response, CommandError>;

/// Generic command result with command error type.
pub type CommandResult<T> = Result<T, CommandError>;

/// Command function response type.
#[derive(Deref, DerefMut)]
pub struct Response(Pin<Box<dyn CommandFuture>>);

impl Response {
    /// Noop.
    pub fn none() -> Self {
        Self::new(move || async move { Ok(()) })
    }

    /// Deletes original message or response. This will ignore any errors from the deletion.
    pub fn clear(ctx: Context, req: impl Into<Request> + Send + 'static) -> Self {
        Self::new(move || async move {
            match req.into() {
                Request::Classic(req) => req.clear(&ctx).await,
                Request::Slash(req) => req.clear(&ctx).await,
                Request::Message(req) => req.clear(&ctx).await,
                Request::User(req) => req.clear(&ctx).await,
            }
            .or(Ok(()))
        })
    }

    /// Creates a new response from a function.
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: CommandFuture + 'static,
    {
        Self(Box::pin(f()))
    }
}

impl Future for Response {
    type Output = CommandResult<()>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Future::poll(self.0.as_mut(), cx)
    }
}

/// Newtype for commands collection.
#[derive(Debug, Default, Clone, IntoIterator, Index)]
pub struct Commands(BTreeMap<&'static str, Arc<BaseCommand>>);

impl Commands {
    /// Get base command by name.
    pub fn get(&self, id: &str) -> Option<&Arc<BaseCommand>> {
        self.0.get(id)
    }

    /// Convert commands to Discord compatible list.
    pub fn twilight_commands(&self) -> Result<Vec<TwilightCommand>, CommandValidationError> {
        self.0
            .values()
            .flat_map(|b| b.twilight_commands())
            .try_collect()
    }

    /// Get reference to the inner list.
    pub const fn inner(&self) -> &BTreeMap<&'static str, Arc<BaseCommand>> {
        &self.0
    }
}

impl Commands {
    pub fn display(&self, ctx: &Context, guild_id: Option<Id<GuildMarker>>) -> AnyResult<String> {
        let mut slash = vec![];
        let mut classic = vec![];
        let mut gui = vec![];

        for (&k, v) in self.0.iter() {
            if guild_id.is_none() && !v.dm_enabled {
                continue;
            }
            if v.command.has_slash() {
                slash.push(k);
            }
            if v.command.has_classic() {
                classic.push(k);
            }
            if v.command.has_message() || v.command.has_user() {
                gui.push(k);
            }
        }

        let slash = slash.join(", ");
        let classic = classic.join(", ");
        let gui = gui.join(", ");

        let mut s = String::new();

        if !slash.is_empty() {
            writeln!(s, "/\t{slash}")?;
        }
        if !classic.is_empty() {
            let prefix = ctx.config.classic_prefix(guild_id)?;
            writeln!(s, "{prefix}\t{classic}")?;
        }
        if !gui.is_empty() {
            writeln!(s, "🖱   {gui}")?;
        }

        Ok(s)
    }
}

/// A type for creating a collection of commands and validating them.
#[derive(Debug, Default, Clone)]
pub struct CommandsBuilder {
    pub list: Vec<BaseCommand>,
}

impl CommandsBuilder {
    /// Create a new list of commands.
    pub fn new() -> Self {
        Self::default()
    }

    /// Assign a command to the list.
    pub fn bind(&mut self, cmd: impl Into<BaseCommand>) -> &mut Self {
        self.list.push(cmd.into());
        self
    }

    /// Validate the list of commands.
    pub fn validate(&self) -> AnyResult<()> {
        let mut set = HashSet::with_capacity(self.list.len());

        for cmd in self.list.iter() {
            // Ensure command itself is valid.
            cmd.validate()?;

            // Ensure it doesn't overlap with other commands.
            anyhow::ensure!(
                set.insert(&cmd.command.name),
                "Duplicate command found: {}",
                cmd.command.name
            );
        }

        Ok(())
    }

    /// Finalize the list of commands.
    pub fn build(self) -> Commands {
        Commands(
            self.list
                .into_iter()
                .map(|b| (b.command.name, Arc::new(b)))
                .collect(),
        )
    }
}
