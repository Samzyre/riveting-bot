use std::pin::Pin;
use std::sync::Arc;

use derive_more::{IsVariant, Unwrap};
use futures::Future;

use crate::commands::request::{ClassicRequest, MessageRequest, SlashRequest, UserRequest};
use crate::commands::CommandResult;
use crate::Context;

// use crate::utils::prelude::*;

pub mod mock {
    use super::*;
    use crate::commands::request::{ClassicRequest, MessageRequest, SlashRequest, UserRequest};
    use crate::commands::{CommandResult, Response};

    pub async fn classic(_ctx: Context, req: ClassicRequest) -> CommandResult {
        println!("CLASSIC REQ: {req:#?}");
        Ok(Response::Clear)
    }

    pub async fn slash(_ctx: Context, req: SlashRequest) -> CommandResult {
        println!("SLASH REQ: {req:#?}");
        Ok(Response::Clear)
    }

    pub async fn message(_ctx: Context, req: MessageRequest) -> CommandResult {
        println!("MESSAGE REQ: {req:#?}");
        Ok(Response::Clear)
    }

    pub async fn user(_ctx: Context, req: UserRequest) -> CommandResult {
        println!("USER REQ: {req:#?}");
        Ok(Response::Clear)
    }
}

/// Non-generic return type for async command functions.
pub type CallFuture = Pin<Box<dyn Future<Output = CommandResult> + Send>>;

macro_rules! function_trait {
    ( $( $request:ty => $var:path )* ) => {
        $(
            impl<F, Fut> Callable<$request> for F
            where
                F: Fn(Context, $request) -> Fut + Send + Sync + 'static,
                Fut: Future<Output = CommandResult> + Send + 'static,
            {
                fn call(&self, ctx: Context, req: $request) -> CallFuture {
                    Box::pin((self)(ctx, req))
                }
            }

            impl Callable<$request> for Arc<dyn Callable<$request>> {
                fn call(&self, ctx: Context, req: $request) -> CallFuture {
                    (**self).call(ctx, req)
                }

                fn into_shared(self) -> Arc<dyn Callable<$request>> {
                    self
                }
            }

            impl<T> IntoFunction<$request> for T
            where
                T: Callable<$request> + 'static,
            {
                fn into_function(self) -> Function {
                    $var(self.into_shared())
                }
            }
        )*
    }
}

function_trait! {
    // Function that can handle basic text command.
    ClassicRequest => Function::Classic
    // Function that can handle interactive text command.
    SlashRequest => Function::Slash
    // Function that can handle GUI-based message command.
    MessageRequest => Function::Message
    // Function that can handle GUI-based user command.
    UserRequest => Function::User
}

pub type ClassicFunction = Arc<dyn Callable<ClassicRequest>>;
pub type SlashFunction = Arc<dyn Callable<SlashRequest>>;
pub type MessageFunction = Arc<dyn Callable<MessageRequest>>;
pub type UserFunction = Arc<dyn Callable<UserRequest>>;

/// Trait for functions that can be called with a generic request.
pub trait Callable<R, O = CallFuture>: Send + Sync {
    fn call(&self, ctx: Context, req: R) -> O;
    fn into_shared(self) -> Arc<dyn Callable<R, O>>
    where
        Self: Sized + 'static,
    {
        Arc::new(self)
    }
}

/// Trait for converting something callable into a specific supported type.
pub trait IntoFunction<R> {
    fn into_function(self) -> Function;
}

/// Supported function types.
#[derive(Clone, Unwrap, IsVariant)]
pub enum Function {
    Classic(ClassicFunction),
    Slash(SlashFunction),
    Message(MessageFunction),
    User(UserFunction),
}

impl Function {
    pub const fn kind(&self) -> FunctionKind {
        match self {
            Self::Classic(_) => FunctionKind::Classic,
            Self::Slash(_) => FunctionKind::Slash,
            Self::Message(_) => FunctionKind::Message,
            Self::User(_) => FunctionKind::User,
        }
    }
}

impl std::fmt::Debug for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Classic(_) => "Function::Classic(_)",
            Self::Slash(_) => "Function::Slash(_)",
            Self::Message(_) => "Function::Message(_)",
            Self::User(_) => "Function::User(_)",
        };
        write!(f, "{text}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionKind {
    Classic,
    Slash,
    Message,
    User,
}
