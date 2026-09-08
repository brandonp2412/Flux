enum Outcome {
    Ok(i64)
    Error(str)
    Pending
}

fn score(outcome: Outcome) -> i64 {
    return match outcome:
        Outcome.Ok(value): value
        Outcome.Error(_): -1
        Outcome.Pending(): 0
}

fn main() -> i64 {
    let outcome: Outcome = Outcome.Ok(42)
    let value: i64 = match outcome:
        Outcome.Ok(payload): payload + 1
        Outcome.Error(_): -1
        Outcome.Pending(): 0
    print(value)
    return score(outcome)
}
