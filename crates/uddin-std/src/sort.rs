//! Sort: sort.
//!
//! The reference sorts with a stable sort of its standard library: insertion sort on blocks of 20, then merges
//! by symmetric comparison and rotation. The order of the comparisons and the swaps is part of the behaviour: a
//! comparison that fails (two kinds that do not compare) stops the sort where it stands, and the list keeps the
//! order the swaps made until then. `slice::sort_by` is not used, as its comparisons come in another order and
//! it may panic on an order that is not total.

use uddin_vm::{BuiltinDef, Count, Exc, LimitReason, ListId, Rt, Value};

pub(crate) const TABLE: &[BuiltinDef] = &[BuiltinDef::new("sort", sort, -1).deep().calls()];

/// Sorts a list in place, by its elements or, given a function, by what the function returns for each. The
/// function runs once for each element, in order, before any comparison; the sort is stable.
fn sort(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() != 1 && args.len() != 2 {
        return Err(rt.type_error(format!("sort() requires 1 or 2 args, got {}", args.len())));
    }
    let Value::List(list) = args[0] else {
        return Err(rt.type_error("sort() requires first argument to be a array"));
    };
    let n = rt.heap.list(list).items.len();
    if n <= 1 {
        return Ok(Value::Null);
    }
    if let Some(&key) = args.get(1) {
        return sort_by_key(rt, list, key);
    }
    let mut data = Vec::new();
    let pos = rt.pos;
    data.try_reserve_exact(n)
        .map_err(|_| Exc::limit(LimitReason::Memory, pos))?;
    data.extend_from_slice(&rt.heap.list(list).items);
    let sorted = stable(&mut data, &mut |a, b| {
        rt.work(Count::Free, 1)?;
        rt.less(a, b)
    });
    rt.heap.list_mut(list).items.copy_from_slice(&data);
    sorted?;
    Ok(Value::Null)
}

/// The form with a key function. The keys come first, each checked for depth and cycles; the list is replaced by
/// a new array of the sorted elements only when every comparison went through.
fn sort_by_key(rt: &mut Rt<'_>, list: ListId, key: Value) -> Result<Value, Exc> {
    if !matches!(key, Value::Func(_) | Value::Builtin(_)) {
        return Err(rt.type_error("sort() requires second argument to be a function"));
    }
    let header = rt.heap.snapshot(list);
    let mut pairs: Vec<(Value, Value)> = Vec::new();
    let pos = rt.pos;
    pairs
        .try_reserve_exact(header.len)
        .map_err(|_| Exc::limit(LimitReason::Memory, pos))?;
    for i in 0..header.len {
        let v = rt.heap.header_item(&header, i);
        let k = rt.call(key, &[v])?;
        rt.guard_deep(&[k])?;
        pairs.push((v, k));
    }
    stable(&mut pairs, &mut |a, b| {
        rt.work(Count::Free, 1)?;
        rt.less(a.1, b.1)
    })?;
    let sorted: Vec<Value> = pairs.iter().map(|p| p.0).collect();
    rt.heap.list_replace(list, &sorted).map_err(rt.oom())?;
    Ok(Value::Null)
}

/// A stable sort of `data` by `less`, in the order of comparisons and swaps of the reference's stable sort.
pub(crate) fn stable<T: Copy, E>(
    data: &mut [T],
    less: &mut dyn FnMut(T, T) -> Result<bool, E>,
) -> Result<(), E> {
    let n = data.len();
    let mut sorter = Sorter { data, less };
    let mut block = 20;
    let (mut a, mut b) = (0, block);
    while b <= n {
        sorter.insertion_sort(a, b)?;
        a = b;
        b += block;
    }
    sorter.insertion_sort(a, n)?;
    while block < n {
        a = 0;
        b = 2 * block;
        while b <= n {
            sorter.sym_merge(a, a + block, b)?;
            a = b;
            b += 2 * block;
        }
        let m = a + block;
        if m < n {
            sorter.sym_merge(a, m, n)?;
        }
        block *= 2;
    }
    Ok(())
}

