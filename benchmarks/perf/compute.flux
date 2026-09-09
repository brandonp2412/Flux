fn main() -> i64 {
    var total: i64 = 0
    for value in 0..100000000:
        total = total + value
    print(total)
    return 0
}
