use ledger_zemu::ZemuGrpcServer;

fn main() {
    env_logger::init();
    let zemu_http_host = "127.0.0.1".into();
    let zemu_http_port = 9998;
    let server = ZemuGrpcServer::new(3002, zemu_http_host, zemu_http_port);
    server.run();
}
