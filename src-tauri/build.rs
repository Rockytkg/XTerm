fn main() {
    // MSVC 链接器默认在 exe 中写入指向 PDB 的 CodeView 调试目录（RSDS），
    // 即使 release profile 已 strip 符号也是如此。仅在 release 下关闭它，
    // 避免发布产物携带调试信息引用；debug 构建保留 PDB 以便开发调试。
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if profile == "release" && target_env == "msvc" {
        println!("cargo:rustc-link-arg=/DEBUG:NONE");
    }

    println!("cargo:rerun-if-changed=vendor/libtelnet/libtelnet.c");
    println!("cargo:rerun-if-changed=vendor/libtelnet/libtelnet.h");
    println!("cargo:rerun-if-changed=vendor/libtelnet/xterm_telnet_shim.c");
    cc::Build::new()
        .file("vendor/libtelnet/libtelnet.c")
        .file("vendor/libtelnet/xterm_telnet_shim.c")
        .include("vendor/libtelnet")
        .warnings(false)
        .compile("xterm_libtelnet");
    tauri_build::build()
}
