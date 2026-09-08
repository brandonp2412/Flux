type Mapper = fn(i64, str) -> bool

fn apply(transform: Mapper) -> bool {
    return transform(42, "Flux")
}

fn accepts(value: i64, label: str) -> bool {
    return true
}

fn main() -> i64 {
    let result: bool = apply(accepts)
    print(result)
    return 0
}
