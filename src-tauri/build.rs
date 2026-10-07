fn main() {
    // MSVC 链接器默认在 exe 中写入指向 PDB 的 CodeView 调试目录（RSDS），
    // 即使 release profile 已 strip 符号也是如此。仅在 release 下关闭它，
    // 避免发布产物携带调试信息引用；debug 构建保留 PDB 以便开发调试。
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if profile == "release" && target_env == "msvc" {
        println!("cargo:rustc-link-arg=/DEBUG:NONE");
    }

    tauri_build::build();

    // generate_context! 在编译期遍历 frontendDist 并逐个 include_bytes!，cargo 只跟踪
    // 上一次编译实际读过的文件。若某次编译撞上 vite build 清空后尚未写全的 dist（典型：
    // pnpm release 编译期间并行跑了 pnpm build），残缺资源会被静默嵌入，运行时报
    // asset not found: index.html，且之后 index.html 的变化不会触发重编译、无法自愈。
    // 非 dev 构建（tauri build）时校验产物完整性并把入口纳入变更跟踪，让残缺状态在
    // 构建开始时直接报错，而不是产出一个打开即崩溃的安装包。
    if !tauri_build::is_dev() {
        let dist_dir = std::path::Path::new("../dist");
        let index_html = dist_dir.join("index.html");
        println!("cargo:rerun-if-changed={}", dist_dir.display());
        println!("cargo:rerun-if-changed={}", index_html.display());
        assert!(
            index_html.is_file() && dist_dir.join("assets").is_dir(),
            "前端产物不完整（{} 或 dist/assets 缺失）：tauri build 会把 dist 整体嵌入二进制。\
             请先运行 pnpm build，且不要在 pnpm release 编译期间并行运行 pnpm build",
            index_html.display()
        );
    }
}
