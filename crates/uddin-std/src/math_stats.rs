//! Math, aggregates: abs clamp gcd lcm lerp max min mean median mode sign std_dev sum variance.
//!
//! Sums and means add as 64-bit floats in the order of the list, so a result carries the rounding of that
//! order; a whole number that comes back as an integer goes through the platform's float to integer conversion.

use std::collections::HashMap;

use uddin_vm::{
    BuiltinDef, Count, Exc, LimitReason, ListId, Rt, Stringify, Value, float_to_int, to_text,
    type_name,
};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("abs", abs, 1).fast_path(),
    BuiltinDef::new("clamp", clamp, 3),
    BuiltinDef::new("gcd", gcd, 2),
    BuiltinDef::new("lcm", lcm, 2),
    BuiltinDef::new("lerp", lerp, 3),
    BuiltinDef::new("max", max, -1).deep(),
    BuiltinDef::new("min", min, -1).deep(),
    BuiltinDef::new("mean", mean, -1),
    BuiltinDef::new("median", median, -1),
    BuiltinDef::new("mode", mode, -1)
        .deep()
        .stringify(Stringify::Text),
    BuiltinDef::new("sign", sign, 1).fast_path(),
    BuiltinDef::new("std_dev", std_dev, -1),
    BuiltinDef::new("sum", sum, -1),
    BuiltinDef::new("variance", variance, -1),
];

/// The value of an integer or a float as a float.
pub(crate) fn number(v: Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(i as f64),
        Value::Float(f) => Some(f),
        _ => None,
    }
}

/// The value of an integer, or a float cut to one the way the platform does it.
fn integer(v: Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(i),
        Value::Float(f) => Some(float_to_int(f)),
        _ => None,
    }
}

/// The error value of a builtin that was given something other than a number. The reference returns it, so a
/// script can keep it, print it, and have `if` raise it.
pub(crate) fn not_a_number(rt: &mut Rt<'_>, name: &str, got: Value) -> Result<Value, Exc> {
    let text = Exc::type_error(
        rt.pos,
        format!("{name}() requires a number, got {}", type_name(got)),
    )
    .text();
    rt.error_value(text.as_bytes())
}

fn abs(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error("abs", 1, args.len()));
    };
    match *arg {
        Value::Int(i) => Ok(Value::Int(if i < 0 { i.wrapping_neg() } else { i })),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        other => Err(rt.type_error(format!("abs() requires a number, got {}", type_name(other)))),
    }
}

fn sign(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error("sign", 1, args.len()));
    };
    let Some(x) = number(*arg) else {
        return not_a_number(rt, "sign", *arg);
    };
    Ok(Value::Int(if x > 0.0 {
        1
    } else if x < 0.0 {
        -1
    } else {
        0
    }))
}

fn clamp(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let [value, low, high] = args else {
        return Err(rt.arity_error("clamp", 3, args.len()));
    };
    let mut x = [0.0; 3];
    for (slot, v) in x.iter_mut().zip(args) {
        let Some(n) = number(*v) else {
            return not_a_number(rt, "clamp", *v);
        };
        *slot = n;
    }
    let [mut val, low_f, high_f] = x;
    if low_f > high_f {
        return Err(Exc::value_error(
            rt.pos,
            "clamp() min must be less than or equal to max",
        ));
    }
    if val < low_f {
        val = low_f;
    } else if val > high_f {
        val = high_f;
    }
    if matches!(
        (*value, *low, *high),
        (Value::Int(_), Value::Int(_), Value::Int(_))
    ) {
        return Ok(Value::Int(float_to_int(val)));
    }
    Ok(Value::Float(val))
}

fn lerp(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() != 3 {
        return Err(rt.arity_error("lerp", 3, args.len()));
    }
    let mut x = [0.0; 3];
    for (slot, v) in x.iter_mut().zip(args) {
        let Some(n) = number(*v) else {
            return not_a_number(rt, "lerp", *v);
        };
        *slot = n;
    }
    let [a, b, t] = x;
    Ok(Value::Float(a + t * (b - a)))
}

/// Greatest common divisor of the magnitudes, which go through a float: past 2^53 they lose their low bits, and
/// the most negative integer stays negative.
fn gcd_of(a: i64, b: i64) -> i64 {
    let magnitude = |n: i64| float_to_int((n as f64).abs());
    let (mut a, mut b) = (magnitude(a), magnitude(b));
    while b != 0 {
        (a, b) = (b, a.wrapping_rem(b));
    }
    a
}

fn two_integers(
    rt: &mut Rt<'_>,
    args: &[Value],
    name: &str,
) -> Result<Result<(i64, i64), Value>, Exc> {
    let mut x = [0; 2];
    for (slot, v) in x.iter_mut().zip(args) {
        let Some(n) = integer(*v) else {
            return not_a_number(rt, name, *v).map(Err);
        };
        *slot = n;
    }
    Ok(Ok((x[0], x[1])))
}

