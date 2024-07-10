mod config_extractor;

use screen_service::screen_service_client::ScreenServiceClient;
use screen_service::ScreenContentRequest;
use tonic::transport::Endpoint;

pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = config_extractor::cli().get_matches();
    let config = config_extractor::extract_config(&matches).expect("Error reading config");
    let server_config = config.server.expect("No server config found");
    let address: Endpoint = format!("http://{}:{}", server_config.address, server_config.port).parse()?;

    let mut client = ScreenServiceClient::connect(address).await?;

    let request = tonic::Request::new(ScreenContentRequest {});

    let response = client.get_screen_content(request).await?;

    println!("RESPONSE={:?}", response);

    Ok(())
}
