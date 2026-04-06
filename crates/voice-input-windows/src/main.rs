use std::env;

fn main() {
    let audio_file = match voice_input_runtime::parse_required_audio_file_arg(env::args().collect())
    {
        Ok(path) => path,
        Err(message) => {
            if message == "help" {
                print_usage();
                std::process::exit(0);
            }
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    if let Err(message) = voice_input_windows::run_smoke(audio_file) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!("用法：cargo run -p voice-input-windows -- --audio-file /path/to/audio.wav");
}
