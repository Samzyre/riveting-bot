use std::borrow::Borrow;
use std::sync::Arc;

use derive_more::{From, IsVariant, Unwrap};
use twilight_mention::ParseMention;
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::id::Id;

use crate::commands::builder::{ArgDesc, ArgKind};
use crate::commands::CommandError;
use crate::utils::prelude::*;

pub mod types {
    use twilight_model::channel::{Attachment, Channel, Message};
    use twilight_model::guild::Role;
    use twilight_model::id::marker::{
        AttachmentMarker, ChannelMarker, GenericMarker, MessageMarker, RoleMarker, UserMarker,
    };
    use twilight_model::id::Id;
    use twilight_model::user::User;

    use crate::commands::arg::Ref;

    pub type ArgBool = bool;
    pub type ArgNumber = f64;
    pub type ArgInteger = i64;
    pub type ArgString = Box<str>;
    pub type ArgChannel = Ref<ChannelMarker, Channel>;
    pub type ArgMessage = Ref<MessageMarker, Message>;
    pub type ArgAttachment = Ref<AttachmentMarker, Attachment>;
    pub type ArgUser = Ref<UserMarker, User>;
    pub type ArgRole = Ref<RoleMarker, Role>;
    pub type ArgMention = Id<GenericMarker>;
}

/// Contained value that is either type `Ref::Id(Id<M>)` or `Ref::Obj(Arc<D>)`.
#[derive(Debug, From, Unwrap, IsVariant)]
pub enum Ref<M, D> {
    Id(Id<M>),
    Obj(Arc<D>),
}

impl<M, D> Clone for Ref<M, D> {
    fn clone(&self) -> Self {
        match self {
            Self::Id(arg0) => Self::Id(*arg0),
            Self::Obj(arg0) => Self::Obj(Arc::clone(arg0)),
        }
    }
}

impl<M, D> Ref<M, D> {
    /// Wrap data to an Arc and return the object variant.
    pub fn from_obj(obj: D) -> Self {
        Self::Obj(Arc::new(obj))
    }
}

impl<M, D> IdExt<M> for Ref<M, D>
where
    D: IdExt<M>,
{
    fn id(&self) -> Id<M> {
        match self {
            Self::Id(id) => *id,
            Self::Obj(obj) => obj.id(),
        }
    }
}

/// Wrapper around `Vec<Arg>` for extra features.
#[derive(Debug, Default, Clone)]
pub struct Args(Box<[Arg]>);

/// Implements convenience methods for getting a certain type of argument.
macro_rules! impl_variant_get {
    ($( $vis:vis fn $method:ident -> $value:ty );* $(;)?) => {
        $(
            /// Finds argument by name and returns the value, if it matches the variant.
            /// # Errors
            /// * Returns `CommandError::MissingArgs` if the arg was not found.
            /// * Returns `CommandError::ArgsMismatch` if the arg was found, but as different type.
            $vis fn $method(&self, name: &str) -> Result<$value, CommandError> {
                self.get(name)
                    .ok_or(CommandError::MissingArgs)
                    .and_then(|a| a.$method().ok_or(CommandError::ArgsMismatch))
            }
        )*
    };
}

impl Args {
    impl_variant_get!(
        pub fn bool -> types::ArgBool;
        pub fn number -> types::ArgNumber;
        pub fn integer -> types::ArgInteger;
        pub fn string -> types::ArgString;
        pub fn channel -> types::ArgChannel;
        pub fn message -> types::ArgMessage;
        pub fn attachment -> types::ArgAttachment;
        pub fn user -> types::ArgUser;
        pub fn role -> types::ArgRole;
        pub fn mention -> types::ArgMention;
    );

    /// Finds argument value by argument name.
    pub fn get(&self, name: &str) -> Option<&ArgValue> {
        self.as_ref()
            .iter()
            .find(|a| a.name == name)
            .map(|a| &a.value)
    }

