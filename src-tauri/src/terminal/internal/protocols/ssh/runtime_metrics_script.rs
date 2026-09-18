//! 远端运行时指标采样脚本（单通道流式协议）。
//!
//! 主流 agentless 监控（Termius、ServerCat、electerm 等）的远端采集都遵循
//! "一条长生命周期 exec 通道 + 远端进程自循环推送"的模型，本脚本即该模型的实现：
//! 远端只启动一个 `sh` 进程，进入 `采样 → 输出分隔符 → sleep` 的循环，
//! Rust 侧按 [`SAMPLE_BLOCK_DELIMITER`] 增量切分采样块。
//!
//! 相比"每次采样新开一条 exec 通道"的旧实现：
//! - 采样周期不再包含通道/PAM/shell 启动开销，间隔精确等于 [`SAMPLE_INTERVAL_SECONDS`]；
//! - 弱设备（单通道交换机、嵌入式 busybox）只承受一次 exec 探测；
//! - sshd 日志（`/var/log/auth.log`、`lastlog`）不再每 2 秒新增一条 session 记录。
//!
//! 脚本必须是 POSIX sh 兼容（busybox ash / dash / 交换机 CLI 受限 shell），
//! 且不得包含单引号（整体经 `sh -lc '...'` 投递）。

/// 采样间隔（秒），通过环境变量注入远端脚本，Rust 侧是唯一事实来源。
/// 1s 是主流实时监控的通用档位（btop / Windows 任务管理器默认档）：
/// 单次采样只是几次 /proc 读取加一次 awk 聚合，远端 CPU 开销可忽略。
pub(super) const SAMPLE_INTERVAL_SECONDS: u64 = 1;
/// 每 N 个采样块附带一次高成本明细（inode、进程/线程数）。
pub(super) const DETAIL_SAMPLE_EVERY: u32 = 5;
/// 采样块结束分隔符。取值保证不会出现在任何 key=value 输出行里，
/// Rust 侧按它切分流；若远端输出被截断，残余块会因缺少分隔符而被丢弃。
pub(super) const SAMPLE_BLOCK_DELIMITER: &str = "@@xterm-metrics-block-end@@";

pub(super) fn runtime_metrics_stream_command() -> String {
    format!(
        "XTERM_METRICS_INTERVAL={} XTERM_METRICS_DETAIL_EVERY={} XTERM_METRICS_DELIMITER='{}' LC_ALL=C sh -lc '{}'",
        SAMPLE_INTERVAL_SECONDS,
        DETAIL_SAMPLE_EVERY,
        SAMPLE_BLOCK_DELIMITER,
        RUNTIME_METRICS_STREAM_SCRIPT
    )
}

pub(super) fn runtime_metrics_script_version() -> &'static str {
    "2026-09-18.3"
}

