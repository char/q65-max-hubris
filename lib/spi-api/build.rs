fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    idol::client::build_client_stub("../../idl/spi.idol", "client_stub.rs")
}
