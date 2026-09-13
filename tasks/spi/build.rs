fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    build_util::build_notifications()?;
    let index = build_util::task_ids()
        .get("wireless")
        .expect("missing wireless task");
    std::fs::write(
        build_util::out_dir().join("caller.rs"),
        format!("const WIRELESS_INDEX: usize = {index};"),
    )?;
    idol::server::build_server_support(
        "../../idl/spi.idol",
        "server_stub.rs",
        idol::server::ServerStyle::InOrder,
    )
}
