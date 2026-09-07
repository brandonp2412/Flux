enum Outcome {
    Ok(i64)
    Error(str)
    Pending
}

fn score(outcome: Outcome) -> i64 {
    match outcome:
        Outcome.Ok(value):
            return value
        Outcome.Error(message):
            print(message)
            return -1
        Outcome.Pending():
            return 0
}

fn main() -> i64 {
    print(score(Outcome.Ok(42)))
    print(score(Outcome.Pending()))
    print(score(Outcome.Error("offline")))
    return 0
}
