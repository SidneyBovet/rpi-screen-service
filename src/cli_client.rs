mod config_extractor;
mod dummy_client;

use crate::config_extractor::{cli, extract_config};
use crate::dummy_client::maybe_dummy_client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = cli().get_matches();
    let config = extract_config(&matches).expect("Error reading config");

    maybe_dummy_client(true, &config).await?;
    Ok(())
}
