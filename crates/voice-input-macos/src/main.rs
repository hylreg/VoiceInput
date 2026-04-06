use std::env;

fn main() {
    let audio_path = match voice_input_runtime::parse_required_audio_file_arg(env::args().collect())
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

    if let Err(message) = voice_input_macos::run_smoke(audio_path) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!("用法：uv run -- cargo run -p voice-input-macos -- --audio-file /path/to/audio.wav");
}
