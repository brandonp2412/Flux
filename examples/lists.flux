fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let middle: i64[] = values[1:4]
    let evens: i64[] = values[::2]
    let reversed: i64[] = values[::-1]
    let reverseMiddle: i64[] = values[3:0:-2]
    let chained: i64[] = values[::-1][1:4:2]
    let reverseWindow: i64[] = values[::-1] | skip 1 | take 2
    print evens.first
    print evens.last
    print reversed.first
    print reversed.last
    print reverseMiddle.first
    print reverseMiddle.last
    print chained.first
    print chained.last
    print reverseWindow.first
    print reverseWindow.last
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
    let safeFirst: i64 = empty | first_or 99
    let safeLast: i64 = empty | last_or 88
    print safeFirst
    print safeLast
    let checks: bool[] = [value > 2 for value in values]
    let hasLarge: bool = checks | any
    let allLarge: bool = checks | every
    print hasLarge
    print allLarge
    let noChecks: bool[] = checks[:0]
    let emptyAny: bool = noChecks | any
    let emptyEvery: bool = noChecks | every
    print emptyAny
    print emptyEvery
    for value in middle:
        print value
    for index, value in middle:
        print(index + value)
    return 0
}
