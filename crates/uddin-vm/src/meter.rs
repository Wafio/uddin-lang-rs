//! The counters a run is held to, and the output it may produce.

use std::time::{Duration, Instant};

use uddin_syntax::Position;

use crate::error::{Exc, LimitReason};

/// What one run may use. A zero in the first five fields means no limit.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_ops: u64,
    pub max_call_depth: u32,
    pub max_nesting: u32,
    pub max_value_len: usize,
    pub max_alloc_bytes: usize,
    /// The most instructions a regular expression may compile to; 0 means no limit.
    pub max_regex_program: usize,
    /// The most units of regex work a run may use, one for each instruction of a program for each character a
    /// search reads; 0 means no limit.
    pub max_regex_work: usize,
    pub deadline: Option<Duration>,
    /// A backstop on the real size of the run heap, far above what the memory limit lets through.
    pub heap_cap: usize,
    /// A heap that grew past this many bytes is dropped after the run.
    pub keep_heap: usize,
    pub capture_print: bool,
    pub print_cap: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_ops: 100_000,
            max_call_depth: 64,
            max_nesting: 256,
            max_value_len: 1 << 20,
            max_alloc_bytes: 4 << 20,
            max_regex_program: 1024,
            max_regex_work: 524_288,
            deadline: Some(Duration::from_millis(5)),
            heap_cap: 64 << 20,
            keep_heap: 256 << 10,
            capture_print: false,
            print_cap: 1 << 20,
        }
    }
}

/// Whether a step of a loop over input is an operation of the script. The reference interpreter counts one at
/// some steps, each node a guarded walk visits for one, and none at others, such as the comparisons of a sort.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Count {
    Op,
    Free,
}

pub struct Meter {
    pub lim: Limits,
    pub ops: u64,
    pub allocated: usize,
    /// Units of regex work used so far.
    pub regex_work: u64,
    pub nesting: u32,
    pub call_depth: u32,
    /// Where the last expression or statement began; limits that name no place of their own report it.
    pub cur_pos: Position,
    work_units: u64,
    deadline_at: Option<Instant>,
}

impl Meter {
    pub fn new(lim: Limits) -> Meter {
        Meter {
            lim,
            ops: 0,
            allocated: 0,
            regex_work: 0,
            nesting: 0,
            call_depth: 0,
            cur_pos: Position::default(),
            work_units: 0,
            deadline_at: None,
        }
    }

    pub fn start(&mut self, lim: Limits) {
        self.deadline_at = lim.deadline.map(|d| Instant::now() + d);
        self.lim = lim;
        self.ops = 0;
        self.allocated = 0;
        self.regex_work = 0;
        self.nesting = 0;
        self.call_depth = 0;
        self.cur_pos = Position::default();
        self.work_units = 0;
    }

    /// Counts one operation.
    #[inline]
    pub fn tick(&mut self) -> Result<(), Exc> {
        self.ops += 1;
        if self.lim.max_ops > 0 && self.ops > self.lim.max_ops {
            return Err(Exc::Limit(LimitReason::OpLimit, self.cur_pos));
        }
        if self.ops & 255 == 0 {
            self.check_deadline()?;
        }
        Ok(())
    }

    /// One step of a loop over input: `count` says whether the step is an operation, `units` how much work it
    /// did (a byte, an element or a node each). The clock is read each time the units pass a multiple of 1,024,
    /// so a loop runs at most that much past the deadline. A copy or a library call cannot be interrupted:
    /// report its size before it starts.
    #[inline]
    pub fn work(&mut self, count: Count, units: u64) -> Result<(), Exc> {
        if count == Count::Op {
            self.tick()?;
        }
        let before = self.work_units;
        self.work_units = before.wrapping_add(units);
        if (before ^ self.work_units) >> 10 != 0 {
            self.check_deadline()?;
        }
        Ok(())
    }

    #[inline]
    pub fn check_deadline(&self) -> Result<(), Exc> {
        match self.deadline_at {
            Some(at) if Instant::now() >= at => {
                Err(Exc::Limit(LimitReason::Canceled, self.cur_pos))
            }
            _ => Ok(()),
        }
    }

    /// Adds `n` bytes to the run's allocation; a negative `n` is an estimate that overflowed.
    pub fn charge(&mut self, pos: Position, n: i64) -> Result<(), Exc> {
        let max = self.lim.max_alloc_bytes;
        if max == 0 {
            return Ok(());
        }
        if n < 0 || n as u64 > (max - self.allocated.min(max)) as u64 {
            self.allocated = max;
            return Err(Exc::Limit(LimitReason::Memory, pos));
        }
        self.allocated += n as usize;
        Ok(())
    }

    /// Adds `units` of regex work to the run's; false when that passes the budget.
    pub fn charge_regex_work(&mut self, units: u64) -> bool {
        let max = self.lim.max_regex_work as u64;
        if max == 0 {
            return true;
        }
        if units > max - self.regex_work.min(max) {
            self.regex_work = max;
            return false;
        }
        self.regex_work += units;
        true
    }

    /// Replaces a reservation made with `charge` by what the call really used.
    pub fn replace_reservation(
        &mut self,
        pos: Position,
        reserved: i64,
        used: i64,
    ) -> Result<(), Exc> {
        self.allocated = self.allocated.saturating_sub(reserved.max(0) as usize);
        self.charge(pos, used)
    }

    /// Stops the run when `n` is not a length a value may have; a negative `n` is an overflowed size.
    pub fn check_length(&self, pos: Position, n: i64) -> Result<(), Exc> {
        let max = self.lim.max_value_len;
        if max > 0 && (n < 0 || n as u64 > max as u64) {
            return Err(Exc::Limit(LimitReason::ValueSize, pos));
        }
        Ok(())
    }
}

/// The text a script prints, kept only when the embedder asks for it.
pub struct Output {
    pub buf: Vec<u8>,
    pub capture: bool,
    pub cap: usize,
    pub truncated: bool,
}

impl Output {
    pub fn new() -> Output {
        Output {
            buf: Vec::new(),
            capture: false,
            cap: 0,
            truncated: false,
        }
    }

    pub fn start(&mut self, capture: bool, cap: usize) {
        self.buf.clear();
        self.capture = capture;
        self.cap = cap;
        self.truncated = false;
    }

    pub fn write(&mut self, data: &[u8]) {
        if !self.capture {
            return;
        }
        let room = self.cap.saturating_sub(self.buf.len());
        let take = data.len().min(room);
        if take < data.len() {
            self.truncated = true;
        }
        if self.buf.try_reserve(take).is_ok() {
            self.buf.extend_from_slice(&data[..take]);
        } else {
            self.truncated = true;
        }
    }
}

impl Default for Output {
    fn default() -> Output {
        Output::new()
    }
}
