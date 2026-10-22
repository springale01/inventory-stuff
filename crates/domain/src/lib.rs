use std::pin::Pin;

pub mod models;
pub mod tools;

pub trait Request {
    type Response: Send;
}

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
    ) -> Pin<Box<dyn Future<Output = Result<T::Response, Self::Error>>>>;
}
