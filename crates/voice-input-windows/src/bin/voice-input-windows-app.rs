#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("这个入口只支持 Windows。");
}

#[cfg(target_os = "windows")]
fn main() {
    if let Err(err) =
        voice_input_windows::run_live_app(voice_input_windows::WindowsLiveAppConfig::default())
    {
        eprintln!("Windows 常驻应用启动失败：{err}");
        std::process::exit(1);
    }
}
