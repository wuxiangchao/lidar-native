
fn main() {
    // 仅在目标操作系统为 Windows 时才执行
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        // 设置应用程序的图标
        res.set_icon("assets/logo.ico");
        // 编译并链接资源
        if let Err(e) = res.compile() {
            eprintln!("Failed to compile Windows resource: {}", e);
        }
    }
}