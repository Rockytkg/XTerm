use crate::terminal::internal::core::SshRuntimeMetrics;

#[derive(Default)]
struct RawMetricsSample {
    cpu_user: Option<u64>,
    cpu_nice: Option<u64>,
    cpu_system: Option<u64>,
    cpu_idle: Option<u64>,
    cpu_iowait: Option<u64>,
    cpu_irq: Option<u64>,
    cpu_softirq: Option<u64>,
    cpu_steal: Option<u64>,
    memory_free: Option<u64>,
    memory_buffers: Option<u64>,
    memory_cached: Option<u64>,
    memory_sreclaimable: Option<u64>,
    memory_available: Option<u64>,
    swap_total: Option<u64>,
    swap_free: Option<u64>,
    vm_page_size: Option<u64>,
    vm_pages_free: Option<u64>,
    vm_pages_active: Option<u64>,
    vm_pages_inactive: Option<u64>,
    vm_pages_speculative: Option<u64>,
    vm_pages_wired: Option<u64>,
    disk_total: Option<u64>,
    disk_used: Option<u64>,
    disk_available: Option<u64>,
    disk_inode_percent: Option<f32>,
    network_rx_bytes: Option<u64>,
    network_tx_bytes: Option<u64>,
    process_count: Option<u64>,
    thread_count: Option<u64>,
    uptime_seconds: Option<u64>,
    load_average: Option<String>,
}

pub(crate) fn empty_runtime_metrics() -> SshRuntimeMetrics {
    SshRuntimeMetrics::default()
}

pub(crate) fn parse_runtime_metrics(output: &str) -> Result<SshRuntimeMetrics, String> {
    let mut raw = RawMetricsSample::default();
    let mut metrics = empty_runtime_metrics();
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "cpu_user" => raw.cpu_user = parse_u64(value),
            "cpu_nice" => raw.cpu_nice = parse_u64(value),
            "cpu_system" => raw.cpu_system = parse_u64(value),
            "cpu_idle" => raw.cpu_idle = parse_u64(value),
            "cpu_iowait" => raw.cpu_iowait = parse_u64(value),
            "cpu_irq" => raw.cpu_irq = parse_u64(value),
            "cpu_softirq" => raw.cpu_softirq = parse_u64(value),
            "cpu_steal" => raw.cpu_steal = parse_u64(value),
            "memory_total" => metrics.memory_total = parse_u64(value),
            "memory_total_kib" => {
                metrics.memory_total = parse_u64(value).map(|v| v.saturating_mul(1024))
            }
            "memory_free" => raw.memory_free = parse_u64(value),
            "memory_free_kib" => raw.memory_free = parse_u64(value).map(|v| v.saturating_mul(1024)),
            "memory_buffers" => raw.memory_buffers = parse_u64(value),
            "memory_buffers_kib" => {
                raw.memory_buffers = parse_u64(value).map(|v| v.saturating_mul(1024))
            }
            "memory_cached" => raw.memory_cached = parse_u64(value),
            "memory_cached_kib" => {
                raw.memory_cached = parse_u64(value).map(|v| v.saturating_mul(1024))
            }
            "memory_sreclaimable" => raw.memory_sreclaimable = parse_u64(value),
            "memory_sreclaimable_kib" => {
                raw.memory_sreclaimable = parse_u64(value).map(|v| v.saturating_mul(1024))
            }
            "memory_available" => raw.memory_available = parse_u64(value),
            "memory_available_kib" => {
                raw.memory_available = parse_u64(value).map(|v| v.saturating_mul(1024))
            }
            "swap_total" => raw.swap_total = parse_u64(value),
            "swap_total_kib" => raw.swap_total = parse_u64(value).map(|v| v.saturating_mul(1024)),
            "swap_free" => raw.swap_free = parse_u64(value),
            "swap_free_kib" => raw.swap_free = parse_u64(value).map(|v| v.saturating_mul(1024)),
            "memory_used" => metrics.memory_used = parse_u64(value),
            "vm_page_size" => raw.vm_page_size = parse_u64(value),
            "vm_pages_free" => raw.vm_pages_free = parse_u64(value),
            "vm_pages_active" => raw.vm_pages_active = parse_u64(value),
            "vm_pages_inactive" => raw.vm_pages_inactive = parse_u64(value),
            "vm_pages_speculative" => raw.vm_pages_speculative = parse_u64(value),
            "vm_pages_wired" => raw.vm_pages_wired = parse_u64(value),
            "disk_total" => raw.disk_total = parse_u64(value),
            "disk_used" => raw.disk_used = parse_u64(value),
            "disk_total_kib" => raw.disk_total = parse_u64(value).map(|v| v.saturating_mul(1024)),
            "disk_used_kib" => raw.disk_used = parse_u64(value).map(|v| v.saturating_mul(1024)),
            "disk_available_kib" => {
                raw.disk_available = parse_u64(value).map(|v| v.saturating_mul(1024))
            }
            "disk_inode_percent" => raw.disk_inode_percent = parse_percent(value),
            "network_rx_bytes" => raw.network_rx_bytes = parse_u64(value),
            "network_tx_bytes" => raw.network_tx_bytes = parse_u64(value),
            "process_count" => raw.process_count = parse_u64(value),
            "thread_count" => raw.thread_count = parse_u64(value),
            "uptime_seconds" => raw.uptime_seconds = parse_u64(value),
            "load" => raw.load_average = Some(value.trim().to_string()),
            _ => {}
        }
    }
    derive_runtime_metrics(&mut metrics, raw);
    Ok(metrics)
}