fn gcd(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() != 2 {
        return Err(rt.arity_error("gcd", 2, args.len()));
    }
    match two_integers(rt, args, "gcd")? {
        Ok((a, b)) => Ok(Value::Int(gcd_of(a, b))),
        Err(error) => Ok(error),
    }
}

fn lcm(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    if args.len() != 2 {
        return Err(rt.arity_error("lcm", 2, args.len()));
    }
    let (a, b) = match two_integers(rt, args, "lcm")? {
        Ok(pair) => pair,
        Err(error) => return Ok(error),
    };
    if a == 0 || b == 0 {
        return Ok(Value::Int(0));
    }
    let product = float_to_int((a.wrapping_mul(b) as f64).abs());
    Ok(Value::Int(product.wrapping_div(gcd_of(a, b))))
}

/// The list in the first argument of a builtin of one argument, or the error that says what it needs.
fn one_list(rt: &Rt<'_>, args: &[Value], name: &str) -> Result<ListId, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error(name, 1, args.len()));
    };
    match *arg {
        Value::List(l) => Ok(l),
        _ => Err(rt.type_error(format!("{name}() requires an array"))),
    }
}

/// The elements of a list as floats, in order.
fn floats(rt: &mut Rt<'_>, list: ListId, name: &str) -> Result<Vec<f64>, Exc> {
    let n = rt.heap.list(list).items.len();
    rt.work(Count::Free, n as u64)?;
    let mut out = Vec::new();
    out.try_reserve_exact(n)
        .map_err(|_| Exc::limit(LimitReason::Memory, rt.pos))?;
    for &v in &rt.heap.list(list).items {
        match number(v) {
            Some(x) => out.push(x),
            None => {
                return Err(rt.type_error(format!("{name}() array must contain only numbers")));
            }
        }
    }
    Ok(out)
}

fn sum(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let list = one_list(rt, args, "sum")?;
    rt.work(Count::Free, rt.heap.list(list).items.len() as u64)?;
    let (mut total, mut has_float) = (0.0, false);
    for &v in &rt.heap.list(list).items {
        match v {
            Value::Int(i) => total += i as f64,
            Value::Float(f) => {
                total += f;
                has_float = true;
            }
            _ => return Err(rt.type_error("sum() array must contain only numbers")),
        }
    }
    Ok(if has_float {
        Value::Float(total)
    } else {
        Value::Int(float_to_int(total))
    })
}

fn mean(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let list = one_list(rt, args, "mean")?;
    if rt.heap.list(list).items.is_empty() {
        return Err(Exc::value_error(rt.pos, "mean() of empty array"));
    }
    let xs = floats(rt, list, "mean")?;
    let total = xs.iter().fold(0.0, |t, x| t + x);
    Ok(Value::Float(total / xs.len() as f64))
}

fn median(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let list = one_list(rt, args, "median")?;
    if rt.heap.list(list).items.is_empty() {
        return Err(Exc::value_error(rt.pos, "median() of empty array"));
    }
    let mut xs = floats(rt, list, "median")?;
    let n = xs.len();
    // The work of the sort is reported before it starts: it cannot be interrupted.
    rt.work(
        Count::Free,
        n as u64 * u64::from(usize::BITS - n.leading_zeros()),
    )?;
    sort_floats(&mut xs);
    Ok(Value::Float(if n % 2 == 0 {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    } else {
        xs[n / 2]
    }))
}

/// The variance of the numbers in a list, the sum of the squared distances to the mean over one less than their
/// count, and zero for fewer than two.
fn variance_of(rt: &mut Rt<'_>, args: &[Value], name: &str) -> Result<Option<f64>, Exc> {
    let list = one_list(rt, args, name)?;
    if rt.heap.list(list).items.len() <= 1 {
        return Ok(None);
    }
    let xs = floats(rt, list, name)?;
    let mean = xs.iter().fold(0.0, |t, x| t + x) / xs.len() as f64;
    let squares = xs.iter().fold(0.0, |t, x| {
        let diff = x - mean;
        t + diff * diff
    });
    Ok(Some(squares / (xs.len() - 1) as f64))
}

fn variance(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Float(
        variance_of(rt, args, "variance")?.unwrap_or(0.0),
    ))
}

fn std_dev(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    Ok(Value::Float(
        variance_of(rt, args, "std_dev")?.map_or(0.0, f64::sqrt),
    ))
}

