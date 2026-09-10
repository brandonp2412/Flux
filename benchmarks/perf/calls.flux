fn adjust(total: i64, value: i64) -> i64 {
    if total > 1000000:
        return total / 2 + value
    else:
        return total + value
}

fn main() -> i64 {
    var total: i64 = 0
    for value in 0..100000000:
        total = adjust(total, value)
    print(total)
    return 0
}
