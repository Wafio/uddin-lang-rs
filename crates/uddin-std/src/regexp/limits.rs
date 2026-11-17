//! The limits the reference puts on regular expressions, and how this build reports them.
//!
//! The reference stops a run whose pattern has more than 1,024 instructions (`regex_size`) and one whose searches
//! have read more than 524,288 units, an instruction for each character (`regex_work`). The machine has no limit
//! reason for either, nor a counter of units per run, so for now a pattern that is too large ends the run as a
//! value that is too large, the units only tell the meter when to read the clock, and the deadline is what bounds
//! the work of a run. Giving the machine the two reasons, a counter and the two limits of the profile changes
//! `too_large` and `charge_work` and nothing else.

use uddin_vm::{Count, Exc, LimitReason, Meter};

/// Builds the error that ends the run for a limit, at the position of the call.
pub(super) type Limit<'a> = &'a dyn Fn(LimitReason) -> Exc;

/// The most instructions a program may have; a pattern may be four bytes long for each of them.
pub(super) const MAX_PROGRAM: i64 = 1024;

/// The run ends because a pattern is too large.
pub(super) fn too_large(limit: Limit<'_>) -> Exc {
    limit(LimitReason::RegexSize)
}

pub(super) fn out_of_memory(limit: Limit<'_>) -> Exc {
    limit(LimitReason::Memory)
}

/// Reports `units` of regex work: one for each instruction of a program for each character a search reads, and
/// for the compile of a program 64 and 40 for each instruction.
pub(super) fn charge_work(meter: &mut Meter, limit: Limit<'_>, units: u64) -> Result<(), Exc> {
    meter.work(Count::Free, units)?;
    if meter.charge_regex_work(units) {
        Ok(())
    } else {
        Err(limit(LimitReason::RegexWork))
    }
}

pub(super) fn compile_units(program: u64) -> u64 {
    64 + 40 * program
}
