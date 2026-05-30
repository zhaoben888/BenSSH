fn main() {
    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        // 将自定义皮肤彻底烧录进 exe 的底层 PE 结构中
        res.set_icon("benssh.ico");
        res.compile().unwrap();
    }
}
