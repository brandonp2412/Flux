fn main() -> i64 {
    var count: i64 = 0
    var total: i64 = 0
    while count < 6:
        count = count + 1
        if count == 3:
            continue
        total = total + count
        if total > 10:
            break
    print(total)
    return total
}