const RUNTIME_METRICS_STREAM_SCRIPT: &str = r#"
xterm_metrics_sample() {
  if [ -r /proc/stat ]; then
    read cpu user nice system idle iowait irq softirq steal rest < /proc/stat
    echo "cpu_user=$user"
    echo "cpu_nice=$nice"
    echo "cpu_system=$system"
    echo "cpu_idle=$idle"
    echo "cpu_iowait=$iowait"
    echo "cpu_irq=$irq"
    echo "cpu_softirq=$softirq"
    echo "cpu_steal=$steal"
  elif command -v sysctl >/dev/null 2>&1; then
    set -- $(sysctl -n kern.cp_time 2>/dev/null)
    if [ "$#" -ge 5 ]; then
      echo "cpu_user=$1"
      echo "cpu_nice=$2"
      echo "cpu_system=$3"
      echo "cpu_irq=$4"
      echo "cpu_idle=$5"
    fi
  fi
  if [ -r /proc/meminfo ]; then
    awk "
      /^MemTotal:/ { print \"memory_total_kib=\" \$2 }
      /^MemFree:/ { print \"memory_free_kib=\" \$2 }
      /^Buffers:/ { print \"memory_buffers_kib=\" \$2 }
      /^Cached:/ { print \"memory_cached_kib=\" \$2 }
      /^SReclaimable:/ { print \"memory_sreclaimable_kib=\" \$2 }
      /^MemAvailable:/ { print \"memory_available_kib=\" \$2 }
      /^SwapTotal:/ { print \"swap_total_kib=\" \$2 }
      /^SwapFree:/ { print \"swap_free_kib=\" \$2 }
    " /proc/meminfo
  elif command -v free >/dev/null 2>&1; then
    free -b 2>/dev/null | awk "/^Mem:/ { print \"memory_total=\" \$2; print \"memory_used=\" \$3; exit }"
  else
    vm_stat 2>/dev/null | awk "
      /page size of/ { print \"vm_page_size=\" \$8 }
      /Pages free/ { gsub(/\\./, \"\", \$3); print \"vm_pages_free=\" \$3 }
      /Pages active/ { gsub(/\\./, \"\", \$3); print \"vm_pages_active=\" \$3 }
      /Pages inactive/ { gsub(/\\./, \"\", \$3); print \"vm_pages_inactive=\" \$3 }
      /Pages speculative/ { gsub(/\\./, \"\", \$3); print \"vm_pages_speculative=\" \$3 }
      /Pages wired/ { gsub(/\\./, \"\", \$4); print \"vm_pages_wired=\" \$4 }"
  fi
  df -Pk / 2>/dev/null | awk "NR==2 { print \"disk_total_kib=\" \$2; print \"disk_used_kib=\" \$3; print \"disk_available_kib=\" \$4; exit }"
  # 磁盘 I/O 只统计整盘设备（剔除分区，避免与整盘重复计数），与 node_exporter 口径一致。
  # sector 固定 512 字节，速率由 Rust 侧对相邻样本差分得出。BSD/macOS 暂无等价
  # 低开销接口，缺省时前端显示 "-"（与 thread_count 的降级策略相同）。
  if [ -r /proc/diskstats ]; then
    awk "\$3 ~ /^(sd[a-z]+|hd[a-z]+|vd[a-z]+|xvd[a-z]+|nvme[0-9]+n[0-9]+|mmcblk[0-9]+|md[0-9]+)$/ { r += \$6; w += \$10 } END { print \"disk_read_sectors=\" r + 0; print \"disk_write_sectors=\" w + 0 }" /proc/diskstats
  fi
  if [ -r /proc/loadavg ]; then
    awk "{ print \"load=\" \$1 \" \" \$2 \" \" \$3 }" /proc/loadavg
  else
    uptime 2>/dev/null | sed -n "s/.*load average[s]*: /load=/p"
  fi
  if [ -r /proc/net/dev ]; then
    awk -F "[: ]+" "\$2 != \"lo\" && NR > 2 { rx += \$3; tx += \$11 } END { print \"network_rx_bytes=\" rx + 0; print \"network_tx_bytes=\" tx + 0 }" /proc/net/dev
  elif command -v netstat >/dev/null 2>&1; then
    netstat -ibn 2>/dev/null | awk "NR > 1 && \$1 !~ /^lo/ { rx += \$7; tx += \$10 } END { print \"network_rx_bytes=\" rx + 0; print \"network_tx_bytes=\" tx + 0 }"
  fi
  if [ -r /proc/uptime ]; then
    awk "{ print \"uptime_seconds=\" int(\$1) }" /proc/uptime 2>/dev/null
  else
    sysctl -n kern.boottime 2>/dev/null | sed -n "s/.*sec = \\([0-9][0-9]*\\).*/\\1/p" | awk "{ print \"uptime_seconds=\" systime() - \$1 }"
  fi
  if [ "$XTERM_METRICS_DETAIL" = "1" ]; then
    df -Pi / 2>/dev/null | awk "NR==2 { gsub(/%/, \"\", \$5); print \"disk_inode_percent=\" \$5; exit }"
    if [ -d /proc ]; then
      find /proc -maxdepth 1 -type d -name "[0-9]*" 2>/dev/null | wc -l | awk "{ print \"process_count=\" \$1 }"
      awk "/^Threads:/ { threads += \$2 } END { print \"thread_count=\" threads + 0 }" /proc/[0-9]*/status 2>/dev/null
    else
      ps -ax 2>/dev/null | awk "NR > 1 { count++ } END { print \"process_count=\" count + 0 }"
    fi
  fi
}
xterm_metrics_i=0
while true; do
  if [ $((xterm_metrics_i % ${XTERM_METRICS_DETAIL_EVERY:-5})) -eq 0 ]; then
    XTERM_METRICS_DETAIL=1
  else
    XTERM_METRICS_DETAIL=0
  fi
  xterm_metrics_sample
  echo "$XTERM_METRICS_DELIMITER"
  xterm_metrics_i=$((xterm_metrics_i + 1))
  sleep "${XTERM_METRICS_INTERVAL:-2}"
done
"#;
