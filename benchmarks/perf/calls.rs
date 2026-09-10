fn adjust(total: i64, value: i64) -> i64 {
    if total > 1_000_000 {
        (total / 2).checked_add(value).expect("benchmark overflow")
    } else {
        total.checked_add(value).expect("benchmark overflow")
    }
}

fn main() {
    let mut total: i64 = 0;
    for value in 0_i64..100_000_000 {
        total = adjust(total, value);
    }
    println!("{total}");
}
