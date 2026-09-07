enum Outcome {
    Ok(i64)
    Error(str)
    Pending
}

fn pass(value: Outcome) -> Outcome {
    return value
}

fn main() -> i64 {
    let first: Outcome = Outcome.Ok(42)
    let second: Outcome = Outcome.Error("offline")
    let third: Outcome = Outcome.Pending()
    let carried: Outcome = pass(first)
    print(1)
    return 0
}
