use self::multiplex_service::MultiplexService;
use axum::body::Bytes;
use axum::{routing::get, Router};
use proto::{
    greeter_server::{Greeter, GreeterServer},
    HelloReply, HelloRequest,
};
use std::net::SocketAddr;
use std::time::Duration;
use tonic::{Response as TonicResponse, Status};
use tower::ServiceBuilder;
use tower_http::{
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod multiplex_service;

mod proto {
    tonic::include_proto!("helloworld");
}

#[derive(Default)]
struct GrpcServiceImpl {}

#[tonic::async_trait]
impl Greeter for GrpcServiceImpl {
    async fn say_hello(
        &self,
        request: tonic::Request<HelloRequest>,
    ) -> Result<TonicResponse<HelloReply>, Status> {
        tracing::info!("Got a gRPC request");

        let reply = HelloReply {
            message: format!("Hello {}!", request.into_inner().name),
        };

        Ok(TonicResponse::new(reply))
    }
}

async fn web_root() -> &'static str {
    tracing::info!("Got a REST request");
    "Hello, World!"
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "example_rest_grpc_multiplex=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let rest = Router::new().route("/", get(web_root))
        .layer(
            TraceLayer::new_for_http()
                .on_body_chunk(|chunk: &Bytes, latency: Duration, _: &tracing::Span| {
                    tracing::trace!(size_bytes = chunk.len(), latency = ?latency, "REST sending body chunk")
                })
                .make_span_with(DefaultMakeSpan::new().include_headers(true))
                .on_response(DefaultOnResponse::new().include_headers(true).latency_unit(LatencyUnit::Micros)),
        );

    let grpc = ServiceBuilder::new()
        .layer(TraceLayer::new_for_grpc()
                   .on_body_chunk(|chunk: &Bytes, latency: Duration, _: &tracing::Span| {
            tracing::trace!(size_bytes = chunk.len(), latency = ?latency, "gRPC sending body chunk")
        })
                   .make_span_with(DefaultMakeSpan::new().include_headers(true))
                   .on_response(DefaultOnResponse::new().include_headers(true).latency_unit(LatencyUnit::Micros))
        )
        .service(GreeterServer::new(GrpcServiceImpl::default()));

    // combine them into one service
    let multiplex_service = MultiplexService::new(rest, grpc);

    let service = ServiceBuilder::new().service(multiplex_service);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(tower::make::Shared::new(service))
        .await
        .unwrap();
}
