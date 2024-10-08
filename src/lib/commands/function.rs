use std::sync::Arc;

use derive_more::{IsVariant, Unwrap};

use crate::commands::prelude::*;
use crate::commands::{AsyncResponse, ResponseFuture};
// use crate::utils::prelude::*;
use crate::Context;

pub mod mock {
    use super::*;

    pub async fn classic(_ctx: Context, req: ClassicRequest) -> CommandResponse {
        println!("CLASSIC REQ: {req:#?}");
        Ok(Response::none())
    }

    pub async fn slash(_ctx: Context, req: SlashRequest) -> CommandResponse {
        println!("SLASH REQ: {req:#?}");
        Ok(Response::none())
    }

    pub async fn message(_ctx: Context, req: MessageRequest) -> CommandResponse {
        println!("MESSAGE REQ: {req:#?}");
        Ok(Response::none())
    }

    pub async fn user(_ctx: Context, req: UserRequest) -> CommandResponse {
        println!("USER REQ: {req:#?}");
        Ok(Response::none())
    }
}

/// Trait for functions that can be called with a generic request.
pub trait Callable<P, O = AsyncResponse>: Send + Sync {
    fn call(&self, params: P) -> O;
    fn into_shared(self) -> Arc<dyn Callable<P, O>>
    where
        Self: Sized + 'static,
    {
        Arc::new(self)
    }
}

impl<R, F, Fut> Callable<(Context, R)> for F
where
    F: Fn(Context, R) -> Fut + Send + Sync + 'static,
    Fut: ResponseFuture + 'static,
{
    fn call(&self, params: (Context, R)) -> AsyncResponse {
        Box::pin((self)(params.0, params.1))
    }
}

impl<R, S, F, Fut> Callable<(Context, R, S)> for F
where
    F: Fn(Context, R, S) -> Fut + Send + Sync + 'static,
    Fut: ResponseFuture + 'static,
{
    fn call(&self, params: (Context, R, S)) -> AsyncResponse {
        Box::pin((self)(params.0, params.1, params.2))
    }
}

impl<P> Callable<P> for Arc<dyn Callable<P>> {
    fn call(&self, params: P) -> AsyncResponse {
        (**self).call(params)
    }

    fn into_shared(self) -> Self {
        self
    }
}

/// Trait for converting something callable into a specific supported type.
pub trait IntoFunction<R> {
    fn into_function(self) -> Function;
}

impl<T> IntoFunction<ClassicRequest> for T
where
    T: Callable<(Context, ClassicRequest)> + 'static,
{
    fn into_function(self) -> Function {
        Function::Classic(self.into_shared())
    }
}

impl<T> IntoFunction<SlashRequest> for T
where
    T: Callable<(Context, SlashRequest)> + 'static,
{
    fn into_function(self) -> Function {
        Function::Slash(self.into_shared())
    }
}

impl<T> IntoFunction<MessageRequest> for T
where
    T: Callable<(Context, MessageRequest)> + 'static,
{
    fn into_function(self) -> Function {
        Function::Message(self.into_shared())
    }
}

impl<T> IntoFunction<UserRequest> for T
where
    T: Callable<(Context, UserRequest)> + 'static,
{
    fn into_function(self) -> Function {
        Function::User(self.into_shared())
    }
}

/// Function that can handle basic text command.
pub type ClassicFunction = Arc<dyn Callable<(Context, ClassicRequest)>>;
/// Function that can handle interactive text command.
pub type SlashFunction = Arc<dyn Callable<(Context, SlashRequest)>>;
/// Function that can handle GUI-based message command.
pub type MessageFunction = Arc<dyn Callable<(Context, MessageRequest)>>;
/// Function that can handle GUI-based user command.
pub type UserFunction = Arc<dyn Callable<(Context, UserRequest)>>;

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