    /// Returns the inner box.
    pub fn into_inner(self) -> Box<[Arg]> {
        self.0
    }
}

impl From<Vec<Arg>> for Args {
    fn from(value: Vec<Arg>) -> Self {
        Self(value.into_boxed_slice())
    }
}

impl AsRef<[Arg]> for Args {
    fn as_ref(&self) -> &[Arg] {
        &self.0
    }
}

/// A type representing an argument with name and value.
#[derive(Debug, Clone)]
pub struct Arg {
    pub name: String,
    pub value: ArgValue,
}

impl Arg {
    pub fn from_desc(desc: &ArgDesc, text: &str) -> AnyResult<Self> {
        Ok(Self {
            name: desc.name.to_string(),
            value: ArgValue::from_kind(&desc.kind, text)?,
        })
    }
}

/// Argument value type with data.
#[derive(Debug, Clone, Unwrap, IsVariant)]
pub enum ArgValue {
    Bool(types::ArgBool),
    Number(types::ArgNumber),
    Integer(types::ArgInteger),
    String(types::ArgString),
    Channel(types::ArgChannel),
    Message(types::ArgMessage),
    Attachment(types::ArgAttachment),
    User(types::ArgUser),
    Role(types::ArgRole),
    Mention(types::ArgMention),
}

impl ArgValue {
    impl_variant_option!(
        pub fn bool(&self: Bool(val)) -> types::ArgBool { *val }
        pub fn number(&self: Number(val)) -> types::ArgNumber { *val }
        pub fn integer(&self: Integer(val)) -> types::ArgInteger { *val }
        pub fn string(&self: String(val)) -> types::ArgString { val.to_owned() }
        pub fn channel(&self: Channel(val)) -> types::ArgChannel { val.to_owned() }
        pub fn message(&self: Message(val)) -> types::ArgMessage { val.to_owned() }
        pub fn attachment(&self: Attachment(val)) -> types::ArgAttachment { val.to_owned() }
        pub fn user(&self: User(val)) -> types::ArgUser { val.to_owned() }
        pub fn role(&self: Role(val)) -> types::ArgRole { val.to_owned() }
        pub fn mention(&self: Mention(val)) -> types::ArgMention { *val }
    );

    /// Create a value from value kind and text.
    pub fn from_kind(kind: &ArgKind, text: &str) -> AnyResult<Self> {
        // TODO: Ensure data parameters.

        /// Try to parse text as a discord mention, otherwise try to parse text as an id number.
        fn parse_mention_or_id<F, A, B>(text: &str, variant: F) -> AnyResult<ArgValue>
        where
            F: Fn(Ref<A, B>) -> ArgValue,
            Id<A>: ParseMention,
        {
            Ok(match Id::parse(text.trim()) {
                Ok(id) => variant(Ref::Id(id)),
                Err(mention_error) => match text.parse() {
                    Ok(id) => variant(Ref::Id(id)),
                    Err(id_parse_error) => {
                        return Err(anyhow::anyhow!("(as id) {id_parse_error}"))
                            .with_context(|| format!("(as mention) {mention_error}"));
                    },
                },
            })
        }

        let val = match kind {
            ArgKind::Bool => Self::Bool(
                text.to_lowercase()
                    .parse()
                    .context("Bool arg parse error")?,
            ),
            ArgKind::Number(_) => Self::Number(text.parse().context("Number arg parse error")?),
            ArgKind::Integer(_) => Self::Integer(text.parse().context("Integer arg parse error")?),
            ArgKind::String(_) => Self::String(text.to_string().into_boxed_str()),
            ArgKind::Channel(_) => {
                parse_mention_or_id(text, Self::Channel).context("Channel arg parse error")?
            },
            ArgKind::Message => {
                Self::Message(Ref::Id(text.parse().context("Message arg parse error")?))
            },
            ArgKind::Attachment => {
                Self::Attachment(Ref::Id(text.parse().context("Attachment arg parse error")?))
            },
            ArgKind::User => {
                parse_mention_or_id(text, Self::User).context("User arg parse error")?
            },
            ArgKind::Role => {
                parse_mention_or_id(text, Self::Role).context("Role arg parse error")?
            },
            ArgKind::Mention => Self::Mention(
                text.parse().context("Mention arg parse error")?, // TODO: Parse from text (if other than id number).
            ),
        };

        Ok(val)
    }
}

