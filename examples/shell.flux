fn increment(value: i64) -> i64 {
    return value + 1
}

fn scale(value: i64, factor: i64) -> i64 {
    return value * factor
}

fn message() -> str {
    return "hello"
}

fn main() -> i64 {
    let result: i64 = increment 2 | scale 5
    print result
    message > "/tmp/flux-shell-output.txt"
    message >> "/tmp/flux-shell-output.txt"
    increment 1 | print &
    return 0
}
