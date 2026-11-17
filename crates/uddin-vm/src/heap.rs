//! The per-run heap. Nothing is freed during a run (the memory limit bounds the total); every growth goes
//! through `try_reserve`, so an allocation that cannot be made is an error, never an abort. Objects live in
//! slots that the next run reuses, so a warm thread allocates nothing.

use std::collections::HashSet;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crate::golist::grown_cap;
use crate::value::{CONST_BASE, FuncId, ListId, MapId, ScopeId, SetId, Str, Value};

/// The heap cannot grow any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Oom;

type Res<T> = Result<T, Oom>;

const SLOT_COST: usize = 64;
const VALUE_COST: usize = 16;
const ENTRY_COST: usize = 24;
/// Maps up to this many entries are searched linearly.
const LINEAR_MAP: usize = 8;

/// The elements of a list: a window onto the array behind it, as a Go slice is. A pop, a shift or an append that
/// fits move the window and leave the array, so a loop that began earlier still reads what the array holds.
pub struct Items {
    buf: Vec<Value>,
    off: usize,
    len: usize,
}

impl Items {
    const fn new() -> Items {
        Items {
            buf: Vec::new(),
            off: 0,
            len: 0,
        }
    }

    fn clear(&mut self) {
        self.buf.clear();
        self.off = 0;
        self.len = 0;
    }

    /// Writes `v` after the last element, over what an earlier pop left there.
    fn push(&mut self, v: Value) -> Res<()> {
        let end = self.off + self.len;
        if end < self.buf.len() {
            self.buf[end] = v;
        } else {
            self.buf.try_reserve(1).map_err(|_| Oom)?;
            self.buf.push(v);
        }
        self.len += 1;
        Ok(())
    }

    fn extend_from_slice(&mut self, vs: &[Value]) -> Res<()> {
        self.buf.try_reserve(vs.len()).map_err(|_| Oom)?;
        for &v in vs {
            self.push(v)?;
        }
        Ok(())
    }

    /// Copies `n` elements from `from` (counted in the window) to the end; the window must end the array.
    fn extend_from_within(&mut self, from: usize, n: usize) -> Res<()> {
        self.buf.try_reserve(n).map_err(|_| Oom)?;
        self.buf.truncate(self.off + self.len);
        self.buf
            .extend_from_within(self.off + from..self.off + from + n);
        self.len += n;
        Ok(())
    }
}

impl Deref for Items {
    type Target = [Value];

    fn deref(&self) -> &[Value] {
        &self.buf[self.off..self.off + self.len]
    }
}

impl DerefMut for Items {
    fn deref_mut(&mut self) -> &mut [Value] {
        &mut self.buf[self.off..self.off + self.len]
    }
}

impl<'a> IntoIterator for &'a Items {
    type Item = &'a Value;
    type IntoIter = std::slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct ListObj {
    pub items: Items,
    /// The capacity the reference implementation's slice header would have; it decides what growth costs.
    pub go_cap: usize,
    /// Which array the reference implementation's slice points into: it changes when an append does not fit and
    /// when `unshift` builds a new array.
    arr: u32,
    /// A loop or a builtin reads this list by a header it took earlier, so a replaced array must be kept.
    shared: bool,
}

/// What a loop or a builtin keeps of a list it reads while the script goes on: the array, where its window
/// began and how long it was.
#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub list: ListId,
    pub arr: u32,
    pub off: usize,
    pub len: usize,
}

/// Where a list stands, for the memory charge of what a call did to it.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub arr: u32,
    pub cap: usize,
    pub len: usize,
}

pub struct SetObj {
    keys: Vec<Value>,
    index: HashSet<Box<[u8]>>,
}

pub struct MapObj {
    pub entries: Vec<(Str, Value)>,
    index: Vec<u32>,
}

pub struct FuncObj {
    pub proto: u32,
    pub closure: ScopeId,
}

pub struct ScopeObj {
    pub vars: Vec<(u32, Value)>,
    /// A closure holds this scope, so it must outlive the call that made it.
    pub captured: bool,
}

pub struct Heap {
    consts: Arc<[u8]>,
    bytes: Vec<u8>,
    lists: Vec<ListObj>,
    n_lists: usize,
    /// Arrays a list left behind while a loop or a builtin still reads them: (list, array, elements).
    retired: Vec<(u32, u32, Vec<Value>)>,
    sets: Vec<SetObj>,
    n_sets: usize,
    maps: Vec<MapObj>,
    n_maps: usize,
    funcs: Vec<FuncObj>,
    scopes: Vec<ScopeObj>,
    n_scopes: usize,
    free_scopes: Vec<u32>,
    used: usize,
    cap: usize,
    seed: RandomState,
}

