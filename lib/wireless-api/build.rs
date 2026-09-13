fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    idol::client::build_client_stub("../../idl/wireless.idol", "client_stub.rs")
}
