# vendored sspi 0.21.3

来源：crates.io `sspi` 0.21.3（Devolutions/sspi-rs），经 `src-tauri/Cargo.toml`
的 `[patch.crates-io]` 生效，替代 registry 版本。`ironrdp-connector` 0.10 依赖
`sspi = "0.21"`（CredSSP/NTLM 用于 RDP NLA 认证）。

## 为什么 vendor

sspi 0.21.x 在 macOS/iOS target 段落里钉了一串 RustCrypto pre-release 依赖
（`curve25519-dalek =5.0.0-rc.1`、`ed25519-dalek =3.0.0-rc.1`、`p256/p384/p521
=0.14.0-rc.14`、`primeorder`/`rustcrypto-ff`/`rustcrypto-group` 等）。这些 pin
会参与全平台依赖解析，与本仓库已有的正式版（russh 的 curve25519-dalek 5.0.0、
ed25519-dalek 3.0.0 等）无法统一。

经 grep 核实，sspi 0.21.3 的 `src/` **完全没有引用这些 crate**（纯传递性版本
钳制残留，上游 0.22.0 已整组删除），因此本副本直接删除这些死依赖段落。

## 补丁点（相对 crates.io 0.21.3，全部在 Cargo.toml，未动任何源码）

删除 macOS/iOS target 段落中的死依赖：`curve25519-dalek`、`ed25519-dalek`、
`p256`、`p384`、`p521`、`pkcs1`、`primeorder`、`rustcrypto-ff`、
`rustcrypto-ff_derive`、`rustcrypto-group`。保留 `async-dnssd` 等实际使用的依赖。

## 升级注意

`ironrdp-connector` 升级到依赖 `sspi 0.22+`（已删除这些 pin）后，删除本目录与
`[patch.crates-io]` 中的 `sspi` 条目即可。