pub(super) fn parse_u64(value: &str) -> Option<u64> {
    value.trim().parse::<u64>().ok()
}

fn parse_percent(value: &str) -> Option<f32> {
    value.trim().trim_end_matches('%').parse::<f32>().ok()
}

fn derive_runtime_metrics(metrics: &mut SshRuntimeMetrics, raw: RawMetricsSample) {
    metrics.load_average = raw.load_average;
    metrics.process_count = raw.process_count;
    metrics.thread_count = raw.thread_count;
    metrics.uptime_seconds = raw.uptime_seconds;
    metrics.disk_inode_percent = raw.disk_inode_percent.map(|v| v.clamp(0.0, 100.0));
    metrics.network_rx_bytes = raw.network_rx_bytes;
    metrics.network_tx_bytes = raw.network_tx_bytes;

    // CPU totals
    let total = [
        raw.cpu_user,
        raw.cpu_nice,
        raw.cpu_system,
        raw.cpu_idle,
        raw.cpu_iowait,
        raw.cpu_irq,
        raw.cpu_softirq,
        raw.cpu_steal,
    ]
    .into_iter()
    .flatten()
    .sum::<u64>();
    if total > 0 {
        metrics.cpu_total = Some(total);
    }
    metrics.cpu_user = raw
        .cpu_user
        .map(|user| user.saturating_add(raw.cpu_nice.unwrap_or(0)));
    metrics.cpu_system = raw.cpu_system.map(|system| {
        system
            .saturating_add(raw.cpu_irq.unwrap_or(0))
            .saturating_add(raw.cpu_softirq.unwrap_or(0))
    });
    metrics.cpu_iowait = raw.cpu_iowait;
    metrics.cpu_steal = raw.cpu_steal;
    metrics.cpu_idle = match (raw.cpu_idle, raw.cpu_iowait) {
        (Some(idle), Some(iowait)) => Some(idle.saturating_add(iowait)),
        (idle, _) => idle,
    };

    // Memory
    if let (Some(total), Some(free), Some(buffers), Some(cached)) = (
        metrics.memory_total,
        raw.memory_free,
        raw.memory_buffers,
        raw.memory_cached,
    ) {
        let reclaimable = raw.memory_sreclaimable.unwrap_or(0);
        let used = total
            .saturating_sub(free)
            .saturating_sub(buffers)
            .saturating_sub(cached);
        let used = used.saturating_sub(reclaimable);
        metrics.memory_used = Some(used);
        metrics.memory_available = raw.memory_available.or(Some(total.saturating_sub(used)));
        metrics.memory_percent = percent(used, total);
    } else if let (Some(total), Some(used)) = (metrics.memory_total, metrics.memory_used) {
        metrics.memory_available = Some(total.saturating_sub(used));
        metrics.memory_percent = percent(used, total);
    } else if let (Some(total), Some(available)) = (metrics.memory_total, raw.memory_available) {
        metrics.memory_available = Some(available);
        metrics.memory_percent = percent(total.saturating_sub(available), total);
    } else if let (
        Some(page_size),
        Some(free),
        Some(active),
        Some(inactive),
        Some(speculative),
        Some(wired),
    ) = (
        raw.vm_page_size,
        raw.vm_pages_free,
        raw.vm_pages_active,
        raw.vm_pages_inactive,
        raw.vm_pages_speculative,
        raw.vm_pages_wired,
    ) {
        let total_pages = free
            .saturating_add(active)
            .saturating_add(inactive)
            .saturating_add(speculative)
            .saturating_add(wired);
        let used_pages = active.saturating_add(inactive).saturating_add(wired);
        metrics.memory_total = Some(total_pages.saturating_mul(page_size));
        metrics.memory_used = Some(used_pages.saturating_mul(page_size));
        metrics.memory_available = Some(free.saturating_add(speculative).saturating_mul(page_size));
        metrics.memory_percent = percent(used_pages, total_pages);
    }
    if let (Some(total), Some(free)) = (raw.swap_total, raw.swap_free) {
        let used = total.saturating_sub(free);
        metrics.swap_total = Some(total);
        metrics.swap_used = Some(used);
        metrics.swap_percent = Some(percent(used, total));
    }

    // Disk
    if let (Some(total), Some(used)) = (raw.disk_total, raw.disk_used) {
        metrics.disk_total = Some(total);
        metrics.disk_used = Some(used);
        metrics.disk_available = raw.disk_available.or(Some(total.saturating_sub(used)));
        metrics.disk_percent = percent(used, total);
    }
}

pub(super) fn percent(value: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        ((value as f32 * 100.0) / total as f32).clamp(0.0, 100.0)
    }
}
