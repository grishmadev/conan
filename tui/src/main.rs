use conanprotocol::init;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt::init();
    let result = init(("ifconfig.me".into(), 80)).await?;
    println!("res: {result:#?}");
    Ok(())
}
