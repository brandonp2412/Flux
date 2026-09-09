fn add(total: i64, value: i64) -> i64 {
    return total + value
}

fn double(value: i64) -> i64 {
    return value * 2
}

fn greaterThanFive(value: i64) -> bool {
    return value > 5
}

fn main() -> i64 {
    var total: i64 = 0
    for iteration in 0..50000000:
        let values: i64[] = [iteration, 2, 3, 4, 5, 6, 7, 8]
        let partial: i64 = values | map double | filter greaterThanFive | fold 0 add
        total = total + partial
    print(total)
    return 0
}
