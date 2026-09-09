trait Operation {
    fn apply(&self, value: i64) -> i64;
}

struct Offset {
    amount: i64,
}

impl Operation for Offset {
    fn apply(&self, value: i64) -> i64 {
        self.amount.checked_add(value).expect("benchmark overflow")
    }
}

fn run(operation: &dyn Operation) -> i64 {
    let mut total = 0_i64;
    for value in 0_i64..100_000_000 {
        total = total
            .checked_add(operation.apply(value))
            .expect("benchmark overflow");
    }
    total
}

fn main() {
    let operation = Offset { amount: 1 };
    println!("{}", run(&operation));
}
