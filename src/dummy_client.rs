use crate::config_extractor::api_config::ApiConfig;
use screen_service::screen_service_client::ScreenServiceClient;
use screen_service::ScreenContentRequest;
use tokio::task::JoinHandle;
use log::info;

pub mod screen_service {
    tonic::include_proto!("screen_service"); // The string specified here must match the proto package name
}

pub fn maybe_dummy_client(start_dummy: bool, api_config: &ApiConfig) -> JoinHandle<()> {
    if !start_dummy {
        return tokio::task::spawn(async { () });
    }
    let config_copy = api_config.clone();
    tokio::spawn(async move {
        info!("Dummy client spawned and running");
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let server_config = config_copy.server.expect("No server config found");
        let address: tonic::transport::Endpoint =
            format!("http://{}:{}", server_config.address, server_config.port)
                .parse()
                .expect("Couldn't parse server config into an address");
        let mut client = ScreenServiceClient::connect(address)
            .await
            .expect("Couldn't start dummy client");
        let request = tonic::Request::new(ScreenContentRequest {});
        let response = client
            .get_screen_content(request)
            .await
            .expect("Couldn't get server reply");

        info!("Response to dummy client:\n{:?}", response);
    })
}