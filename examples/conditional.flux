fn choose(flag: bool, value: i64 = 5) -> i64 {
    if flag:
        return value
    return 7
}

fn main() -> i64 {
    let enabled: bool = false
    let value: i64 = choose(enabled)
    print(value)
    return value
}
