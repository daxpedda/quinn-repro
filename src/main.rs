use std::sync::Arc;

use anyhow::{Error, Result};
use futures_util::{StreamExt, TryStreamExt};
use quinn::{
    Certificate, CertificateChain, ClientConfig, Endpoint, NewConnection, PrivateKey, ServerConfig,
};
use tokio::sync::Notify;
use tracing::{info, Level};

#[tokio::main]
async fn main() -> Result<()> {
    let file = std::fs::File::create("log.txt")?;
    let (non_blocking, _guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_max_level(Level::TRACE)
        .init();

    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let public_key = Certificate::from_der(&certificate.serialize_der()?)?;
    let private_key = PrivateKey::from_der(&certificate.serialize_private_key_der())?;

    loop {
        info!("start!");

        let notified = Arc::new(Notify::new());
        let notifier = Arc::clone(&notified);

        let server = {
            let public_key = public_key.clone();
            let private_key = private_key.clone();

            tokio::spawn(async move {
                let mut server = Endpoint::builder();
                let mut server_config = ServerConfig::default();
                server_config
                    .certificate(CertificateChain::from_certs(vec![public_key]), private_key)?;
                server.listen(server_config);

                info!("did we get stuck?");
                let (endpoint, mut incoming) = server.bind(&"[::1]:5000".parse()?)?;
                info!("we didn't!");

                notifier.notify_one();

                let connecting = incoming.next().await.unwrap();
                let NewConnection {
                    mut uni_streams, ..
                } = connecting.await?;
                let receiver = uni_streams.try_next().await?.unwrap();
                let message = receiver.read_to_end(4096).await?;
                assert_eq!(&[0; 4096], message.as_slice());

                // uncomment this to prevent the program from getting stuck!
                //endpoint.wait_idle().await;
                info!("server finish!");
                Result::<_, Error>::Ok(())
            })
        };

        notified.notified().await;

        let client = {
            let public_key = public_key.clone();

            tokio::spawn(async move {
                let mut client = Endpoint::builder();
                let mut client_config = ClientConfig::default();
                client_config.add_certificate_authority(public_key)?;
                client.default_client_config(client_config);
                let (endpoint, _) = client.bind(&"[::1]:5001".parse()?)?;

                let NewConnection { connection, .. } = endpoint
                    .connect(&"[::1]:5000".parse()?, "localhost")?
                    .await?;
                let mut sender = connection.open_uni().await?;
                sender.write_all(&[0; 4096]).await?;
                sender.finish().await?;

                // uncomment this to prevent the program from getting stuck!
                //endpoint.wait_idle().await;
                info!("client finish!");
                Result::<_, Error>::Ok(())
            })
        };

        client.await??;
        server.await??;

        info!("finish!");
    }
}
