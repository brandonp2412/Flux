struct Point {
    x: i64
}

fn square(value: i64) -> i64 { value * value }
fn point(value: i64) -> Point { Point { x: value } }
fn checked(value: i64) -> (i64, error) { pair(value) }

fn choose(flag: bool) -> i64 {
    if flag:
        return 7
    return 2
}

fn pair(value: i64) -> (i64, error) {
    return value, nil
}

fn main() -> i64 {
    print(square(6))
    print(choose(false))
    let item: Point = point(9)
    print(item.x)
    let value: i64, err: error = checked(12)
    print(value)
    print(err)
    return 0
}
