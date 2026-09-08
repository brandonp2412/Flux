fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let middle: i64[] = values[1:4]
    print values.length
    print middle.is_empty
    print middle.is_not_empty
    print values.first
    print values.last
    let one: i64[] = values[2:3]
    print one.single
    print middle[0]
    print values[-1]
    let doubled: i64[] = [value * 2 for value in values if value > 2]
    print doubled[0]
    print doubled[-1]
    let window: i64[] = values | skip 1 | take 3
    print window.length
    print window.first
    print window.last
    let empty: i64[] = values[:0]
    let safe_first: i64 = empty | first_or 99
    let safe_last: i64 = empty | last_or 88
    print safe_first
    print safe_last
    let checks: bool[] = [value > 2 for value in values]
    let has_large: bool = checks | any
    let all_large: bool = checks | every
    print has_large
    print all_large
    let no_checks: bool[] = checks[:0]
    let empty_any: bool = no_checks | any
    let empty_every: bool = no_checks | every
    print empty_any
    print empty_every
    for value in middle:
        print value
    for index, value in middle:
        print(index + value)
    return 0
}
