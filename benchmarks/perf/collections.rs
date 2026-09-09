fn main() {
    let mut total = 0_i64;
    for iteration in 0_i64..50_000_000 {
        let values = [iteration, 2, 3, 4, 5, 6, 7, 8];
        let partial = values
            .iter()
            .map(|value| value.checked_mul(2).expect("benchmark overflow"))
            .filter(|value| *value > 5)
            .try_fold(0_i64, |sum, value| sum.checked_add(value))
            .expect("benchmark overflow");
        total = total.checked_add(partial).expect("benchmark overflow");
    }
    println!("{total}");
}