impl TryFrom<CommandOptionValue> for ArgValue {
    type Error = &'static str;

    fn try_from(value: CommandOptionValue) -> Result<Self, Self::Error> {
        match value {
            CommandOptionValue::Boolean(b) => Ok(Self::Bool(b)),
            CommandOptionValue::Number(n) => Ok(Self::Number(n)),
            CommandOptionValue::Integer(i) => Ok(Self::Integer(i)),
            CommandOptionValue::String(s) => Ok(Self::String(s.into_boxed_str())),
            CommandOptionValue::Channel(id) => Ok(Self::Channel(Ref::Id(id))),
            CommandOptionValue::Mentionable(id) => Ok(Self::Mention(id)),
            CommandOptionValue::Attachment(id) => Ok(Self::Attachment(Ref::Id(id))),
            CommandOptionValue::User(id) => Ok(Self::User(Ref::Id(id))),
            CommandOptionValue::Role(id) => Ok(Self::Role(Ref::Id(id))),
            CommandOptionValue::Focused(_s, _c) => todo!(), // FIXME: To be implemented
            CommandOptionValue::SubCommand(_) | CommandOptionValue::SubCommandGroup(_) => {
                Err("Cannot convert subcommand or group to argument value")
            },
        }
    }
}

/// Extension for `Option<ArgValue>` to get an option of matching variant.
pub trait ArgValueExt {
    fn bool(&self) -> Option<types::ArgBool>;
    fn number(&self) -> Option<types::ArgNumber>;
    fn integer(&self) -> Option<types::ArgInteger>;
    fn string(&self) -> Option<types::ArgString>;
    fn channel(&self) -> Option<types::ArgChannel>;
    fn message(&self) -> Option<types::ArgMessage>;
    fn attachment(&self) -> Option<types::ArgAttachment>;
    fn user(&self) -> Option<types::ArgUser>;
    fn role(&self) -> Option<types::ArgRole>;
    fn mention(&self) -> Option<types::ArgMention>;
}

impl<T> ArgValueExt for Option<T>
where
    T: Borrow<ArgValue>,
{
    fn bool(&self) -> Option<types::ArgBool> {
        self.as_ref().and_then(|v| v.borrow().bool())
    }

    fn number(&self) -> Option<types::ArgNumber> {
        self.as_ref().and_then(|v| v.borrow().number())
    }

    fn integer(&self) -> Option<types::ArgInteger> {
        self.as_ref().and_then(|v| v.borrow().integer())
    }

    fn string(&self) -> Option<types::ArgString> {
        self.as_ref().and_then(|v| v.borrow().string())
    }

    fn channel(&self) -> Option<types::ArgChannel> {
        self.as_ref().and_then(|v| v.borrow().channel())
    }

    fn message(&self) -> Option<types::ArgMessage> {
        self.as_ref().and_then(|v| v.borrow().message())
    }

    fn attachment(&self) -> Option<types::ArgAttachment> {
        self.as_ref().and_then(|v| v.borrow().attachment())
    }

    fn user(&self) -> Option<types::ArgUser> {
        self.as_ref().and_then(|v| v.borrow().user())
    }

    fn role(&self) -> Option<types::ArgRole> {
        self.as_ref().and_then(|v| v.borrow().role())
    }

    fn mention(&self) -> Option<types::ArgMention> {
        self.as_ref().and_then(|v| v.borrow().mention())
    }
}
