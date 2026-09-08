fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let middle: i64[] = values[1:4]
    print middle[0]
    print values[-1]
    let doubled: i64[] = [value * 2 for value in values if value > 2]
    print doubled[0]
    print doubled[-1]
    return 0
}
