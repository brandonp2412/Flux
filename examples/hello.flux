fn square(value: i64) -> i64 {
    return value * value
}

fn main() -> i64 {
    print("Flux bootstrap compiler")
    for i in 0..5:
        let squared: i64 = square(i)
        print(squared)
    if square(3) == 9:
        print("typechecked and native")
    return 0
}
