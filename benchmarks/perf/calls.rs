fn classify(value: i64) -> i64 {
    if value < 25_000_000 {
        1
    } else if value < 75_000_000 {
        2
    } else {
        3
    }
}

fn main() {
    let mut total: i64 = 0;
    for value in 0_i64..100_000_000 {
        total = total.checked_add(classify(value)).expect("benchmark overflow");
    }
    println!("{total}");
}
