use std::env;

fn main() {
    let (audio_file, backend) =
        match voice_input_runtime::parse_audio_file_with_optional_backend_arg(
            env::args().collect(),
            voice_input_linux::LinuxBackendKind::IBus,
            voice_input_linux::parse_backend_kind,
        ) {
            Ok(args) => args,
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

    if let Err(message) = voice_input_linux::run_smoke(audio_file, backend) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!(
        "用法：cargo run -p voice-input-linux --features ibus -- --audio-file /path/to/audio.wav [--backend ibus|fcitx5]"
    );
}
