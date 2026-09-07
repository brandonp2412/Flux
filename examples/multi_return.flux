fn divide(value: i64, by: i64) -> (i64, bool) {
    if by == 0:
        return 0, false
    return value / by, true
}

fn main() -> i64 {
    let result: i64, ok: bool = divide(84, 2)
    if ok:
        print(result)

    let failed: i64, failed_ok: bool = divide(1, 0)
    if !failed_ok:
        print("recoverable error")
    return failed
}