fn span<'a>(consts: &'a [u8], bytes: &'a [u8], s: Str) -> &'a [u8] {
    if s.off >= CONST_BASE {
        let o = (s.off - CONST_BASE) as usize;
        &consts[o..o + s.len as usize]
    } else {
        let o = s.off as usize;
        &bytes[o..o + s.len as usize]
    }
}

impl Heap {
    pub fn new(cap: usize) -> Heap {
        Heap {
            consts: Arc::from(Vec::new()),
            bytes: Vec::new(),
            lists: Vec::new(),
            n_lists: 0,
            retired: Vec::new(),
            sets: Vec::new(),
            n_sets: 0,
            maps: Vec::new(),
            n_maps: 0,
            funcs: Vec::new(),
            scopes: Vec::new(),
            n_scopes: 0,
            free_scopes: Vec::new(),
            used: 0,
            cap,
            seed: RandomState::new(),
        }
    }

    /// Starts a run over the constant pool `consts`. A heap that grew past `keep` bytes is dropped, so one
    /// hostile run does not pin its memory to the thread.
    pub fn reset(&mut self, consts: Arc<[u8]>, cap: usize, keep: usize) {
        if self.retained() > keep {
            let seed = self.seed.clone();
            *self = Heap::new(cap);
            self.seed = seed;
        }
        self.consts = consts;
        self.cap = cap;
        self.bytes.clear();
        self.n_lists = 0;
        self.retired.clear();
        self.n_sets = 0;
        self.n_maps = 0;
        self.funcs.clear();
        self.n_scopes = 0;
        self.free_scopes.clear();
        self.used = 0;
    }

    /// Bytes this heap holds on to between runs.
    pub fn retained(&self) -> usize {
        self.bytes.capacity()
            + self
                .lists
                .iter()
                .map(|l| l.items.buf.capacity() * VALUE_COST + SLOT_COST)
                .sum::<usize>()
            + self
                .sets
                .iter()
                .map(|s| {
                    s.keys.capacity() * VALUE_COST + s.index.capacity() * ENTRY_COST + SLOT_COST
                })
                .sum::<usize>()
            + self
                .maps
                .iter()
                .map(|m| m.entries.capacity() * ENTRY_COST + m.index.capacity() * 4 + SLOT_COST)
                .sum::<usize>()
            + self
                .scopes
                .iter()
                .map(|s| s.vars.capacity() * ENTRY_COST + SLOT_COST)
                .sum::<usize>()
            + self.funcs.capacity() * 8
    }

    fn account(&mut self, n: usize) -> Res<()> {
        self.used = self.used.saturating_add(n);
        if self.used > self.cap {
            Err(Oom)
        } else {
            Ok(())
        }
    }

    pub fn str_bytes(&self, s: Str) -> &[u8] {
        span(&self.consts, &self.bytes, s)
    }

    pub fn new_str(&mut self, data: &[u8]) -> Res<Str> {
        let off = self.bytes.len();
        self.account(data.len())?;
        self.bytes.try_reserve(data.len()).map_err(|_| Oom)?;
        if off + data.len() >= CONST_BASE as usize {
            return Err(Oom);
        }
        self.bytes.extend_from_slice(data);
        Ok(Str {
            off: off as u32,
            len: data.len() as u32,
        })
    }

    pub fn concat(&mut self, a: Str, b: Str) -> Res<Str> {
        if a.len == 0 {
            return Ok(b);
        }
        if b.len == 0 {
            return Ok(a);
        }
        let n = a.len as usize + b.len as usize;
        let off = self.bytes.len();
        self.account(n)?;
        self.bytes.try_reserve(n).map_err(|_| Oom)?;
        if off + n >= CONST_BASE as usize {
            return Err(Oom);
        }
        for s in [a, b] {
            if s.is_const() {
                let o = (s.off - CONST_BASE) as usize;
                self.bytes
                    .extend_from_slice(&self.consts[o..o + s.len as usize]);
            } else {
                let o = s.off as usize;
                self.bytes.extend_from_within(o..o + s.len as usize);
            }
        }
        Ok(Str {
            off: off as u32,
            len: n as u32,
        })
    }

