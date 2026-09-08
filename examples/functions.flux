type Mapper = fn(i64) -> i64

fn double(value: i64) -> i64 {
    return value * 2
}

fn increment(value: i64) -> i64 {
    return value + 1
}

fn apply(transform: Mapper, value: i64) -> i64 {
    return transform(value)
}

fn choose(double_it: bool) -> Mapper {
    if double_it:
        return double
    return increment
}

fn main() -> i64 {
    let mapper: Mapper = double
    print(apply(mapper, 21))
    let selected: Mapper = choose(false)
    print(selected(41))
    return 0
}
