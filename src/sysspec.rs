//! Detect host CPU/RAM and recommend tuning parameters.
//!
//! Returns sensible defaults for:
//!   - Tokio worker threads (matches CPU cores)
//!   - HTTP concurrency (capped per-domain to avoid getting banned)
//!   - Cache sizes (proportional to RAM)
//!   - Per-host TCP pool size

use sysinfo::System;

#[derive(Debug, Clone, Copy)]
pub struct SysSpec {
    /// Number of physical CPU cores
    pub cpu_cores: usize,
    /// Total system RAM in MiB
    pub total_mem_mib: u64,
    /// Available system RAM in MiB
    pub avail_mem_mib: u64,
}

impl SysSpec {
    pub fn detect() -> Self {
        let mut sys = System::new();
        sys.refresh_memory();
        sys.refresh_cpu_all();

        let cpu_cores = num_cpus::get().max(1);
        let total_mem_mib = sys.total_memory() / 1024 / 1024;
        let avail_mem_mib = sys.available_memory() / 1024 / 1024;

        Self {
            cpu_cores,
            total_mem_mib,
            avail_mem_mib,
        }
    }

    /// Recommended Tokio worker threads
    pub fn worker_threads(&self) -> usize {
        // 1 worker per logical core, min 2, max 32
        self.cpu_cores.clamp(2, 32)
    }

    /// Recommended HTTP concurrency (global limit on outbound requests)
    pub fn http_concurrency(&self) -> usize {
        // Scale with cores but cap to avoid getting banned by upstreams
        let base = self.cpu_cores * 4;
        base.clamp(8, 100)
    }

    /// Recommended scrape result cache capacity (entries)
    pub fn scrape_cache_capacity(&self) -> u64 {
        // Roughly 1 entry per MB of RAM, capped
        match self.total_mem_mib {
            ..=1024 => 500,
            1025..=4096 => 2_000,
            4097..=16384 => 10_000,
            _ => 50_000,
        }
    }

    /// Recommended search cache capacity
    pub fn search_cache_capacity(&self) -> u64 {
        self.scrape_cache_capacity() / 4
    }

    /// Recommended TCP pool max idle connections per host
    #[allow(dead_code)]
    pub fn pool_per_host(&self) -> usize {
        (self.cpu_cores * 8).clamp(16, 256)
    }

    /// Profile name for logging
    pub fn profile(&self) -> &'static str {
        match self.total_mem_mib {
            ..=1024 => "minimal (laptop / 1GB)",
            1025..=4096 => "small (laptop / 2-4GB)",
            4097..=16384 => "standard (workstation / 8-16GB)",
            _ => "production (VPS / 32GB+)",
        }
    }

    /// Pretty-print summary
    #[allow(dead_code)]
    pub fn summary(&self) -> String {
        format!(
            "CPU: {} cores · RAM: {} / {} MiB · profile: {}\n  → tokio_threads={} · http_concurrency={} · pool_per_host={} · scrape_cache={} · search_cache={}",
            self.cpu_cores,
            self.avail_mem_mib,
            self.total_mem_mib,
            self.profile(),
            self.worker_threads(),
            self.http_concurrency(),
            self.pool_per_host(),
            self.scrape_cache_capacity(),
            self.search_cache_capacity(),
        )
    }
}
