//! The compiler and machine of the uddin script language, with the limits a sandboxed run is held to.

#![forbid(unsafe_code)]

mod builtin;
mod bytecode;
mod compile;
mod error;
mod fmtv;
mod golist;
pub mod gomath;
mod heap;
mod host;
mod meter;
mod ops;
mod text;
mod value;
mod vm;

pub use builtin::{BuiltinDef, BuiltinFn, Env, Fresh, Hook, Rt, Stringify};
pub use bytecode::Program;
pub use compile::{CompileError, compile};
pub use error::{ErrClass, Exc, LimitReason, error_text_parts};
pub use fmtv::to_text;
pub use heap::{Header, Heap, Items, ListObj, Oom, SetObj, Shape};
pub use host::{Json, RequestContext, Verdict};
pub use meter::{Count, Limits, Meter, Output};
pub use ops::{
    JSON_BYTE_COST, LIST_COST, MAP_COST, MAP_ENTRY_COST, OBJECT_COST, RANGE_ARG_BOUND,
    RANGE_ELEM_COST, REGEX_BASE_COST, REGEX_BYTES_PER_INST, REGEX_INST_COST, REGEX_MATCH_COST,
    REGEX_PARSE_BYTE_COST, SCALAR_COST, SPLIT_PART_COST, WORD_COST, checked_product, sum_estimate,
    truthy,
};
pub use text::{
    decode_rune, float_to_int, format_float_g, format_float_g6, format_float_json, format_int,
    quote_for_error,
};
pub use value::{
    FuncId, ListId, MapId, SetId, Str, Value, host_type_name, type_name, type_name_str,
};
pub use vm::{Ended, HostValue, RunState};
