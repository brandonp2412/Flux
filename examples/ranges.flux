fn main() -> i64 {
    var total: i64 = 0
    for value in 2..=4:
        total = total + value
    for value in 0..2:
        total = total + value
    print(total)
    return 0
}
