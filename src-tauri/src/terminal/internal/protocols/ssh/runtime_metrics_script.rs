pub(super) fn runtime_metrics_command(include_detail: bool) -> String {
    format!(
        "XTERM_METRICS_DETAIL={} LC_ALL=C sh -lc '{}'",
        if include_detail { 1 } else { 0 },
        RUNTIME_METRICS_SCRIPT
    )
}

pub(super) fn runtime_metrics_script_version() -> &'static str {
    "2026-05-15.1"
}

const RUNTIME_METRICS_SCRIPT: &str = r#"
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
if [ "${XTERM_METRICS_DETAIL:-0}" = "1" ]; then
  df -Pi / 2>/dev/null | awk "NR==2 { gsub(/%/, \"\", \$5); print \"disk_inode_percent=\" \$5; exit }"
  if [ -d /proc ]; then
    find /proc -maxdepth 1 -type d -name "[0-9]*" 2>/dev/null | wc -l | awk "{ print \"process_count=\" \$1 }"
    awk "/^Threads:/ { threads += \$2 } END { print \"thread_count=\" threads + 0 }" /proc/[0-9]*/status 2>/dev/null
  else
    ps -ax 2>/dev/null | awk "NR > 1 { count++ } END { print \"process_count=\" count + 0 }"
  fi
fi
"#;