    /// `s` repeated `n` times.
    pub fn repeat(&mut self, s: Str, n: usize) -> Res<Str> {
        if s.len == 0 || n == 0 {
            return Ok(Str::EMPTY);
        }
        let total = (s.len as usize).checked_mul(n).ok_or(Oom)?;
        let off = self.bytes.len();
        self.account(total)?;
        self.bytes.try_reserve(total).map_err(|_| Oom)?;
        if off + total >= CONST_BASE as usize {
            return Err(Oom);
        }
        if s.is_const() {
            let o = (s.off - CONST_BASE) as usize;
            self.bytes
                .extend_from_slice(&self.consts[o..o + s.len as usize]);
        } else {
            let o = s.off as usize;
            self.bytes.extend_from_within(o..o + s.len as usize);
        }
        // Doubling copies each byte a constant number of times, however large `n` is.
        while self.bytes.len() - off < total {
            let have = self.bytes.len() - off;
            let take = have.min(total - have);
            self.bytes.extend_from_within(off..off + take);
        }
        Ok(Str {
            off: off as u32,
            len: total as u32,
        })
    }

    pub fn new_list(&mut self, reserve: usize, go_cap: usize) -> Res<ListId> {
        let id = self.n_lists;
        self.account(SLOT_COST)?;
        if id < self.lists.len() {
            let l = &mut self.lists[id];
            l.items.clear();
            l.go_cap = go_cap;
            l.arr = 0;
            l.shared = false;
            l.items.buf.try_reserve(reserve).map_err(|_| Oom)?;
        } else {
            self.lists.try_reserve(1).map_err(|_| Oom)?;
            let mut items = Items::new();
            items.buf.try_reserve(reserve).map_err(|_| Oom)?;
            self.lists.push(ListObj {
                items,
                go_cap,
                arr: 0,
                shared: false,
            });
        }
        self.n_lists += 1;
        Ok(ListId(id as u32))
    }

    /// Appends the items of `src` to `dst`, which must be another list.
    pub fn list_extend(&mut self, dst: ListId, src: ListId) -> Res<()> {
        let n = self.lists[src.0 as usize].items.len();
        self.account(n * VALUE_COST)?;
        let (d, s) = self.two_lists(dst, src);
        d.items.extend_from_slice(&s.items)
    }

    /// Appends the items of `src`, `times` over, to `dst`, which must be another list.
    pub fn list_extend_repeated(&mut self, dst: ListId, src: ListId, times: usize) -> Res<()> {
        let n = self.lists[src.0 as usize].items.len();
        if n == 0 || times == 0 {
            return Ok(());
        }
        let start = self.lists[dst.0 as usize].items.len();
        self.list_extend(dst, src)?;
        let total = n * times;
        loop {
            let have = self.lists[dst.0 as usize].items.len() - start;
            if have >= total {
                return Ok(());
            }
            let take = have.min(total - have);
            self.account(take * VALUE_COST)?;
            self.lists[dst.0 as usize]
                .items
                .extend_from_within(start, take)?;
        }
    }

    fn two_lists(&mut self, a: ListId, b: ListId) -> (&mut ListObj, &ListObj) {
        let (a, b) = (a.0 as usize, b.0 as usize);
        assert_ne!(a, b, "a list is never joined with itself");
        if a < b {
            let (lo, hi) = self.lists.split_at_mut(b);
            (&mut lo[a], &hi[0])
        } else {
            let (lo, hi) = self.lists.split_at_mut(a);
            (&mut hi[0], &lo[b])
        }
    }

    pub fn list(&self, id: ListId) -> &ListObj {
        &self.lists[id.0 as usize]
    }

    pub fn list_mut(&mut self, id: ListId) -> &mut ListObj {
        &mut self.lists[id.0 as usize]
    }

    /// Writes `v` after the last element of a list under construction; its capacity was given when it was made.
    pub fn list_push(&mut self, id: ListId, v: Value) -> Res<()> {
        self.account(VALUE_COST)?;
        self.lists[id.0 as usize].items.push(v)
    }

