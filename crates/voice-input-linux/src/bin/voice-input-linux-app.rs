use std::env;

use voice_input_linux::{parse_live_args, print_live_usage, run_live_with_args};

fn main() {
    let args = match parse_live_args(env::args().collect()) {
        Ok(args) => args,
        Err(message) => {
            if message == "help" {
                print_live_usage();
                std::process::exit(0);
            }
            eprintln!("{message}");
            print_live_usage();
            std::process::exit(2);
        }
    };

    if let Err(err) = run_live_with_args(args) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
