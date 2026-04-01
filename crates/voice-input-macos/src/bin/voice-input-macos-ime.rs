#![allow(unexpected_cfgs)]

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("这个入口只支持 macOS。");
}

#[cfg(target_os = "macos")]
fn main() {
    use cocoa::base::nil;
    use cocoa::foundation::NSString;
    use objc::runtime::Class;
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        voice_input_macos::register_input_controller_class();

        let bundle_id = NSString::alloc(nil).init_str("com.example.voiceinput.inputmethod");
        let connection_name =
            NSString::alloc(nil).init_str("com.example.voiceinput.inputmethod_Connection");

        let imk_server_class = Class::get("IMKServer").expect("加载 IMKServer");
        let server: *mut objc::runtime::Object = msg_send![imk_server_class, alloc];
        let _server: *mut objc::runtime::Object =
            msg_send![server, initWithName: connection_name bundleIdentifier: bundle_id];

        println!("VoiceInput 系统级入口已启动");
        println!("bundle_id=com.example.voiceinput.inputmethod");
        println!("connection_name=com.example.voiceinput.inputmethod_Connection");
        println!("controller_class=VoiceInputInputController");

        if let Err(err) = voice_input_macos::run_live_app(voice_input_macos::MacLiveAppConfig {
            show_status_item: false,
            commit_backend: voice_input_macos::MacCommitBackend::InputMethodKit,
            ..Default::default()
        }) {
            eprintln!("实时输入链路启动失败：{err}");
            std::process::exit(1);
        }
    }
}