/// The value that occurs most often, two values being the same when they print the same: `1` and `1.0` are one,
/// and the one that comes last is the one returned. A tie goes to the value that reached the highest count first,
/// where the reference picks any.
fn mode(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let list = one_list(rt, args, "mode")?;
    let n = rt.heap.list(list).items.len();
    if n == 0 {
        return Err(Exc::value_error(rt.pos, "mode() of empty array"));
    }
    // How many times each key occurred. The key that reaches the highest count first is the best so far; the
    // value kept for it is the last one seen with that key.
    let mut counts: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut key = Vec::new();
    let mut best: Option<(usize, Value, Vec<u8>)> = None;
    for i in 0..n {
        let v = rt.heap.list(list).items[i];
        key.clear();
        to_text(rt.env, rt.heap, rt.meter, v, true, 0, &mut key)?;
        let count = match counts.get_mut(key.as_slice()) {
            Some(c) => {
                *c += 1;
                *c
            }
            None => {
                counts.insert(key.clone(), 1);
                1
            }
        };
        match &mut best {
            Some((top, value, top_key)) if *top_key == key => {
                *top = count;
                *value = v;
            }
            Some((top, ..)) if count <= *top => {}
            _ => best = Some((count, v, key.clone())),
        }
    }
    Ok(best.map_or(Value::Null, |(_, v, _)| v))
}

fn extreme(rt: &mut Rt<'_>, args: &[Value], name: &str, want_greater: bool) -> Result<Value, Exc> {
    if args.is_empty() {
        return Err(rt.type_error(format!("{name}() requires at least 1 argument")));
    }
    // The candidates are the elements of a list given first, or else the arguments.
    let list = match args[0] {
        Value::List(l) => Some(l),
        _ => None,
    };
    let count = match list {
        Some(l) => rt.heap.list(l).items.len(),
        None => args.len(),
    };
    if count == 0 {
        return Err(Exc::value_error(
            rt.pos,
            format!("{name}() cannot be applied to empty array"),
        ));
    }
    let at = |rt: &Rt<'_>, i: usize| match list {
        Some(l) => rt.heap.list(l).items[i],
        None => args[i],
    };
    let mut best = at(rt, 0);
    for i in 1..count {
        let v = at(rt, i);
        let replace = if want_greater {
            rt.less(best, v)?
        } else {
            rt.less(v, best)?
        };
        if replace {
            best = v;
        }
    }
    Ok(best)
}

fn max(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    extreme(rt, args, "max", true)
}

fn min(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    extreme(rt, args, "min", false)
}

/// Not-a-number values order before every other, as the standard slice sort of the reference does.
fn less(a: f64, b: f64) -> bool {
    a.is_nan() && !b.is_nan() || a < b
}

/// The sort of the reference for floats, a pattern-defeating quicksort, so equal values (`0` and `-0`) end in
/// the order it leaves them.
fn sort_floats(data: &mut [f64]) {
    let n = data.len();
    pdqsort(data, 0, n, (usize::BITS - n.leading_zeros()) as usize);
}

#[derive(PartialEq)]
enum Hint {
    Unknown,
    Increasing,
    Decreasing,
}

fn insertion_sort(d: &mut [f64], a: usize, b: usize) {
    for i in a + 1..b {
        let mut j = i;
        while j > a && less(d[j], d[j - 1]) {
            d.swap(j, j - 1);
            j -= 1;
        }
    }
}

fn sift_down(d: &mut [f64], lo: usize, hi: usize, first: usize) {
    let mut root = lo;
    loop {
        let mut child = 2 * root + 1;
        if child >= hi {
            break;
        }
        if child + 1 < hi && less(d[first + child], d[first + child + 1]) {
            child += 1;
        }
        if !less(d[first + root], d[first + child]) {
            return;
        }
        d.swap(first + root, first + child);
        root = child;
    }
}

fn heap_sort(d: &mut [f64], a: usize, b: usize) {
    let (first, hi) = (a, b - a);
    for i in (0..=(hi.saturating_sub(1)) / 2).rev() {
        sift_down(d, i, hi, first);
    }
    for i in (0..hi).rev() {
        d.swap(first, first + i);
        sift_down(d, 0, i, first);
    }
}

fn pdqsort(d: &mut [f64], mut a: usize, mut b: usize, mut limit: usize) {
    const MAX_INSERTION: usize = 12;
    let mut was_balanced = true;
    let mut was_partitioned = true;
    loop {
        let length = b - a;
        if length <= MAX_INSERTION {
            insertion_sort(d, a, b);
            return;
        }
        if limit == 0 {
            heap_sort(d, a, b);
            return;
        }
        if !was_balanced {
            break_patterns(d, a, b);
            limit -= 1;
        }
        let (mut pivot, mut hint) = choose_pivot(d, a, b);
        if hint == Hint::Decreasing {
            d[a..b].reverse();
            pivot = (b - 1) - (pivot - a);
            hint = Hint::Increasing;
        }
        if was_balanced
            && was_partitioned
            && hint == Hint::Increasing
            && partial_insertion_sort(d, a, b)
        {
            return;
        }
        if a > 0 && !less(d[a - 1], d[pivot]) {
            a = partition_equal(d, a, b, pivot);
            continue;
        }
        let (mid, already) = partition(d, a, b, pivot);
        was_partitioned = already;
        let (left_len, right_len) = (mid - a, b - mid);
        let balance_threshold = length / 8;
        if left_len < right_len {
            was_balanced = left_len >= balance_threshold;
            pdqsort(d, a, mid, limit);
            a = mid + 1;
        } else {
            was_balanced = right_len >= balance_threshold;
            pdqsort(d, mid + 1, b, limit);
            b = mid;
        }
    }
}

