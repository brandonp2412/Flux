fn classify(value: i64) -> str {
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    else:
        return "positive"
}

fn main() -> i64 {
    print(classify(-1))
    print(classify(0))
    print(classify(4))
    return 0
}
