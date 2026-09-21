# vendored picky 7.0.0-rc.25

来源：crates.io `picky` 7.0.0-rc.25（Devolutions/picky-rs），经 `src-tauri/Cargo.toml`
的 `[patch.crates-io]` 生效，替代 registry 版本。

## 为什么 vendor

`ironrdp-connector` 0.10 精确钉住 `picky = "=7.0.0-rc.25"`（连 IronRDP master 也是如此），
而 rc.25 把一整套 RustCrypto 依赖钉在 pre-release 上（`curve25519-dalek =5.0.0-rc.1`、
`ed25519-dalek =3.0.0-rc.1`、`p256/p384/p521 =0.14.0-rc.14` 等），与本仓库已有的
正式版（curve25519-dalek 5.0.0、ed25519-dalek 3.0.0，来自 russh 等）无法统一解析。

上游 rc.26 正是做这件事的：放宽这些 pin 到正式版。经核对 rc.25 与 rc.26 的 `src/`
**完全相同**，仅 Cargo.toml 的依赖声明有差异，因此本副本保留 `version = "7.0.0-rc.25"`
（满足 connector 的精确 pin），仅移植 rc.26 的依赖放宽。

## 补丁点（相对 crates.io rc.25，全部在 Cargo.toml，未动任何源码）

- 放宽：`ed25519-dalek =3.0.0-rc.1 → "3"`、`x25519-dalek =3.0.0-rc.1 → "3"`、
  `p256/p384/p521 =0.14.0-rc.14 → "0.14"`、`aes-gcm =0.11.0-rc.4 → "0.11"`。
- 删除死依赖（rc.26 同样删除，且 rc.25 源码未引用）：
  `curve25519-dalek`、`ecdsa`、`primeorder`、`rustcrypto-ff`、`rustcrypto-ff_derive`、`rustcrypto-group`。
- 保留不变：`rsa =0.10.0-rc.18`、`pkcs1 =0.8.0-rc.4`（本仓库锁文件本就是这些版本）、
  `argon2`/`blake2` 的 rc pin（仅在 `putty` 可选 feature 下启用，connector 不使用）。
- 新增 `[lints.rust] unused_imports = "allow"`：上游源码在 `signature.rs` 顶层有
  一处多余的 `SignatureEncoding as _` 导入，作为 path patch 编译时告警会透传到
  本仓库构建输出；为保持 src/ 与上游逐字节一致，在清单层静默而不改源码。

## 升级注意

IronRDP 升级 connector 到依赖 `picky rc.26+`（或 7.0.0 正式版）后，删除本目录与
`[patch.crates-io]` 中的 `picky` 条目即可。
