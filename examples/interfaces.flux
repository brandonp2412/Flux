interface Storage {
    fn load(path: str) -> (str, error)
    fn save(path: str, data: str, *, durable: bool) -> error
}

interface Clock {
    fn now() -> i64
}

fn main() -> i64 {
    print("interface declarations are compile-time contracts")
    return 0
}
