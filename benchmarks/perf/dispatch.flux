interface Operation {
    fn apply(value: i64) -> i64
}

struct Offset {
    amount: i64
}

impl Operation for Offset {
    apply: applyOffset
}

fn applyOffset(operation: Offset, value: i64) -> i64 {
    return operation.amount + value
}

fn run(operation: Operation) -> i64 {
    var total: i64 = 0
    for value in 0..100000000:
        total = total + Operation.apply(operation, value)
    return total
}

fn main() -> i64 {
    let operation: Operation = Operation(Offset { amount: 1 })
    print(run(operation))
    return 0
}
