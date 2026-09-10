fn sumRange(limit: i64) -> i64 {
    var total: i64 = 0
    for value in 0..limit:
        total = total + value
    return total
}

fn main() -> i64 {
    let total: i64 = sumRange(100000000)
    print(total)
    return 0
}
