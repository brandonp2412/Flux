fn add(left: i64, right: i64) -> i64 {
    return left + right
}

fn allPositive(current: bool, value: i64) -> bool {
    return current && value > 0
}

fn double(value: i64) -> i64 {
    return value * 2
}

fn greaterThanTwo(value: i64) -> bool {
    return value > 2
}

fn main() -> i64 {
    let values: i64[] = [1, 2, 3, 4, 5]
    let middle: i64[] = values[1:4]
    let evens: i64[] = values[::2]
    let reversed: i64[] = values[::-1]
    let reverseMiddle: i64[] = values[3:0:-2]
    let chained: i64[] = values[::-1][1:4:2]
    let reverseWindow: i64[] = values[::-1] | skip 1 | take 2
    let spreadValues: i64[] = [0, ...middle, ...reversed[::2], 9]
    print spreadValues.length
    print spreadValues.first
    print spreadValues.last
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
    print middle.isEmpty
    print middle.isNotEmpty
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
    let total: i64 = values | reduce add
    let positive: bool = values | fold true allPositive
    let emptyTotal: i64 = empty | fold 7 add
    print total
    print positive
    print emptyTotal
    let mapped: i64[] = reversed | map double
    let filtered: i64[] = values | filter greaterThanTwo
    let selected: i64[] = values | where greaterThanTwo
    let mappedFiltered: i64[] = values | map double | filter greaterThanTwo
    let mappedTotal: i64 = values | map double | reduce add
    let filteredTotal: i64 = values | map double | filter greaterThanTwo | reduce add
    let joined: i64[] = evens | concat reversed[:2]
    let joinedDoubled: i64[] = evens | concat reversed[:2] | map double
    let duplicated: i64[] = values | concat values[::-1]
    let uniqueValues: i64[] = duplicated | distinct
    let uniqueDoubled: i64[] = duplicated | distinct | map double
    let nested: i64[][] = [evens, reversed[:2]]
    let flattened: i64[] = nested | flatten
    let flattenedTotal: i64 = nested | flatten | reduce add
    let sortable: i64[] = [4, 1, 3, 2, 3]
    let ordered: i64[] = sortable[::-1] | sorted
    let orderedDoubled: i64[] = sortable[::-1] | sorted | map double
    let chunks: i64[][] = reversed | chunked 2
    let firstChunk: i64[] = chunks[0]
    let rejoined: i64[] = chunks | flatten
    print mapped.first
    print mapped.last
    print filtered.length
    print filtered.first
    print filtered.last
    print selected.length
    print mappedFiltered.length
    print mappedFiltered.first
    print mappedFiltered.last
    print mappedTotal
    print filteredTotal
    print joined.length
    print joined.first
    print joined.last
    print joinedDoubled.first
    print joinedDoubled.last
    print uniqueValues.length
    print uniqueValues.first
    print uniqueValues.last
    print uniqueDoubled.first
    print uniqueDoubled.last
    print flattened.length
    print flattened.first
    print flattened.last
    print flattenedTotal
    print sortable.first
    print sortable.last
    print ordered.first
    print ordered.last
    print orderedDoubled.first
    print orderedDoubled.last
    print chunks.length
    print firstChunk.first
    print firstChunk.last
    print rejoined.last
    for value in middle:
        print value
    for index, value in middle:
        print(index + value)
    return 0
}