    /// `append(list, vals...)` of the reference: in place while the capacity lasts, else into a new array of the
    /// capacity its growth rule gives.
    pub fn list_append(&mut self, id: ListId, vals: &[Value]) -> Res<()> {
        let idx = id.0 as usize;
        let new_len = self.lists[idx].items.len + vals.len();
        if vals.is_empty() {
            return Ok(());
        }
        self.account(vals.len() * VALUE_COST)?;
        if new_len > self.lists[idx].go_cap {
            let cap = grown_cap(self.lists[idx].go_cap, new_len);
            self.replace_array(id, new_len)?;
            self.lists[idx].go_cap = cap;
        }
        self.lists[idx].items.extend_from_slice(vals)
    }

    /// `list = list[:len-1]` and the element that was last, or `None` for an empty list.
    pub fn list_pop(&mut self, id: ListId) -> Option<Value> {
        let items = &mut self.lists[id.0 as usize].items;
        if items.len == 0 {
            return None;
        }
        items.len -= 1;
        Some(items.buf[items.off + items.len])
    }

    /// `list = list[1:]` and the element that was first, or `None` for an empty list. The capacity shrinks by one.
    pub fn list_shift(&mut self, id: ListId) -> Option<Value> {
        let l = &mut self.lists[id.0 as usize];
        if l.items.len == 0 {
            return None;
        }
        let first = l.items.buf[l.items.off];
        l.items.off += 1;
        l.items.len -= 1;
        l.go_cap = l.go_cap.saturating_sub(1);
        Some(first)
    }

    /// A new array of exactly the length, with `vals` in front of the elements the list had.
    pub fn list_unshift(&mut self, id: ListId, vals: &[Value]) -> Res<()> {
        let idx = id.0 as usize;
        let total = vals.len() + self.lists[idx].items.len;
        self.account(total * VALUE_COST)?;
        let mut fresh = Vec::new();
        fresh.try_reserve_exact(total).map_err(|_| Oom)?;
        fresh.extend_from_slice(vals);
        fresh.extend_from_slice(&self.lists[idx].items);
        self.install_array(id, fresh)?;
        self.lists[idx].go_cap = total;
        Ok(())
    }

    /// Makes a new array of exactly the length of `vals` the array of a list, as an assignment of a fresh slice to
    /// the list does.
    pub fn list_replace(&mut self, id: ListId, vals: &[Value]) -> Res<()> {
        self.account(vals.len() * VALUE_COST)?;
        let mut fresh = Vec::new();
        fresh.try_reserve_exact(vals.len()).map_err(|_| Oom)?;
        fresh.extend_from_slice(vals);
        self.install_array(id, fresh)?;
        self.lists[id.0 as usize].go_cap = vals.len();
        Ok(())
    }

    /// Moves a list that outgrew its array into a new one holding the same elements, with room for `len`.
    fn replace_array(&mut self, id: ListId, len: usize) -> Res<()> {
        let idx = id.0 as usize;
        let mut fresh = Vec::new();
        fresh.try_reserve(len).map_err(|_| Oom)?;
        fresh.extend_from_slice(&self.lists[idx].items);
        self.install_array(id, fresh)
    }

    /// Makes `fresh` the array of a list, and keeps the old one when a loop or a builtin still reads it.
    fn install_array(&mut self, id: ListId, fresh: Vec<Value>) -> Res<()> {
        let idx = id.0 as usize;
        if self.lists[idx].shared {
            self.retired.try_reserve(1).map_err(|_| Oom)?;
        }
        let l = &mut self.lists[idx];
        let len = fresh.len();
        let old = std::mem::replace(&mut l.items.buf, fresh);
        let old_arr = l.arr;
        l.arr += 1;
        l.items.off = 0;
        l.items.len = len;
        if l.shared {
            self.retired.push((id.0, old_arr, old));
        }
        Ok(())
    }

    /// Takes what a loop or a builtin needs to read a list the way the reference does: the header it has now.
    pub fn snapshot(&mut self, id: ListId) -> Header {
        let l = &mut self.lists[id.0 as usize];
        l.shared = true;
        Header {
            list: id,
            arr: l.arr,
            off: l.items.off,
            len: l.items.len,
        }
    }

    /// The `i`th element of the array a header was taken from, as it is now.
    pub fn header_item(&self, h: &Header, i: usize) -> Value {
        self.read_at(h.list, h.arr, h.off + i)
    }

    /// The element at `pos` of the array `arr` of a list, whether the list still uses it or not.
    pub fn read_at(&self, list: ListId, arr: u32, pos: usize) -> Value {
        let l = &self.lists[list.0 as usize];
        let buf = if l.arr == arr {
            &l.items.buf
        } else {
            match self
                .retired
                .iter()
                .find(|(id, a, _)| *id == list.0 && *a == arr)
            {
                Some((_, _, buf)) => buf,
                None => return Value::Null,
            }
        };
        buf.get(pos).copied().unwrap_or(Value::Null)
    }