struct Sorter<'a, T, E> {
    data: &'a mut [T],
    less: &'a mut dyn FnMut(T, T) -> Result<bool, E>,
}

impl<T: Copy, E> Sorter<'_, T, E> {
    fn less(&mut self, i: usize, j: usize) -> Result<bool, E> {
        (self.less)(self.data[i], self.data[j])
    }

    fn insertion_sort(&mut self, a: usize, b: usize) -> Result<(), E> {
        for i in a + 1..b {
            let mut j = i;
            while j > a && self.less(j, j - 1)? {
                self.data.swap(j, j - 1);
                j -= 1;
            }
        }
        Ok(())
    }

    /// Merges the sorted runs `a..m` and `m..b`.
    fn sym_merge(&mut self, a: usize, m: usize, b: usize) -> Result<(), E> {
        if m - a == 1 {
            let (mut i, mut j) = (m, b);
            while i < j {
                let h = (i + j) >> 1;
                if self.less(h, a)? {
                    i = h + 1;
                } else {
                    j = h;
                }
            }
            for k in a..i - 1 {
                self.data.swap(k, k + 1);
            }
            return Ok(());
        }
        if b - m == 1 {
            let (mut i, mut j) = (a, m);
            while i < j {
                let h = (i + j) >> 1;
                if !self.less(m, h)? {
                    i = h + 1;
                } else {
                    j = h;
                }
            }
            for k in (i + 1..=m).rev() {
                self.data.swap(k, k - 1);
            }
            return Ok(());
        }
        let mid = (a + b) >> 1;
        let n = mid + m;
        let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
        let p = n - 1;
        while start < r {
            let c = (start + r) >> 1;
            if !self.less(p - c, c)? {
                start = c + 1;
            } else {
                r = c;
            }
        }
        let end = n - start;
        if start < m && m < end {
            self.rotate(start, m, end);
        }
        if a < start && start < mid {
            self.sym_merge(a, start, mid)?;
        }
        if mid < end && end < b {
            self.sym_merge(mid, end, b)?;
        }
        Ok(())
    }

    /// Turns `x u v y` into `x v u y` for the blocks `u = a..m` and `v = m..b`.
    fn rotate(&mut self, a: usize, m: usize, b: usize) {
        let mut i = m - a;
        let mut j = b - m;
        while i != j {
            if i > j {
                self.swap_range(m - i, m, j);
                i -= j;
            } else {
                self.swap_range(m - i, m + j - i, i);
                j -= i;
            }
        }
        self.swap_range(m - i, m, i);
    }

    fn swap_range(&mut self, a: usize, b: usize, n: usize) {
        for i in 0..n {
            self.data.swap(a + i, b + i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(values: &[i64]) -> Vec<i64> {
        let mut data: Vec<Value> = values.iter().map(|&v| Value::Int(v)).collect();
        let mut less = |a: Value, b: Value| -> Result<bool, ()> {
            match (a, b) {
                (Value::Int(x), Value::Int(y)) => Ok(x < y),
                _ => Err(()),
            }
        };
        stable(&mut data, &mut less).unwrap();
        data.iter()
            .map(|v| match v {
                Value::Int(i) => *i,
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn sorts_lists_of_every_small_length() {
        for n in 0..100i64 {
            let values: Vec<i64> = (0..n).map(|i| (i * 37 + 11) % 23 - 9).collect();
            let mut want = values.clone();
            want.sort();
            assert_eq!(order(&values), want, "length {n}");
        }
    }

    #[test]
    fn a_failing_comparison_stops_the_sort() {
        let mut data: Vec<Value> = [3, 2, 1].iter().map(|&v| Value::Int(v)).collect();
        let mut calls = 0;
        let mut less = |_: Value, _: Value| -> Result<bool, &'static str> {
            calls += 1;
            if calls == 2 { Err("stop") } else { Ok(true) }
        };
        assert_eq!(stable(&mut data, &mut less), Err("stop"));
    }
}
