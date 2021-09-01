/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::net::{SocketAddr, ToSocketAddrs};

use clap::{App, Arg, SubCommand};

pub const EVENT_CHANNEL_SIZE: usize = 32;


#[derive(Debug, Clone)]
pub struct TestClientConfig {
    pub address: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub test: bool,
    pub pa_source_name: Option<String>,
    pub test_client_config: Option<TestClientConfig>,
    pub gui: Option<()>,
}

/// Parse command line args. Program may exit when running this.
pub fn parse_cmd_args() -> Config {
    let matches = App::new("Multidevice Server")
        .arg(
            Arg::with_name("test")
                .short("t")
                .long("test")
                .help("Print 'test' and close the program."),
        )
        .arg(
            Arg::with_name("pa-source-name")
                .short("s")
                .long("pa-source-name")
                .help("Name of PulseAudio audio source. The program will start recording the specified audio source.")
                .takes_value(true),
        )
        .subcommand(SubCommand::with_name("client")
            .about("Run the program in command line test client mode.")
            .arg(Arg::with_name("server-address")
                .help("Multidevice server address and port number. Example: 'localhost:8080'")
                .short("a")
                .long("server-address")
                .required(true)
                .takes_value(true)))
        .subcommand(SubCommand::with_name("gui")
                .about("Run Gtk GUI application for controlling the server."))
        .get_matches();

    let pa_source_name = matches.value_of("pa-source-name").map(|s| s.to_owned());

    let test_client_config = matches.subcommand_matches("client").map(|args| {
        let address = args
            .value_of("server-address")
            .unwrap()
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        TestClientConfig {
            address,
        }
    });

    let gui = matches.subcommand_matches("gui").map(|_| ());

    Config {
        test: matches.is_present("test"),
        pa_source_name,
        test_client_config,
        gui,
    }
}
