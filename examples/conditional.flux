const FALLBACK: i64 = 7 if true else 9

fn choose(flag: bool, value: i64 = 5 if true else 6) -> i64 {
    return value if flag else FALLBACK
}

fn main() -> i64 {
    let enabled: bool = false
    let value: i64 = choose(enabled)
    let nested: i64 = 1 if enabled else 2 if value == 7 else 3
    print(value)
    print(nested)
    return 0
}