    /// Where a list stands now: its array, capacity and length.
    pub fn shape(&self, id: ListId) -> Shape {
        let l = &self.lists[id.0 as usize];
        Shape {
            arr: l.arr,
            cap: l.go_cap,
            len: l.items.len,
        }
    }

    pub fn new_set(&mut self) -> Res<SetId> {
        let id = self.n_sets;
        self.account(SLOT_COST)?;
        if id < self.sets.len() {
            let s = &mut self.sets[id];
            s.keys.clear();
            s.index.clear();
        } else {
            self.sets.try_reserve(1).map_err(|_| Oom)?;
            self.sets.push(SetObj {
                keys: Vec::new(),
                index: HashSet::new(),
            });
        }
        self.n_sets += 1;
        Ok(SetId(id as u32))
    }

    pub fn set_len(&self, id: SetId) -> usize {
        self.sets[id.0 as usize].index.len()
    }

    /// The members in the order they were added.
    pub fn set_keys(&self, id: SetId) -> &[Value] {
        &self.sets[id.0 as usize].keys
    }

    pub fn set_has(&self, id: SetId, key: &[u8]) -> bool {
        self.sets[id.0 as usize].index.contains(key)
    }

    /// Adds a member under its key text, last in the order; false when the key is already there.
    pub fn set_insert(&mut self, id: SetId, key: &[u8], v: Value) -> Res<bool> {
        if self.set_has(id, key) {
            return Ok(false);
        }
        self.account(2 * ENTRY_COST + key.len())?;
        let s = &mut self.sets[id.0 as usize];
        s.index.try_reserve(1).map_err(|_| Oom)?;
        s.keys.try_reserve(1).map_err(|_| Oom)?;
        let mut owned = Vec::new();
        owned.try_reserve_exact(key.len()).map_err(|_| Oom)?;
        owned.extend_from_slice(key);
        s.index.insert(owned.into_boxed_slice());
        s.keys.push(v);
        Ok(true)
    }

    /// Forgets a key without touching the order list; true when it was there.
    pub fn set_forget(&mut self, id: SetId, key: &[u8]) -> bool {
        self.sets[id.0 as usize].index.remove(key)
    }

    /// Takes the `i`th member out of the order list without touching the keys.
    pub fn set_drop_order(&mut self, id: SetId, i: usize) {
        self.sets[id.0 as usize].keys.remove(i);
    }

    pub fn new_map(&mut self, reserve: usize) -> Res<MapId> {
        let id = self.n_maps;
        self.account(SLOT_COST)?;
        if id < self.maps.len() {
            let m = &mut self.maps[id];
            m.entries.clear();
            m.index.clear();
            m.entries.try_reserve(reserve).map_err(|_| Oom)?;
        } else {
            self.maps.try_reserve(1).map_err(|_| Oom)?;
            let mut entries = Vec::new();
            entries.try_reserve(reserve).map_err(|_| Oom)?;
            self.maps.push(MapObj {
                entries,
                index: Vec::new(),
            });
        }
        self.n_maps += 1;
        Ok(MapId(id as u32))
    }

    pub fn map_entries(&self, id: MapId) -> &[(Str, Value)] {
        &self.maps[id.0 as usize].entries
    }

    pub fn map_len(&self, id: MapId) -> usize {
        self.maps[id.0 as usize].entries.len()
    }

    fn hash(&self, key: &[u8]) -> u64 {
        self.seed.hash_one(key)
    }

    fn map_find(&self, id: MapId, key: Str) -> Option<usize> {
        let m = &self.maps[id.0 as usize];
        let kb = span(&self.consts, &self.bytes, key);
        if m.index.is_empty() {
            return m
                .entries
                .iter()
                .position(|&(k, _)| k.len == key.len && span(&self.consts, &self.bytes, k) == kb);
        }
        let mask = m.index.len() - 1;
        let mut slot = self.hash(kb) as usize & mask;
        loop {
            match m.index[slot] {
                0 => return None,
                e => {
                    let i = e as usize - 1;
                    let k = m.entries[i].0;
                    if k.len == key.len && span(&self.consts, &self.bytes, k) == kb {
                        return Some(i);
                    }
                }
            }
            slot = (slot + 1) & mask;
        }
    }

