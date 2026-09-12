fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    build_util::build_notifications()?;
    idol::server::build_server_support(
        "../../idl/usb.idol",
        "server_stub.rs",
        idol::server::ServerStyle::InOrder,
    )
}
