use crate::value::RtValue;

/// Default hard cap on arena entries (`AC-13`) preventing unbounded heap exhaustion.
pub const DEFAULT_MAX_ARENA_ENTRIES: usize = 16_777_216;

/// Safe, 1-based handle-indexed runtime arena (`handle == 0` is always invalid!).
///
/// Validates every handle lookup with zero panics and zero UB (`AC-13`).
#[derive(Clone, Debug)]
pub struct RtArena {
    entries: Vec<RtValue>,
    max_entries: usize,
}

impl RtArena {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries: max_entries.max(1),
        }
    }

    pub fn with_default_cap() -> Self {
        Self::new(DEFAULT_MAX_ARENA_ENTRIES)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Insert a value and return its 1-based handle (`1..=max_entries`).
    pub fn insert(&mut self, val: RtValue) -> Result<u64, String> {
        if self.entries.len() >= self.max_entries {
            return Err(format!(
                "arena entry limit ({}) exhausted",
                self.max_entries
            ));
        }
        self.entries.push(val);
        Ok(self.entries.len() as u64)
    }

    /// Safely resolve a 1-based handle, rejecting `0` and out-of-range/stale handles.
    pub fn get(&self, handle: u64) -> Result<&RtValue, String> {
        if handle == 0 {
            return Err("invalid null arena handle 0".to_string());
        }
        let idx = (handle - 1) as usize;
        self.entries
            .get(idx)
            .ok_or_else(|| format!("invalid or stale arena handle {handle}"))
    }

    /// Safely resolve a mutable reference to a 1-based handle (`P0-3`, zero `unsafe`).
    pub fn get_mut(&mut self, handle: u64) -> Result<&mut RtValue, String> {
        if handle == 0 {
            return Err("invalid null arena handle 0".to_string());
        }
        let idx = (handle - 1) as usize;
        self.entries
            .get_mut(idx)
            .ok_or_else(|| format!("invalid or stale arena handle {handle}"))
    }

    /// Reclaim all arena slots allocated above `mark` (deterministic loop-region reset).
    pub fn truncate(&mut self, mark: usize) {
        if mark < self.entries.len() {
            self.entries.truncate(mark);
        }
    }
}
