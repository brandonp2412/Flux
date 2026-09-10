fn fibonacci(value: i64) -> i64 {
    if value < 2:
        return value
    return fibonacci(value - 1) + fibonacci(value - 2)
}

fn main() -> i64 {
    let result: i64 = fibonacci(38)
    print(result)
    return 0
}