    pub fn map_get(&self, id: MapId, key: Str) -> Option<Value> {
        self.map_find(id, key)
            .map(|i| self.maps[id.0 as usize].entries[i].1)
    }

    pub fn map_contains(&self, id: MapId, key: Str) -> bool {
        self.map_find(id, key).is_some()
    }

    /// Sets `key`; true when the key is new.
    pub fn map_set(&mut self, id: MapId, key: Str, v: Value) -> Res<bool> {
        if let Some(i) = self.map_find(id, key) {
            self.maps[id.0 as usize].entries[i].1 = v;
            return Ok(false);
        }
        self.account(ENTRY_COST * 2)?;
        let (consts, bytes) = (&self.consts, &self.bytes);
        let seed = &self.seed;
        let m = &mut self.maps[id.0 as usize];
        m.entries.try_reserve(1).map_err(|_| Oom)?;
        let n = m.entries.len() + 1;
        if n > LINEAR_MAP && n * 2 > m.index.len() {
            let size = (n * 4).next_power_of_two().max(32);
            let mut index = Vec::new();
            index.try_reserve_exact(size).map_err(|_| Oom)?;
            index.resize(size, 0u32);
            let mask = size - 1;
            for (i, &(k, _)) in m.entries.iter().enumerate() {
                let mut slot = seed.hash_one(span(consts, bytes, k)) as usize & mask;
                while index[slot] != 0 {
                    slot = (slot + 1) & mask;
                }
                index[slot] = i as u32 + 1;
            }
            m.index = index;
        }
        m.entries.push((key, v));
        if !m.index.is_empty() {
            let mask = m.index.len() - 1;
            let mut slot = seed.hash_one(span(consts, bytes, key)) as usize & mask;
            while m.index[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            m.index[slot] = n as u32;
        }
        Ok(true)
    }

    pub fn new_scope(&mut self) -> Res<ScopeId> {
        if let Some(id) = self.free_scopes.pop() {
            let s = &mut self.scopes[id as usize];
            s.vars.clear();
            s.captured = false;
            return Ok(ScopeId(id));
        }
        self.account(SLOT_COST)?;
        let id = self.n_scopes;
        if id < self.scopes.len() {
            let s = &mut self.scopes[id];
            s.vars.clear();
            s.captured = false;
        } else {
            self.scopes.try_reserve(1).map_err(|_| Oom)?;
            self.scopes.push(ScopeObj {
                vars: Vec::new(),
                captured: false,
            });
        }
        self.n_scopes += 1;
        Ok(ScopeId(id as u32))
    }

    /// Gives a scope back once nothing can reach it: it was popped and no closure captured it.
    pub fn release_scope(&mut self, id: ScopeId) {
        if !self.scopes[id.0 as usize].captured && self.free_scopes.try_reserve(1).is_ok() {
            self.free_scopes.push(id.0);
        }
    }

    pub fn capture_scope(&mut self, id: ScopeId) {
        self.scopes[id.0 as usize].captured = true;
    }

    pub fn scope(&self, id: ScopeId) -> &ScopeObj {
        &self.scopes[id.0 as usize]
    }

    pub fn scope_get(&self, id: ScopeId, name: u32) -> Option<Value> {
        self.scopes[id.0 as usize]
            .vars
            .iter()
            .find(|&&(n, _)| n == name)
            .map(|&(_, v)| v)
    }

    pub fn scope_set(&mut self, id: ScopeId, name: u32, v: Value) -> Res<()> {
        let vars = &mut self.scopes[id.0 as usize].vars;
        if let Some(slot) = vars.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = v;
            return Ok(());
        }
        self.used = self.used.saturating_add(ENTRY_COST);
        if self.used > self.cap {
            return Err(Oom);
        }
        let vars = &mut self.scopes[id.0 as usize].vars;
        vars.try_reserve(1).map_err(|_| Oom)?;
        vars.push((name, v));
        Ok(())
    }

    pub fn new_func(&mut self, proto: u32, closure: ScopeId) -> Res<FuncId> {
        self.account(16)?;
        self.funcs.try_reserve(1).map_err(|_| Oom)?;
        self.funcs.push(FuncObj { proto, closure });
        Ok(FuncId(self.funcs.len() as u32 - 1))
    }

    pub fn func(&self, id: FuncId) -> &FuncObj {
        &self.funcs[id.0 as usize]
    }

