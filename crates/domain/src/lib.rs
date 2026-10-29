use std::pin::Pin;

pub mod models;
pub mod tools;
/// Abstractions for later

pub trait Request {
    type Response: Send;
}
/// Abstraction for later
pub trait Handle<T>
where
    T: Request,
{
    type Error: Send;
    type Context;

    fn execute(
        &self,
        ctx: Self::Context,
        req: T,
    ) -> Pin<Box<dyn Future<Output = Result<T::Response, Self::Error>> + Send>>;
}