fn partition(d: &mut [f64], a: usize, b: usize, pivot: usize) -> (usize, bool) {
    d.swap(a, pivot);
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    while i <= j && less(d[i as usize], d[a]) {
        i += 1;
    }
    while i <= j && !less(d[j as usize], d[a]) {
        j -= 1;
    }
    if i > j {
        d.swap(j as usize, a);
        return (j as usize, true);
    }
    d.swap(i as usize, j as usize);
    i += 1;
    j -= 1;
    loop {
        while i <= j && less(d[i as usize], d[a]) {
            i += 1;
        }
        while i <= j && !less(d[j as usize], d[a]) {
            j -= 1;
        }
        if i > j {
            break;
        }
        d.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    d.swap(j as usize, a);
    (j as usize, false)
}

fn partition_equal(d: &mut [f64], a: usize, b: usize, pivot: usize) -> usize {
    d.swap(a, pivot);
    let (mut i, mut j) = (a as isize + 1, b as isize - 1);
    loop {
        while i <= j && !less(d[a], d[i as usize]) {
            i += 1;
        }
        while i <= j && less(d[a], d[j as usize]) {
            j -= 1;
        }
        if i > j {
            break;
        }
        d.swap(i as usize, j as usize);
        i += 1;
        j -= 1;
    }
    i as usize
}

fn partial_insertion_sort(d: &mut [f64], a: usize, b: usize) -> bool {
    const MAX_STEPS: usize = 5;
    const SHORTEST_SHIFTING: usize = 50;
    let mut i = a + 1;
    for _ in 0..MAX_STEPS {
        while i < b && !less(d[i], d[i - 1]) {
            i += 1;
        }
        if i == b {
            return true;
        }
        if b - a < SHORTEST_SHIFTING {
            return false;
        }
        d.swap(i, i - 1);
        if i - a >= 2 {
            for j in (1..i).rev() {
                if !less(d[j], d[j - 1]) {
                    break;
                }
                d.swap(j, j - 1);
            }
        }
        if b - i >= 2 {
            for j in i + 1..b {
                if !less(d[j], d[j - 1]) {
                    break;
                }
                d.swap(j, j - 1);
            }
        }
    }
    false
}

fn break_patterns(d: &mut [f64], a: usize, b: usize) {
    let length = b - a;
    if length >= 8 {
        let mut random = length as u64;
        let modulus = 1u64 << (usize::BITS - length.leading_zeros());
        let mid = a + (length / 4) * 2;
        for idx in mid - 1..=mid + 1 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let mut other = (random & (modulus - 1)) as usize;
            if other >= length {
                other -= length;
            }
            d.swap(idx, a + other);
        }
    }
}

fn choose_pivot(d: &[f64], a: usize, b: usize) -> (usize, Hint) {
    const SHORTEST_NINTHER: usize = 50;
    const MAX_SWAPS: usize = 4 * 3;
    let l = b - a;
    let mut swaps = 0;
    let mut i = a + l / 4;
    let mut j = a + l / 4 * 2;
    let mut k = a + l / 4 * 3;
    if l >= 8 {
        if l >= SHORTEST_NINTHER {
            i = median_of(d, i - 1, i, i + 1, &mut swaps);
            j = median_of(d, j - 1, j, j + 1, &mut swaps);
            k = median_of(d, k - 1, k, k + 1, &mut swaps);
        }
        j = median_of(d, i, j, k, &mut swaps);
    }
    match swaps {
        0 => (j, Hint::Increasing),
        MAX_SWAPS => (j, Hint::Decreasing),
        _ => (j, Hint::Unknown),
    }
}

fn order2(d: &[f64], a: usize, b: usize, swaps: &mut usize) -> (usize, usize) {
    if less(d[b], d[a]) {
        *swaps += 1;
        return (b, a);
    }
    (a, b)
}

fn median_of(d: &[f64], a: usize, b: usize, c: usize, swaps: &mut usize) -> usize {
    let (a, b) = order2(d, a, b, swaps);
    let (b, _) = order2(d, b, c, swaps);
    order2(d, a, b, swaps).1
}
