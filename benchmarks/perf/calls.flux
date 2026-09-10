fn classify(value: i64) -> i64 {
    if value < 25000000:
        return 1
    elif value < 75000000:
        return 2
    else:
        return 3
}

fn main() -> i64 {
    var total: i64 = 0
    for value in 0..100000000:
        total = total + classify(value)
    print(total)
    return 0
}
