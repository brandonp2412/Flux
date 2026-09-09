fn main() {
    let mut total: i64 = 0;
    for value in 0_i64..100_000_000 {
        total = total.checked_add(value).expect("benchmark overflow");
    }
    println!("{total}");
}
