use clap::{App, Arg};

pub struct Config {
    pub test: bool,
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
        .get_matches();

    Config {
        test: matches.is_present("test"),
    }
}
