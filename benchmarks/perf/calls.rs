fn adjust(value: i64) -> i64 {
    value.checked_add(3).expect("benchmark overflow")
}

fn main() {
    let mut total: i64 = 0;
    for value in 0_i64..100_000_000 {
        total = total.checked_add(adjust(value)).expect("benchmark overflow");
    }
    println!("{total}");
}
