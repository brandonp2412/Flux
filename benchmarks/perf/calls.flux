fn adjust(value: i64) -> i64 {
    return value + 3
}

fn main() -> i64 {
    var total: i64 = 0
    for value in 0..100000000:
        total = total + adjust(value)
    print(total)
    return 0
}
