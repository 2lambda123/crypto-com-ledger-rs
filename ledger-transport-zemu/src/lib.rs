mod errors;
mod zemu;
mod zemu_grpc;

use grpc::prelude::*;
use grpc::ClientConf;
use ledger_apdu::{APDUAnswer, APDUCommand};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};

use zemu::ExchangeRequest;
use zemu_grpc::ZemuCommandClient;

use crate::zemu::ExchangeReply;
use crate::zemu_grpc::{ZemuCommand, ZemuCommandServer};
pub use errors::LedgerZemuError;

pub struct TransportZemuGrpc {
    client: ZemuCommandClient,
}

impl TransportZemuGrpc {
    pub fn new(host: &str, port: u16) -> Result<Self, LedgerZemuError> {
        let client = ZemuCommandClient::new_plain(host, port, ClientConf::new())
            .map_err(|_| LedgerZemuError::ConnectError)?;
        Ok(Self { client })
    }

    pub async fn exchange(&self, command: &APDUCommand) -> Result<APDUAnswer, LedgerZemuError> {
        let mut request = ExchangeRequest::new();
        request.set_command(command.serialize());
        let response: ExchangeReply = self
            .client
            .exchange(grpc::RequestOptions::new(), request)
            .drop_metadata()
            .await
            .map_err(|e| {
                log::error!("grpc response error: {:?}", e);
                LedgerZemuError::ResponseError
            })?;
        Ok(APDUAnswer::from_answer(response.reply))
    }
}

pub struct ZemuGrpcServer {
    grpc_port: u16,
    zemu_host: String,
    zemu_port: u16,
}

/// http request to zemu
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
struct ZemuRequest {
    apdu_hex: String,
}

/// http response from zemu
#[derive(Deserialize, Debug, Clone)]
struct ZemuResponse {
    data: String,
    error: Option<String>,
}

struct GrpcServerImpl {
    zemu_host: String,
    zemu_port: u16,
}

impl GrpcServerImpl {
    pub fn new(zemu_host: String, zemu_port: u16) -> Self {
        Self {
            zemu_host,
            zemu_port,
        }
    }

    pub fn zemu_url(&self) -> String {
        format!("http://{}:{}", self.zemu_host, self.zemu_port)
    }
}

impl ZemuCommand for GrpcServerImpl {
    fn exchange(
        &self,
        _: ::grpc::ServerHandlerContext,
        req: ::grpc::ServerRequestSingle<ExchangeRequest>,
        grpc_resp: ::grpc::ServerResponseUnarySink<ExchangeReply>,
    ) -> ::grpc::Result<()> {
        let message: ExchangeRequest = req.message;
        let command = message.get_command();
        log::debug!("get exchange request: {:?}", command);
        let raw_command = hex::encode(command);
        let request = ZemuRequest {
            apdu_hex: raw_command,
        };
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let zemu_url = self.zemu_url();
        log::debug!("zemu url: {}", zemu_url);
        let client = HttpClient::new();
        let mut resp = client
            .post(&zemu_url)
            .headers(headers)
            .json(&request)
            .send()
            .map_err(|e| {
                log::error!("create http client error: {:?}", e);
                grpc::Error::Other("send zemu request error")
            })?;
        log::debug!("zemu http response: {:?}", resp);

        if resp.status().is_success() {
            let result: ZemuResponse = resp.json().map_err(|e| {
                log::error!("error response: {:?}", e);
                grpc::Error::Other("zemu response error")
            })?;
            if result.error.is_none() {
                let mut reply = ExchangeReply::new();
                reply.set_reply(hex::decode(result.data).expect("decode error"));
                grpc_resp.finish(reply)
            } else {
                Err(grpc::Error::Other("http response error"))
            }
        } else {
            Err(grpc::Error::Other("http status error"))
        }
    }
}

impl ZemuGrpcServer {
    pub fn new(grpc_port: u16, zemu_host: String, zemu_port: u16) -> Self {
        Self {
            grpc_port,
            zemu_host,
            zemu_port,
        }
    }

    pub fn run(&self) {
        let grpc_server_impl = GrpcServerImpl::new(self.zemu_host.clone(), self.zemu_port);
        let service_def = ZemuCommandServer::new_service_def(grpc_server_impl);
        let mut server_builder = grpc::ServerBuilder::new_plain();
        server_builder.add_service(service_def);
        server_builder.http.set_port(self.grpc_port);
        let _server = server_builder.build().expect("build");
        loop {
            std::thread::park()
        }
    }
}