    pub fn set_func_closure(&mut self, id: FuncId, closure: ScopeId) {
        self.funcs[id.0 as usize].closure = closure;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_of(h: &mut Heap, cap: usize, items: &[i64]) -> ListId {
        let id = h.new_list(items.len(), cap).unwrap();
        for &i in items {
            h.list_push(id, Value::Int(i)).unwrap();
        }
        id
    }

    fn int(i: i64) -> Value {
        Value::Int(i)
    }

    fn ints(h: &Heap, id: ListId) -> Vec<i64> {
        h.list(id)
            .items
            .iter()
            .map(|v| match v {
                Value::Int(i) => *i,
                _ => panic!("not an integer"),
            })
            .collect()
    }

    fn seen(h: &Heap, header: &Header) -> Vec<i64> {
        (0..header.len)
            .map(|i| match h.header_item(header, i) {
                Value::Int(n) => n,
                _ => panic!("not an integer"),
            })
            .collect()
    }

    #[test]
    fn a_pop_and_an_append_that_fits_change_what_an_earlier_header_reads() {
        let mut h = Heap::new(1 << 20);
        let l = list_of(&mut h, 4, &[1, 2, 3, 4]);
        let header = h.snapshot(l);
        h.list_pop(l);
        assert_eq!(seen(&h, &header), [1, 2, 3, 4]);
        h.list_append(l, &[int(9)]).unwrap();
        assert_eq!(ints(&h, l), [1, 2, 3, 9]);
        assert_eq!(seen(&h, &header), [1, 2, 3, 9]);
        h.list_mut(l).items[0] = int(7);
        assert_eq!(seen(&h, &header), [7, 2, 3, 9]);
    }

    #[test]
    fn an_append_past_the_capacity_moves_the_list_and_not_the_header() {
        let mut h = Heap::new(1 << 20);
        let l = list_of(&mut h, 2, &[1, 2]);
        let header = h.snapshot(l);
        let before = h.shape(l);
        h.list_append(l, &[int(3)]).unwrap();
        let after = h.shape(l);
        assert_ne!(before.arr, after.arr);
        assert_eq!(after.cap, 4);
        h.list_mut(l).items[0] = int(7);
        assert_eq!(ints(&h, l), [7, 2, 3]);
        assert_eq!(seen(&h, &header), [1, 2]);
    }

    #[test]
    fn a_shift_moves_the_window_and_takes_a_slot_of_the_capacity() {
        let mut h = Heap::new(1 << 20);
        let l = list_of(&mut h, 4, &[1, 2, 3]);
        let header = h.snapshot(l);
        let before = h.shape(l);
        assert!(matches!(h.list_shift(l), Some(Value::Int(1))));
        let after = h.shape(l);
        assert_eq!((after.arr, after.cap, after.len), (before.arr, 3, 2));
        h.list_append(l, &[int(4)]).unwrap();
        assert_eq!(h.shape(l).arr, before.arr);
        assert_eq!(ints(&h, l), [2, 3, 4]);
        assert_eq!(seen(&h, &header), [1, 2, 3]);
    }

    #[test]
    fn an_unshift_builds_a_new_array_of_the_exact_size() {
        let mut h = Heap::new(1 << 20);
        let l = list_of(&mut h, 16, &[2, 3]);
        let before = h.shape(l);
        h.list_unshift(l, &[int(0), int(1)]).unwrap();
        let after = h.shape(l);
        assert_ne!(before.arr, after.arr);
        assert_eq!((after.cap, after.len), (4, 4));
        assert_eq!(ints(&h, l), [0, 1, 2, 3]);
    }

    #[test]
    fn a_set_keeps_its_members_in_order_and_forgets_a_key_apart_from_the_order() {
        let mut h = Heap::new(1 << 20);
        let s = h.new_set().unwrap();
        assert!(h.set_insert(s, b"i:1", Value::Int(1)).unwrap());
        assert!(h.set_insert(s, b"s:a", Value::Null).unwrap());
        assert!(!h.set_insert(s, b"i:1", Value::Int(1)).unwrap());
        assert_eq!(h.set_len(s), 2);
        assert!(h.set_forget(s, b"i:1"));
        assert!(!h.set_has(s, b"i:1"));
        assert_eq!(h.set_keys(s).len(), 2);
        h.set_drop_order(s, 0);
        assert_eq!(h.set_keys(s).len(), 1);
    }
}
