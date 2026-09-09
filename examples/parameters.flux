const defaultCount: i64 = 3

fn describe(prefix: str, suffix: str = "!", *, count: i64 = defaultCount, label: str) -> i64 {
    print(prefix)
    print(suffix)
    print(label)
    return count
}

fn main() -> i64 {
    print(describe("hello", label: "world"))
    print(describe("hi", "?", count: 5, label: "there"))
    return 0
}
