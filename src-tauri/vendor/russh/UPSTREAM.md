# Vendored russh

- 上游：<https://github.com/Eugeny/russh>，版本 0.61.2（crates.io 副本原样复制）。
- 引入方式：`src-tauri/Cargo.toml` 的 `[patch.crates-io]` 指向本目录，对所有依赖方
  （包括 russh-sftp）生效。

## 本地补丁（均标注 `PATCH(xterm)` 注释）

目的：兼容行为不规范的旧交换机 / 服务器（发送 GBK 编码文本、name-list 尾部逗号等）。
russh 对 SSH 字符串做严格 UTF-8 解码、对 name-list 条目做严格 RFC 4251 §5 校验，
遇到这类设备会以 `SshEncoding: character encoding invalid` 直接断连。补丁参照
OpenSSH 的宽容策略（`match.c` 的 `match_list` 静默跳过空条目），条目仅用于算法匹配，
垃圾条目永远不会被选中；GHSA-4r3c-5hpg-58qr 引入的长度/数量上限全部保留。

- `src/helpers.rs`：`NameList::from_encoded_string` 跳过空条目与非 ASCII 条目；
  `NameList::decode` 改为读取原始字节后有损转换（因此删除了仅剩这一处用途的
  `LimitedString`）。
- `src/negotiation.rs`：KEXINIT 中仅作信息用途且被丢弃的 languages 字段、
  cipher server-to-client 字段按字节串读取跳过，不再做 UTF-8 校验。
- `src/client/encrypted.rs`：新增 `decode_string_lossy` 辅助函数，应用于
  `USERAUTH_BANNER` banner、keyboard-interactive 的 name/instructions/prompt、
  EXT_INFO 扩展名、CHANNEL_OPEN_FAILURE 描述、CHANNEL_REQUEST / GLOBAL_REQUEST
  请求名、exit-signal 的信号名/错误信息/语言标签、userauth_pk_ok 算法名。
- `src/client/mod.rs`：`process_disconnect` 的断开原因与语言标签有损解码。

上游 main 分支仍为严格解码，升级 russh 版本时需同步移植上述补丁。
