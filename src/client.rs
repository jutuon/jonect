//! Test client

use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpStream, runtime::Runtime};

use crate::{config::TestClientConfig, server::device::protocol::ServerMessage};

use crate::server::device::{protocol::{ClientInfo, ClientMessage, ProtocolDeserializer}};

use std::convert::TryInto;

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

        let message = ClientMessage::ClientInfo(ClientInfo::new("test"));

        let data = serde_json::to_vec(&message).unwrap();
        let data_len: i32 = data
            .len()
            .try_into()
            .unwrap();

        stream.write_all(&data_len.to_be_bytes()).await.unwrap();
        stream.write_all(&data).await.unwrap();

        loop {
            let message_len = stream.read_i32().await.unwrap();
            if message_len.is_negative() {
                panic!("message_len.is_negative()");
            }

            let mut deserializer = ProtocolDeserializer::new();
            let message: ServerMessage = deserializer
                .read_server_message(&mut stream, message_len)
                .await
                .unwrap();

            println!("{:?}", message);
        }

    }
}
