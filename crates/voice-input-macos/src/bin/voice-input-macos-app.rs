#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("这个入口只支持 macOS。");
}

#[cfg(target_os = "macos")]
fn main() {
    if let Err(err) =
        voice_input_macos::run_live_app(voice_input_macos::MacLiveAppConfig::default())
    {
        eprintln!("macOS 常驻应用启动失败：{err}");
        std::process::exit(1);
    }
}
