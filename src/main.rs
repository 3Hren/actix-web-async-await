#![feature(async_await)]
#![feature(await_macro)]
#![feature(arbitrary_self_types)]

#[macro_use]
extern crate serde_derive;

use std::{future::Future, marker::PhantomData};

use futures::{self, future::FutureResult, Async, Future as PrevFuture, IntoFuture, Poll};
use futures_util::{
    compat::{Compat, Future01CompatExt},
    future::FutureExt,
};

use actix_service::{NewService, Service};
use actix_web::{
    dev::{ServiceRequest, ServiceResponse},
    web::{self, Data, Json},
    App, Error, FromRequest, HttpResponse, HttpServer, Responder,
};

// ================================================================================================

pub fn to_async<H, T, F, U>(handler: H) -> AsyncHandler<H, T, F, U>
where
    H: AsyncFactory<T, F, U>,
    T: FromRequest,
    F: Future<Output = U>,
    U: Responder,
{
    AsyncHandler::new(handler)
}

// ================================================================================================

/// Async handler converter factory
pub trait AsyncFactory<T, F, U>: Clone + 'static
where
    F: Future<Output = U>,
    U: Responder,
{
    fn call(&self, param: T) -> F;
}

impl<H, F, U> AsyncFactory<(), F, U> for H
where
    H: Fn() -> F + Clone + 'static,
    F: Future<Output = U>,
    U: Responder,
{
    fn call(&self, (): ()) -> F {
        (self)()
    }
}

impl<H, U0, F, U> AsyncFactory<(U0,), F, U> for H
where
    H: Fn(U0) -> F + Clone + 'static,
    F: Future<Output = U>,
    U: Responder,
{
    fn call(&self, args: (U0,)) -> F {
        (self)(args.0)
    }
}

impl<H, U0, U1, F, U> AsyncFactory<(U0, U1), F, U> for H
where
    H: Fn(U0, U1) -> F + Clone + 'static,
    F: Future<Output = U>,
    U: Responder,
{
    fn call(&self, args: (U0, U1)) -> F {
        (self)(args.0, args.1)
    }
}

// ================================================================================================

pub struct AsyncHandler<H, T, F, U>
where
    T: 'static,
    H: AsyncFactory<T, F, U>,
    F: Future<Output = U> + 'static,
    U: Responder + 'static,
{
    handler: H,
    _phantom: PhantomData<(T, F, U)>,
}

impl<H, T, F, U> AsyncHandler<H, T, F, U>
where
    T: FromRequest + 'static,
    H: AsyncFactory<T, F, U>,
    F: Future<Output = U> + 'static,
    U: Responder + 'static,
{
    pub fn new(handler: H) -> Self {
        AsyncHandler { handler, _phantom: PhantomData }
    }

    pub async fn execute(handler: H, req: ServiceRequest) -> Result<ServiceResponse, Error> {
        let (req, mut payload) = req.into_parts();

        let request = T::from_request(&req, &mut payload).into_future().compat().await.map_err(Into::into)?;
        let resp = handler.call(request).await;

        match resp.respond_to(&req).into_future().compat().await {
            Ok(resp) => {
                Ok(ServiceResponse::new(req, resp))
            }
            Err(err) => Err(err.into()),
        }
    }
}

impl<H, T, F, U> Clone for AsyncHandler<H, T, F, U>
where
    T: 'static,
    H: AsyncFactory<T, F, U>,
    F: Future<Output = U> + 'static,
    U: Responder + 'static,
{
    fn clone(&self) -> Self {
        AsyncHandler {
            handler: self.handler.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<H, T, F, U> Service for AsyncHandler<H, T, F, U>
where
    T: FromRequest + 'static,
    H: AsyncFactory<T, F, U>,
    F: Future<Output = U> + 'static,
    U: Responder + 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse;
    type Error = Error;
    type Future = Box<dyn PrevFuture<Item = ServiceResponse, Error = Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        Ok(Async::Ready(()))
    }

    fn call(&mut self, req: ServiceRequest) -> Self::Future {
        Box::new(Compat::new(AsyncHandler::execute(self.handler.clone(), req).boxed_local()))
    }
}

impl<H, T, F, U> NewService for AsyncHandler<H, T, F, U>
where
    T: FromRequest + 'static,
    H: AsyncFactory<T, F, U>,
    F: Future<Output = U> + 'static,
    U: Responder + 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse;
    type Error = Error;
    type Config = ();
    type Service = AsyncHandler<H, T, F, U>;
    type InitError = ();
    type Future = FutureResult<Self::Service, Self::InitError>;

    fn new_service(&self, (): &Self::Config) -> Self::Future {
        futures::future::ok(self.clone())
    }
}

// ================================================================================================
// ================================================================================================
// ================================================================================================
// ================================================================================================

#[derive(Debug, Deserialize)]
struct InfoRequest {
    name: String,
}

#[derive(Serialize)]
struct InfoResponse {}

#[derive(Clone)]
struct Server {
    id: u64,
}

impl Server {
    pub async fn ping() -> HttpResponse {
        HttpResponse::Ok().finish()
    }

    pub async fn info(self: Data<Self>, request: Json<InfoRequest>) -> Result<Json<InfoResponse>, Error> {
        dbg!(&self.id);
        dbg!(&request);

        Ok(Json(InfoResponse {}))
    }
}

// ================================================================================================

fn main() -> std::io::Result<()> {
    let server = Server { id: 42 };

    HttpServer::new(move || {
        let server = server.clone();
        App::new().data(server).service(
            web::scope("v1")
                .service(web::service("/ping").finish(to_async(Server::ping)))
                .service(web::service("/info").finish(to_async(Server::info)))
        )
    })
    .bind("127.0.0.1:8080")?
    .run()
}
