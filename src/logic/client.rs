//! Test client

use tokio::{net::TcpStream, runtime::Runtime, io::AsyncReadExt};

use crate::config::TestClientConfig;

use super::server::device::protocol::{ProtocolDeserializer, ServerInfo};


pub struct TestClient {
    config: TestClientConfig,
}



impl TestClient {
    pub fn new(config: TestClientConfig) -> Self {
        TestClient {
            config
        }
    }

    pub fn run(self) {
        let rt = Runtime::new().unwrap();

        let server = AsyncClient::new(self.config);

        rt.block_on(server.run());
    }
}


struct AsyncClient {
    config: TestClientConfig,
}

impl AsyncClient {
    pub fn new(config: TestClientConfig) -> Self {
        Self {
            config,
        }
    }

    pub async fn run(self) {
        let mut stream = TcpStream::connect(self.config.address).await.unwrap();

        loop {
            let message_len = stream.read_u32().await.unwrap();
            if message_len > i32::max_value() as u32 {
                panic!("message_len > i32::max_value()");
            }

            let mut deserializer = ProtocolDeserializer::new();
            let message: ServerInfo = deserializer
                .read_async(&mut stream, message_len)
                .await
                .unwrap();

            println!("{:?}", message);
        }

    }
}
